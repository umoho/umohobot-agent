use rig::completion::ToolDefinition;
use rig::tool::Tool;
use teloxide::types::{ChatId, ParseMode};

use crate::{
    platforms::{ReplyHandle, TelegramOutbox},
    tools::protocol::{
        ChatBatchRequest, ChatBatchResult, ChatOp, ChatOpOutcome, ChatOpStatus, MessageDraft,
        MessageLocator, MessageRef, ToolContext, ToolIssue, chat_batch_request_schema,
    },
    tools::{ToolKind, ToolRisk, ToolSpec},
};

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
                    Err(issue) => ChatOpOutcome {
                        index,
                        op: ChatOp::StartTyping {
                            target: issue.target,
                        },
                        status: ChatOpStatus::BestEffort,
                        message_ref: None,
                        issue: Some(issue.issue),
                    },
                };
                planned_count += 1;
                outcome
            }
            ChatOp::StopTyping { target } => {
                let outcome =
                    execute_stop_typing(outbox, &context, index, target, best_effort).await;
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

fn parse_chat_id(room_id: &str) -> Result<ChatId, ToolIssue> {
    room_id.parse::<i64>().map(ChatId).map_err(|_| {
        ToolIssue::new(
            "invalid_room_id",
            "room_id must be a numeric Telegram chat id",
        )
    })
}

#[allow(deprecated)]
fn parse_parse_mode(value: Option<&str>) -> Result<Option<ParseMode>, ToolIssue> {
    let Some(value) = value else {
        return Ok(None);
    };

    let normalized = value.trim().to_ascii_lowercase();
    let mode = match normalized.as_str() {
        "html" => ParseMode::Html,
        "markdown" => ParseMode::Markdown,
        "markdownv2" | "markdown_v2" => ParseMode::MarkdownV2,
        _ => {
            return Err(ToolIssue::new(
                "invalid_parse_mode",
                format!("unsupported parse_mode: {value}"),
            ));
        }
    };

    Ok(Some(mode))
}

async fn execute_send(
    outbox: &TelegramOutbox,
    context: &ToolContext,
    index: usize,
    draft: MessageDraft,
) -> Result<SendResult, FailedOperation> {
    let chat_id = parse_chat_id(context.room_id.as_str()).map_err(|issue| FailedOperation {
        issue,
        draft: draft.clone(),
    })?;
    let parse_mode =
        parse_parse_mode(draft.parse_mode.as_deref()).map_err(|issue| FailedOperation {
            issue,
            draft: draft.clone(),
        })?;
    let reply_to = draft
        .reply_to
        .as_ref()
        .and_then(|locator| locator.resolved_message_id(context))
        .map(str::to_string);

    match outbox
        .send_draft(
            chat_id,
            context.thread_id.as_deref(),
            draft.text.as_str(),
            parse_mode,
            draft.disable_web_page_preview,
            draft.silent,
            reply_to.as_deref(),
        )
        .await
    {
        Ok(message) => {
            let message_ref = reply_handle_to_message_ref(context, &message);
            Ok(SendResult {
                outcome: ChatOpOutcome {
                    index,
                    op: ChatOp::Send {
                        draft: draft.clone(),
                    },
                    status: ChatOpStatus::Applied,
                    message_ref: Some(message_ref),
                    issue: None,
                },
                text: draft.text,
            })
        }
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
) -> Result<EditResult, FailedEditOperation> {
    let chat_id =
        parse_chat_id(target.resolved_room_id(context)).map_err(|issue| FailedEditOperation {
            issue,
            target: target.clone(),
            draft: draft.clone(),
        })?;
    let message_id = target
        .resolved_message_id(context)
        .ok_or_else(|| FailedEditOperation {
            issue: ToolIssue::new(
                "missing_target",
                "edit targets must resolve to a message_id",
            ),
            target: target.clone(),
            draft: draft.clone(),
        })?
        .to_string();
    let parse_mode =
        parse_parse_mode(draft.parse_mode.as_deref()).map_err(|issue| FailedEditOperation {
            issue,
            target: target.clone(),
            draft: draft.clone(),
        })?;

    match outbox
        .edit_draft(
            chat_id,
            message_id.as_str(),
            draft.text.as_str(),
            parse_mode,
            draft.disable_web_page_preview,
        )
        .await
    {
        Ok(()) => Ok(EditResult {
            outcome: ChatOpOutcome {
                index,
                op: ChatOp::Edit {
                    target: target.clone(),
                    draft: draft.clone(),
                },
                status: ChatOpStatus::Applied,
                message_ref: Some(target.to_message_ref(context, message_id)),
                issue: None,
            },
            text: draft.text,
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
    let chat_id =
        parse_chat_id(target.resolved_room_id(context)).map_err(|issue| FailedDeleteOperation {
            issue,
            target: target.clone(),
        })?;
    let message_id = target
        .resolved_message_id(context)
        .ok_or_else(|| FailedDeleteOperation {
            issue: ToolIssue::new(
                "missing_target",
                "delete targets must resolve to a message_id",
            ),
            target: target.clone(),
        })?
        .to_string();

    match outbox.delete_message(chat_id, message_id.as_str()).await {
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
    let chat_id =
        parse_chat_id(target.resolved_room_id(context)).map_err(|issue| FailedDeleteOperation {
            issue,
            target: target.clone(),
        })?;
    match outbox
        .send_typing(chat_id, target.resolved_thread_id(context).as_deref())
        .await
    {
        Ok(()) => Ok(ChatOpOutcome {
            index,
            op: ChatOp::StartTyping { target },
            status: ChatOpStatus::BestEffort,
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
    let chat_id = match parse_chat_id(target.resolved_room_id(context)) {
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

    match outbox
        .send_typing(chat_id, target.resolved_thread_id(context).as_deref())
        .await
    {
        Ok(()) => ChatOpOutcome {
            index,
            op: ChatOp::StopTyping {
                target: target.clone(),
            },
            status: ChatOpStatus::BestEffort,
            message_ref: None,
            issue: None,
        },
        Err(err) => ChatOpOutcome {
            index,
            op: ChatOp::StopTyping {
                target: target.clone(),
            },
            status: ChatOpStatus::BestEffort,
            message_ref: None,
            issue: Some(ToolIssue::new("telegram_typing_failed", err.to_string())),
        },
    }
}

fn reply_handle_to_message_ref(context: &ToolContext, message: &ReplyHandle) -> MessageRef {
    let mut message_ref = context.message_ref(message.message_id.clone());
    message_ref.platform = message.platform.as_str().to_string();
    message_ref.room_id = message.room_id.clone();
    message_ref
}

#[derive(Clone, Debug)]
struct SendResult {
    outcome: ChatOpOutcome,
    text: String,
}

#[derive(Clone, Debug)]
struct EditResult {
    outcome: ChatOpOutcome,
    text: String,
}

#[derive(Clone, Debug)]
struct TrackedMessage {
    order: usize,
    message_ref: MessageRef,
    text: Option<String>,
    deleted: bool,
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
        state.text = text;
        state.deleted = deleted;
        state.message_ref = message_ref;
    } else {
        states.push(TrackedMessage {
            order,
            message_ref,
            text,
            deleted,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tool_definition_mentions_chat_batch() {
        let tool = ChatBatchTool::new();
        let definition = tool.definition(String::new()).await;

        assert_eq!(definition.name, "chat.batch");
        let params = definition.parameters.to_string();
        assert!(params.contains("\"context\""));
        assert!(params.contains("\"operations\""));
    }

    #[tokio::test]
    async fn call_returns_planned_batch_shape_without_bound_outbox() {
        let tool = ChatBatchTool::new();
        let result = tool
            .call(ChatBatchRequest {
                context: ToolContext::new("telegram", "telegram:1", "1"),
                operations: vec![
                    ChatOp::Send {
                        draft: MessageDraft::new("hello"),
                    },
                    ChatOp::StopTyping {
                        target: MessageLocator::new("1"),
                    },
                ],
                best_effort: true,
            })
            .await
            .unwrap();

        assert_eq!(result.requested_count, 2);
        assert_eq!(result.planned_count, 2);
        assert!(!result.partial_failure);
        assert_eq!(result.outcomes.len(), 2);
        assert_eq!(
            result.outcomes[0].status,
            crate::tools::protocol::ChatOpStatus::Applied
        );
        assert!(result.outcomes[0].message_ref.is_some());
        assert_eq!(
            result.outcomes[1].status,
            crate::tools::protocol::ChatOpStatus::BestEffort
        );
        assert!(result.final_message.is_some());
        assert_eq!(result.final_visible_text.as_deref(), Some("hello"));
    }
}
