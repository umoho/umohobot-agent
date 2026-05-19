use rig::completion::ToolDefinition;
use rig::tool::Tool;

use crate::{
    platforms::PlatformKind,
    storage::{Storage, StorageError, SummaryRecord, ThreadScope},
    tools::protocol::{
        HistoryDirection, HistoryItem, HistoryQueryRequest, HistoryQueryResult, HistoryWindow,
        MessageRef, ToolContext, ToolIssue, history_query_request_schema,
    },
    tools::{ToolKind, ToolRisk, ToolSpec},
};

#[derive(Clone, Debug)]
pub struct HistoryQueryTool {
    storage: Storage,
}

#[derive(Debug, thiserror::Error)]
pub enum HistoryQueryError {
    #[error("history query limit must be greater than zero")]
    EmptyWindow,
    #[error("history query window is invalid: after_seq must be less than before_seq")]
    InvalidWindowRange,
    #[error("unsupported platform in tool context: {0}")]
    UnsupportedPlatform(String),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

impl HistoryQueryTool {
    pub fn new() -> Self {
        Self {
            storage: Storage::new(None),
        }
    }

    pub fn with_storage(storage: Storage) -> Self {
        Self { storage }
    }

    pub fn spec() -> ToolSpec {
        ToolSpec {
            kind: ToolKind::Custom("storage".to_string()),
            name: Self::NAME.to_string(),
            description: "Query a bounded history window for the current thread. The tool is host-side and returns model-friendly, structured history items.".to_string(),
            risk: ToolRisk::Medium,
        }
    }
}

impl Default for HistoryQueryTool {
    fn default() -> Self {
        Self::new()
    }
}

impl Tool for HistoryQueryTool {
    const NAME: &'static str = "thread.history.query";

