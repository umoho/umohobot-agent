use std::collections::HashMap;
use std::sync::Arc;

use agent::AgentHandle;
use chrono::{DateTime, Duration, Utc};
use dptree;
use telegram_host::TelegramHost;
use teloxide::dispatching::UpdateFilterExt;
use teloxide::prelude::*;
use teloxide::types::{ChatId, Message, Update};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct TriggerConfig {
    pub idle_timeout: Duration,
    pub max_thread_length: usize,
}

impl Default for TriggerConfig {
    fn default() -> Self {
        Self {
            idle_timeout: Duration::seconds(300),
            max_thread_length: 100,
        }
    }
}

struct ThreadEntry {
    thread_id: Uuid,
    last_activity: DateTime<Utc>,
    message_count: usize,
}

type ChatMap = Arc<RwLock<HashMap<ChatId, ThreadEntry>>>;

pub struct TelegramTrigger {
    host: TelegramHost,
    agent: Arc<dyn AgentHandle>,
    system_prompt: String,
    config: TriggerConfig,
    chat_map: ChatMap,
}

impl TelegramTrigger {
    pub fn new(
        host: TelegramHost,
        agent: Arc<dyn AgentHandle>,
        system_prompt: impl Into<String>,
        config: TriggerConfig,
    ) -> Self {
        Self {
            host,
            agent,
            system_prompt: system_prompt.into(),
            config,
            chat_map: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn start(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let bot = self.host.bot().clone();
        let agent = self.agent;
        let system_prompt = Arc::new(self.system_prompt);
        let config = Arc::new(self.config);
        let chat_map = self.chat_map;

        let handler = Update::filter_message().endpoint(handle_message);

        let dependencies = dptree::deps![agent, system_prompt, config, chat_map];

        info!("starting Telegram bot dispatcher");
        Dispatcher::builder(bot, handler)
            .dependencies(dependencies)
            .build()
            .dispatch()
            .await;

        Ok(())
    }
}

async fn handle_message(
    msg: Message,
    agent: Arc<dyn AgentHandle>,
    system_prompt: Arc<String>,
    config: Arc<TriggerConfig>,
    chat_map: ChatMap,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let chat_id = msg.chat.id;
    let text = match msg.text() {
        Some(t) => t,
        None => return Ok(()),
    };

    debug!(%chat_id, text_len = text.len(), "received message");

    // TODO: handle non-text content (images, stickers, etc.)
    if text.trim().is_empty() {
        warn!(%chat_id, "received non-text or empty message, ignoring");
        return Ok(());
    }

    let thread_id = resolve_thread(chat_id, &chat_map, &*agent, &system_prompt, &config).await;

    match agent.run_turn(thread_id, text).await {
        Ok(text) => {
            if !text.is_empty() {
                debug!(%chat_id, "agent response: {text}");
            }
        }
        Err(e) => {
            error!(%chat_id, error = %e, "agent error");
        }
    }

    Ok(())
}

async fn resolve_thread(
    chat_id: ChatId,
    chat_map: &ChatMap,
    agent: &dyn AgentHandle,
    system_prompt: &str,
    config: &TriggerConfig,
) -> Uuid {
    let mut map = chat_map.write().await;
    let now = Utc::now();

    match map.get_mut(&chat_id) {
        Some(entry) => {
            let expired = now - entry.last_activity > config.idle_timeout
                || entry.message_count >= config.max_thread_length;

            if expired {
                let thread_id = Uuid::new_v4();
                agent.get_or_create_thread(thread_id, system_prompt).await;
                *entry = ThreadEntry {
                    thread_id,
                    last_activity: now,
                    message_count: 1,
                };
                info!(%chat_id, %thread_id, "new thread (expired)");
                thread_id
            } else {
                entry.last_activity = now;
                entry.message_count += 1;
                entry.thread_id
            }
        }
        None => {
            let thread_id = Uuid::new_v4();
            agent.get_or_create_thread(thread_id, system_prompt).await;
            map.insert(
                chat_id,
                ThreadEntry {
                    thread_id,
                    last_activity: now,
                    message_count: 1,
                },
            );
            info!(%chat_id, %thread_id, "new thread (first message)");
            thread_id
        }
    }
}
