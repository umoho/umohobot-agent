use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

fn is_false(value: &bool) -> bool {
    !*value
}

fn default_true() -> bool {
    true
}

fn default_limit() -> usize {
    20
}

fn default_newest_first() -> HistoryDirection {
    HistoryDirection::NewestFirst
}

fn object_schema(
    properties: impl IntoIterator<Item = (&'static str, Value)>,
    required: &[&'static str],
) -> Value {
    let properties = properties
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect::<Map<String, Value>>();

    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn string_schema(description: &str) -> Value {
    json!({
        "type": "string",
        "description": description,
    })
}

fn optional_string_schema(description: &str) -> Value {
    string_schema(description)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolContext {
    pub platform: String,
    pub thread_key: String,
    pub room_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder_message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

impl ToolContext {
    pub fn new(
        platform: impl Into<String>,
        thread_key: impl Into<String>,
        room_id: impl Into<String>,
    ) -> Self {
        Self {
            platform: platform.into(),
            thread_key: thread_key.into(),
            room_id: room_id.into(),
            thread_id: None,
            turn_id: None,
            actor_id: None,
            actor_name: None,
            placeholder_message_id: None,
            request_id: None,
        }
    }

    pub fn message_ref(&self, message_id: impl Into<String>) -> MessageRef {
        let mut message_ref = MessageRef::new(
            self.platform.clone(),
            self.room_id.clone(),
            message_id.into(),
        );
        message_ref.thread_id = self.thread_id.clone();
        message_ref
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageRef {
    pub platform: String,
    pub room_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_name: Option<String>,
}

impl MessageRef {
    pub fn new(
        platform: impl Into<String>,
        room_id: impl Into<String>,
        message_id: impl Into<String>,
    ) -> Self {
        Self {
            platform: platform.into(),
            room_id: room_id.into(),
            thread_id: None,
            message_id: message_id.into(),
            sender_id: None,
            sender_name: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageLocator {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

impl MessageLocator {
    pub fn new(room_id: impl Into<String>) -> Self {
        Self {
            platform: None,
            room_id: Some(room_id.into()),
            thread_id: None,
            message_id: None,
        }
    }

    pub fn resolved_room_id<'a>(&'a self, context: &'a ToolContext) -> &'a str {
        self.room_id.as_deref().unwrap_or(context.room_id.as_str())
    }

    pub fn resolved_thread_id(&self, context: &ToolContext) -> Option<String> {
        self.thread_id.clone().or_else(|| context.thread_id.clone())
    }

    pub fn resolved_platform<'a>(&'a self, context: &'a ToolContext) -> &'a str {
        self.platform
            .as_deref()
            .unwrap_or(context.platform.as_str())
    }

    pub fn resolved_message_id<'a>(&'a self, context: &'a ToolContext) -> Option<&'a str> {
        self.message_id
            .as_deref()
            .or(context.placeholder_message_id.as_deref())
    }

    pub fn to_message_ref(
        &self,
        context: &ToolContext,
        message_id: impl Into<String>,
    ) -> MessageRef {
        let mut message_ref = MessageRef::new(
            self.resolved_platform(context).to_string(),
            self.resolved_room_id(context).to_string(),
            message_id,
        );
        message_ref.thread_id = self.resolved_thread_id(context);
        message_ref
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageDraft {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parse_mode: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub disable_web_page_preview: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub silent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<MessageLocator>,
}

impl MessageDraft {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            parse_mode: None,
            disable_web_page_preview: false,
            silent: false,
            reply_to: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ChatOp {
    Send {
        draft: MessageDraft,
    },
    Edit {
        target: MessageLocator,
        draft: MessageDraft,
    },
    Delete {
        target: MessageLocator,
    },
    StartTyping {
        target: MessageLocator,
    },
    StopTyping {
        target: MessageLocator,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatBatchRequest {
    pub context: ToolContext,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operations: Vec<ChatOp>,
    #[serde(default = "default_true", skip_serializing_if = "is_false")]
    pub best_effort: bool,
}

impl ChatBatchRequest {
    pub fn new(context: ToolContext) -> Self {
        Self {
            context,
            operations: Vec::new(),
            best_effort: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatOpStatus {
    Applied,
    Planned,
    BestEffort,
    Rejected,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolIssue {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub retryable: bool,
}

impl ToolIssue {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatOpOutcome {
    pub index: usize,
    pub op: ChatOp,
    pub status: ChatOpStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_ref: Option<MessageRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<ToolIssue>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatBatchResult {
    pub context: ToolContext,
    pub requested_count: usize,
    pub planned_count: usize,
    pub rejected_count: usize,
    #[serde(default)]
    pub partial_failure: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_message: Option<MessageRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_visible_text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outcomes: Vec<ChatOpOutcome>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ToolIssue>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryDirection {
    NewestFirst,
    OldestFirst,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryWindow {
    pub limit: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_seq: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_seq: Option<i64>,
    #[serde(default = "default_newest_first")]
    pub direction: HistoryDirection,
    #[serde(default, skip_serializing_if = "is_false")]
    pub include_summary: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub include_system_notes: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub include_tool_events: bool,
}

impl Default for HistoryWindow {
    fn default() -> Self {
        Self::latest(default_limit())
    }
}

impl HistoryWindow {
    pub fn latest(limit: usize) -> Self {
        Self {
            limit,
            before_seq: None,
            after_seq: None,
            direction: HistoryDirection::NewestFirst,
            include_summary: true,
            include_system_notes: false,
            include_tool_events: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryQueryRequest {
    pub context: ToolContext,
    pub window: HistoryWindow,
}

impl HistoryQueryRequest {
    pub fn new(context: ToolContext, window: HistoryWindow) -> Self {
        Self { context, window }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryItem {
    pub message: MessageRef,
    pub seq: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub kind: String,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    pub body: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<String>,
    #[serde(default)]
    pub visible_to_model: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryQueryResult {
    pub context: ToolContext,
    pub window: HistoryWindow,
    pub item_count: usize,
    #[serde(default)]
    pub truncated: bool,
    #[serde(default)]
    pub complete: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<HistoryItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ToolIssue>,
}

impl HistoryQueryResult {
    pub fn empty(request: HistoryQueryRequest) -> Self {
        Self {
            context: request.context,
            window: request.window,
            item_count: 0,
            truncated: false,
            complete: false,
            items: Vec::new(),
            warnings: vec![ToolIssue::new(
                "history_backend_unavailable",
                "history lookup is not connected yet",
            )],
        }
    }
}

fn stable_message_id(context: &ToolContext, index: usize, kind: &str) -> String {
    format!(
        "{}:{}:{}:{}",
        context.platform, context.thread_key, index, kind
    )
}

fn tracked_message_text(op: &ChatOp) -> Option<String> {
    match op {
        ChatOp::Send { draft } | ChatOp::Edit { draft, .. } => {
            Some(draft.text.clone()).filter(|text| !text.trim().is_empty())
        }
        _ => None,
    }
}

impl ChatBatchResult {
    pub fn planned(request: ChatBatchRequest) -> Self {
        let mut planned_count = 0usize;
        let mut rejected_count = 0usize;
        let mut outcomes = Vec::with_capacity(request.operations.len());
        let mut warnings = Vec::new();
        let mut final_message = None;
        let mut final_visible_text = None;
        let mut last_visible_index = None::<usize>;

        for (index, op) in request.operations.into_iter().enumerate() {
            let outcome = match op {
                ChatOp::Send { draft } => {
                    if draft.text.trim().is_empty() {
                        rejected_count += 1;
                        ChatOpOutcome {
                            index,
                            op: ChatOp::Send { draft },
                            status: ChatOpStatus::Rejected,
                            message_ref: None,
                            issue: Some(ToolIssue::new(
                                "empty_message",
                                "chat draft text must not be empty",
                            )),
                        }
                    } else {
                        planned_count += 1;
                        let message_ref = request.context.message_ref(stable_message_id(
                            &request.context,
                            index,
                            "send",
                        ));
                        final_message = Some(message_ref.clone());
                        final_visible_text = Some(draft.text.clone());
                        last_visible_index = Some(index);
                        ChatOpOutcome {
                            index,
                            op: ChatOp::Send { draft },
                            status: ChatOpStatus::Planned,
                            message_ref: Some(message_ref),
                            issue: None,
                        }
                    }
                }
                ChatOp::Edit { target, draft } => {
                    if draft.text.trim().is_empty() {
                        rejected_count += 1;
                        ChatOpOutcome {
                            index,
                            op: ChatOp::Edit { target, draft },
                            status: ChatOpStatus::Rejected,
                            message_ref: None,
                            issue: Some(ToolIssue::new(
                                "empty_message",
                                "chat draft text must not be empty",
                            )),
                        }
                    } else {
                        planned_count += 1;
                        let message_ref = target.to_message_ref(
                            &request.context,
                            stable_message_id(&request.context, index, "edit"),
                        );
                        final_message = Some(message_ref.clone());
                        final_visible_text = Some(draft.text.clone());
                        last_visible_index = Some(index);
                        ChatOpOutcome {
                            index,
                            op: ChatOp::Edit { target, draft },
                            status: ChatOpStatus::Planned,
                            message_ref: Some(message_ref),
                            issue: None,
                        }
                    }
                }
                ChatOp::Delete { target } => {
                    planned_count += 1;
                    let message_ref = target.to_message_ref(
                        &request.context,
                        stable_message_id(&request.context, index, "delete"),
                    );
                    if final_message
                        .as_ref()
                        .map(|message| message.message_id == message_ref.message_id)
                        .unwrap_or(false)
                    {
                        final_message = None;
                        final_visible_text = None;
                        last_visible_index = None;
                    }
                    ChatOpOutcome {
                        index,
                        op: ChatOp::Delete { target },
                        status: ChatOpStatus::Planned,
                        message_ref: Some(message_ref),
                        issue: None,
                    }
                }
                ChatOp::StartTyping { target } => {
                    planned_count += 1;
                    ChatOpOutcome {
                        index,
                        op: ChatOp::StartTyping { target },
                        status: ChatOpStatus::BestEffort,
                        message_ref: None,
                        issue: None,
                    }
                }
                ChatOp::StopTyping { target } => {
                    planned_count += 1;
                    ChatOpOutcome {
                        index,
                        op: ChatOp::StopTyping { target },
                        status: ChatOpStatus::BestEffort,
                        message_ref: None,
                        issue: None,
                    }
                }
            };

            if matches!(outcome.status, ChatOpStatus::Rejected) {
                warnings.push(ToolIssue::new(
                    "chat_operation_rejected",
                    "one or more chat operations were rejected",
                ));
            }

            if let Some(text) = tracked_message_text(&outcome.op) {
                if last_visible_index == Some(outcome.index) {
                    final_visible_text = Some(text);
                }
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

        ChatBatchResult {
            context: request.context,
            requested_count: outcomes.len(),
            planned_count,
            rejected_count,
            partial_failure,
            final_message,
            final_visible_text,
            outcomes,
            warnings,
        }
    }
}

pub fn tool_context_schema() -> Value {
    object_schema(
        vec![
            (
                "platform",
                string_schema("Platform name, such as telegram."),
            ),
            ("thread_key", string_schema("Stable thread scope key.")),
            (
                "room_id",
                string_schema("Platform room or chat identifier."),
            ),
            (
                "thread_id",
                optional_string_schema("Platform thread or topic identifier."),
            ),
            (
                "turn_id",
                optional_string_schema("Internal turn identifier."),
            ),
            (
                "actor_id",
                optional_string_schema("Actor or user identifier."),
            ),
            ("actor_name", optional_string_schema("Actor display name.")),
            (
                "placeholder_message_id",
                optional_string_schema("Placeholder message to update."),
            ),
            (
                "request_id",
                optional_string_schema("Correlation identifier."),
            ),
        ],
        &["platform", "thread_key", "room_id"],
    )
}

pub fn message_ref_schema() -> Value {
    object_schema(
        vec![
            ("platform", string_schema("Platform name.")),
            ("room_id", string_schema("Room or chat identifier.")),
            (
                "thread_id",
                optional_string_schema("Thread or topic identifier."),
            ),
            ("message_id", string_schema("Message identifier.")),
            ("sender_id", optional_string_schema("Sender identifier.")),
            (
                "sender_name",
                optional_string_schema("Sender display name."),
            ),
        ],
        &["platform", "room_id", "message_id"],
    )
}

pub fn message_locator_schema() -> Value {
    object_schema(
        vec![
            ("platform", optional_string_schema("Platform name.")),
            (
                "room_id",
                optional_string_schema("Room or chat identifier."),
            ),
            (
                "thread_id",
                optional_string_schema("Thread or topic identifier."),
            ),
            ("message_id", optional_string_schema("Message identifier.")),
        ],
        &[],
    )
}

pub fn message_draft_schema() -> Value {
    object_schema(
        vec![
            ("text", string_schema("Message text.")),
            (
                "parse_mode",
                optional_string_schema("Formatting mode, if any."),
            ),
            (
                "disable_web_page_preview",
                json!({
                    "type": "boolean",
                    "description": "Disable web page previews.",
                }),
            ),
            (
                "silent",
                json!({
                    "type": "boolean",
                    "description": "Send without notification if supported.",
                }),
            ),
            ("reply_to", message_locator_schema()),
        ],
        &["text"],
    )
}

fn chat_op_schema() -> Value {
    json!({
        "oneOf": [
            {
                "type": "object",
                "properties": {
                    "op": { "const": "send" },
                    "draft": message_draft_schema(),
                },
                "required": ["op", "draft"],
                "additionalProperties": false,
            },
            {
                "type": "object",
                "properties": {
                    "op": { "const": "edit" },
                    "target": message_locator_schema(),
                    "draft": message_draft_schema(),
                },
                "required": ["op", "target", "draft"],
                "additionalProperties": false,
            },
            {
                "type": "object",
                "properties": {
                    "op": { "const": "delete" },
                    "target": message_locator_schema(),
                },
                "required": ["op", "target"],
                "additionalProperties": false,
            },
            {
                "type": "object",
                "properties": {
                    "op": { "const": "start_typing" },
                    "target": message_locator_schema(),
                },
                "required": ["op", "target"],
                "additionalProperties": false,
            },
            {
                "type": "object",
                "properties": {
                    "op": { "const": "stop_typing" },
                    "target": message_locator_schema(),
                },
                "required": ["op", "target"],
                "additionalProperties": false,
            }
        ]
    })
}

pub fn chat_batch_request_schema() -> Value {
    object_schema(
        vec![
            ("context", tool_context_schema()),
            (
                "operations",
                json!({
                    "type": "array",
                    "items": chat_op_schema(),
                }),
            ),
            (
                "best_effort",
                json!({
                    "type": "boolean",
                    "description": "Allow non-critical operations to degrade instead of failing the whole batch.",
                    "default": true,
                }),
            ),
        ],
        &["context", "operations"],
    )
}

fn history_window_schema() -> Value {
    object_schema(
        vec![
            (
                "limit",
                json!({
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 200,
                    "description": "Maximum number of history items to return.",
                }),
            ),
            (
                "before_seq",
                json!({
                    "type": "integer",
                    "description": "Return items before this sequence number.",
                }),
            ),
            (
                "after_seq",
                json!({
                    "type": "integer",
                    "description": "Return items after this sequence number.",
                }),
            ),
            (
                "direction",
                json!({
                    "type": "string",
                    "enum": ["newest_first", "oldest_first"],
                }),
            ),
            (
                "include_summary",
                json!({
                    "type": "boolean",
                    "description": "Include summary events in the result.",
                    "default": true,
                }),
            ),
            (
                "include_system_notes",
                json!({
                    "type": "boolean",
                    "description": "Include system note events in the result.",
                    "default": false,
                }),
            ),
            (
                "include_tool_events",
                json!({
                    "type": "boolean",
                    "description": "Include tool call and tool result events in the result.",
                    "default": true,
                }),
            ),
        ],
        &["limit"],
    )
}

pub fn history_query_request_schema() -> Value {
    object_schema(
        vec![
            ("context", tool_context_schema()),
            ("window", history_window_schema()),
        ],
        &["context", "window"],
    )
}

pub fn history_item_schema() -> Value {
    object_schema(
        vec![
            ("message", message_ref_schema()),
            (
                "seq",
                json!({
                    "type": "integer",
                    "description": "Monotonic sequence number inside the thread.",
                }),
            ),
            (
                "turn_id",
                optional_string_schema("Turn identifier, if known."),
            ),
            ("kind", string_schema("Event kind.")),
            ("role", string_schema("Conversation role.")),
            ("speaker", optional_string_schema("Speaker label.")),
            ("body", string_schema("Rendered body text.")),
            (
                "attachments",
                json!({
                    "type": "array",
                    "items": { "type": "string" },
                }),
            ),
            (
                "visible_to_model",
                json!({
                    "type": "boolean",
                    "description": "Whether the item should be surfaced to the model.",
                }),
            ),
        ],
        &["message", "seq", "kind", "role", "body"],
    )
}

pub fn history_query_result_schema() -> Value {
    object_schema(
        vec![
            ("context", tool_context_schema()),
            ("window", history_window_schema()),
            (
                "item_count",
                json!({
                    "type": "integer",
                    "minimum": 0,
                }),
            ),
            (
                "truncated",
                json!({
                    "type": "boolean",
                }),
            ),
            (
                "complete",
                json!({
                    "type": "boolean",
                }),
            ),
            (
                "items",
                json!({
                    "type": "array",
                    "items": history_item_schema(),
                }),
            ),
            (
                "warnings",
                json!({
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "code": { "type": "string" },
                            "message": { "type": "string" },
                            "retryable": { "type": "boolean" },
                        },
                        "required": ["code", "message", "retryable"],
                        "additionalProperties": false,
                    },
                }),
            ),
        ],
        &["context", "window", "item_count", "truncated", "complete"],
    )
}
