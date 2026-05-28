use std::sync::Arc;

use agent::{AgentError, AgentHandle, AgentRuntime};
use regex::Regex;
use rig_core::completion::ToolDefinition;
use rig_core::tool::Tool;
use serde::Deserialize;
use serde_json::{Value, json};
use telegram_host::{MessageCache, TelegramHost, ThreadChatMap, UpdateStore};
use teloxide::types::ChatId;
use uuid::Uuid;

use crate::ToolError;

fn default_limit() -> usize {
    10
}

fn extract_message<'a>(upd: &'a Value) -> Option<&'a Value> {
    for key in &[
        "message",
        "edited_message",
        "channel_post",
        "edited_channel_post",
        "business_message",
        "edited_business_message",
    ] {
        if let Some(msg) = upd.get(key) {
            if !msg.is_null() {
                return Some(msg);
            }
        }
    }
    None
}

fn extract_i64(val: &Value, key: &str) -> Option<i64> {
    val.get(key)?.as_i64()
}

fn walk_strings(val: &Value, f: &mut impl FnMut(&str)) {
    match val {
        Value::String(s) => f(s),
        Value::Object(m) => m.values().for_each(|v| walk_strings(v, f)),
        Value::Array(a) => a.iter().for_each(|v| walk_strings(v, f)),
        _ => {}
    }
}

fn matches_keyword(val: &Value, keyword: &str, mode: &str) -> bool {
    if keyword.is_empty() {
        return true;
    }
    let mut found = false;
    walk_strings(val, &mut |s| {
        if found {
            return;
        }
        let matched = match mode {
            "contains_case" => s.contains(keyword),
            "regex" => Regex::new(keyword).map(|r| r.is_match(s)).unwrap_or(false),
            _ => s.to_lowercase().contains(&keyword.to_lowercase()),
        };
        if matched {
            found = true;
        }
    });
    found
}

fn message_type(msg: &Value) -> &str {
    if msg
        .get("text")
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false)
        && msg.get("photo").is_none()
        && msg.get("video").is_none()
    {
        "text"
    } else if msg.get("photo").is_some() {
        "photo"
    } else if msg.get("video").is_some() {
        "video"
    } else if msg.get("audio").is_some() {
        "audio"
    } else if msg.get("document").is_some() {
        "document"
    } else if msg.get("animation").is_some() {
        "animation"
    } else if msg.get("sticker").is_some() {
        "sticker"
    } else if msg.get("dice").is_some() {
        "dice"
    } else if msg.get("poll").is_some() {
        "poll"
    } else if msg.get("video_note").is_some() {
        "video_note"
    } else if msg.get("voice").is_some() {
        "voice"
    } else if msg.get("location").is_some() {
        "location"
    } else if msg.get("venue").is_some() {
        "venue"
    } else {
        "unknown"
    }
}

fn matches_type_filter(msg: &Value, type_filter: &str) -> bool {
    if type_filter.is_empty() {
        return true;
    }
    let msg_type = message_type(msg);
    type_filter.split(',').any(|t| t.trim() == msg_type)
}

fn parse_date(s: &str) -> Option<i64> {
    if let Ok(ts) = s.parse::<i64>() {
        return Some(ts);
    }
    if let Some(offset) = s.strip_prefix('-') {
        let now = chrono::Utc::now().timestamp();
        let duration = if offset.ends_with("d") {
            let days: i64 = offset.trim_end_matches('d').parse().ok()?;
            days * 86400
        } else if offset.ends_with("h") {
            let hours: i64 = offset.trim_end_matches('h').parse().ok()?;
            hours * 3600
        } else if offset.ends_with("m") {
            let mins: i64 = offset.trim_end_matches('m').parse().ok()?;
            mins * 60
        } else if offset.ends_with("M") {
            let months: i64 = offset.trim_end_matches('M').parse().ok()?;
            months * 30 * 86400
        } else {
            return None;
        };
        return Some(now - duration);
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.timestamp());
    }
    None
}

