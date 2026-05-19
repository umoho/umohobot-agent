mod normalize;
mod thread;

pub use normalize::{TelegramInboundMessage, TelegramNormalizedMessage};
pub use thread::{ThreadKey, ThreadScope};

use chrono::{DateTime, Duration, Utc};
use teloxide::types::Message;

#[derive(Clone, Debug)]
pub struct TelegramTrigger {
    bot_name: String,
    thread_router: thread::TelegramThreadRouter,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TelegramTriggerResult {
    pub normalized: TelegramNormalizedMessage,
    pub thread_scope: ThreadScope,
    pub was_new_thread: bool,
}

impl TelegramTrigger {
    pub fn new(bot_name: impl Into<String>, idle_timeout: Duration) -> Self {
        Self {
            bot_name: bot_name.into(),
            thread_router: thread::TelegramThreadRouter::new(idle_timeout),
        }
    }

    #[must_use]
    pub fn normalize(&self, message: &Message) -> TelegramNormalizedMessage {
        TelegramNormalizedMessage::from_message(message, &self.bot_name)
    }

    #[must_use]
    pub fn trigger(&self, message: &Message) -> TelegramTriggerResult {
        self.trigger_at(message, message.date)
    }

    #[must_use]
    pub fn trigger_at(
        &self,
        message: &Message,
        observed_at: DateTime<Utc>,
    ) -> TelegramTriggerResult {
        let normalized = self.normalize(message);
        let selection = self
            .thread_router
            .select(&normalized.platform_message, observed_at);

        TelegramTriggerResult {
            normalized,
            thread_scope: selection.thread_scope,
            was_new_thread: selection.was_new_thread,
        }
    }
}
