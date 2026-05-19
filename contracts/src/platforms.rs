use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformKind {
    Telegram,
    Discord,
    Matrix,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    Image,
    Sticker,
    File,
    Voice,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentInfo {
    pub kind: AttachmentKind,
    pub file_id: Option<String>,
    pub file_unique_id: Option<String>,
    pub file_name: Option<String>,
    pub mime_type: Option<String>,
    pub url: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub size_bytes: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageEntityInfo {
    pub kind: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MessageBody {
    Text {
        text: String,
        entities: Vec<MessageEntityInfo>,
    },
    Caption {
        text: String,
        entities: Vec<MessageEntityInfo>,
    },
    Empty,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformMessageKind {
    Text,
    Photo,
    Sticker,
    File,
    Voice,
    Media,
    Service,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplyMetadata {
    pub message_id: String,
    pub sender_id: String,
    pub kind: PlatformMessageKind,
    pub body: MessageBody,
    pub attachments: Vec<AttachmentInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlatformMessage {
    pub platform: PlatformKind,
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
pub struct ReplyHandle {
    pub platform: PlatformKind,
    pub room_id: String,
    pub message_id: String,
}

#[derive(Clone, Debug, Default)]
pub struct TelegramTransport;

#[derive(Clone, Debug, Default)]
pub struct DiscordTransport;

#[derive(Clone, Debug, Default)]
pub struct MatrixTransport;

#[derive(Clone, Debug, Default)]
pub struct PlatformHub {
    telegram: TelegramTransport,
    discord: DiscordTransport,
    matrix: MatrixTransport,
}

impl PlatformKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Telegram => "telegram",
            Self::Discord => "discord",
            Self::Matrix => "matrix",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "telegram" => Some(Self::Telegram),
            "discord" => Some(Self::Discord),
            "matrix" => Some(Self::Matrix),
            _ => None,
        }
    }
}

impl AttachmentKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Sticker => "sticker",
            Self::File => "file",
            Self::Voice => "voice",
        }
    }
}

impl AttachmentInfo {
    pub fn describe(&self) -> String {
        let mut fields = Vec::new();

        if let Some(file_id) = self.file_id.as_deref() {
            fields.push(format!("file_id={file_id}"));
        }
        if let Some(file_unique_id) = self.file_unique_id.as_deref() {
            fields.push(format!("file_unique_id={file_unique_id}"));
        }
        if let Some(file_name) = self.file_name.as_deref() {
            fields.push(format!("file_name={file_name}"));
        }
        if let Some(mime_type) = self.mime_type.as_deref() {
            fields.push(format!("mime_type={mime_type}"));
        }
        if let Some(url) = self.url.as_deref() {
            fields.push(format!("url={url}"));
        }
        if let Some(width) = self.width {
            fields.push(format!("width={width}"));
        }
        if let Some(height) = self.height {
            fields.push(format!("height={height}"));
        }
        if let Some(size_bytes) = self.size_bytes {
            fields.push(format!("size_bytes={size_bytes}"));
        }

        if fields.is_empty() {
            self.kind.as_str().to_string()
        } else {
            format!("{}({})", self.kind.as_str(), fields.join(", "))
        }
    }
}

impl MessageBody {
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text { text, .. } | Self::Caption { text, .. } => Some(text),
            Self::Empty => None,
        }
    }

    pub fn entities(&self) -> &[MessageEntityInfo] {
        match self {
            Self::Text { entities, .. } | Self::Caption { entities, .. } => entities.as_slice(),
            Self::Empty => &[],
        }
    }

    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::Text { .. } => "text",
            Self::Caption { .. } => "caption",
            Self::Empty => "empty",
        }
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }
}

impl PlatformMessageKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Photo => "photo",
            Self::Sticker => "sticker",
            Self::File => "file",
            Self::Voice => "voice",
            Self::Media => "media",
            Self::Service => "service",
            Self::Unknown => "unknown",
        }
    }
}

impl ReplyMetadata {
    pub fn describe(&self) -> String {
        let mut fields = vec![
            format!("message_id={}", self.message_id),
            format!("sender_id={}", self.sender_id),
            format!("kind={}", self.kind.as_str()),
            format!("body_kind={}", self.body.kind_label()),
        ];

        if let Some(text) = self.body.text().filter(|text| !text.trim().is_empty()) {
            fields.push(format!("text={text}"));
        }
        if !self.attachments.is_empty() {
            fields.push(format!(
                "attachments={}",
                self.attachments
                    .iter()
                    .map(AttachmentInfo::describe)
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }

        fields.join(", ")
    }
}

impl PlatformMessage {
    pub fn text(&self) -> Option<&str> {
        self.body.text()
    }

    pub fn body_entities(&self) -> &[MessageEntityInfo] {
        self.body.entities()
    }

    pub fn prompt_text(&self) -> String {
        let mut lines = Vec::new();

        if let Some(text) = self.text().filter(|text| !text.trim().is_empty()) {
            lines.push(text.to_string());
        }

        let mut context = Vec::new();
        if !matches!(self.kind, PlatformMessageKind::Text) {
            context.push(format!("message_kind={}", self.kind.as_str()));
        }
        if !matches!(self.body, MessageBody::Text { .. }) {
            context.push(format!("body_kind={}", self.body.kind_label()));
        }
        if !self.attachments.is_empty() {
            context.push(format!(
                "attachments={}",
                self.attachments
                    .iter()
                    .map(AttachmentInfo::describe)
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        if let Some(reply) = &self.reply {
            context.push(format!("reply={}", reply.describe()));
        }
        if self.is_mention
            && (!context.is_empty()
                || self
                    .text()
                    .map(|text| text.trim().is_empty())
                    .unwrap_or(true))
        {
            context.push("bot_mentioned=true".to_string());
        }

        if context.is_empty() {
            return lines.join("\n");
        }

        if !lines.is_empty() {
            lines.push(String::new());
        }

        lines.push("context:".to_string());
        lines.extend(context.into_iter().map(|line| format!("- {line}")));
        lines.join("\n")
    }
}

impl TelegramTransport {
    pub fn kind(&self) -> PlatformKind {
        PlatformKind::Telegram
    }
}

impl DiscordTransport {
    pub fn kind(&self) -> PlatformKind {
        PlatformKind::Discord
    }
}

impl MatrixTransport {
    pub fn kind(&self) -> PlatformKind {
        PlatformKind::Matrix
    }
}

impl PlatformHub {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn supported_kinds(&self) -> Vec<PlatformKind> {
        vec![
            self.telegram.kind(),
            self.discord.kind(),
            self.matrix.kind(),
        ]
    }

    pub fn describe(&self) -> String {
        self.supported_kinds()
            .into_iter()
            .map(|kind| kind.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}