async fn resolve_chat_id(
    thread_id: Uuid,
    thread_chat_map: &ThreadChatMap,
    agent: &Arc<dyn AgentHandle>,
) -> Option<i64> {
    {
        let map = thread_chat_map.read().await;
        if let Some(&chat_id) = map.get(&thread_id) {
            return Some(chat_id);
        }
    }
    let mut current = thread_id;
    loop {
        let thread = agent.get_thread(current).await?;
        match thread.parent_thread_id {
            Some(parent_id) => {
                {
                    let map = thread_chat_map.read().await;
                    if let Some(&chat_id) = map.get(&parent_id) {
                        return Some(chat_id);
                    }
                }
                current = parent_id;
            }
            None => return None,
        }
    }
}

fn build_list_entry(update_id: i64, msg: &Value) -> Value {
    json!({
        "updateId": update_id,
        "msgId": extract_i64(msg, "message_id"),
        "userId": msg.pointer("/from/id").and_then(|v| v.as_i64()),
        "username": msg.pointer("/from/username").and_then(|v| v.as_str()),
        "firstName": msg.pointer("/from/first_name").and_then(|v| v.as_str()),
        "text": msg.get("text").and_then(|v| v.as_str()).or_else(|| msg.get("caption").and_then(|v| v.as_str())).unwrap_or(""),
        "date": extract_i64(msg, "date"),
        "type": message_type(msg),
        "replyToMsgId": msg.pointer("/reply_to_message/message_id").and_then(|v| v.as_i64()),
    })
}

fn build_find_entry(update_id: i64, msg: &Value) -> Value {
    json!({
        "updateId": update_id,
        "msgId": extract_i64(msg, "message_id"),
        "userId": msg.pointer("/from/id").and_then(|v| v.as_i64()),
        "date": extract_i64(msg, "date"),
    })
}

fn build_read_entry(update_id: i64, upd: &Value, fields: Option<&str>) -> Value {
    let msg = extract_message(upd);
    match fields {
        Some(f) => {
            let mut result = json!({"updateId": update_id});
            if let Some(m) = msg {
                result["msgId"] = json!(extract_i64(m, "message_id"));
            }
            for field in f.split(',') {
                let field = field.trim();
                if field.is_empty() {
                    continue;
                }
                let json_path = format!("$.{}", field);
                if let Ok(path) = serde_json_path::JsonPath::parse(&json_path) {
                    let nodes = path.query(upd).all();
                    if let Some(node) = nodes.into_iter().next() {
                        result[field] = node.clone();
                    }
                }
            }
            result
        }
        None => match msg {
            Some(m) => build_list_entry(update_id, m),
            None => json!({"updateId": update_id}),
        },
    }
}

fn query_update_store<'a>(
    store: &'a [(i64, Value)],
    keyword: Option<&str>,
    user_id: Option<i64>,
    type_filter: Option<&str>,
    date_from: Option<i64>,
    date_to: Option<i64>,
    match_mode: &str,
    before_message_id: Option<i32>,
    limit: usize,
) -> Vec<(i64, &'a Value)> {
    let keyword = keyword.unwrap_or("");
    let type_filter = type_filter.unwrap_or("");

    let mut results: Vec<(i64, &Value)> = Vec::new();

    for (update_id, upd) in store.iter().rev() {
        if let Some(msg) = extract_message(upd) {
            if !matches_type_filter(msg, type_filter) {
                continue;
            }

            let msg_id = extract_i64(msg, "message_id").unwrap_or(0) as i32;
            if let Some(before_id) = before_message_id {
                if msg_id >= before_id {
                    continue;
                }
            }

            if let Some(uid) = user_id {
                let from_id = msg.pointer("/from/id").and_then(|v| v.as_i64());
                if from_id != Some(uid) {
                    continue;
                }
            }

            let msg_date = extract_i64(msg, "date").unwrap_or(0);
            if let Some(from) = date_from {
                if msg_date < from {
                    continue;
                }
            }
            if let Some(to) = date_to {
                if msg_date > to {
                    continue;
                }
            }

            if !matches_keyword(upd, keyword, match_mode) {
                continue;
            }

            results.push((*update_id, msg));
            if results.len() >= limit {
                break;
            }
        }
    }

    results.reverse();
    results
}

// ---------- telegram_query_list ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListMessagesArgs {
    #[serde(default = "default_limit")]
    pub limit: usize,
    pub before_message_id: Option<i32>,
}

