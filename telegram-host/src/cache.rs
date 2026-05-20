use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use serde_json::{Value, json};
use teloxide::types::{ChatId, Message};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct MessageCache {
    inner: Arc<RwLock<MessageCacheData>>,
}

struct MessageCacheData {
    per_chat: HashMap<ChatId, VecDeque<Message>>,
    max_per_chat: usize,
}

impl MessageCache {
    pub fn new(max_per_chat: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(MessageCacheData {
                per_chat: HashMap::new(),
                max_per_chat,
            })),
        }
    }

    pub async fn push(&self, msg: Message) {
        let mut data = self.inner.write().await;
        let max = data.max_per_chat;
        let queue = data.per_chat.entry(msg.chat.id).or_default();
        if queue.len() >= max {
            queue.pop_front();
        }
        queue.push_back(msg);
    }

    pub async fn push_edited(&self, msg: Message) {
        let mut data = self.inner.write().await;
        let max = data.max_per_chat;
        let queue = data.per_chat.entry(msg.chat.id).or_default();
        if let Some(existing) = queue.iter_mut().find(|m| m.id == msg.id) {
            *existing = msg;
        } else if queue.len() >= max {
            queue.pop_front();
            queue.push_back(msg);
        } else {
            queue.push_back(msg);
        }
    }

    pub async fn get_message(&self, chat_id: ChatId, message_id: i32) -> Option<Value> {
        let data = self.inner.read().await;
        let queue = data.per_chat.get(&chat_id)?;
        for msg in queue {
            if msg.id.0 == message_id {
                return Some(message_to_value(msg));
            }
        }
        None
    }

    pub async fn list_messages(
        &self,
        chat_id: ChatId,
        limit: usize,
        before_message_id: Option<i32>,
    ) -> Vec<Value> {
        let data = self.inner.read().await;
        let queue = match data.per_chat.get(&chat_id) {
            Some(q) => q,
            None => return vec![],
        };

        let mut results: Vec<Value> = Vec::new();
        for msg in queue.iter().rev() {
            if let Some(before_id) = before_message_id {
                if msg.id.0 >= before_id {
                    continue;
                }
            }
            results.push(message_to_value(msg));
            if results.len() >= limit {
                break;
            }
        }
        results.reverse();
        results
    }

    pub async fn search_messages(&self, chat_id: ChatId, query: &str, limit: usize) -> Vec<Value> {
        let query_lower = query.to_lowercase();
        let data = self.inner.read().await;
        let queue = match data.per_chat.get(&chat_id) {
            Some(q) => q,
            None => return vec![],
        };

        let mut results: Vec<Value> = Vec::new();
        for msg in queue.iter().rev() {
            let text = msg.text().unwrap_or("");
            if text.to_lowercase().contains(&query_lower) {
                results.push(message_to_value(msg));
                if results.len() >= limit {
                    break;
                }
            }
        }
        results.reverse();
        results
    }

    pub async fn messages_by_user(
        &self,
        chat_id: ChatId,
        user_id: i64,
        limit: usize,
    ) -> Vec<Value> {
        let data = self.inner.read().await;
        let queue = match data.per_chat.get(&chat_id) {
            Some(q) => q,
            None => return vec![],
        };

        let mut results: Vec<Value> = Vec::new();
        for msg in queue.iter().rev() {
            if msg.from.as_ref().is_some_and(|u| u.id.0 as i64 == user_id) {
                results.push(message_to_value(msg));
                if results.len() >= limit {
                    break;
                }
            }
        }
        results.reverse();
        results
    }
}

fn message_to_value(msg: &Message) -> Value {
    json!({
        "messageId": msg.id.0,
        "userId": msg.from.as_ref().map(|u| u.id.0),
        "username": msg.from.as_ref().and_then(|u| u.username.as_deref()),
        "firstName": msg.from.as_ref().map(|u| &u.first_name),
        "text": msg.text(),
        "date": msg.date,
        "replyToMessageId": msg.reply_to_message().as_ref().map(|m| m.id.0),
        "editDate": msg.edit_date(),
    })
}
