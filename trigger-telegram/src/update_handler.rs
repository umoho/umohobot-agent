use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent::AgentHandle;
use telegram_host::{MessageCache, TelegramHost, ThreadChatMap, UpdateStore};
use teloxide::types::{Update, UpdateKind};

use crate::TriggerConfig;
use crate::chat_worker::{ChatSenders, dispatch_to_worker};
use crate::resolve::ChatMap;

fn update_chat_id(upd: &Update) -> Option<i64> {
    Some(match &upd.kind {
        UpdateKind::Message(msg) => msg.chat.id.0,
        UpdateKind::EditedMessage(msg) => msg.chat.id.0,
        UpdateKind::ChannelPost(msg) => msg.chat.id.0,
        UpdateKind::EditedChannelPost(msg) => msg.chat.id.0,
        UpdateKind::BusinessMessage(msg) => msg.chat.id.0,
        UpdateKind::EditedBusinessMessage(msg) => msg.chat.id.0,
        _ => return None,
    })
}

pub(crate) async fn save_update(telegram_dir: &Path, upd: &Update) {
    let Some(chat_id) = update_chat_id(upd) else {
        return;
    };
    let chat_dir = telegram_dir.join(format!("chat-{chat_id}"));
    let path = chat_dir.join(format!("update-{}.json", upd.id.0));
    let tmp = chat_dir.join(format!(".update-{}.tmp", upd.id.0));
    if let Ok(json) = serde_json::to_string(upd) {
        tokio::fs::create_dir_all(&chat_dir).await.ok();
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
    thread_chat_map: ThreadChatMap,
    updates: UpdateStore,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    save_update(&telegram_dir, &upd).await;

    if let Some(chat_id) = update_chat_id(&upd) {
        if let Ok(val) = serde_json::to_value(&upd) {
            let mut store = updates.write().await;
            store
                .entry(chat_id)
                .or_default()
                .push((upd.id.0 as i64, val));
        }
    }

    if let UpdateKind::Message(msg) = upd.kind {
        cache.push(msg.clone()).await;
        dispatch_to_worker(
            msg,
            chat_senders,
            agent,
            config,
            chat_map,
            telegram_dir,
            thread_chat_map,
            updates,
        )
        .await;
    }

    Ok(())
}
