use contracts::protocol::{
    ChatBatchRequest, ChatBatchResult, ChatOp, ChatOpOutcome, ChatOpStatus, MessageDraft,
    MessageLocator, MessageRef, ToolContext, ToolIssue, chat_batch_request_schema,
};
use contracts::{ReplyHandle, ToolKind, ToolRisk, ToolSpec};
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use teloxide::types::{ChatId, ParseMode};

use crate::outbox::TelegramOutbox;

#[derive(Clone, Debug, Default)]
pub struct ChatBatchTool {
    outbox: Option<TelegramOutbox>,
}

#[derive(Debug, thiserror::Error)]
pub enum ChatBatchError {
    #[error("chat batch request must contain at least one operation")]
    EmptyBatch,
}

impl ChatBatchTool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_outbox(outbox: TelegramOutbox) -> Self {
        Self {
            outbox: Some(outbox),
        }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            kind: ToolKind::Custom("telegram".to_string()),
            name: Self::NAME.to_string(),
            description: "Execute a sequential batch of chat actions for the current thread. The host can send, edit, delete, and emit typing indicators.".to_string(),
            risk: ToolRisk::High,
        }
    }

    fn outbox(&self) -> Option<&TelegramOutbox> {
        self.outbox.as_ref()
    }
}

impl Tool for ChatBatchTool {
    const NAME: &'static str = "chat.batch";

    type Error = ChatBatchError;
    type Args = ChatBatchRequest;
    type Output = ChatBatchResult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Execute a sequential batch of chat actions for the current thread. The host can send, edit, delete, and emit typing indicators.".to_string(),
            parameters: chat_batch_request_schema(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        if args.operations.is_empty() {
            return Err(ChatBatchError::EmptyBatch);
        }

        match self.outbox() {
            Some(outbox) if outbox.is_bound() => Ok(execute_batch(outbox, args).await),
            None => Ok(ChatBatchResult::planned(args)),
            Some(_) => Ok(ChatBatchResult::planned(args)),
        }
    }
}

