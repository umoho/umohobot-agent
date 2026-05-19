use std::sync::{Arc, RwLock};

use crate::platforms::{PlatformKind, ReplyHandle};
use teloxide::payloads::{SendChatActionSetters, SendMessageSetters};
use teloxide::prelude::Requester;
use teloxide::types::{ChatAction, ChatId, MessageId, ThreadId};
use thiserror::Error;
use tracing::{info, warn};

#[derive(Clone, Debug, Default)]
pub struct TelegramOutbox {
    bot: Arc<RwLock<Option<teloxide::Bot>>>,
}

#[derive(Debug, Error)]
pub enum TelegramOutboxError {
    #[error("telegram bot is not bound yet")]
    Unbound,
}

impl TelegramOutbox {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn bind_bot(&self, bot: teloxide::Bot) {
        let mut guard = self
            .bot
            .write()
            .expect("telegram outbox lock should not be poisoned");
        *guard = Some(bot);
    }

    fn bot(&self) -> Result<teloxide::Bot, teloxide::RequestError> {
        self.bot
            .read()
            .expect("telegram outbox lock should not be poisoned")
            .as_ref()
            .cloned()
            .ok_or_else(|| {
                let error = std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "telegram bot is not bound yet",
                );
                teloxide::RequestError::Io(Arc::new(error))
            })
    }

    pub fn reply_handle(&self, room_id: String, message_id: String) -> ReplyHandle {
        ReplyHandle {
            platform: PlatformKind::Telegram,
            room_id,
            message_id,
        }
    }

    pub async fn send_placeholder(
        &self,
        chat_id: ChatId,
        thread_id: Option<&str>,
        text: &str,
    ) -> Result<ReplyHandle, teloxide::RequestError> {
        self.send_text(chat_id, thread_id, text).await
    }

    pub async fn send_text(
        &self,
        chat_id: ChatId,
        thread_id: Option<&str>,
        text: &str,
    ) -> Result<ReplyHandle, teloxide::RequestError> {
        let bot = self.bot()?;
        let mut request = bot.send_message(chat_id, text.to_string());
        if let Some(thread_id) = parse_thread_id(thread_id) {
            request = request.message_thread_id(thread_id);
        }

        match request.await {
            Ok(message) => {
                info!(
                    chat_id = %chat_id,
                    message_id = %message.id,
                    "telegram message sent"
                );
                Ok(self.reply_handle(chat_id.to_string(), message.id.to_string()))
            }
            Err(err) => {
                warn!(
                    error = %err,
                    chat_id = %chat_id,
                    "failed to send telegram message"
                );
                Err(err)
            }
        }
    }

    pub async fn edit_text(
        &self,
        chat_id: ChatId,
        message_id: &str,
        text: &str,
    ) -> Result<(), teloxide::RequestError> {
        let Some(message_id) = parse_message_id(message_id) else {
            return Err(invalid_message_id_error(message_id));
        };

        let bot = self.bot()?;
        bot.edit_message_text(chat_id, message_id, text.to_string())
            .await
            .map(|_| ())
    }

    pub async fn delete_message(
        &self,
        chat_id: ChatId,
        message_id: &str,
    ) -> Result<(), teloxide::RequestError> {
        let Some(message_id) = parse_message_id(message_id) else {
            return Err(invalid_message_id_error(message_id));
        };

        let bot = self.bot()?;
        bot.delete_message(chat_id, message_id).await.map(|_| ())
    }

    pub async fn send_typing(
        &self,
        chat_id: ChatId,
        thread_id: Option<&str>,
    ) -> Result<(), teloxide::RequestError> {
        let bot = self.bot()?;
        let mut request = bot.send_chat_action(chat_id, ChatAction::Typing);
        if let Some(thread_id) = parse_thread_id(thread_id) {
            request = request.message_thread_id(thread_id);
        }

        request.await.map(|_| ())
    }
}

fn parse_message_id(value: &str) -> Option<MessageId> {
    value.parse::<i32>().ok().map(MessageId)
}

pub(crate) fn parse_thread_id(value: Option<&str>) -> Option<ThreadId> {
    value.and_then(|value| match value.parse::<i32>() {
        Ok(id) => Some(ThreadId(MessageId(id))),
        Err(err) => {
            warn!(thread_id = %value, error = %err, "failed to parse telegram thread id");
            None
        }
    })
}

fn invalid_message_id_error(value: &str) -> teloxide::RequestError {
    let error = std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!("invalid telegram message id: {value}"),
    );
    teloxide::RequestError::Io(Arc::new(error))
}
