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

impl ChatBatchResult {
    pub fn planned(request: ChatBatchRequest) -> Self {
        let mut planned_count = 0usize;
        let mut rejected_count = 0usize;
        let mut outcomes = Vec::with_capacity(request.operations.len());
        let mut message_states: Vec<TrackedMessage> = Vec::new();

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
                                "message text must not be empty",
                            )),
                        }
                    } else {
                        planned_count += 1;
                        let message_ref = request.context.message_ref(stable_message_id(
                            &request.context,
                            index,
                            "send",
                        ));
                        message_states.push(TrackedMessage {
                            order: index,
                            message_ref: message_ref.clone(),
                            text: Some(draft.text.clone()),
                            deleted: false,
                        });
                        ChatOpOutcome {
                            index,
                            op: ChatOp::Send { draft },
                            status: ChatOpStatus::Applied,
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
                                "edited text must not be empty",
                            )),
                        }
                    } else {
                        let message_ref =
                            match resolve_target_message_ref(&request.context, &target) {
                                Some(message_ref) => message_ref,
                                None => {
                                    rejected_count += 1;
                                    outcomes.push(ChatOpOutcome {
                                        index,
                                        op: ChatOp::Edit { target, draft },
                                        status: ChatOpStatus::Rejected,
                                        message_ref: None,
                                        issue: Some(ToolIssue::new(
                                            "missing_target",
                                            "edit targets must resolve to a message_id",
                                        )),
                                    });
                                    continue;
                                }
                            };

                        planned_count += 1;
                        upsert_message_state(
                            &mut message_states,
                            index,
                            message_ref.clone(),
                            Some(draft.text.clone()),
                            false,
                        );
                        ChatOpOutcome {
                            index,
                            op: ChatOp::Edit { target, draft },
                            status: ChatOpStatus::Applied,
                            message_ref: Some(message_ref),
                            issue: None,
                        }
                    }
                }
                ChatOp::Delete { target } => {
                    let message_ref = match resolve_target_message_ref(&request.context, &target) {
                        Some(message_ref) => message_ref,
                        None => {
                            rejected_count += 1;
                            outcomes.push(ChatOpOutcome {
                                index,
                                op: ChatOp::Delete { target },
                                status: ChatOpStatus::Rejected,
                                message_ref: None,
                                issue: Some(ToolIssue::new(
                                    "missing_target",
                                    "delete targets must resolve to a message_id",
                                )),
                            });
                            continue;
                        }
                    };

                    planned_count += 1;
                    if let Some(state) = message_states
                        .iter_mut()
                        .find(|state| state.message_ref.message_id == message_ref.message_id)
                    {
                        state.deleted = true;
                    }
                    ChatOpOutcome {
                        index,
                        op: ChatOp::Delete { target },
                        status: ChatOpStatus::Applied,
                        message_ref: Some(message_ref),
                        issue: None,
                    }
                }
                ChatOp::StartTyping { target } => {
                    if target.resolved_room_id(&request.context).is_empty() {
                        rejected_count += 1;
                        ChatOpOutcome {
                            index,
                            op: ChatOp::StartTyping { target },
                            status: ChatOpStatus::Rejected,
                            message_ref: None,
                            issue: Some(ToolIssue::new(
                                "missing_target",
                                "typing indicators need at least a room_id",
                            )),
                        }
                    } else {
                        planned_count += 1;
                        ChatOpOutcome {
                            index,
                            op: ChatOp::StartTyping { target },
                            status: ChatOpStatus::BestEffort,
                            message_ref: None,
                            issue: None,
                        }
                    }
                }
                ChatOp::StopTyping { target } => {
                    if target.resolved_room_id(&request.context).is_empty() {
                        if request.best_effort {
                            planned_count += 1;
                            ChatOpOutcome {
                                index,
                                op: ChatOp::StopTyping { target },
                                status: ChatOpStatus::BestEffort,
                                message_ref: None,
                                issue: Some(ToolIssue::new(
                                    "best_effort_stop_typing",
                                    "typing stop cannot be fully resolved without a room_id",
                                )),
                            }
                        } else {
                            rejected_count += 1;
                            ChatOpOutcome {
                                index,
                                op: ChatOp::StopTyping { target },
                                status: ChatOpStatus::Rejected,
                                message_ref: None,
                                issue: Some(ToolIssue::new(
                                    "missing_target",
                                    "typing stop targets must include a room_id",
                                )),
                            }
                        }
                    } else {
                        planned_count += 1;
                        ChatOpOutcome {
                            index,
                            op: ChatOp::StopTyping { target },
                            status: ChatOpStatus::BestEffort,
                            message_ref: None,
                            issue: None,
                        }
                    }
                }
            };

            outcomes.push(outcome);
        }

        let partial_failure = rejected_count > 0;
        let mut warnings = Vec::new();
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

        Self {
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

#[derive(Clone, Debug)]
struct TrackedMessage {
    order: usize,
    message_ref: MessageRef,
    text: Option<String>,
    deleted: bool,
}

fn stable_message_id(context: &ToolContext, index: usize, kind: &str) -> String {
    if let Some(request_id) = context.request_id.as_deref() {
        format!("{request_id}:{kind}:{index}")
    } else if let Some(turn_id) = context.turn_id.as_deref() {
        format!("{turn_id}:{kind}:{index}")
    } else {
        format!("{}:{kind}:{index}", context.thread_key)
    }
}

fn resolve_target_message_ref(
    context: &ToolContext,
    target: &MessageLocator,
) -> Option<MessageRef> {
    target
        .resolved_message_id(context)
        .map(|message_id| target.to_message_ref(context, message_id.to_string()))
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HistoryDirection {
    #[default]
    NewestFirst,
    OldestFirst,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryWindow {
    #[serde(default = "default_limit")]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_batch_request_round_trips() {
        let request = ChatBatchRequest {
            context: ToolContext {
                platform: "telegram".to_string(),
                thread_key: "telegram:1".to_string(),
                room_id: "1".to_string(),
                thread_id: Some("topic-1".to_string()),
                turn_id: Some("turn-7".to_string()),
                actor_id: Some("user-1".to_string()),
                actor_name: Some("Alex".to_string()),
                placeholder_message_id: Some("msg-9".to_string()),
                request_id: Some("req-1".to_string()),
            },
            operations: vec![
                ChatOp::Send {
                    draft: MessageDraft {
                        text: "hello".to_string(),
                        parse_mode: Some("Markdown".to_string()),
                        disable_web_page_preview: true,
                        silent: false,
                        reply_to: Some(MessageLocator {
                            platform: Some("telegram".to_string()),
                            room_id: Some("1".to_string()),
                            thread_id: Some("topic-1".to_string()),
                            message_id: Some("msg-1".to_string()),
                        }),
                    },
                },
                ChatOp::StopTyping {
                    target: MessageLocator::new("1"),
                },
            ],
            best_effort: true,
        };

        let encoded = serde_json::to_string(&request).unwrap();
        let decoded: ChatBatchRequest = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, request);
    }

    #[test]
    fn history_query_result_round_trips() {
        let result = HistoryQueryResult {
            context: ToolContext::new("telegram", "telegram:1", "1"),
            window: HistoryWindow {
                limit: 3,
                before_seq: Some(10),
                after_seq: None,
                direction: HistoryDirection::NewestFirst,
                include_summary: true,
                include_system_notes: false,
                include_tool_events: true,
            },
            item_count: 1,
            truncated: false,
            complete: true,
            items: vec![HistoryItem {
                message: MessageRef::new("telegram", "1", "msg-1"),
                seq: 42,
                turn_id: Some("turn-9".to_string()),
                kind: "inbound_message".to_string(),
                role: "user".to_string(),
                speaker: Some("Alex".to_string()),
                body: "hello".to_string(),
                attachments: vec!["image".to_string()],
                visible_to_model: true,
            }],
            warnings: Vec::new(),
        };

        let encoded = serde_json::to_string(&result).unwrap();
        let decoded: HistoryQueryResult = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, result);
    }

    #[test]
    fn chat_batch_result_round_trips_with_message_refs() {
        let result = ChatBatchResult {
            context: ToolContext::new("telegram", "telegram:1", "1"),
            requested_count: 2,
            planned_count: 2,
            rejected_count: 0,
            partial_failure: false,
            final_message: Some(MessageRef::new("telegram", "1", "msg-2")),
            final_visible_text: Some("hello".to_string()),
            outcomes: vec![ChatOpOutcome {
                index: 0,
                op: ChatOp::Send {
                    draft: MessageDraft::new("hello"),
                },
                status: ChatOpStatus::Applied,
                message_ref: Some(MessageRef::new("telegram", "1", "msg-2")),
                issue: None,
            }],
            warnings: Vec::new(),
        };

        let encoded = serde_json::to_string(&result).unwrap();
        let decoded: ChatBatchResult = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, result);
    }

    #[test]
    fn schemas_include_required_fields() {
        let schema = chat_batch_request_schema();
        let text = schema.to_string();

        assert!(text.contains("\"context\""));
        assert!(text.contains("\"operations\""));
        assert!(text.contains("\"best_effort\""));
    }
}
