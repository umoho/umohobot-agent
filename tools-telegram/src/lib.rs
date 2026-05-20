use rig_core::completion::ToolDefinition;
use rig_core::tool::Tool;
use rig_core::tool::server::ToolServerHandle;
use serde::Deserialize;
use serde_json::json;
use telegram_host::{MessageCache, TelegramHost};
use teloxide::RequestError;
use teloxide::payloads::{EditMessageTextSetters, SendMessageSetters};
use teloxide::prelude::Requester;
use teloxide::types::{ChatAction, ChatId, MessageId, ParseMode, ReplyParameters};

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Telegram error: {0}")]
    Telegram(#[from] telegram_host::TelegramError),
    #[error("Telegram request error: {0}")]
    Request(#[from] RequestError),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageArgs {
    pub chat_id: i64,
    pub text: String,
    pub reply_to_message_id: Option<i32>,
    pub parse_mode: Option<ParseMode>,
}

pub struct SendMessageTool {
    pub host: TelegramHost,
}

impl Tool for SendMessageTool {
    const NAME: &'static str = "telegram_sendMessage";

    type Error = ToolError;
    type Args = SendMessageArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_sendMessage".into(),
            description: "Send a text message to the Telegram chat".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chatId": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "text": {
                        "type": "string",
                        "description": "Message text"
                    },
                    "replyToMessageId": {
                        "type": "integer",
                        "description": "Optional: ID of the message to reply to"
                    },
                    "parseMode": {
                        "type": "string",
                        "enum": ["MarkdownV2", "HTML", "Markdown"],
                        "description": "Optional: parse mode for the message text"
                    }
                },
                "required": ["chatId", "text"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut req = self
            .host
            .bot()
            .send_message(ChatId(args.chat_id), &args.text);
        if let Some(reply_id) = args.reply_to_message_id {
            req = req.reply_parameters(ReplyParameters::new(MessageId(reply_id)));
        }
        if let Some(parse_mode) = args.parse_mode {
            req = req.parse_mode(parse_mode);
        }
        req.await?;
        Ok(format!("Message sent to chat {}", args.chat_id))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditMessageArgs {
    pub chat_id: i64,
    pub message_id: i32,
    pub text: String,
    pub parse_mode: Option<ParseMode>,
}

pub struct EditMessageTool {
    pub host: TelegramHost,
}

impl Tool for EditMessageTool {
    const NAME: &'static str = "telegram_editMessage";

    type Error = ToolError;
    type Args = EditMessageArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_editMessage".into(),
            description: "Edit a previously sent message in the Telegram chat".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chatId": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "messageId": {
                        "type": "integer",
                        "description": "ID of the message to edit"
                    },
                    "text": {
                        "type": "string",
                        "description": "New text for the message"
                    },
                    "parseMode": {
                        "type": "string",
                        "enum": ["MarkdownV2", "HTML", "Markdown"],
                        "description": "Optional: parse mode for the message text"
                    }
                },
                "required": ["chatId", "messageId", "text"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut req = self.host.bot().edit_message_text(
            ChatId(args.chat_id),
            MessageId(args.message_id),
            &args.text,
        );
        if let Some(parse_mode) = args.parse_mode {
            req = req.parse_mode(parse_mode);
        }
        req.await?;
        Ok(format!("Message {} edited", args.message_id))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteMessageArgs {
    pub chat_id: i64,
    pub message_id: i32,
}

pub struct DeleteMessageTool {
    pub host: TelegramHost,
}

impl Tool for DeleteMessageTool {
    const NAME: &'static str = "telegram_deleteMessage";

    type Error = ToolError;
    type Args = DeleteMessageArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_deleteMessage".into(),
            description: "Delete a message from the Telegram chat".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chatId": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "messageId": {
                        "type": "integer",
                        "description": "ID of the message to delete"
                    }
                },
                "required": ["chatId", "messageId"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.host
            .bot()
            .delete_message(ChatId(args.chat_id), MessageId(args.message_id))
            .await?;
        Ok(format!("Message {} deleted", args.message_id))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendChatActionArgs {
    pub chat_id: i64,
    pub action: ChatAction,
}

pub struct SendChatActionTool {
    pub host: TelegramHost,
}

impl Tool for SendChatActionTool {
    const NAME: &'static str = "telegram_sendChatAction";

    type Error = ToolError;
    type Args = SendChatActionArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_sendChatAction".into(),
            description: "Broadcast a chat action (typing indicator, uploading status, etc.)"
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chatId": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "action": {
                        "type": "string",
                        "enum": [
                            "typing",
                            "upload_photo",
                            "record_video",
                            "upload_video",
                            "record_voice",
                            "upload_voice",
                            "upload_document",
                            "find_location",
                            "record_video_note",
                            "upload_video_note"
                        ],
                        "description": "Type of chat action to broadcast"
                    }
                },
                "required": ["chatId", "action"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.host
            .bot()
            .send_chat_action(ChatId(args.chat_id), args.action)
            .await?;
        Ok(format!("Chat action broadcast in chat {}", args.chat_id))
    }
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

fn default_limit() -> usize {
    20
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

pub async fn register_telegram_tools(
    handle: &ToolServerHandle,
    host: TelegramHost,
    cache: MessageCache,
) -> Result<(), rig_core::tool::server::ToolServerError> {
    handle
        .add_tool(SendMessageTool { host: host.clone() })
        .await?;
    handle
        .add_tool(EditMessageTool { host: host.clone() })
        .await?;
    handle
        .add_tool(DeleteMessageTool { host: host.clone() })
        .await?;
    handle
        .add_tool(SendChatActionTool { host: host.clone() })
        .await?;
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
