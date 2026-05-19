pub mod platforms;
pub mod protocol;
pub mod tools;

pub use platforms::{
    AttachmentInfo, AttachmentKind, DiscordTransport, MatrixTransport, MessageBody,
    MessageEntityInfo, PlatformHub, PlatformKind, PlatformMessage, PlatformMessageKind,
    ReplyHandle, ReplyMetadata, TelegramTransport,
};
pub use protocol::{
    ChatBatchRequest, ChatBatchResult, ChatOp, ChatOpOutcome, ChatOpStatus, MessageDraft,
    MessageLocator, MessageRef, ToolContext, ToolIssue, chat_batch_request_schema,
    message_draft_schema, message_locator_schema, message_ref_schema, tool_context_schema,
};
pub use tools::{ToolCall, ToolKind, ToolRegistry, ToolResult, ToolRisk, ToolSpec};
