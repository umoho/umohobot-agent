use rig_core::completion::ToolDefinition;
use rig_core::tool::Tool;
use rig_core::tool::server::ToolServerHandle;
use serde::Deserialize;
use serde_json::json;
use telegram_host::TelegramHost;
use teloxide::types::ChatId;

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Telegram error: {0}")]
    Telegram(#[from] telegram_host::TelegramError),
}

#[derive(Deserialize)]
pub struct SendMessageArgs {
    pub chat_id: i64,
    pub text: String,
}

pub struct SendMessageTool {
    pub host: TelegramHost,
}

impl Tool for SendMessageTool {
    const NAME: &'static str = "send_message";

    type Error = ToolError;
    type Args = SendMessageArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "send_message".into(),
            description: "Send a text message to the Telegram chat".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chat_id": {
                        "type": "integer",
                        "description": "Telegram chat ID to send the message to"
                    },
                    "text": {
                        "type": "string",
                        "description": "The message text to send"
                    }
                },
                "required": ["chat_id", "text"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.host
            .send_message(ChatId(args.chat_id), &args.text)
            .await?;
        Ok(format!("Message sent to chat {}", args.chat_id))
    }
}

#[derive(Deserialize)]
pub struct EditMessageArgs {
    pub chat_id: i64,
    pub message_id: i32,
    pub text: String,
}

pub struct EditMessageTool {
    pub host: TelegramHost,
}

impl Tool for EditMessageTool {
    const NAME: &'static str = "edit_message";

    type Error = ToolError;
    type Args = EditMessageArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "edit_message".into(),
            description: "Edit a previously sent message in the Telegram chat".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chat_id": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "message_id": {
                        "type": "integer",
                        "description": "ID of the message to edit"
                    },
                    "text": {
                        "type": "string",
                        "description": "New text for the message"
                    }
                },
                "required": ["chat_id", "message_id", "text"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.host
            .edit_message(ChatId(args.chat_id), args.message_id, &args.text)
            .await?;
        Ok(format!("Message {} edited", args.message_id))
    }
}

#[derive(Deserialize)]
pub struct DeleteMessageArgs {
    pub chat_id: i64,
    pub message_id: i32,
}

pub struct DeleteMessageTool {
    pub host: TelegramHost,
}

impl Tool for DeleteMessageTool {
    const NAME: &'static str = "delete_message";

    type Error = ToolError;
    type Args = DeleteMessageArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "delete_message".into(),
            description: "Delete a message from the Telegram chat".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chat_id": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "message_id": {
                        "type": "integer",
                        "description": "ID of the message to delete"
                    }
                },
                "required": ["chat_id", "message_id"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.host
            .delete_message(ChatId(args.chat_id), args.message_id)
            .await?;
        Ok(format!("Message {} deleted", args.message_id))
    }
}

#[derive(Deserialize)]
pub struct SetTypingArgs {
    pub chat_id: i64,
}

pub struct SetTypingTool {
    pub host: TelegramHost,
}

impl Tool for SetTypingTool {
    const NAME: &'static str = "set_typing";

    type Error = ToolError;
    type Args = SetTypingArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "set_typing".into(),
            description: "Show a typing indicator in the Telegram chat".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chat_id": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    }
                },
                "required": ["chat_id"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.host.set_typing(ChatId(args.chat_id)).await?;
        Ok(format!("Typing indicator set in chat {}", args.chat_id))
    }
}

#[derive(Deserialize)]
pub struct ResetTypingArgs {
    pub chat_id: i64,
}

pub struct ResetTypingTool {
    pub host: TelegramHost,
}

impl Tool for ResetTypingTool {
    const NAME: &'static str = "reset_typing";

    type Error = ToolError;
    type Args = ResetTypingArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "reset_typing".into(),
            description: "Remove the typing indicator from the Telegram chat".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chat_id": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    }
                },
                "required": ["chat_id"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        self.host.reset_typing(ChatId(args.chat_id)).await?;
        Ok(format!("Typing indicator reset in chat {}", args.chat_id))
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
    handle
        .add_tool(SetTypingTool { host: host.clone() })
        .await?;
    handle.add_tool(ResetTypingTool { host }).await?;
    Ok(())
}
