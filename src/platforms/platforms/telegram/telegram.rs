use crate::{app::ReplyPlan, config::Config};

use super::super::{
    AttachmentInfo, AttachmentKind, MessageBody, MessageEntityInfo, PlatformKind, PlatformMessage,
    PlatformMessageKind, ReplyHandle, ReplyMetadata,
};
use teloxide::types::{
    Document, Message, MessageEntityKind, MessageEntityRef, PhotoSize, Sticker, Voice,
};

#[derive(Clone, Debug)]
pub struct TelegramRuntimeConfig {
    pub bot_name: String,
    pub placeholder_text: String,
    pub message_edit_throttle_ms: u64,
}

impl TelegramRuntimeConfig {
    pub fn from_config(config: &Config) -> Self {
        Self {
            bot_name: config.bot_name.clone(),
            placeholder_text: config.placeholder_text.clone(),
            message_edit_throttle_ms: config.message_edit_throttle_ms,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TelegramRuntime {
    config: TelegramRuntimeConfig,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TelegramReplyStep {
    SendPlaceholder {
        room_id: String,
        text: String,
    },
    EditPlaceholder {
        room_id: String,
        text: String,
        throttle_ms: u64,
    },
}

#[derive(Clone, Debug)]
pub struct TelegramReplyScript {
    pub platform: PlatformKind,
    pub room_id: String,
    pub thread_id: Option<String>,
    pub placeholder_text: String,
    pub final_text: String,
    pub edit_in_place: bool,
    pub edit_throttle_ms: u64,
    pub steps: Vec<TelegramReplyStep>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramInboundMessage {
    pub room_id: String,
    pub thread_id: Option<String>,
    pub message_id: String,
    pub sender_id: String,
    pub kind: PlatformMessageKind,
    pub body: MessageBody,
    pub attachments: Vec<AttachmentInfo>,
    pub reply: Option<ReplyMetadata>,
    pub is_mention: bool,
}

impl TelegramRuntime {
    pub fn new(config: TelegramRuntimeConfig) -> Self {
        Self { config }
    }

    pub fn from_config(config: &Config) -> Self {
        Self::new(TelegramRuntimeConfig::from_config(config))
    }

    pub fn describe(&self) -> String {
        format!(
            "telegram(bot={}, edit_throttle_ms={})",
            self.config.bot_name, self.config.message_edit_throttle_ms
        )
    }

    pub fn supports_placeholder_edit_flow(&self) -> bool {
        true
    }

    pub fn placeholder_text(&self) -> &str {
        &self.config.placeholder_text
    }

    pub fn inbound_from_message(&self, message: &Message) -> TelegramInboundMessage {
        let snapshot = snapshot_message(message);
        let is_mention = body_mentions_bot(&snapshot.body, &self.config.bot_name);

        TelegramInboundMessage {
            room_id: message.chat.id.to_string(),
            thread_id: message.thread_id.map(|thread_id| thread_id.0.to_string()),
            message_id: message.id.to_string(),
            sender_id: snapshot.sender_id,
            kind: snapshot.kind,
            body: snapshot.body,
            attachments: snapshot.attachments,
            reply: message.reply_to_message().map(reply_metadata_from_message),
            is_mention,
        }
    }

    pub fn normalize_inbound(&self, inbound: TelegramInboundMessage) -> PlatformMessage {
        inbound.into()
    }

    pub fn build_reply_script(&self, plan: &ReplyPlan) -> TelegramReplyScript {
        let placeholder_text = if plan.placeholder_text.is_empty() {
            self.config.placeholder_text.clone()
        } else {
            plan.placeholder_text.clone()
        };

        TelegramReplyScript {
            platform: plan.platform.clone(),
            room_id: plan.room_id.clone(),
            thread_id: plan.thread_id.clone(),
            placeholder_text: placeholder_text.clone(),
            final_text: plan.final_text.clone(),
            edit_in_place: true,
            edit_throttle_ms: self.config.message_edit_throttle_ms,
            steps: vec![
                TelegramReplyStep::SendPlaceholder {
                    room_id: plan.room_id.clone(),
                    text: placeholder_text,
                },
                TelegramReplyStep::EditPlaceholder {
                    room_id: plan.room_id.clone(),
                    text: plan.final_text.clone(),
                    throttle_ms: self.config.message_edit_throttle_ms,
                },
            ],
        }
    }

    pub fn reply_handle(&self, room_id: String, message_id: String) -> ReplyHandle {
        ReplyHandle {
            platform: PlatformKind::Telegram,
            room_id,
            message_id,
        }
    }
}

impl From<TelegramInboundMessage> for PlatformMessage {
    fn from(inbound: TelegramInboundMessage) -> Self {
        Self {
            platform: PlatformKind::Telegram,
            room_id: inbound.room_id,
            thread_id: inbound.thread_id,
            message_id: inbound.message_id,
            sender_id: inbound.sender_id,
            kind: inbound.kind,
            body: inbound.body,
            attachments: inbound.attachments,
            reply: inbound.reply,
            is_mention: inbound.is_mention,
        }
    }
}

#[derive(Clone, Debug)]
struct MessageSnapshot {
    kind: PlatformMessageKind,
    body: MessageBody,
    attachments: Vec<AttachmentInfo>,
    sender_id: String,
}

fn snapshot_message(message: &Message) -> MessageSnapshot {
    MessageSnapshot {
        kind: classify_message_kind(message),
        body: body_from_message(message),
        attachments: attachments_from_message(message),
        sender_id: sender_id_from_message(message),
    }
}

fn classify_message_kind(message: &Message) -> PlatformMessageKind {
    if message.text().is_some() {
        PlatformMessageKind::Text
    } else if message.photo().is_some() {
        PlatformMessageKind::Photo
    } else if message.sticker().is_some() {
        PlatformMessageKind::Sticker
    } else if message.document().is_some() {
        PlatformMessageKind::File
    } else if message.voice().is_some() {
        PlatformMessageKind::Voice
    } else if message.audio().is_some()
        || message.animation().is_some()
        || message.video().is_some()
        || message.video_note().is_some()
        || message.dice().is_some()
        || message.poll().is_some()
        || message.checklist().is_some()
    {
        PlatformMessageKind::Media
    } else if is_service_message(message) {
        PlatformMessageKind::Service
    } else {
        PlatformMessageKind::Unknown
    }
}

fn body_from_message(message: &Message) -> MessageBody {
    if let Some(text) = message.text() {
        MessageBody::Text {
            text: text.to_string(),
            entities: parsed_entities(message.parse_entities()),
        }
    } else if let Some(caption) = message.caption() {
        MessageBody::Caption {
            text: caption.to_string(),
            entities: parsed_entities(message.parse_caption_entities()),
        }
    } else {
        MessageBody::Empty
    }
}

fn parsed_entities(entities: Option<Vec<MessageEntityRef<'_>>>) -> Vec<MessageEntityInfo> {
    entities
        .unwrap_or_default()
        .into_iter()
        .map(|entity| MessageEntityInfo {
            kind: entity.kind().clone(),
            text: entity.text().to_string(),
        })
        .collect()
}

fn attachments_from_message(message: &Message) -> Vec<AttachmentInfo> {
    let mut attachments = Vec::new();

    if let Some(photo_sizes) = message.photo() {
        if let Some(photo) = photo_sizes
            .iter()
            .max_by_key(|photo| photo.width * photo.height)
        {
            attachments.push(attachment_from_photo(photo));
        }
    }

    if let Some(sticker) = message.sticker() {
        attachments.push(attachment_from_sticker(sticker));
    }

    if let Some(document) = message.document() {
        attachments.push(attachment_from_document(document));
    }

    if let Some(voice) = message.voice() {
        attachments.push(attachment_from_voice(voice));
    }

    attachments
}

fn attachment_from_photo(photo: &PhotoSize) -> AttachmentInfo {
    AttachmentInfo {
        kind: AttachmentKind::Image,
        file_id: Some(photo.file.id.to_string()),
        file_unique_id: Some(photo.file.unique_id.to_string()),
        file_name: None,
        mime_type: None,
        url: None,
        width: Some(photo.width),
        height: Some(photo.height),
        size_bytes: Some(photo.file.size),
    }
}

fn attachment_from_sticker(sticker: &Sticker) -> AttachmentInfo {
    AttachmentInfo {
        kind: AttachmentKind::Sticker,
        file_id: Some(sticker.file.id.to_string()),
        file_unique_id: Some(sticker.file.unique_id.to_string()),
        file_name: None,
        mime_type: None,
        url: None,
        width: Some(sticker.width.into()),
        height: Some(sticker.height.into()),
        size_bytes: Some(sticker.file.size),
    }
}

fn attachment_from_document(document: &Document) -> AttachmentInfo {
    AttachmentInfo {
        kind: AttachmentKind::File,
        file_id: Some(document.file.id.to_string()),
        file_unique_id: Some(document.file.unique_id.to_string()),
        file_name: document.file_name.clone(),
        mime_type: document.mime_type.as_ref().map(ToString::to_string),
        url: None,
        width: None,
        height: None,
        size_bytes: Some(document.file.size),
    }
}

fn attachment_from_voice(voice: &Voice) -> AttachmentInfo {
    AttachmentInfo {
        kind: AttachmentKind::Voice,
        file_id: Some(voice.file.id.to_string()),
        file_unique_id: Some(voice.file.unique_id.to_string()),
        file_name: None,
        mime_type: voice.mime_type.as_ref().map(ToString::to_string),
        url: None,
        width: None,
        height: None,
        size_bytes: Some(voice.file.size),
    }
}

fn reply_metadata_from_message(message: &Message) -> ReplyMetadata {
    let snapshot = snapshot_message(message);

    ReplyMetadata {
        message_id: message.id.to_string(),
        sender_id: snapshot.sender_id,
        kind: snapshot.kind,
        body: snapshot.body,
        attachments: snapshot.attachments,
    }
}

fn sender_id_from_message(message: &Message) -> String {
    message
        .from
        .as_ref()
        .map(|user| user.id.to_string())
        .or_else(|| message.sender_chat.as_ref().map(|chat| chat.id.to_string()))
        .unwrap_or_else(|| "unknown".to_string())
}

fn body_mentions_bot(body: &MessageBody, bot_name: &str) -> bool {
    let normalized_bot_name = normalize_bot_name(bot_name);

    body.entities()
        .iter()
        .any(|entity| entity_mentions_bot(entity, &normalized_bot_name))
}

fn entity_mentions_bot(entity: &MessageEntityInfo, normalized_bot_name: &str) -> bool {
    match &entity.kind {
        MessageEntityKind::Mention => {
            normalize_bot_name(entity.text.as_str()) == normalized_bot_name
        }
        MessageEntityKind::BotCommand => entity
            .text
            .rsplit_once('@')
            .map(|(_, username)| normalize_bot_name(username) == normalized_bot_name)
            .unwrap_or(false),
        _ => false,
    }
}

fn normalize_bot_name(value: &str) -> String {
    value.trim().trim_start_matches('@').to_ascii_lowercase()
}

fn is_service_message(message: &Message) -> bool {
    message.new_chat_members().is_some()
        || message.left_chat_member().is_some()
        || message.new_chat_title().is_some()
        || message.new_chat_photo().is_some()
        || message.is_delete_chat_photo()
        || message.is_group_chat_created()
        || message.is_super_group_chat_created()
        || message.is_channel_chat_created()
        || message.message_auto_delete_timer_changed().is_some()
        || message.pinned_message().is_some()
        || message.chat_migration().is_some()
        || message.write_access_allowed().is_some()
        || message.forum_topic_created().is_some()
        || message.forum_topic_edited().is_some()
        || message.forum_topic_closed().is_some()
        || message.forum_topic_reopened().is_some()
        || message.general_forum_topic_hidden().is_some()
        || message.general_forum_topic_unhidden().is_some()
        || message.giveaway().is_some()
        || message.giveaway_completed().is_some()
        || message.giveaway_created().is_some()
        || message.giveaway_winners().is_some()
        || message.paid_message_price_changed().is_some()
        || message.gift_info().is_some()
        || message.unique_gift_info().is_some()
        || message.video_chat_scheduled().is_some()
        || message.video_chat_started().is_some()
        || message.video_chat_ended().is_some()
        || message.video_chat_participants_invited().is_some()
        || message.web_app_data().is_some()
        || message.connected_website().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_reply_script_uses_placeholder_then_edit_flow() {
        let runtime = TelegramRuntime::new(TelegramRuntimeConfig {
            bot_name: "bot".to_string(),
            placeholder_text: "正在处理...".to_string(),
            message_edit_throttle_ms: 750,
        });
        let script = runtime.build_reply_script(&ReplyPlan {
            platform: PlatformKind::Telegram,
            room_id: "room".to_string(),
            thread_id: None,
            placeholder_text: String::new(),
            final_text: "done".to_string(),
            notes: Vec::new(),
        });

        assert!(script.edit_in_place);
        assert_eq!(script.steps.len(), 2);
        assert!(matches!(
            script.steps[0],
            TelegramReplyStep::SendPlaceholder { ref text, .. } if text == "正在处理..."
        ));
        assert!(matches!(
            script.steps[1],
            TelegramReplyStep::EditPlaceholder { ref text, throttle_ms, .. }
                if text == "done" && throttle_ms == 750
        ));
    }

    #[test]
    fn normalize_inbound_preserves_message_structure() {
        let runtime = TelegramRuntime::new(TelegramRuntimeConfig {
            bot_name: "bot".to_string(),
            placeholder_text: "正在处理...".to_string(),
            message_edit_throttle_ms: 750,
        });
        let inbound = TelegramInboundMessage {
            room_id: "room".to_string(),
            thread_id: None,
            message_id: "msg-1".to_string(),
            sender_id: "user-1".to_string(),
            kind: PlatformMessageKind::Photo,
            body: MessageBody::Caption {
                text: "看看这张图".to_string(),
                entities: vec![],
            },
            attachments: vec![AttachmentInfo {
                kind: AttachmentKind::Image,
                file_id: Some("file-id".to_string()),
                file_unique_id: Some("file-unique-id".to_string()),
                file_name: None,
                mime_type: None,
                url: None,
                width: Some(1024),
                height: Some(768),
                size_bytes: Some(12_345),
            }],
            reply: Some(ReplyMetadata {
                message_id: "reply-1".to_string(),
                sender_id: "user-2".to_string(),
                kind: PlatformMessageKind::Text,
                body: MessageBody::Text {
                    text: "原始消息".to_string(),
                    entities: vec![],
                },
                attachments: vec![],
            }),
            is_mention: true,
        };

        let normalized = runtime.normalize_inbound(inbound.clone());

        assert_eq!(normalized.platform, PlatformKind::Telegram);
        assert_eq!(normalized.room_id, inbound.room_id);
        assert_eq!(normalized.message_id, inbound.message_id);
        assert_eq!(normalized.sender_id, inbound.sender_id);
        assert_eq!(normalized.kind, inbound.kind);
        assert_eq!(normalized.body, inbound.body);
        assert_eq!(normalized.attachments, inbound.attachments);
        assert_eq!(normalized.reply, inbound.reply);
        assert!(normalized.is_mention);
    }

    #[test]
    fn body_mentions_bot_detects_mentions_and_bot_commands() {
        let mention_body = MessageBody::Text {
            text: "@Bot 请处理".to_string(),
            entities: vec![MessageEntityInfo {
                kind: MessageEntityKind::Mention,
                text: "@Bot".to_string(),
            }],
        };
        let command_body = MessageBody::Text {
            text: "/start@Bot".to_string(),
            entities: vec![MessageEntityInfo {
                kind: MessageEntityKind::BotCommand,
                text: "/start@Bot".to_string(),
            }],
        };

        assert!(body_mentions_bot(&mention_body, "bot"));
        assert!(body_mentions_bot(&command_body, "bot"));
        assert!(!body_mentions_bot(&MessageBody::Empty, "bot"));
    }
}
