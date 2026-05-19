use contracts::{
    AttachmentInfo, AttachmentKind, MessageBody, MessageEntityInfo, PlatformKind, PlatformMessage,
    PlatformMessageKind, ReplyMetadata,
};
use serde::{Deserialize, Serialize};
use teloxide::types::{Document, Message, PhotoSize, Sticker, Voice};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelegramNormalizedMessage {
    pub inbound: TelegramInboundMessage,
    pub platform_message: PlatformMessage,
}

impl TelegramInboundMessage {
    #[must_use]
    pub fn from_message(message: &Message, bot_name: impl AsRef<str>) -> Self {
        let snapshot = snapshot_message(message);
        let is_mention = body_mentions_bot(&snapshot.body, bot_name.as_ref());

        Self {
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
}

impl TelegramNormalizedMessage {
    #[must_use]
    pub fn from_message(message: &Message, bot_name: impl AsRef<str>) -> Self {
        let inbound = TelegramInboundMessage::from_message(message, bot_name);
        let platform_message = inbound.clone().into();

        Self {
            inbound,
            platform_message,
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

fn parsed_entities(
    entities: Option<Vec<teloxide::types::MessageEntityRef<'_>>>,
) -> Vec<MessageEntityInfo> {
    entities
        .unwrap_or_default()
        .into_iter()
        .map(|entity| MessageEntityInfo {
            kind: format!("{:?}", entity.kind()),
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
    match entity.kind.as_str() {
        "Mention" | "mention" => normalize_bot_name(entity.text.as_str()) == normalized_bot_name,
        "BotCommand" | "bot_command" => entity
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
    use serde_json::json;
    use teloxide::types::Message;

    fn message_from_value(value: serde_json::Value) -> Message {
        serde_json::from_value(value).expect("telegram message")
    }

    #[test]
    fn normalize_inbound_preserves_message_structure() {
        let message = message_from_value(json!({
            "message_id": 42,
            "date": 1_700_000_000,
            "chat": {
                "id": -100_123,
                "type": "supergroup",
                "title": "Support",
                "is_forum": true,
                "username": "support"
            },
            "from": {
                "id": 1001,
                "is_bot": false,
                "first_name": "Alice",
                "username": "alice"
            },
            "message_thread_id": 777,
            "text": "@Bot 请看",
            "entities": [
                {
                    "type": "mention",
                    "offset": 0,
                    "length": 4
                }
            ],
            "reply_to_message": {
                "message_id": 41,
                "date": 1_699_999_990,
                "chat": {
                    "id": -100_123,
                    "type": "supergroup",
                    "title": "Support",
                    "is_forum": true,
                    "username": "support"
                },
                "from": {
                    "id": 1002,
                    "is_bot": false,
                    "first_name": "Bob"
                },
                "text": "原始消息"
            }
        }));

        let normalized = TelegramNormalizedMessage::from_message(&message, "bot");

        assert_eq!(normalized.inbound.room_id, "-100123");
        assert_eq!(normalized.inbound.thread_id, Some("777".to_string()));
        assert_eq!(normalized.inbound.message_id, "42");
        assert_eq!(normalized.inbound.sender_id, "1001");
        assert_eq!(normalized.inbound.kind, PlatformMessageKind::Text);
        assert!(normalized.inbound.is_mention);
        assert!(normalized.inbound.attachments.is_empty());
        assert_eq!(normalized.platform_message.platform, PlatformKind::Telegram);
        assert_eq!(normalized.platform_message.room_id, "-100123");
        assert_eq!(
            normalized.platform_message.thread_id.as_deref(),
            Some("777")
        );
        assert_eq!(normalized.platform_message.message_id, "42");
        assert_eq!(normalized.platform_message.sender_id, "1001");
        assert_eq!(normalized.platform_message.kind, PlatformMessageKind::Text);
        assert_eq!(normalized.platform_message.text(), Some("@Bot 请看"));
        assert!(normalized.platform_message.is_mention);

        let reply = normalized
            .platform_message
            .reply
            .as_ref()
            .expect("reply metadata");
        assert_eq!(reply.message_id, "41");
        assert_eq!(reply.sender_id, "1002");
        assert_eq!(reply.kind, PlatformMessageKind::Text);
        assert_eq!(reply.body.text(), Some("原始消息"));
    }
}
