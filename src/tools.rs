pub mod chat;
pub mod history;
pub mod protocol;
pub mod tools;

pub use chat::{ChatBatchError, ChatBatchTool};
pub use history::{HistoryQueryError, HistoryQueryTool};
pub use protocol::{
    ChatBatchRequest, ChatBatchResult, ChatOp, ChatOpOutcome, ChatOpStatus, HistoryDirection,
    HistoryItem, HistoryQueryRequest, HistoryQueryResult, HistoryWindow, MessageDraft,
    MessageLocator, MessageRef, ToolContext, ToolIssue,
};
pub use tools::{ToolCall, ToolKind, ToolRegistry, ToolResult, ToolRisk, ToolSpec};
