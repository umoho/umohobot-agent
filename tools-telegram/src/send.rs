use crate::{ToolError, build_input_file, parse_dice_emoji};
use rig_core::completion::ToolDefinition;
use rig_core::tool::Tool;
use rig_core::tool::server::ToolServerHandle;
use serde::Deserialize;
use serde_json::json;
use telegram_host::TelegramHost;
use teloxide::payloads::{
    SendAnimationSetters, SendAudioSetters, SendDiceSetters, SendDocumentSetters,
    SendMediaGroupSetters, SendMessageSetters, SendPhotoSetters, SendPollSetters,
    SendStickerSetters, SendVideoSetters, SendVoiceSetters,
};
use teloxide::prelude::Requester;
use teloxide::types::{
    ChatAction, ChatId, InputMedia, InputMediaPhoto, InputMediaVideo, InputPollOption, MessageId,
    ParseMode, PollType, ReplyParameters,
};

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
            description: "Send a text message to a Telegram chat".into(),
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
                        "description": "Optional message ID to reply to"
                    },
                    "parseMode": {
                        "type": "string",
                        "description": "Parse mode: MarkdownV2, HTML, or Markdown"
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
            .send_message(ChatId(args.chat_id), args.text);
        if let Some(rm) = args.reply_to_message_id {
            req = req.reply_parameters(ReplyParameters::new(MessageId(rm)));
        }
        if let Some(pm) = args.parse_mode {
            req = req.parse_mode(pm);
        }
        req.await?;
        Ok(format!("Message sent to chat {}", args.chat_id))
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
            description: "Broadcast a chat action (typing, uploading, etc.) in a Telegram chat"
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
                        "description": "Type of action to broadcast",
                        "enum": ["typing", "upload_photo", "record_video", "upload_video", "record_voice", "upload_voice", "upload_document", "find_location", "record_video_note", "upload_video_note"]
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
pub struct SendPhotoArgs {
    pub chat_id: i64,
    pub file_id: Option<String>,
    pub url: Option<String>,
    pub caption: Option<String>,
    pub parse_mode: Option<ParseMode>,
    pub has_spoiler: Option<bool>,
    pub show_caption_above_media: Option<bool>,
    pub reply_to_message_id: Option<i32>,
}

pub struct SendPhotoTool {
    pub host: TelegramHost,
}

impl Tool for SendPhotoTool {
    const NAME: &'static str = "telegram_sendPhoto";

    type Error = ToolError;
    type Args = SendPhotoArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_sendPhoto".into(),
            description:
                "Send a photo to the Telegram chat. Either fileId or url must be provided.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chatId": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "fileId": {
                        "type": "string",
                        "description": "Telegram file ID of an existing photo"
                    },
                    "url": {
                        "type": "string",
                        "description": "URL of the photo to send"
                    },
                    "caption": {
                        "type": "string",
                        "description": "Photo caption"
                    },
                    "parseMode": {
                        "type": "string",
                        "description": "Parse mode: MarkdownV2, HTML, or Markdown"
                    },
                    "hasSpoiler": {
                        "type": "boolean",
                        "description": "Pass True if the photo needs to be covered with a spoiler"
                    },
                    "showCaptionAboveMedia": {
                        "type": "boolean",
                        "description": "Pass True if the caption must be shown above the media"
                    },
                    "replyToMessageId": {
                        "type": "integer",
                        "description": "Optional message ID to reply to"
                    }
                },
                "required": ["chatId"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let input_file = build_input_file(args.file_id, args.url)?;
        let mut req = self.host.bot().send_photo(ChatId(args.chat_id), input_file);
        if let Some(c) = args.caption {
            req = req.caption(c);
        }
        if let Some(pm) = args.parse_mode {
            req = req.parse_mode(pm);
        }
        if args.has_spoiler.unwrap_or(false) {
            req = req.has_spoiler(true);
        }
        if args.show_caption_above_media.unwrap_or(false) {
            req = req.show_caption_above_media(true);
        }
        if let Some(rm) = args.reply_to_message_id {
            req = req.reply_parameters(ReplyParameters::new(MessageId(rm)));
        }
        req.await?;
        Ok(format!("Photo sent to chat {}", args.chat_id))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendVideoArgs {
    pub chat_id: i64,
    pub file_id: Option<String>,
    pub url: Option<String>,
    pub caption: Option<String>,
    pub parse_mode: Option<ParseMode>,
    pub has_spoiler: Option<bool>,
    pub show_caption_above_media: Option<bool>,
    pub reply_to_message_id: Option<i32>,
}

pub struct SendVideoTool {
    pub host: TelegramHost,
}

impl Tool for SendVideoTool {
    const NAME: &'static str = "telegram_sendVideo";

    type Error = ToolError;
    type Args = SendVideoArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_sendVideo".into(),
            description:
                "Send a video to the Telegram chat. Either fileId or url must be provided.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chatId": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "fileId": {
                        "type": "string",
                        "description": "Telegram file ID of an existing video"
                    },
                    "url": {
                        "type": "string",
                        "description": "URL of the video to send"
                    },
                    "caption": {
                        "type": "string",
                        "description": "Video caption"
                    },
                    "parseMode": {
                        "type": "string",
                        "description": "Parse mode: MarkdownV2, HTML, or Markdown"
                    },
                    "hasSpoiler": {
                        "type": "boolean",
                        "description": "Pass True if the video needs to be covered with a spoiler"
                    },
                    "showCaptionAboveMedia": {
                        "type": "boolean",
                        "description": "Pass True if the caption must be shown above the media"
                    },
                    "replyToMessageId": {
                        "type": "integer",
                        "description": "Optional message ID to reply to"
                    }
                },
                "required": ["chatId"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let input_file = build_input_file(args.file_id, args.url)?;
        let mut req = self.host.bot().send_video(ChatId(args.chat_id), input_file);
        if let Some(c) = args.caption {
            req = req.caption(c);
        }
        if let Some(pm) = args.parse_mode {
            req = req.parse_mode(pm);
        }
        if args.has_spoiler.unwrap_or(false) {
            req = req.has_spoiler(true);
        }
        if args.show_caption_above_media.unwrap_or(false) {
            req = req.show_caption_above_media(true);
        }
        if let Some(rm) = args.reply_to_message_id {
            req = req.reply_parameters(ReplyParameters::new(MessageId(rm)));
        }
        req.await?;
        Ok(format!("Video sent to chat {}", args.chat_id))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendAudioArgs {
    pub chat_id: i64,
    pub file_id: Option<String>,
    pub url: Option<String>,
    pub caption: Option<String>,
    pub parse_mode: Option<ParseMode>,
    pub reply_to_message_id: Option<i32>,
}

pub struct SendAudioTool {
    pub host: TelegramHost,
}

impl Tool for SendAudioTool {
    const NAME: &'static str = "telegram_sendAudio";

    type Error = ToolError;
    type Args = SendAudioArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_sendAudio".into(),
            description: "Send an audio file to the Telegram chat".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chatId": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "fileId": {
                        "type": "string",
                        "description": "Telegram file ID of an existing audio file"
                    },
                    "url": {
                        "type": "string",
                        "description": "URL of the audio file to send"
                    },
                    "caption": {
                        "type": "string",
                        "description": "Audio caption"
                    },
                    "parseMode": {
                        "type": "string",
                        "description": "Parse mode: MarkdownV2, HTML, or Markdown"
                    },
                    "replyToMessageId": {
                        "type": "integer",
                        "description": "Optional message ID to reply to"
                    }
                },
                "required": ["chatId"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let input_file = build_input_file(args.file_id, args.url)?;
        let mut req = self.host.bot().send_audio(ChatId(args.chat_id), input_file);
        if let Some(c) = args.caption {
            req = req.caption(c);
        }
        if let Some(pm) = args.parse_mode {
            req = req.parse_mode(pm);
        }
        if let Some(rm) = args.reply_to_message_id {
            req = req.reply_parameters(ReplyParameters::new(MessageId(rm)));
        }
        req.await?;
        Ok(format!("Audio sent to chat {}", args.chat_id))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendDocumentArgs {
    pub chat_id: i64,
    pub file_id: Option<String>,
    pub url: Option<String>,
    pub caption: Option<String>,
    pub parse_mode: Option<ParseMode>,
    pub reply_to_message_id: Option<i32>,
}

pub struct SendDocumentTool {
    pub host: TelegramHost,
}

impl Tool for SendDocumentTool {
    const NAME: &'static str = "telegram_sendDocument";

    type Error = ToolError;
    type Args = SendDocumentArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_sendDocument".into(),
            description: "Send a file/document to the Telegram chat".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chatId": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "fileId": {
                        "type": "string",
                        "description": "Telegram file ID of an existing file"
                    },
                    "url": {
                        "type": "string",
                        "description": "URL of the file to send"
                    },
                    "caption": {
                        "type": "string",
                        "description": "Document caption"
                    },
                    "parseMode": {
                        "type": "string",
                        "description": "Parse mode: MarkdownV2, HTML, or Markdown"
                    },
                    "replyToMessageId": {
                        "type": "integer",
                        "description": "Optional message ID to reply to"
                    }
                },
                "required": ["chatId"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let input_file = build_input_file(args.file_id, args.url)?;
        let mut req = self
            .host
            .bot()
            .send_document(ChatId(args.chat_id), input_file);
        if let Some(c) = args.caption {
            req = req.caption(c);
        }
        if let Some(pm) = args.parse_mode {
            req = req.parse_mode(pm);
        }
        if let Some(rm) = args.reply_to_message_id {
            req = req.reply_parameters(ReplyParameters::new(MessageId(rm)));
        }
        req.await?;
        Ok(format!("Document sent to chat {}", args.chat_id))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendAnimationArgs {
    pub chat_id: i64,
    pub file_id: Option<String>,
    pub url: Option<String>,
    pub caption: Option<String>,
    pub parse_mode: Option<ParseMode>,
    pub has_spoiler: Option<bool>,
    pub show_caption_above_media: Option<bool>,
    pub reply_to_message_id: Option<i32>,
}

pub struct SendAnimationTool {
    pub host: TelegramHost,
}

impl Tool for SendAnimationTool {
    const NAME: &'static str = "telegram_sendAnimation";

    type Error = ToolError;
    type Args = SendAnimationArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_sendAnimation".into(),
            description: "Send an animation (GIF) to the Telegram chat".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chatId": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "fileId": {
                        "type": "string",
                        "description": "Telegram file ID of an existing animation"
                    },
                    "url": {
                        "type": "string",
                        "description": "URL of the animation to send"
                    },
                    "caption": {
                        "type": "string",
                        "description": "Animation caption"
                    },
                    "parseMode": {
                        "type": "string",
                        "description": "Parse mode: MarkdownV2, HTML, or Markdown"
                    },
                    "hasSpoiler": {
                        "type": "boolean",
                        "description": "Pass True if the animation needs to be covered with a spoiler"
                    },
                    "showCaptionAboveMedia": {
                        "type": "boolean",
                        "description": "Pass True if the caption must be shown above the media"
                    },
                    "replyToMessageId": {
                        "type": "integer",
                        "description": "Optional message ID to reply to"
                    }
                },
                "required": ["chatId"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let input_file = build_input_file(args.file_id, args.url)?;
        let mut req = self
            .host
            .bot()
            .send_animation(ChatId(args.chat_id), input_file);
        if let Some(c) = args.caption {
            req = req.caption(c);
        }
        if let Some(pm) = args.parse_mode {
            req = req.parse_mode(pm);
        }
        if args.has_spoiler.unwrap_or(false) {
            req = req.has_spoiler(true);
        }
        if args.show_caption_above_media.unwrap_or(false) {
            req = req.show_caption_above_media(true);
        }
        if let Some(rm) = args.reply_to_message_id {
            req = req.reply_parameters(ReplyParameters::new(MessageId(rm)));
        }
        req.await?;
        Ok(format!("Animation sent to chat {}", args.chat_id))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendVoiceArgs {
    pub chat_id: i64,
    pub file_id: Option<String>,
    pub url: Option<String>,
    pub caption: Option<String>,
    pub parse_mode: Option<ParseMode>,
    pub duration: Option<u32>,
    pub reply_to_message_id: Option<i32>,
}

pub struct SendVoiceTool {
    pub host: TelegramHost,
}

impl Tool for SendVoiceTool {
    const NAME: &'static str = "telegram_sendVoice";

    type Error = ToolError;
    type Args = SendVoiceArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_sendVoice".into(),
            description: "Send a voice message to the Telegram chat".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chatId": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "fileId": {
                        "type": "string",
                        "description": "Telegram file ID of an existing voice message"
                    },
                    "url": {
                        "type": "string",
                        "description": "URL of the voice message to send"
                    },
                    "caption": {
                        "type": "string",
                        "description": "Voice message caption"
                    },
                    "parseMode": {
                        "type": "string",
                        "description": "Parse mode: MarkdownV2, HTML, or Markdown"
                    },
                    "duration": {
                        "type": "integer",
                        "description": "Duration of the voice message in seconds"
                    },
                    "replyToMessageId": {
                        "type": "integer",
                        "description": "Optional message ID to reply to"
                    }
                },
                "required": ["chatId"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let input_file = build_input_file(args.file_id, args.url)?;
        let mut req = self.host.bot().send_voice(ChatId(args.chat_id), input_file);
        if let Some(c) = args.caption {
            req = req.caption(c);
        }
        if let Some(pm) = args.parse_mode {
            req = req.parse_mode(pm);
        }
        if let Some(d) = args.duration {
            req = req.duration(d);
        }
        if let Some(rm) = args.reply_to_message_id {
            req = req.reply_parameters(ReplyParameters::new(MessageId(rm)));
        }
        req.await?;
        Ok(format!("Voice sent to chat {}", args.chat_id))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendStickerArgs {
    pub chat_id: i64,
    pub file_id: Option<String>,
    pub url: Option<String>,
    pub emoji: Option<String>,
    pub reply_to_message_id: Option<i32>,
}

pub struct SendStickerTool {
    pub host: TelegramHost,
}

impl Tool for SendStickerTool {
    const NAME: &'static str = "telegram_sendSticker";

    type Error = ToolError;
    type Args = SendStickerArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_sendSticker".into(),
            description: "Send a sticker to the Telegram chat".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chatId": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "fileId": {
                        "type": "string",
                        "description": "Telegram file ID of an existing sticker"
                    },
                    "url": {
                        "type": "string",
                        "description": "URL of the sticker to send"
                    },
                    "emoji": {
                        "type": "string",
                        "description": "Emoji associated with the sticker"
                    },
                    "replyToMessageId": {
                        "type": "integer",
                        "description": "Optional message ID to reply to"
                    }
                },
                "required": ["chatId"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let input_file = build_input_file(args.file_id, args.url)?;
        let mut req = self
            .host
            .bot()
            .send_sticker(ChatId(args.chat_id), input_file);
        if let Some(e) = args.emoji {
            req = req.emoji(e);
        }
        if let Some(rm) = args.reply_to_message_id {
            req = req.reply_parameters(ReplyParameters::new(MessageId(rm)));
        }
        req.await?;
        Ok(format!("Sticker sent to chat {}", args.chat_id))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendDiceArgs {
    pub chat_id: i64,
    pub emoji: Option<String>,
    pub reply_to_message_id: Option<i32>,
}

pub struct SendDiceTool {
    pub host: TelegramHost,
}

impl Tool for SendDiceTool {
    const NAME: &'static str = "telegram_sendDice";

    type Error = ToolError;
    type Args = SendDiceArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_sendDice".into(),
            description: "Send a dice with random value. Supported emoji values: dice(🎲), darts(🎯), bowling(🎳), basketball(🏀), football(⚽), slot_machine(🎰)".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chatId": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "emoji": {
                        "type": "string",
                        "description": "Emoji for the dice: dice, darts, bowling, basketball, football, slot_machine"
                    },
                    "replyToMessageId": {
                        "type": "integer",
                        "description": "Optional message ID to reply to"
                    }
                },
                "required": ["chatId"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut req = self.host.bot().send_dice(ChatId(args.chat_id));
        if let Some(e) = args.emoji {
            if let Some(de) = parse_dice_emoji(&e) {
                req = req.emoji(de);
            }
        }
        if let Some(rm) = args.reply_to_message_id {
            req = req.reply_parameters(ReplyParameters::new(MessageId(rm)));
        }
        req.await?;
        Ok(format!("Dice sent to chat {}", args.chat_id))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendPollArgs {
    pub chat_id: i64,
    pub question: String,
    pub options: Vec<InputPollOption>,
    pub is_anonymous: Option<bool>,
    pub poll_type: Option<String>,
    pub allows_multiple_answers: Option<bool>,
    pub correct_option_id: Option<u8>,
    pub explanation: Option<String>,
    pub explanation_parse_mode: Option<ParseMode>,
    pub open_period: Option<u16>,
    pub is_closed: Option<bool>,
    pub reply_to_message_id: Option<i32>,
}

pub struct SendPollTool {
    pub host: TelegramHost,
}

impl Tool for SendPollTool {
    const NAME: &'static str = "telegram_sendPoll";

    type Error = ToolError;
    type Args = SendPollArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_sendPoll".into(),
            description: "Send a poll to the Telegram chat".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chatId": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "question": {
                        "type": "string",
                        "description": "Poll question, 1-300 characters"
                    },
                    "options": {
                        "type": "array",
                        "items": { "type": "object" },
                        "description": "List of answer options"
                    },
                    "isAnonymous": {
                        "type": "boolean",
                        "description": "True if the poll needs to be anonymous, defaults to True"
                    },
                    "pollType": {
                        "type": "string",
                        "description": "Poll type: quiz or regular"
                    },
                    "allowsMultipleAnswers": {
                        "type": "boolean",
                        "description": "True if the poll allows multiple answers"
                    },
                    "correctOptionId": {
                        "type": "integer",
                        "description": "0-based identifier of the correct answer option, required for quiz"
                    },
                    "explanation": {
                        "type": "string",
                        "description": "Text shown when user chooses an incorrect answer in quiz"
                    },
                    "explanationParseMode": {
                        "type": "string",
                        "description": "Parse mode for explanation: MarkdownV2, HTML, or Markdown"
                    },
                    "openPeriod": {
                        "type": "integer",
                        "description": "Amount of time in seconds the poll will be active, 5-600"
                    },
                    "isClosed": {
                        "type": "boolean",
                        "description": "Pass True if the poll needs to be immediately closed"
                    },
                    "replyToMessageId": {
                        "type": "integer",
                        "description": "Optional message ID to reply to"
                    }
                },
                "required": ["chatId", "question", "options"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let mut req = self
            .host
            .bot()
            .send_poll(ChatId(args.chat_id), args.question, args.options);
        if let Some(a) = args.is_anonymous {
            req = req.is_anonymous(a);
        }
        if let Some(pt) = args.poll_type {
            match pt.to_lowercase().as_str() {
                "quiz" => req = req.type_(PollType::Quiz),
                "regular" => req = req.type_(PollType::Regular),
                _ => {}
            }
        }
        if args.allows_multiple_answers.unwrap_or(false) {
            req = req.allows_multiple_answers(true);
        }
        if let Some(co) = args.correct_option_id {
            req = req.correct_option_id(co);
        }
        if let Some(e) = args.explanation {
            req = req.explanation(e);
        }
        if let Some(epm) = args.explanation_parse_mode {
            req = req.explanation_parse_mode(epm);
        }
        if let Some(op) = args.open_period {
            req = req.open_period(op);
        }
        if args.is_closed.unwrap_or(false) {
            req = req.is_closed(true);
        }
        if let Some(rm) = args.reply_to_message_id {
            req = req.reply_parameters(ReplyParameters::new(MessageId(rm)));
        }
        req.await?;
        Ok(format!("Poll sent to chat {}", args.chat_id))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MediaGroupItem {
    #[serde(rename = "type")]
    item_type: String,
    file_id: Option<String>,
    url: Option<String>,
    caption: Option<String>,
    parse_mode: Option<ParseMode>,
    has_spoiler: Option<bool>,
    show_caption_above_media: Option<bool>,
}

fn convert_to_inputmedia(item: MediaGroupItem) -> Result<InputMedia, ToolError> {
    let file = build_input_file(item.file_id, item.url)?;
    match item.item_type.as_str() {
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
            if item.show_caption_above_media.unwrap_or(false) {
                p = p.show_caption_above_media(true);
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
            if item.show_caption_above_media.unwrap_or(false) {
                v = v.show_caption_above_media(true);
            }
            Ok(InputMedia::Video(v))
        }
        _ => Err(ToolError::MissingFileSource),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMediaGroupArgs {
    pub chat_id: i64,
    pub media: String,
    pub reply_to_message_id: Option<i32>,
}

pub struct SendMediaGroupTool {
    pub host: TelegramHost,
}

impl Tool for SendMediaGroupTool {
    const NAME: &'static str = "telegram_sendMediaGroup";

    type Error = ToolError;
    type Args = SendMediaGroupArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "telegram_sendMediaGroup".into(),
            description: "Send a media group (album) to the Telegram chat. The media parameter is a JSON array of objects with fields: type (photo/video), fileId, url, caption, parseMode, hasSpoiler, showCaptionAboveMedia.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "chatId": {
                        "type": "integer",
                        "description": "Telegram chat ID"
                    },
                    "media": {
                        "type": "string",
                        "description": "JSON array of media items"
                    },
                    "replyToMessageId": {
                        "type": "integer",
                        "description": "Optional message ID to reply to"
                    }
                },
                "required": ["chatId", "media"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let items: Vec<MediaGroupItem> = serde_json::from_str(&args.media)?;
        let media_items: Result<Vec<InputMedia>, ToolError> =
            items.into_iter().map(convert_to_inputmedia).collect();
        let media_items = media_items?;
        let mut req = self
            .host
            .bot()
            .send_media_group(ChatId(args.chat_id), media_items);
        if let Some(rm) = args.reply_to_message_id {
            req = req.reply_parameters(ReplyParameters::new(MessageId(rm)));
        }
        req.await?;
        Ok(format!("Media group sent to chat {}", args.chat_id))
    }
}

pub async fn register_send_tools(
    handle: &ToolServerHandle,
    host: TelegramHost,
) -> Result<(), rig_core::tool::server::ToolServerError> {
    handle
        .add_tool(SendMessageTool { host: host.clone() })
        .await?;
    handle
        .add_tool(SendChatActionTool { host: host.clone() })
        .await?;
    handle
        .add_tool(SendPhotoTool { host: host.clone() })
        .await?;
    handle
        .add_tool(SendVideoTool { host: host.clone() })
        .await?;
    handle
        .add_tool(SendAudioTool { host: host.clone() })
        .await?;
    handle
        .add_tool(SendDocumentTool { host: host.clone() })
        .await?;
    handle
        .add_tool(SendAnimationTool { host: host.clone() })
        .await?;
    handle
        .add_tool(SendVoiceTool { host: host.clone() })
        .await?;
    handle
        .add_tool(SendStickerTool { host: host.clone() })
        .await?;
    handle.add_tool(SendDiceTool { host: host.clone() }).await?;
    handle.add_tool(SendPollTool { host: host.clone() }).await?;
    handle.add_tool(SendMediaGroupTool { host }).await?;
    Ok(())
}
