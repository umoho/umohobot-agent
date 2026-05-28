mod download;
mod edit;
mod query;
mod send;

pub use download::*;
pub use edit::*;
pub use query::*;
pub use send::*;

use std::sync::Arc;

use agent::AgentHandle;

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Telegram error: {0}")]
    Telegram(#[from] telegram_host::TelegramError),
    #[error("Telegram request error: {0}")]
    Request(#[from] teloxide::RequestError),
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    #[error("Either fileId or url must be provided")]
    MissingFileSource,
    #[error("JSON parse error: {0}")]
    JsonParse(#[from] serde_json::Error),
    #[error("Invalid dice emoji: {0}")]
    InvalidDiceEmoji(String),
    #[error("Query tools require an active chat context")]
    NotInAgentContext,
    #[error("Chat not found for the current thread")]
    ChatNotFound,
}

use teloxide::types::{FileId, InputFile};
pub(crate) fn build_input_file(
    file_id: Option<String>,
    url: Option<String>,
) -> Result<InputFile, ToolError> {
    match (file_id, url) {
        (Some(fid), _) => Ok(InputFile::file_id(FileId(fid))),
        (_, Some(u)) => {
            let parsed = url::Url::parse(&u).map_err(|e| ToolError::InvalidUrl(e.to_string()))?;
            Ok(InputFile::url(parsed))
        }
        (None, None) => Err(ToolError::MissingFileSource),
    }
}

use teloxide::types::DiceEmoji;
pub(crate) fn parse_dice_emoji(s: &str) -> Option<DiceEmoji> {
    match s.to_lowercase().as_str() {
        "dice" | "🎲" => Some(DiceEmoji::Dice),
        "darts" | "🎯" => Some(DiceEmoji::Darts),
        "bowling" | "🎳" => Some(DiceEmoji::Bowling),
        "basketball" | "🏀" => Some(DiceEmoji::Basketball),
        "football" | "⚽" => Some(DiceEmoji::Football),
        "slot_machine" | "slotmachine" | "🎰" => Some(DiceEmoji::SlotMachine),
        _ => None,
    }
}

use agent::{AgentError, AgentRuntime};
use telegram_host::{MessageCache, TelegramHost, ThreadChatMap, UpdateStore};
pub async fn register_telegram_tools(
    runtime: &AgentRuntime,
    host: TelegramHost,
    cache: MessageCache,
    updates: UpdateStore,
    thread_chat_map: ThreadChatMap,
    agent: Arc<dyn AgentHandle>,
) -> Result<(), AgentError> {
    send::register_send_tools(runtime, host.clone()).await?;
    edit::register_edit_tools(runtime, host.clone()).await?;
    query::register_query_tools(runtime, host, cache, updates, thread_chat_map, agent).await?;
    Ok(())
}
