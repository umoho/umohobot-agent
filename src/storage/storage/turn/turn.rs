use chrono::{DateTime, Utc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl TurnStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "running" => Some(Self::Running),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TurnStart {
    pub thread_id: i64,
    pub trigger_event_id: i64,
    pub provider: String,
    pub model: String,
    pub prompt_version: i64,
    pub context_hash: Option<String>,
    pub placeholder_message_id: Option<String>,
    pub lease_until: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug)]
pub struct TurnFinish {
    pub turn_id: i64,
    pub status: TurnStatus,
    pub final_message_id: Option<String>,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub tool_calls: u64,
    pub estimated_usage: bool,
    pub error_code: Option<String>,
    pub ended_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug)]
pub struct TurnRecord {
    pub id: i64,
    pub thread_id: i64,
    pub trigger_event_id: i64,
    pub status: TurnStatus,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub provider: String,
    pub model: String,
    pub prompt_version: i64,
    pub context_hash: Option<String>,
    pub placeholder_message_id: Option<String>,
    pub final_message_id: Option<String>,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub tool_calls: u64,
    pub estimated_usage: bool,
    pub error_code: Option<String>,
    pub lease_until: Option<DateTime<Utc>>,
}