    type Error = HistoryQueryError;
    type Args = HistoryQueryRequest;
    type Output = HistoryQueryResult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Query a bounded history window for the current thread. The tool is host-side and returns model-friendly, structured history items.".to_string(),
            parameters: history_query_request_schema(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        if args.window.limit == 0 {
            return Err(HistoryQueryError::EmptyWindow);
        }

        if matches!(args.window.direction, HistoryDirection::NewestFirst)
            && args
                .window
                .after_seq
                .zip(args.window.before_seq)
                .is_some_and(|(after, before)| after >= before)
        {
            return Err(HistoryQueryError::InvalidWindowRange);
        }
        if matches!(args.window.direction, HistoryDirection::OldestFirst)
            && args
                .window
                .after_seq
                .zip(args.window.before_seq)
                .is_some_and(|(after, before)| after >= before)
        {
            return Err(HistoryQueryError::InvalidWindowRange);
        }

        let platform = PlatformKind::from_str(&args.context.platform)
            .ok_or_else(|| HistoryQueryError::UnsupportedPlatform(args.context.platform.clone()))?;
        let scope = ThreadScope::new(
            platform,
            args.context.room_id.clone(),
            args.context.thread_id.clone(),
        );

        let result = load_history_window(&self.storage, args, scope).await?;
        Ok(result)
    }
}

async fn load_history_window(
    storage: &Storage,
    request: HistoryQueryRequest,
    scope: ThreadScope,
) -> Result<HistoryQueryResult, HistoryQueryError> {
    let HistoryQueryRequest { context, window } = request;
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    let mut truncated = false;
    let mut complete = true;

    let lower_bound = window.after_seq.unwrap_or(0);
    let upper_bound = window.before_seq.unwrap_or(i64::MAX);
    let mut cursor = lower_bound;
    let page_limit = i64::try_from(window.limit.min(200)).unwrap_or(200);

    loop {
        let slice = storage
            .load_thread_history_slice(&scope, cursor, upper_bound, page_limit)
            .await?;
        let Some(slice) = slice else {
            warnings.push(ToolIssue::new(
                "thread_not_found",
                "no thread history was found for the requested scope",
            ));
            complete = false;
            break;
        };

        if slice.events.is_empty() {
            break;
        }

        cursor = slice.events.last().map(|event| event.seq).unwrap_or(cursor);
        for event in slice.events {
            if event_matches_filters(&event.kind, &window) {
                items.push(history_item_from_event(&context, &event));
                if items.len() >= window.limit {
                    truncated = true;
                    complete = false;
                    break;
                }
            }
        }

        if truncated {
            break;
        }
    }

    if window.include_summary {
        if let Some(summary) = load_summary(storage, &scope, upper_bound).await? {
            items.push(history_item_from_summary(&context, &summary));
        }
    }

    items.sort_by(|left, right| match window.direction {
        HistoryDirection::OldestFirst => left
            .seq
            .cmp(&right.seq)
            .then_with(|| left.message.message_id.cmp(&right.message.message_id)),
        HistoryDirection::NewestFirst => right
            .seq
            .cmp(&left.seq)
            .then_with(|| right.message.message_id.cmp(&left.message.message_id)),
    });

    if items.len() > window.limit {
        truncated = true;
        complete = false;
        items.truncate(window.limit);
    }

    Ok(HistoryQueryResult {
        context,
        window,
        item_count: items.len(),
        truncated,
        complete,
        items,
        warnings,
    })
}

async fn load_summary(
    storage: &Storage,
    scope: &ThreadScope,
    before_seq_exclusive: i64,
) -> Result<Option<SummaryRecord>, HistoryQueryError> {
    let Some(thread) = storage
        .load_latest_thread(scope)
        .await?
        .filter(|thread| thread.scope.thread_key() == scope.thread_key())
    else {
        return Ok(None);
    };

    let summary = if before_seq_exclusive == i64::MAX {
        storage.load_latest_summary(thread.id).await?
    } else {
        storage
            .load_latest_summary_before_seq(thread.id, before_seq_exclusive)
            .await?
    };

    Ok(summary)
}

fn event_matches_filters(kind: &crate::storage::EventKind, window: &HistoryWindow) -> bool {
    match kind {
        crate::storage::EventKind::InboundMessage | crate::storage::EventKind::AssistantMessage => {
            true
        }
        crate::storage::EventKind::ToolCall | crate::storage::EventKind::ToolResult => {
            window.include_tool_events
        }
        crate::storage::EventKind::Summary => window.include_summary,
        crate::storage::EventKind::SystemNote => window.include_system_notes,
    }
}

fn history_item_from_event(
    context: &ToolContext,
    event: &crate::storage::EventRecord,
) -> HistoryItem {
    HistoryItem {
        message: message_ref_from_event(context, event),
        seq: event.seq,
        turn_id: event.turn_id.map(|turn_id| turn_id.to_string()),
        kind: event.kind.as_str().to_string(),
        role: history_role(event.kind).to_string(),
        speaker: event
            .sender_name
            .clone()
            .or_else(|| event.sender_id.clone()),
        body: render_event_body_text(&event.content),
        attachments: extract_attachments(&event.content),
        visible_to_model: event.visible_to_model,
    }
}

fn history_item_from_summary(context: &ToolContext, summary: &SummaryRecord) -> HistoryItem {
    HistoryItem {
        message: context.message_ref(format!(
            "summary:{}:{}",
            summary.thread_id, summary.upto_seq
        )),
        seq: summary.upto_seq,
        turn_id: None,
        kind: "summary".to_string(),
        role: "system".to_string(),
        speaker: None,
        body: summary.summary_text.trim().to_string(),
        attachments: Vec::new(),
        visible_to_model: true,
    }
}

fn message_ref_from_event(
    context: &ToolContext,
    event: &crate::storage::EventRecord,
) -> MessageRef {
    let message_id = event
        .platform_message_id
        .clone()
        .unwrap_or_else(|| format!("event:{}", event.id));
    let mut message_ref = context.message_ref(message_id);
    message_ref.sender_id = event.sender_id.clone();
    message_ref.sender_name = event.sender_name.clone();
    message_ref
}

fn history_role(kind: crate::storage::EventKind) -> &'static str {
    match kind {
        crate::storage::EventKind::InboundMessage => "user",
        crate::storage::EventKind::AssistantMessage => "assistant",
        crate::storage::EventKind::ToolCall | crate::storage::EventKind::ToolResult => "tool",
        crate::storage::EventKind::Summary | crate::storage::EventKind::SystemNote => "system",
    }
}

