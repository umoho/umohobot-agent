use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use agent::{AgentHandle, Message};
use teloxide::types::{ChatId, Message as TgMessage};
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tracing::{debug, error, warn};

use crate::TriggerConfig;
use crate::compact_format;
use crate::format_available_models;
use crate::message_format::build_user_content;
use crate::resolve::{ChatMap, ResolveResult, resolve_thread, save_chat_map};

pub(crate) type ChatSenders = Arc<Mutex<HashMap<ChatId, mpsc::UnboundedSender<TgMessage>>>>;

pub(crate) async fn dispatch_to_worker(
    msg: TgMessage,
    chat_senders: ChatSenders,
    agent: Arc<dyn AgentHandle>,
    config: Arc<TriggerConfig>,
    chat_map: ChatMap,
    telegram_dir: PathBuf,
) {
    let chat_id = msg.chat.id;
    let map = chat_senders.lock().await;

    if let Some(sender) = map.get(&chat_id) {
        let _ = sender.send(msg);
        return;
    }
    drop(map);

    let (tx, rx) = mpsc::unbounded_channel();
    let _ = tx.send(msg);
    chat_senders.lock().await.insert(chat_id, tx);

    tokio::spawn(chat_worker(
        chat_id,
        rx,
        agent,
        config,
        chat_map,
        telegram_dir,
    ));
}

async fn chat_worker(
    chat_id: ChatId,
    mut rx: mpsc::UnboundedReceiver<TgMessage>,
    agent: Arc<dyn AgentHandle>,
    config: Arc<TriggerConfig>,
    chat_map: ChatMap,
    telegram_dir: PathBuf,
) {
    let system_prompt = crate::SYSTEM_PROMPT.to_owned();
    let models_str = format_available_models(&config.available_models);

    loop {
        let Some(first) = rx.recv().await else {
            return;
        };

        let mut batch = vec![first];
        while let Ok(msg) = rx.try_recv() {
            batch.push(msg);
        }

        let mut agent_messages = Vec::new();
        for msg in &batch {
            let raw_text = msg
                .text()
                .or_else(|| msg.caption())
                .unwrap_or("")
                .to_owned();
            match build_user_content(msg, &raw_text) {
                Ok(content) if !content.is_empty() => {
                    agent_messages.push(Message::User { content });
                }
                _ => warn!(%chat_id, "skipping empty message in batch"),
            }
        }

        if agent_messages.is_empty() {
            continue;
        }

        let batch_len = agent_messages.len();

        if batch_len > config.max_thread_length as usize {
            for chunk in agent_messages.chunks(config.max_thread_length) {
                let resolve = resolve_thread(
                    chat_id,
                    &system_prompt,
                    &models_str,
                    &chat_map,
                    &*agent,
                    &config,
                    chunk.len(),
                )
                .await;
                save_chat_map(&telegram_dir, &chat_map).await;
                run_compact_and_turn(
                    chat_id,
                    resolve,
                    chunk.to_vec(),
                    &*agent,
                    &config,
                    &system_prompt,
                    &models_str,
                )
                .await;
            }
            continue;
        }

        let resolve = resolve_thread(
            chat_id,
            &system_prompt,
            &models_str,
            &chat_map,
            &*agent,
            &config,
            batch_len,
        )
        .await;
        save_chat_map(&telegram_dir, &chat_map).await;
        run_compact_and_turn(
            chat_id,
            resolve,
            agent_messages,
            &*agent,
            &config,
            &system_prompt,
            &models_str,
        )
        .await;
    }
}

pub(crate) async fn run_compact_and_turn(
    chat_id: ChatId,
    resolve: ResolveResult,
    agent_messages: Vec<Message>,
    agent: &dyn AgentHandle,
    config: &TriggerConfig,
    system_prompt: &str,
    models_str: &str,
) {
    if let Some(old_id) = resolve.old_thread_id {
        if let Ok(msgs) = agent.get_thread_messages(old_id).await {
            let text = compact_format::format_for_compact(&msgs);
            if let Ok(summary) = agent
                .compact_thread(&text, old_id, resolve.thread_id, crate::COMPACT_PROMPT)
                .await
            {
                if !summary.is_empty() && summary != "无" {
                    let full_system = format!(
                        "{}\n\n# 上一轮对话摘要\n{}",
                        system_prompt
                            .replace("{chat_id}", &chat_id.0.to_string())
                            .replace("{available_models}", &models_str)
                            .replace("{system_prompt}", &config.system_prompt),
                        summary
                    );
                    agent
                        .set_system_message(resolve.thread_id, &full_system)
                        .await;
                }
            }
        }
    }

    match agent.run_turn(resolve.thread_id, agent_messages).await {
        Ok((text, _usage)) => {
            if !text.is_empty() {
                debug!(%chat_id, "agent response: {text}");
            }
        }
        Err(e) => {
            error!(%chat_id, error = %e, "agent error");
        }
    }
}
