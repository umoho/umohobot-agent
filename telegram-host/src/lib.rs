mod cache;

pub use cache::MessageCache;

use teloxide::Bot;
use teloxide::prelude::Requester;
use teloxide::types::{ChatId, FileId, Message, MessageId};
use tracing::debug;

#[derive(Debug, thiserror::Error)]
pub enum TelegramError {
    #[error("Telegram API error: {0}")]
    Api(#[from] teloxide::RequestError),
}

#[derive(Clone)]
pub struct TelegramHost {
    bot: Bot,
    token: String,
}

impl TelegramHost {
    pub fn new(token: impl Into<String>) -> Self {
        let token = token.into();
        let bot = Bot::new(&token);
        Self { bot, token }
    }

    pub fn from_env() -> Self {
        let bot = Bot::from_env();
        let token = bot.token().to_owned();
        Self { bot, token }
    }

    pub fn bot(&self) -> &Bot {
        &self.bot
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub async fn get_file_url(&self, file_id: &FileId) -> Result<String, TelegramError> {
        let file = self.bot.get_file(file_id.clone()).await?;
        Ok(format!(
            "{}file/bot{}/{}",
            self.bot.api_url(),
            self.token,
            file.path,
        ))
    }

    pub async fn send_message(
        &self,
        chat_id: ChatId,
        text: &str,
    ) -> Result<Message, TelegramError> {
        debug!(%chat_id, text_len = text.len(), "sending telegram message");
        Ok(self.bot.send_message(chat_id, text).await?)
    }

    pub async fn edit_message(
        &self,
        chat_id: ChatId,
        message_id: i32,
        text: &str,
    ) -> Result<Message, TelegramError> {
        debug!(%chat_id, %message_id, text_len = text.len(), "editing telegram message");
        Ok(self
            .bot
            .edit_message_text(chat_id, MessageId(message_id), text)
            .await?)
    }

    pub async fn delete_message(
        &self,
        chat_id: ChatId,
        message_id: i32,
    ) -> Result<(), TelegramError> {
        debug!(%chat_id, %message_id, "deleting telegram message");
        self.bot
            .delete_message(chat_id, MessageId(message_id))
            .await?;
        Ok(())
    }
}
