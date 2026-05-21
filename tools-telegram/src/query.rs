use rig_core::completion::ToolDefinition;
use rig_core::tool::Tool;
use rig_core::tool::server::ToolServerHandle;
use serde::Deserialize;
use serde_json::json;
use telegram_host::{MessageCache, TelegramHost};
use teloxide::types::ChatId;

use crate::ToolError;

fn default_limit() -> usize {
    20
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryMessageArgs {
    pub chat_id: i64,
    pub message_id: i32,
}

pub struct QueryMessageTool {
    pub host: TelegramHost,
    pub cache: MessageCache,
}

impl Tool for QueryMessageTool {
    const NAME: &'static str = "telegram_query_message";

    type Error = ToolError;
    type Args = QueryMessageArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_query_message".into(),
            description: "Get a single message by its ID from the chat history".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chatId": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "messageId": {
                        "type": "integer",
                        "description": "ID of the message to retrieve"
                    }
                },
                "required": ["chatId", "messageId"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let result = self
            .cache
            .get_message(ChatId(args.chat_id), args.message_id)
            .await;
        Ok(match result {
            Some(v) => v.to_string(),
            None => "null".into(),
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryMessagesArgs {
    pub chat_id: i64,
    #[serde(default = "default_limit")]
    pub limit: usize,
    pub before_message_id: Option<i32>,
}

pub struct QueryMessagesTool {
    pub host: TelegramHost,
    pub cache: MessageCache,
}

impl Tool for QueryMessagesTool {
    const NAME: &'static str = "telegram_query_messages";

    type Error = ToolError;
    type Args = QueryMessagesArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_query_messages".into(),
            description: "List recent messages in a chat, optionally before a specific message ID"
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chatId": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Number of messages to return, default 20"
                    },
                    "beforeMessageId": {
                        "type": "integer",
                        "description": "Optional: only return messages older than this ID (for pagination)"
                    }
                },
                "required": ["chatId"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let results = self
            .cache
            .list_messages(ChatId(args.chat_id), args.limit, args.before_message_id)
            .await;
        Ok(serde_json::to_string(&results).unwrap_or_else(|_| "[]".into()))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuerySearchArgs {
    pub chat_id: i64,
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

pub struct QuerySearchTool {
    pub host: TelegramHost,
    pub cache: MessageCache,
}

impl Tool for QuerySearchTool {
    const NAME: &'static str = "telegram_query_search";

    type Error = ToolError;
    type Args = QuerySearchArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_query_search".into(),
            description: "Search message history by keyword in a chat".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chatId": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "query": {
                        "type": "string",
                        "description": "Keyword to search for in message text"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum results to return, default 20"
                    }
                },
                "required": ["chatId", "query"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let results = self
            .cache
            .search_messages(ChatId(args.chat_id), &args.query, args.limit)
            .await;
        Ok(serde_json::to_string(&results).unwrap_or_else(|_| "[]".into()))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryMessagesByUserArgs {
    pub chat_id: i64,
    pub user_id: i64,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

pub struct QueryMessagesByUserTool {
    pub host: TelegramHost,
    pub cache: MessageCache,
}

impl Tool for QueryMessagesByUserTool {
    const NAME: &'static str = "telegram_query_messages_by_user";

    type Error = ToolError;
    type Args = QueryMessagesByUserArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_query_messages_by_user".into(),
            description: "List recent messages sent by a specific user in a chat".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chatId": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "userId": {
                        "type": "integer",
                        "description": "User ID to filter by"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum results to return, default 20"
                    }
                },
                "required": ["chatId", "userId"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let results = self
            .cache
            .messages_by_user(ChatId(args.chat_id), args.user_id, args.limit)
            .await;
        Ok(serde_json::to_string(&results).unwrap_or_else(|_| "[]".into()))
    }
}

pub async fn register_query_tools(
    handle: &ToolServerHandle,
    host: TelegramHost,
    cache: MessageCache,
) -> Result<(), rig_core::tool::server::ToolServerError> {
    handle
        .add_tool(QueryMessageTool {
            host: host.clone(),
            cache: cache.clone(),
        })
        .await?;
    handle
        .add_tool(QueryMessagesTool {
            host: host.clone(),
            cache: cache.clone(),
        })
        .await?;
    handle
        .add_tool(QuerySearchTool {
            host: host.clone(),
            cache: cache.clone(),
        })
        .await?;
    handle
        .add_tool(QueryMessagesByUserTool { host, cache })
        .await?;
    Ok(())
}