fn render_event_body_text(content: &serde_json::Value) -> String {
    let text = extract_text(content);
    let attachments = content
        .get("attachments")
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .map(describe_json_attachment)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let reply = content
        .get("reply")
        .and_then(|value| value.as_object())
        .map(describe_json_reply);
    let mention = content
        .get("is_mention")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let message_kind = content.get("kind").and_then(|value| value.as_str());
    let body_kind = content.get("body_kind").and_then(|value| value.as_str());
    render_body_text(
        text.as_deref(),
        message_kind,
        body_kind,
        &attachments,
        reply.as_deref(),
        mention,
    )
}

fn extract_text(content: &serde_json::Value) -> Option<String> {
    for key in [
        "text",
        "summary_text",
        "output",
        "message",
        "content",
        "note",
    ] {
        if let Some(text) = content.get(key).and_then(|value| value.as_str()) {
            if !text.trim().is_empty() {
                return Some(text.to_string());
            }
        }
    }

    content.as_str().map(ToString::to_string)
}

fn extract_attachments(content: &serde_json::Value) -> Vec<String> {
    content
        .get("attachments")
        .and_then(|value| value.as_array())
        .map(|values| values.iter().map(describe_json_attachment).collect())
        .unwrap_or_default()
}

fn describe_json_attachment(value: &serde_json::Value) -> String {
    let mut fields = Vec::new();
    if let Some(kind) = value.get("kind").and_then(|value| value.as_str()) {
        fields.push(kind.to_string());
    }
    if let Some(file_id) = value.get("file_id").and_then(|value| value.as_str()) {
        fields.push(format!("file_id={file_id}"));
    }
    if let Some(file_unique_id) = value.get("file_unique_id").and_then(|value| value.as_str()) {
        fields.push(format!("file_unique_id={file_unique_id}"));
    }
    if let Some(file_name) = value.get("file_name").and_then(|value| value.as_str()) {
        fields.push(format!("file_name={file_name}"));
    }
    if let Some(mime_type) = value.get("mime_type").and_then(|value| value.as_str()) {
        fields.push(format!("mime_type={mime_type}"));
    }
    if let Some(url) = value.get("url").and_then(|value| value.as_str()) {
        fields.push(format!("url={url}"));
    }
    if let Some(width) = value.get("width").and_then(|value| value.as_u64()) {
        fields.push(format!("width={width}"));
    }
    if let Some(height) = value.get("height").and_then(|value| value.as_u64()) {
        fields.push(format!("height={height}"));
    }
    if let Some(size_bytes) = value.get("size_bytes").and_then(|value| value.as_u64()) {
        fields.push(format!("size_bytes={size_bytes}"));
    }
    fields.join(", ")
}

