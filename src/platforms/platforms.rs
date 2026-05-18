#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PlatformKind {
    Telegram,
    Discord,
    Matrix,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttachmentKind {
    Text,
    Image,
    Sticker,
    File,
    Voice,
}

#[derive(Clone, Debug)]
pub struct AttachmentInfo {
    pub kind: AttachmentKind,
    pub url: Option<String>,
    pub mime_type: Option<String>,
    pub file_name: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PlatformMessage {
    pub platform: PlatformKind,
    pub room_id: String,
    pub sender_id: String,
    pub text: String,
    pub attachments: Vec<AttachmentInfo>,
    pub reply_to_message_id: Option<String>,
    pub is_mention: bool,
}

#[derive(Clone, Debug)]
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
            PlatformKind::Telegram => "telegram",
            PlatformKind::Discord => "discord",
            PlatformKind::Matrix => "matrix",
        }
    }
}

impl TelegramTransport {
    pub fn kind(&self) -> PlatformKind {
        PlatformKind::Telegram
    }

    pub fn supports_placeholder_edit_flow(&self) -> bool {
        true
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

    pub fn supports_placeholder_edit_flow(&self) -> bool {
        self.telegram.supports_placeholder_edit_flow()
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

pub mod telegram;

pub use telegram::{
    TelegramInboundMessage, TelegramReplyScript, TelegramReplyStep, TelegramRuntime,
    TelegramRuntimeConfig,
};
