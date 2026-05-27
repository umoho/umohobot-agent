use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use agent::AgentHandle;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use teloxide::types::ChatId;
use tokio::sync::RwLock;
use tracing::info;
use uuid::Uuid;

use crate::TriggerConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ThreadEntry {
    pub thread_id: Uuid,
    pub last_activity: DateTime<Utc>,
    pub message_count: usize,
}

pub(crate) type ChatMap = Arc<RwLock<HashMap<ChatId, ThreadEntry>>>;

pub(crate) struct ResolveResult {
    pub thread_id: Uuid,
    pub old_thread_id: Option<Uuid>,
}

pub(crate) async fn resolve_thread(
    chat_id: ChatId,
    system_prompt: &str,
    _compact_prompt: &str,
    format_available_models: &str,
    chat_map: &ChatMap,
    agent: &dyn AgentHandle,
    config: &TriggerConfig,
    batch_len: usize,
) -> ResolveResult {
    let mut map = chat_map.write().await;
    let now = Utc::now();

    match map.get_mut(&chat_id) {
        Some(entry) => {
            let expired = now - entry.last_activity > config.idle_timeout
                || entry.message_count + batch_len > config.max_thread_length;

            if expired {
                let old_thread_id = entry.thread_id;
                let thread = agent.create_thread().await;
                let thread_id = thread.id;
                agent
                    .append_system_message(
                        thread_id,
                        &system_prompt
                            .replace("{chat_id}", &chat_id.0.to_string())
                            .replace("{available_models}", &format_available_models)
                            .replace("{system_prompt}", &config.system_prompt),
                    )
                    .await;
                *entry = ThreadEntry {
                    thread_id,
                    last_activity: now,
                    message_count: batch_len,
                };
                info!(%chat_id, %thread_id, "new thread (expired)");
                ResolveResult {
                    thread_id,
                    old_thread_id: Some(old_thread_id),
                }
            } else {
                entry.last_activity = now;
                entry.message_count += batch_len;
                ResolveResult {
                    thread_id: entry.thread_id,
                    old_thread_id: None,
                }
            }
        }
        None => {
            let thread = agent.create_thread().await;
            let thread_id = thread.id;
            agent
                .append_system_message(
                    thread_id,
                    &system_prompt
                        .replace("{chat_id}", &chat_id.0.to_string())
                        .replace("{available_models}", &format_available_models)
                        .replace("{system_prompt}", &config.system_prompt),
                )
                .await;
            map.insert(
                chat_id,
                ThreadEntry {
                    thread_id,
                    last_activity: now,
                    message_count: batch_len,
                },
            );
            info!(%chat_id, %thread_id, "new thread (first message)");
            ResolveResult {
                thread_id,
                old_thread_id: None,
            }
        }
    }
}

pub(crate) async fn save_chat_map(telegram_dir: &Path, chat_map: &ChatMap) {
    let path = telegram_dir.join("chat_map.json");
    let tmp = telegram_dir.join(".chat_map.tmp");
    let map = chat_map.read().await;
    if let Ok(json) = serde_json::to_string(&*map) {
        let _ = tokio::fs::write(&tmp, &json).await;
        let _ = tokio::fs::rename(&tmp, &path).await;
    }
}
