pub mod platforms;

pub use platforms::{
    AttachmentInfo, AttachmentKind, DiscordTransport, PlatformHub as Platforms, PlatformKind,
    PlatformMessage, ReplyHandle, TelegramTransport,
};
