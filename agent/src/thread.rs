use chrono::{DateTime, Duration, Utc};
use rig_core::message::{Message, ToolCall, ToolResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub messages: Vec<Message>,
}

impl Thread {
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            created_at: now,
            last_activity: now,
            messages: vec![],
        }
    }

    pub fn append_user(&mut self, content: impl Into<String>) {
        self.messages.push(Message::user(content));
        self.last_activity = Utc::now();
    }

    pub fn append_assistant(&mut self, content: impl Into<String>) {
        self.messages.push(Message::assistant(content));
        self.last_activity = Utc::now();
    }

    pub fn append_tool_call(&mut self, call: ToolCall) {
        self.messages.push(Message::from(call));
        self.last_activity = Utc::now();
    }

    pub fn append_tool_result(&mut self, result: ToolResult) {
        self.messages.push(Message::from(result));
        self.last_activity = Utc::now();
    }

    pub fn is_expired(&self, idle_timeout: Duration) -> bool {
        Utc::now() - self.last_activity > idle_timeout
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}