async fn execute_batch(outbox: &TelegramOutbox, request: ChatBatchRequest) -> ChatBatchResult {
    let ChatBatchRequest {
        context,
        operations,
        best_effort,
    } = request;
    let requested_count = operations.len();
    let mut planned_count = 0usize;
    let mut rejected_count = 0usize;
    let mut outcomes = Vec::with_capacity(requested_count);
    let mut warnings = Vec::new();
    let mut message_states = Vec::new();

    for (index, op) in operations.into_iter().enumerate() {
        let outcome = match op {
            ChatOp::Send { draft } => match execute_send(outbox, &context, index, draft).await {
                Ok(result) => {
                    planned_count += 1;
                    if let Some(message_ref) = result.outcome.message_ref.clone() {
                        message_states.push(TrackedMessage {
                            order: index,
                            message_ref,
                            text: Some(result.text.clone()),
                            deleted: false,
                        });
                    }
                    result.outcome
                }
                Err(issue) => {
                    rejected_count += 1;
                    ChatOpOutcome {
                        index,
                        op: ChatOp::Send { draft: issue.draft },
                        status: ChatOpStatus::Rejected,
                        message_ref: None,
                        issue: Some(issue.issue),
                    }
                }
            },
            ChatOp::Edit { target, draft } => {
                match execute_edit(outbox, &context, index, target, draft).await {
                    Ok(result) => {
                        planned_count += 1;
                        if let Some(message_ref) = result.outcome.message_ref.clone() {
                            upsert_message_state(
                                &mut message_states,
                                index,
                                message_ref,
                                Some(result.text.clone()),
                                false,
                            );
                        }
                        result.outcome
                    }
                    Err(issue) => {
                        rejected_count += 1;
                        ChatOpOutcome {
                            index,
                            op: ChatOp::Edit {
                                target: issue.target,
                                draft: issue.draft,
                            },
                            status: ChatOpStatus::Rejected,
                            message_ref: None,
                            issue: Some(issue.issue),
                        }
                    }
                }
            }
            ChatOp::Delete { target } => {
                match execute_delete(outbox, &context, index, target).await {
                    Ok(outcome) => {
                        planned_count += 1;
                        if let Some(message_ref) = outcome.message_ref.clone() {
                            if let Some(state) = message_states.iter_mut().find(|state| {
                                state.message_ref.message_id == message_ref.message_id
                            }) {
                                state.deleted = true;
                            }
                        }
                        outcome
                    }
                    Err(issue) => {
                        rejected_count += 1;
                        ChatOpOutcome {
                            index,
                            op: ChatOp::Delete {
                                target: issue.target,
                            },
                            status: ChatOpStatus::Rejected,
                            message_ref: None,
                            issue: Some(issue.issue),
                        }
                    }
                }
            }
            ChatOp::StartTyping { target } => {
                let outcome = match execute_start_typing(outbox, &context, index, target).await {
                    Ok(outcome) => outcome,
                    Err(issue) => {
                        if best_effort {
                            ChatOpOutcome {
                                index,
                                op: ChatOp::StartTyping {
                                    target: issue.target,
                                },
                                status: ChatOpStatus::BestEffort,
                                message_ref: None,
                                issue: Some(issue.issue),
                            }
                        } else {
                            rejected_count += 1;
                            ChatOpOutcome {
                                index,
                                op: ChatOp::StartTyping {
                                    target: issue.target,
                                },
                                status: ChatOpStatus::Rejected,
                                message_ref: None,
                                issue: Some(issue.issue),
                            }
                        }
                    }
                };
                planned_count += 1;
                outcome
            }
            ChatOp::StopTyping { target } => {
                let outcome =
                    execute_stop_typing(outbox, &context, index, target, best_effort).await;
                if matches!(outcome.status, ChatOpStatus::Rejected) {
                    rejected_count += 1;
                }
                planned_count += 1;
                outcome
            }
        };

        if matches!(outcome.status, ChatOpStatus::Rejected) {
            warnings.push(ToolIssue::new(
                "chat_operation_rejected",
                "one or more chat operations were rejected",
            ));
        }

        outcomes.push(outcome);
    }

    let partial_failure = rejected_count > 0;
    if partial_failure {
        warnings.push(ToolIssue::new(
            "partial_failure",
            "one or more chat operations were rejected",
        ));
    }

    let final_message = message_states
        .iter()
        .filter(|state| !state.deleted)
        .max_by_key(|state| state.order)
        .map(|state| state.message_ref.clone());
    let final_visible_text = final_message.as_ref().and_then(|message_ref| {
        message_states
            .iter()
            .find(|state| state.message_ref.message_id == message_ref.message_id)
            .and_then(|state| state.text.clone())
    });

    ChatBatchResult {
        context,
        requested_count,
        planned_count,
        rejected_count,
        partial_failure,
        final_message,
        final_visible_text,
        outcomes,
        warnings,
    }
}

struct FailedOperation {
    issue: ToolIssue,
    draft: MessageDraft,
}

struct FailedEditOperation {
    issue: ToolIssue,
    target: MessageLocator,
    draft: MessageDraft,
}

struct FailedDeleteOperation {
    issue: ToolIssue,
    target: MessageLocator,
}

struct ExecutedOperation {
    outcome: ChatOpOutcome,
    text: String,
}

struct TrackedMessage {
    order: usize,
    message_ref: MessageRef,
    text: Option<String>,
    deleted: bool,
}