pub struct ListMessagesTool {
    pub host: TelegramHost,
    pub cache: MessageCache,
    pub updates: UpdateStore,
    pub thread_chat_map: ThreadChatMap,
    pub agent: Arc<dyn AgentHandle>,
}

impl Tool for ListMessagesTool {
    const NAME: &'static str = "telegram_query_list";

    type Error = ToolError;
    type Args = ListMessagesArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_query_list".into(),
            description:
                "List recent messages in the current chat. Supports pagination via beforeMessageId."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "description": "Max results to return, default 10"
                    },
                    "beforeMessageId": {
                        "type": "integer",
                        "description": "Pagination cursor: only return messages older than this ID"
                    }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let thread_id = agent::CURRENT_PARENT_THREAD_ID
            .try_with(|id| *id)
            .map_err(|_| ToolError::NotInAgentContext)?;

        let chat_id = resolve_chat_id(thread_id, &self.thread_chat_map, &self.agent)
            .await
            .ok_or(ToolError::ChatNotFound)?;

        // Try memory cache first
        let cached = self
            .cache
            .list_messages(ChatId(chat_id), args.limit, args.before_message_id)
            .await;

        if cached.len() >= args.limit {
            return Ok(serde_json::to_string(&cached).unwrap_or_else(|_| "[]".into()));
        }

        // Fall back to disk store
        let store = self.updates.read().await;
        let chat_updates = match store.get(&chat_id) {
            Some(u) => u,
            None => return Ok(serde_json::to_string(&cached).unwrap_or_else(|_| "[]".into())),
        };

        let results = query_update_store(
            chat_updates,
            None,
            None,
            None,
            None,
            None,
            "contains",
            args.before_message_id,
            args.limit,
        );

        let entries: Vec<Value> = results
            .into_iter()
            .map(|(update_id, msg)| build_list_entry(update_id, msg))
            .collect();

        Ok(serde_json::to_string(&entries).unwrap_or_else(|_| "[]".into()))
    }
}

// ---------- telegram_query_find ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindMessagesArgs {
    pub keyword: Option<String>,
    pub user_id: Option<i64>,
    #[serde(rename = "type")]
    pub msg_type: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    #[serde(default = "default_match_mode")]
    pub match_mode: String,
    pub before_message_id: Option<i32>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_match_mode() -> String {
    "contains".into()
}

pub struct FindMessagesTool {
    pub host: TelegramHost,
    pub cache: MessageCache,
    pub updates: UpdateStore,
    pub thread_chat_map: ThreadChatMap,
    pub agent: Arc<dyn AgentHandle>,
}

impl Tool for FindMessagesTool {
    const NAME: &'static str = "telegram_query_find";

    type Error = ToolError;
    type Args = FindMessagesArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_query_find".into(),
            description: "Search message history in the current chat. Returns lightweight entries (updateId, msgId, userId, date) without message body. Supports filtering by keyword, user, type, date range, regex. Use telegram_query_read to view full content.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "keyword": {
                        "type": "string",
                        "description": "Keyword to search for across the full Update JSON"
                    },
                    "userId": {
                        "type": "integer",
                        "description": "Filter by sender user ID"
                    },
                    "type": {
                        "type": "string",
                        "description": "Message type filter, comma-separated. E.g.: 'photo,video,document'"
                    },
                    "dateFrom": {
                        "type": "string",
                        "description": "Start date. Unix timestamp, relative offset (e.g. '-7d', '-2h', '-30m'), or RFC3339"
                    },
                    "dateTo": {
                        "type": "string",
                        "description": "End date. Same format as dateFrom"
                    },
                    "matchMode": {
                        "type": "string",
                        "default": "contains",
                        "description": "Match mode: contains (case-insensitive), contains_case, regex"
                    },
                    "beforeMessageId": {
                        "type": "integer",
                        "description": "Pagination cursor: only return messages older than this ID"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results to return, default 10"
                    }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let thread_id = agent::CURRENT_PARENT_THREAD_ID
            .try_with(|id| *id)
            .map_err(|_| ToolError::NotInAgentContext)?;

        let chat_id = resolve_chat_id(thread_id, &self.thread_chat_map, &self.agent)
            .await
            .ok_or(ToolError::ChatNotFound)?;

        let date_from = args.date_from.as_deref().and_then(parse_date);
        let date_to = args.date_to.as_deref().and_then(parse_date);

        let store = self.updates.read().await;
        let chat_updates = match store.get(&chat_id) {
            Some(u) => u,
            None => return Ok("[]".into()),
        };

        let results = query_update_store(
            chat_updates,
            args.keyword.as_deref(),
            args.user_id,
            args.msg_type.as_deref(),
            date_from,
            date_to,
            &args.match_mode,
            args.before_message_id,
            args.limit,
        );

        let entries: Vec<Value> = results
            .into_iter()
            .map(|(update_id, msg)| build_find_entry(update_id, msg))
            .collect();

        Ok(serde_json::to_string(&entries).unwrap_or_else(|_| "[]".into()))
    }
}

