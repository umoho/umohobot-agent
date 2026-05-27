use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent::AgentHandle;
use telegram_host::{MessageCache, TelegramHost};
use teloxide::types::{Update, UpdateKind};

use crate::TriggerConfig;
use crate::chat_worker::{ChatSenders, dispatch_to_worker};
use crate::resolve::ChatMap;

pub(crate) async fn save_update(telegram_dir: &Path, upd: &Update) {
    let update_dir = telegram_dir.join("updates");
    let path = update_dir.join(format!("update-{}.json", upd.id.0));
    let tmp = update_dir.join(format!(".update-{}.tmp", upd.id.0));
    if let Ok(json) = serde_json::to_string(upd) {
        if tokio::fs::write(&tmp, &json).await.is_ok() {
            let _ = tokio::fs::rename(&tmp, &path).await;
        }
    }
}

pub(crate) async fn handle_root_update(
    upd: Update,
    agent: Arc<dyn AgentHandle>,
    config: Arc<TriggerConfig>,
    chat_map: ChatMap,
    cache: MessageCache,
    _host: TelegramHost,
    chat_senders: ChatSenders,
    telegram_dir: PathBuf,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    save_update(&telegram_dir, &upd).await;

    if let UpdateKind::Message(msg) = upd.kind {
        cache.push(msg.clone()).await;
        dispatch_to_worker(msg, chat_senders, agent, config, chat_map, telegram_dir).await;
    }

    Ok(())
}
