use crate::{ToolError, build_input_file};
use agent::{AgentError, AgentRuntime};
use rig_core::completion::ToolDefinition;
use rig_core::tool::Tool;
use serde::Deserialize;
use serde_json::json;
use telegram_host::TelegramHost;
use teloxide::payloads::{
    EditMessageCaptionSetters, EditMessageTextSetters, SetMessageReactionSetters,
};
use teloxide::prelude::Requester;
use teloxide::types::{
    ChatId, InputMedia, InputMediaAnimation, InputMediaAudio, InputMediaDocument, InputMediaPhoto,
    InputMediaVideo, MessageId, ParseMode, ReactionType,
};

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
                        "description": "Parse mode for the message text (HTML, MarkdownV2, etc.)"
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
        if let Some(pm) = args.parse_mode {
            req = req.parse_mode(pm);
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
pub struct DeleteMessagesArgs {
    pub chat_id: i64,
    pub message_ids: Vec<i32>,
}

pub struct DeleteMessagesTool {
    pub host: TelegramHost,
}

impl Tool for DeleteMessagesTool {
    const NAME: &'static str = "telegram_deleteMessages";

    type Error = ToolError;
    type Args = DeleteMessagesArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_deleteMessages".into(),
            description: "Delete multiple messages from the Telegram chat (1-100 messages)".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chatId": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "messageIds": {
                        "type": "array",
                        "items": {
                            "type": "integer"
                        },
                        "description": "Array of message IDs to delete (1-100)"
                    }
                },
                "required": ["chatId", "messageIds"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let n = args.message_ids.len();
        self.host
            .bot()
            .delete_messages(
                ChatId(args.chat_id),
                args.message_ids.iter().map(|&id| MessageId(id)),
            )
            .await?;
        Ok(format!("{} messages deleted from chat {}", n, args.chat_id))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditMessageCaptionArgs {
    pub chat_id: i64,
    pub message_id: i32,
    pub caption: Option<String>,
    pub parse_mode: Option<ParseMode>,
}

pub struct EditMessageCaptionTool {
    pub host: TelegramHost,
}

impl Tool for EditMessageCaptionTool {
    const NAME: &'static str = "telegram_editMessageCaption";

    type Error = ToolError;
    type Args = EditMessageCaptionArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_editMessageCaption".into(),
            description: "Edit the caption of a media message in the Telegram chat. Omit caption to remove it."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chatId": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "messageId": {
                        "type": "integer",
                        "description": "ID of the message"
                    },
                    "caption": {
                        "type": "string",
                        "description": "New caption (omit to remove caption)"
                    },
                    "parseMode": {
                        "type": "string",
                        "description": "Parse mode for the caption"
                    }
                },
                "required": ["chatId", "messageId"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut req = self
            .host
            .bot()
            .edit_message_caption(ChatId(args.chat_id), MessageId(args.message_id));
        if let Some(c) = args.caption {
            req = req.caption(c);
        }
        if let Some(pm) = args.parse_mode {
            req = req.parse_mode(pm);
        }
        req.await?;
        Ok(format!("Message {} caption edited", args.message_id))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MediaEditDef {
    #[serde(rename = "type")]
    media_type: String,
    file_id: Option<String>,
    url: Option<String>,
    caption: Option<String>,
    parse_mode: Option<ParseMode>,
    has_spoiler: Option<bool>,
    show_caption_above_media: Option<bool>,
}

fn convert_to_inputmedia(item: MediaEditDef) -> Result<InputMedia, ToolError> {
    let file = build_input_file(item.file_id, item.url)?;
    match item.media_type.as_str() {
        "photo" => {
            let mut p = InputMediaPhoto::new(file);
            if let Some(c) = item.caption {
                p = p.caption(c);
            }
            if let Some(pm) = item.parse_mode {
                p = p.parse_mode(pm);
            }
            if item.has_spoiler.unwrap_or(false) {
                p = p.spoiler();
            }
            if let Some(val) = item.show_caption_above_media {
                p = p.show_caption_above_media(val);
            }
            Ok(InputMedia::Photo(p))
        }
        "video" => {
            let mut v = InputMediaVideo::new(file);
            if let Some(c) = item.caption {
                v = v.caption(c);
            }
            if let Some(pm) = item.parse_mode {
                v = v.parse_mode(pm);
            }
            if item.has_spoiler.unwrap_or(false) {
                v = v.spoiler();
            }
            if let Some(val) = item.show_caption_above_media {
                v = v.show_caption_above_media(val);
            }
            Ok(InputMedia::Video(v))
        }
        "animation" => {
            let mut a = InputMediaAnimation::new(file);
            if let Some(c) = item.caption {
                a = a.caption(c);
            }
            if let Some(pm) = item.parse_mode {
                a = a.parse_mode(pm);
            }
            if item.has_spoiler.unwrap_or(false) {
                a = a.spoiler();
            }
            if let Some(val) = item.show_caption_above_media {
                a = a.show_caption_above_media(val);
            }
            Ok(InputMedia::Animation(a))
        }
        "audio" => {
            let mut a = InputMediaAudio::new(file);
            if let Some(c) = item.caption {
                a = a.caption(c);
            }
            if let Some(pm) = item.parse_mode {
                a = a.parse_mode(pm);
            }
            Ok(InputMedia::Audio(a))
        }
        "document" => {
            let mut d = InputMediaDocument::new(file);
            if let Some(c) = item.caption {
                d = d.caption(c);
            }
            if let Some(pm) = item.parse_mode {
                d = d.parse_mode(pm);
            }
            Ok(InputMedia::Document(d))
        }
        _ => Err(ToolError::MissingFileSource),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditMessageMediaArgs {
    pub chat_id: i64,
    pub message_id: i32,
    pub media: String,
}

pub struct EditMessageMediaTool {
    pub host: TelegramHost,
}

impl Tool for EditMessageMediaTool {
    const NAME: &'static str = "telegram_editMessageMedia";

    type Error = ToolError;
    type Args = EditMessageMediaArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_editMessageMedia".into(),
            description: "Replace the media in a message. The media parameter is a JSON object with fields: type (photo/video/animation/audio/document), fileId, url, caption, parseMode, hasSpoiler, showCaptionAboveMedia."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chatId": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "messageId": {
                        "type": "integer",
                        "description": "ID of the message"
                    },
                    "media": {
                        "type": "string",
                        "description": "JSON object with fields: type (photo/video/animation/audio/document), fileId, url, caption, parseMode, hasSpoiler, showCaptionAboveMedia"
                    }
                },
                "required": ["chatId", "messageId", "media"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let def: MediaEditDef = serde_json::from_str(&args.media)?;
        let media = convert_to_inputmedia(def)?;
        self.host
            .bot()
            .edit_message_media(ChatId(args.chat_id), MessageId(args.message_id), media)
            .await?;
        Ok(format!("Message {} media edited", args.message_id))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetMessageReactionArgs {
    pub chat_id: i64,
    pub message_id: i32,
    pub reaction: Option<String>,
    pub is_big: Option<bool>,
}

pub struct SetMessageReactionTool {
    pub host: TelegramHost,
}

impl Tool for SetMessageReactionTool {
    const NAME: &'static str = "telegram_setMessageReaction";

    type Error = ToolError;
    type Args = SetMessageReactionArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_setMessageReaction".into(),
            description: "Set a reaction on a message. The reaction parameter is a JSON array of ReactionType objects, e.g. [{\"type\":\"emoji\",\"emoji\":\"👍\"}]. Pass null/empty to remove reactions."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chatId": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "messageId": {
                        "type": "integer",
                        "description": "ID of the message"
                    },
                    "reaction": {
                        "type": "string",
                        "description": "JSON array of ReactionType objects, e.g. [{\"type\":\"emoji\",\"emoji\":\"👍\"}]. Pass null/empty to remove."
                    },
                    "isBig": {
                        "type": "boolean",
                        "description": "Whether to use a big reaction"
                    }
                },
                "required": ["chatId", "messageId"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut req = self
            .host
            .bot()
            .set_message_reaction(ChatId(args.chat_id), MessageId(args.message_id));
        if let Some(r) = args.reaction.filter(|s| !s.is_empty()) {
            let reactions: Vec<ReactionType> = serde_json::from_str(&r)?;
            req = req.reaction(reactions);
        }
        if let Some(b) = args.is_big {
            req = req.is_big(b);
        }
        req.await?;
        Ok(format!("Reaction set on message {}", args.message_id))
    }
}

pub async fn register_edit_tools(
    runtime: &AgentRuntime,
    host: TelegramHost,
) -> Result<(), AgentError> {
    runtime
        .register_tool(EditMessageTool { host: host.clone() })
        .await?;
    runtime
        .register_tool(DeleteMessageTool { host: host.clone() })
        .await?;
    runtime
        .register_tool(DeleteMessagesTool { host: host.clone() })
        .await?;
    runtime
        .register_tool(EditMessageCaptionTool { host: host.clone() })
        .await?;
    runtime
        .register_tool(EditMessageMediaTool { host: host.clone() })
        .await?;
    runtime
        .register_tool(SetMessageReactionTool { host })
        .await?;
    Ok(())
}