async fn execute_send(
    outbox: &TelegramOutbox,
    context: &ToolContext,
    index: usize,
    draft: MessageDraft,
) -> Result<ExecutedOperation, FailedOperation> {
    let text = draft.text.clone();
    let disable_web_page_preview = draft.disable_web_page_preview;
    let silent = draft.silent;
    let room_id = draft
        .reply_to
        .as_ref()
        .map(|reply| reply.resolved_room_id(context).to_string())
        .unwrap_or_else(|| context.room_id.clone());
    let chat_id = match parse_chat_id(&room_id) {
        Ok(chat_id) => chat_id,
        Err(issue) => return Err(FailedOperation { issue, draft }),
    };
    let reply_to = draft
        .reply_to
        .as_ref()
        .and_then(|reply| reply.resolved_message_id(context))
        .map(str::to_string);
    let thread_id = draft
        .reply_to
        .as_ref()
        .and_then(|reply| reply.thread_id.clone())
        .or_else(|| context.thread_id.clone());
    let parse_mode = match parse_parse_mode(draft.parse_mode.as_deref()) {
        Ok(parse_mode) => parse_mode,
        Err(issue) => return Err(FailedOperation { issue, draft }),
    };

    match outbox
        .send_draft(
            chat_id,
            thread_id.as_deref(),
            &draft.text,
            parse_mode,
            disable_web_page_preview,
            silent,
            reply_to.as_deref(),
        )
        .await
    {
        Ok(message) => Ok(ExecutedOperation {
            outcome: ChatOpOutcome {
                index,
                op: ChatOp::Send { draft },
                status: ChatOpStatus::Applied,
                message_ref: Some(reply_handle_to_message_ref(context, &message)),
                issue: None,
            },
            text,
        }),
        Err(err) => Err(FailedOperation {
            issue: ToolIssue::new("telegram_send_failed", err.to_string()),
            draft,
        }),
    }
}

async fn execute_edit(
    outbox: &TelegramOutbox,
    context: &ToolContext,
    index: usize,
    target: MessageLocator,
    draft: MessageDraft,
) -> Result<ExecutedOperation, FailedEditOperation> {
    let text = draft.text.clone();
    let room_id = target.resolved_room_id(context).to_string();
    let chat_id = match parse_chat_id(&room_id) {
        Ok(chat_id) => chat_id,
        Err(issue) => {
            return Err(FailedEditOperation {
                issue,
                target,
                draft,
            });
        }
    };
    let Some(message_id) = target.resolved_message_id(context).map(str::to_string) else {
        return Err(FailedEditOperation {
            issue: ToolIssue::new("missing_message_id", "edit target requires a message id"),
            target,
            draft,
        });
    };
    let parse_mode = match parse_parse_mode(draft.parse_mode.as_deref()) {
        Ok(parse_mode) => parse_mode,
        Err(issue) => {
            return Err(FailedEditOperation {
                issue,
                target,
                draft,
            });
        }
    };
    let disable_web_page_preview = draft.disable_web_page_preview;

    match outbox
        .edit_draft(
            chat_id,
            &message_id,
            &draft.text,
            parse_mode,
            disable_web_page_preview,
        )
        .await
    {
        Ok(()) => Ok(ExecutedOperation {
            text,
            outcome: {
                let message_ref = target.to_message_ref(context, message_id.clone());
                ChatOpOutcome {
                    index,
                    op: ChatOp::Edit {
                        target: target.clone(),
                        draft,
                    },
                    status: ChatOpStatus::Applied,
                    message_ref: Some(message_ref),
                    issue: None,
                }
            },
        }),
        Err(err) => Err(FailedEditOperation {
            issue: ToolIssue::new("telegram_edit_failed", err.to_string()),
            target,
            draft,
        }),
    }
}

async fn execute_delete(
    outbox: &TelegramOutbox,
    context: &ToolContext,
    index: usize,
    target: MessageLocator,
) -> Result<ChatOpOutcome, FailedDeleteOperation> {
    let room_id = target.resolved_room_id(context).to_string();
    let chat_id = match parse_chat_id(&room_id) {
        Ok(chat_id) => chat_id,
        Err(issue) => {
            return Err(FailedDeleteOperation { issue, target });
        }
    };
    let Some(message_id) = target.resolved_message_id(context).map(str::to_string) else {
        return Err(FailedDeleteOperation {
            issue: ToolIssue::new("missing_message_id", "delete target requires a message id"),
            target,
        });
    };

    match outbox.delete_message(chat_id, &message_id).await {
        Ok(()) => Ok(ChatOpOutcome {
            index,
            op: ChatOp::Delete {
                target: target.clone(),
            },
            status: ChatOpStatus::Applied,
            message_ref: Some(target.to_message_ref(context, message_id)),
            issue: None,
        }),
        Err(err) => Err(FailedDeleteOperation {
            issue: ToolIssue::new("telegram_delete_failed", err.to_string()),
            target,
        }),
    }
}

