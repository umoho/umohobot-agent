use rig_core::completion::ToolDefinition;
use rig_core::tool::Tool;
use rig_core::tool::server::ToolServerHandle;
use serde::Deserialize;
use serde_json::json;
use telegram_host::TelegramHost;
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
    const NAME: &'static str = "telegram.sendMessage";

    type Error = ToolError;
    type Args = SendMessageArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram.sendMessage".into(),
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
    const NAME: &'static str = "telegram.editMessage";

    type Error = ToolError;
    type Args = EditMessageArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram.editMessage".into(),
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
    const NAME: &'static str = "telegram.deleteMessage";

    type Error = ToolError;
    type Args = DeleteMessageArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram.deleteMessage".into(),
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
    const NAME: &'static str = "telegram.sendChatAction";

    type Error = ToolError;
    type Args = SendChatActionArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram.sendChatAction".into(),
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

pub async fn register_telegram_tools(
    handle: &ToolServerHandle,
    host: TelegramHost,
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
    handle.add_tool(SendChatActionTool { host }).await?;
    Ok(())
}
