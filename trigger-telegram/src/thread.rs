use std::{
    collections::HashMap,
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use chrono::{DateTime, Duration, Utc};
use contracts::PlatformMessage;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThreadKey(String);

impl ThreadKey {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThreadScope {
    thread_key: ThreadKey,
}

impl ThreadScope {
    #[must_use]
    pub fn new(thread_key: impl Into<String>) -> Self {
        Self {
            thread_key: ThreadKey::new(thread_key),
        }
    }

    #[must_use]
    pub fn thread_key(&self) -> &ThreadKey {
        &self.thread_key
    }
}

impl fmt::Display for ThreadScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.thread_key.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TelegramConversationKey {
    room_id: String,
    thread_id: Option<String>,
}

impl TelegramConversationKey {
    fn from_message(message: &PlatformMessage) -> Self {
        Self {
            room_id: message.room_id.clone(),
            thread_id: message.thread_id.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TelegramThreadRouter {
    idle_timeout: Duration,
    state: Arc<Mutex<TelegramThreadState>>,
}

#[derive(Clone, Debug)]
pub(crate) struct ThreadSelection {
    pub thread_scope: ThreadScope,
    pub was_new_thread: bool,
}

#[derive(Debug)]
struct TelegramThreadState {
    process_nonce: String,
    next_sequence: u64,
    active_threads: HashMap<TelegramConversationKey, ActiveThread>,
}

#[derive(Clone, Debug)]
struct ActiveThread {
    thread_key: ThreadKey,
    last_activity_at: DateTime<Utc>,
}

static ROUTER_INSTANCE_COUNTER: AtomicU64 = AtomicU64::new(1);

impl TelegramThreadRouter {
    pub(crate) fn new(idle_timeout: Duration) -> Self {
        Self {
            idle_timeout,
            state: Arc::new(Mutex::new(TelegramThreadState::new())),
        }
    }

    pub(crate) fn select(
        &self,
        message: &PlatformMessage,
        observed_at: DateTime<Utc>,
    ) -> ThreadSelection {
        let conversation = TelegramConversationKey::from_message(message);
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());

        state.select(conversation, observed_at, self.idle_timeout)
    }
}

impl TelegramThreadState {
    fn new() -> Self {
        let process_id = std::process::id();
        let started_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let instance = ROUTER_INSTANCE_COUNTER.fetch_add(1, Ordering::Relaxed);

        Self {
            process_nonce: format!("{process_id:x}-{started_at:x}-{instance:x}"),
            next_sequence: 0,
            active_threads: HashMap::new(),
        }
    }

    fn select(
        &mut self,
        conversation: TelegramConversationKey,
        observed_at: DateTime<Utc>,
        idle_timeout: Duration,
    ) -> ThreadSelection {
        if let Some(active) = self.active_threads.get_mut(&conversation) {
            let idle = observed_at.signed_duration_since(active.last_activity_at);
            if idle <= idle_timeout {
                if observed_at > active.last_activity_at {
                    active.last_activity_at = observed_at;
                }

                return ThreadSelection {
                    thread_scope: ThreadScope::new(active.thread_key.as_str()),
                    was_new_thread: false,
                };
            }
        }

        let thread_key = self.mint_thread_key();
        self.active_threads.insert(
            conversation,
            ActiveThread {
                thread_key: thread_key.clone(),
                last_activity_at: observed_at,
            },
        );

        ThreadSelection {
            thread_scope: ThreadScope::new(thread_key.as_str()),
            was_new_thread: true,
        }
    }

    fn mint_thread_key(&mut self) -> ThreadKey {
        self.next_sequence += 1;
        ThreadKey::new(format!(
            "thread-telegram-{}-{}",
            self.process_nonce, self.next_sequence
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use contracts::{
        MessageBody, PlatformKind, PlatformMessage, PlatformMessageKind, ReplyMetadata,
    };

    fn test_message(room_id: &str, thread_id: Option<&str>) -> PlatformMessage {
        PlatformMessage {
            platform: PlatformKind::Telegram,
            room_id: room_id.to_string(),
            thread_id: thread_id.map(ToString::to_string),
            message_id: "msg-1".to_string(),
            sender_id: "user-1".to_string(),
            kind: PlatformMessageKind::Text,
            body: MessageBody::Text {
                text: "hello".to_string(),
                entities: vec![],
            },
            attachments: vec![],
            reply: Some(ReplyMetadata {
                message_id: "reply-1".to_string(),
                sender_id: "user-2".to_string(),
                kind: PlatformMessageKind::Text,
                body: MessageBody::Text {
                    text: "reply".to_string(),
                    entities: vec![],
                },
                attachments: vec![],
            }),
            is_mention: false,
        }
    }

    #[test]
    fn thread_scope_wraps_opaque_thread_keys() {
        let scope = ThreadScope::new("thread-1");

        assert_eq!(scope.thread_key().as_str(), "thread-1");
        assert_eq!(scope.to_string(), "thread-1");
    }

    #[test]
    fn router_reuses_thread_before_timeout_and_mints_after_timeout() {
        let router = TelegramThreadRouter::new(Duration::seconds(30));
        let message = test_message("room-1", Some("topic-1"));
        let t0 = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();

        let first = router.select(&message, t0);
        let second = router.select(&message, t0 + Duration::seconds(10));
        let third = router.select(&message, t0 + Duration::seconds(41));

        assert!(first.was_new_thread);
        assert!(!second.was_new_thread);
        assert!(third.was_new_thread);
        assert_eq!(first.thread_scope, second.thread_scope);
        assert_ne!(first.thread_scope, third.thread_scope);
    }

    #[test]
    fn router_separates_distinct_conversations() {
        let router = TelegramThreadRouter::new(Duration::seconds(30));
        let t0 = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();

        let topic_a = test_message("room-1", Some("topic-a"));
        let topic_b = test_message("room-1", Some("topic-b"));

        let first = router.select(&topic_a, t0);
        let second = router.select(&topic_b, t0 + Duration::seconds(1));

        assert!(first.was_new_thread);
        assert!(second.was_new_thread);
        assert_ne!(first.thread_scope, second.thread_scope);
    }
}