async fn execute_start_typing(
    outbox: &TelegramOutbox,
    context: &ToolContext,
    index: usize,
    target: MessageLocator,
) -> Result<ChatOpOutcome, FailedDeleteOperation> {
    let room_id = target.resolved_room_id(context).to_string();
    let chat_id = match parse_chat_id(&room_id) {
        Ok(chat_id) => chat_id,
        Err(issue) => {
            return Err(FailedDeleteOperation { issue, target });
        }
    };
    let thread_id = target.resolved_thread_id(context);

    match outbox.send_typing(chat_id, thread_id.as_deref()).await {
        Ok(()) => Ok(ChatOpOutcome {
            index,
            op: ChatOp::StartTyping { target },
            status: ChatOpStatus::Applied,
            message_ref: None,
            issue: None,
        }),
        Err(err) => Err(FailedDeleteOperation {
            issue: ToolIssue::new("telegram_typing_failed", err.to_string()),
            target,
        }),
    }
}

async fn execute_stop_typing(
    outbox: &TelegramOutbox,
    context: &ToolContext,
    index: usize,
    target: MessageLocator,
    best_effort: bool,
) -> ChatOpOutcome {
    let room_id = target.resolved_room_id(context).to_string();
    let chat_id = match parse_chat_id(&room_id) {
        Ok(chat_id) => chat_id,
        Err(issue) => {
            return ChatOpOutcome {
                index,
                op: ChatOp::StopTyping { target },
                status: if best_effort {
                    ChatOpStatus::BestEffort
                } else {
                    ChatOpStatus::Rejected
                },
                message_ref: None,
                issue: Some(issue),
            };
        }
    };
    let thread_id = target.resolved_thread_id(context);

    match outbox.send_typing(chat_id, thread_id.as_deref()).await {
        Ok(()) => ChatOpOutcome {
            index,
            op: ChatOp::StopTyping { target },
            status: ChatOpStatus::Applied,
            message_ref: None,
            issue: None,
        },
        Err(err) => ChatOpOutcome {
            index,
            op: ChatOp::StopTyping { target },
            status: if best_effort {
                ChatOpStatus::BestEffort
            } else {
                ChatOpStatus::Rejected
            },
            message_ref: None,
            issue: Some(ToolIssue::new("telegram_typing_failed", err.to_string())),
        },
    }
}

fn reply_handle_to_message_ref(context: &ToolContext, message: &ReplyHandle) -> MessageRef {
    let mut message_ref = MessageRef::new(
        message.platform.as_str(),
        message.room_id.clone(),
        message.message_id.clone(),
    );
    message_ref.thread_id = context.thread_id.clone();
    message_ref
}

fn parse_chat_id(room_id: &str) -> Result<ChatId, ToolIssue> {
    room_id.parse::<i64>().map(ChatId).map_err(|_| {
        ToolIssue::new(
            "invalid_room_id",
            format!("invalid telegram room id: {room_id}"),
        )
    })
}

#[allow(deprecated)]
fn parse_parse_mode(value: Option<&str>) -> Result<Option<ParseMode>, ToolIssue> {
    match value {
        None => Ok(None),
        Some("Markdown") => Ok(Some(ParseMode::Markdown)),
        Some("MarkdownV2") => Ok(Some(ParseMode::MarkdownV2)),
        Some("HTML") => Ok(Some(ParseMode::Html)),
        Some(other) => Err(ToolIssue::new(
            "unsupported_parse_mode",
            format!("unsupported parse mode: {other}"),
        )),
    }
}

fn upsert_message_state(
    states: &mut Vec<TrackedMessage>,
    order: usize,
    message_ref: MessageRef,
    text: Option<String>,
    deleted: bool,
) {
    if let Some(state) = states
        .iter_mut()
        .find(|state| state.message_ref.message_id == message_ref.message_id)
    {
        state.order = order;
        state.message_ref = message_ref;
        state.text = text;
        state.deleted = deleted;
        return;
    }

    states.push(TrackedMessage {
        order,
        message_ref,
        text,
        deleted,
    });
}