fn describe_json_reply(value: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut fields = Vec::new();
    if let Some(message_id) = value.get("message_id").and_then(|value| value.as_str()) {
        fields.push(format!("message_id={message_id}"));
    }
    if let Some(sender_id) = value.get("sender_id").and_then(|value| value.as_str()) {
        fields.push(format!("sender_id={sender_id}"));
    }
    if let Some(kind) = value.get("kind").and_then(|value| value.as_str()) {
        fields.push(format!("kind={kind}"));
    }
    if let Some(body_kind) = value.get("body_kind").and_then(|value| value.as_str()) {
        fields.push(format!("body_kind={body_kind}"));
    }
    if let Some(text) = value.get("text").and_then(|value| value.as_str()) {
        if !text.trim().is_empty() {
            fields.push(format!("text={text}"));
        }
    }
    if let Some(attachments) = value.get("attachments").and_then(|value| value.as_array()) {
        if !attachments.is_empty() {
            fields.push(format!(
                "attachments={}",
                attachments
                    .iter()
                    .map(describe_json_attachment)
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
    }
    if fields.is_empty() {
        "reply".to_string()
    } else {
        fields.join(", ")
    }
}

fn render_body_text(
    text: Option<&str>,
    message_kind: Option<&str>,
    body_kind: Option<&str>,
    attachments: &[String],
    reply: Option<&str>,
    mention: bool,
) -> String {
    let mut lines = Vec::new();
    if let Some(text) = text.filter(|value| !value.trim().is_empty()) {
        lines.push(text.to_string());
    }

    let mut context_lines = Vec::new();
    if let Some(kind) = message_kind.filter(|value| *value != "text") {
        context_lines.push(format!("message_kind={kind}"));
    }
    if let Some(kind) = body_kind.filter(|value| *value != "text") {
        context_lines.push(format!("body_kind={kind}"));
    }
    if !attachments.is_empty() {
        context_lines.push(format!("attachments={}", attachments.join("; ")));
    }
    if let Some(reply) = reply.filter(|value| !value.trim().is_empty()) {
        context_lines.push(format!("reply={reply}"));
    }
    if mention && (context_lines.is_empty() || lines.is_empty()) {
        context_lines.push("bot_mentioned=true".to_string());
    }

    if context_lines.is_empty() {
        return lines.join("\n");
    }

    if !lines.is_empty() {
        lines.push(String::new());
    }
    lines.push("context:".to_string());
    lines.extend(context_lines.into_iter().map(|line| format!("- {line}")));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;

    use crate::{
        platforms::PlatformKind,
        storage::{EventKind, InboundMessageRecord, NewEvent, SummaryWrite, ThreadScope},
    };

    fn unique_storage() -> Storage {
        let mut path = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("umohobot-history-{nonce}"));
        Storage::new(Some(path))
    }

    #[tokio::test]
    async fn tool_definition_mentions_history_query() {
        let tool = HistoryQueryTool::new();
        let definition = tool.definition(String::new()).await;

        assert_eq!(definition.name, "thread.history.query");
        let params = definition.parameters.to_string();
        assert!(params.contains("\"window\""));
        assert!(params.contains("\"context\""));
    }

    #[tokio::test]
    async fn call_returns_real_history_slice_with_filters() {
        let storage = unique_storage();
        storage.ensure_ready().await.unwrap();
        let scope = ThreadScope::new(PlatformKind::Telegram, "chat-1", None);
        let observation = storage
            .observe_message(InboundMessageRecord {
                scope: scope.clone(),
                platform_message_id: "msg-1".to_string(),
                sender_id: "user-1".to_string(),
                sender_name: Some("Alice".to_string()),
                reply_to_platform_message_id: None,
                content: json!({
                    "kind": "text",
                    "body_kind": "text",
                    "text": "hello",
                }),
                visible_to_model: true,
                lease_until: None,
            })
            .await
            .unwrap();
        let thread_id = observation.thread.id;

        storage
            .append_event(NewEvent {
                thread_id,
                turn_id: None,
                kind: EventKind::AssistantMessage,
                sender_id: Some("bot".to_string()),
                sender_name: Some("Bot".to_string()),
                platform_message_id: Some("msg-2".to_string()),
                reply_to_platform_message_id: Some("msg-1".to_string()),
                content: json!({
                    "kind": "text",
                    "body_kind": "text",
                    "text": "reply",
                }),
                visible_to_model: true,
                created_at: None,
            })
            .await
            .unwrap();
        storage
            .append_event(NewEvent {
                thread_id,
                turn_id: None,
                kind: EventKind::ToolCall,
                sender_id: Some("bot".to_string()),
                sender_name: Some("Bot".to_string()),
                platform_message_id: Some("msg-3".to_string()),
                reply_to_platform_message_id: Some("msg-2".to_string()),
                content: json!({
                    "name": "calc",
                    "arguments": {"expr": "1+1"},
                }),
                visible_to_model: true,
                created_at: None,
            })
            .await
            .unwrap();
        storage
            .append_summary(SummaryWrite {
                thread_id,
                upto_seq: 2,
                summary_text: "summary text".to_string(),
                model: "test-model".to_string(),
                prompt_version: 1,
                created_at: None,
            })
            .await
            .unwrap();

        let tool = HistoryQueryTool::with_storage(storage);
        let result = tool
            .call(HistoryQueryRequest {
                context: ToolContext::new("telegram", "telegram:1", "chat-1"),
                window: HistoryWindow {
                    limit: 10,
                    before_seq: None,
                    after_seq: None,
                    direction: HistoryDirection::NewestFirst,
                    include_summary: true,
                    include_system_notes: false,
                    include_tool_events: false,
                },
            })
            .await
            .unwrap();

        assert_eq!(result.item_count, 3);
        assert!(result.complete);
        assert!(!result.truncated);
        assert!(result.warnings.is_empty());
        assert!(result.items.iter().any(|item| item.kind == "summary"));
        assert!(
            result
                .items
                .iter()
                .any(|item| item.kind == "assistant_message")
        );
        assert!(
            result
                .items
                .iter()
                .any(|item| item.kind == "inbound_message")
        );
        assert!(!result.items.iter().any(|item| item.kind == "tool_call"));
    }
}
