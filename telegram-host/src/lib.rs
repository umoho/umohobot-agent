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
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
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

    pub async fn download_file_base64(&self, file_id: &FileId) -> Result<String, TelegramError> {
        let file = self.bot.get_file(file_id.clone()).await?;
        let url = format!("{}file/bot{}/{}", self.bot.api_url(), self.token, file.path);
        let bytes = self.bot.client().get(&url).send().await?.bytes().await?;
        use base64::Engine as _;
        Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
    }

    pub async fn download_file_bytes(
        &self,
        file_id: &FileId,
    ) -> Result<(Vec<u8>, String), TelegramError> {
        let file = self.bot.get_file(file_id.clone()).await?;
        let url = format!("{}file/bot{}/{}", self.bot.api_url(), self.token, file.path);
        let bytes = self.bot.client().get(&url).send().await?.bytes().await?;
        Ok((bytes.to_vec(), file.unique_id.to_string()))
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