// ---------- telegram_query_read ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadMessagesArgs {
    pub message_ids: Vec<i32>,
    pub fields: Option<String>,
}

pub struct ReadMessagesTool {
    pub host: TelegramHost,
    pub cache: MessageCache,
    pub updates: UpdateStore,
    pub thread_chat_map: ThreadChatMap,
    pub agent: Arc<dyn AgentHandle>,
}

impl Tool for ReadMessagesTool {
    const NAME: &'static str = "telegram_query_read";

    type Error = ToolError;
    type Args = ReadMessagesArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_query_read".into(),
            description: "Read full message details by IDs. Without fields, returns simplified format. With fields, extracts specified JSONPath (comma-separated). E.g.: 'message.text, message.from.username, message.chat.id'".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "messageIds": {
                        "type": "array",
                        "items": {"type": "integer"},
                        "description": "List of message IDs to read"
                    },
                    "fields": {
                        "type": "string",
                        "description": "Optional: comma-separated JSONPath fields. E.g. 'message.text, message.from.username'. Omit for simplified format"
                    }
                },
                "required": ["messageIds"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let thread_id = agent::CURRENT_PARENT_THREAD_ID
            .try_with(|id| *id)
            .map_err(|_| ToolError::NotInAgentContext)?;

        let chat_id = resolve_chat_id(thread_id, &self.thread_chat_map, &self.agent)
            .await
            .ok_or(ToolError::ChatNotFound)?;

        let store = self.updates.read().await;
        let chat_updates = match store.get(&chat_id) {
            Some(u) => u,
            None => return Ok("[]".into()),
        };

        let fields = args.fields.as_deref();

        let mut entries = Vec::new();
        for msg_id in &args.message_ids {
            for (update_id, upd) in chat_updates {
                if let Some(msg) = extract_message(upd) {
                    if extract_i64(msg, "message_id") == Some(*msg_id as i64) {
                        entries.push(build_read_entry(*update_id, upd, fields));
                        break;
                    }
                }
            }
        }

        // Try memory cache for any not found
        for msg_id in &args.message_ids {
            if !entries
                .iter()
                .any(|e| e["msgId"].as_i64() == Some(*msg_id as i64))
            {
                if let Some(cached) = self.cache.get_message(ChatId(chat_id), *msg_id).await {
                    entries.push(cached);
                }
            }
        }

        Ok(serde_json::to_string(&entries).unwrap_or_else(|_| "[]".into()))
    }
}

// ---------- registration ----------

pub async fn register_query_tools(
    runtime: &AgentRuntime,
    host: TelegramHost,
    cache: MessageCache,
    updates: UpdateStore,
    thread_chat_map: ThreadChatMap,
    agent: Arc<dyn AgentHandle>,
) -> Result<(), AgentError> {
    runtime
        .register_tool(ListMessagesTool {
            host: host.clone(),
            cache: cache.clone(),
            updates: updates.clone(),
            thread_chat_map: thread_chat_map.clone(),
            agent: agent.clone(),
        })
        .await?;
    runtime
        .register_tool(FindMessagesTool {
            host: host.clone(),
            cache: cache.clone(),
            updates: updates.clone(),
            thread_chat_map: thread_chat_map.clone(),
            agent: agent.clone(),
        })
        .await?;
    runtime
        .register_tool(ReadMessagesTool {
            host: host.clone(),
            cache: cache.clone(),
            updates: updates.clone(),
            thread_chat_map: thread_chat_map.clone(),
            agent,
        })
        .await?;
    Ok(())
}
