use std::fmt;

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::event::EventRecord;

pub mod history;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ThreadKey(String);

impl ThreadKey {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for ThreadKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<String> for ThreadKey {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for ThreadKey {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadScope {
    thread_key: ThreadKey,
}

impl ThreadScope {
    pub fn new(thread_key: impl Into<String>) -> Self {
        Self {
            thread_key: ThreadKey::new(thread_key),
        }
    }

    pub fn thread_key(&self) -> &ThreadKey {
        &self.thread_key
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadState {
    Active,
    Draining,
    Closed,
}

impl ThreadState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Draining => "draining",
            Self::Closed => "closed",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "draining" => Some(Self::Draining),
            "closed" => Some(Self::Closed),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ThreadRecord {
    pub id: i64,
    pub thread_key: ThreadKey,
    pub state: ThreadState,
    pub opened_at: DateTime<Utc>,
    pub last_activity_at: DateTime<Utc>,
    pub lease_until: Option<DateTime<Utc>>,
    pub closed_at: Option<DateTime<Utc>>,
    pub parent_thread_id: Option<i64>,
    pub summary_cursor: i64,
    pub turn_count: i64,
    pub version: i64,
}

impl ThreadRecord {
    pub fn thread_key(&self) -> &ThreadKey {
        &self.thread_key
    }
}

#[derive(Clone, Debug)]
pub struct InboundMessageRecord {
    pub scope: ThreadScope,
    pub parent_thread_id: Option<i64>,
    pub platform_message_id: String,
    pub sender_id: String,
    pub sender_name: Option<String>,
    pub reply_to_platform_message_id: Option<String>,
    pub content: Value,
    pub visible_to_model: bool,
    pub lease_until: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug)]
pub struct MessageObservation {
    pub thread: ThreadRecord,
    pub event: EventRecord,
    pub was_new_thread: bool,
}

pub use history::ThreadHistorySliceRecord;

pub mod thread {
    pub use super::{
        InboundMessageRecord, MessageObservation, ThreadHistorySliceRecord, ThreadKey,
        ThreadRecord, ThreadScope, ThreadState,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_scope_wraps_opaque_thread_keys() {
        let scope = ThreadScope::new("thread-1");

        assert_eq!(scope.thread_key().as_str(), "thread-1");
    }
}
