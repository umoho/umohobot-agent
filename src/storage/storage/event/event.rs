use chrono::{DateTime, Utc};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    InboundMessage,
    AssistantMessage,
    ToolCall,
    ToolResult,
    Summary,
    SystemNote,
}

impl EventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InboundMessage => "inbound_message",
            Self::AssistantMessage => "assistant_message",
            Self::ToolCall => "tool_call",
            Self::ToolResult => "tool_result",
            Self::Summary => "summary",
            Self::SystemNote => "system_note",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "inbound_message" => Some(Self::InboundMessage),
            "assistant_message" => Some(Self::AssistantMessage),
            "tool_call" => Some(Self::ToolCall),
            "tool_result" => Some(Self::ToolResult),
            "summary" => Some(Self::Summary),
            "system_note" => Some(Self::SystemNote),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct NewEvent {
    pub thread_id: i64,
    pub turn_id: Option<i64>,
    pub kind: EventKind,
    pub sender_id: Option<String>,
    pub sender_name: Option<String>,
    pub platform_message_id: Option<String>,
    pub reply_to_platform_message_id: Option<String>,
    pub content: Value,
    pub visible_to_model: bool,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug)]
pub struct EventRecord {
    pub id: i64,
    pub thread_id: i64,
    pub seq: i64,
    pub turn_id: Option<i64>,
    pub kind: EventKind,
    pub sender_id: Option<String>,
    pub sender_name: Option<String>,
    pub platform_message_id: Option<String>,
    pub reply_to_platform_message_id: Option<String>,
    pub content: Value,
    pub visible_to_model: bool,
    pub created_at: DateTime<Utc>,
}
