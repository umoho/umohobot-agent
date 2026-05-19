use rig::tool::server::{ToolServer, ToolServerHandle};

pub mod protocol {
    pub use contracts::protocol::{
        ChatBatchRequest, ChatBatchResult, ChatOp, ChatOpOutcome, ChatOpStatus, MessageDraft,
        MessageLocator, MessageRef, ToolContext, ToolIssue, chat_batch_request_schema,
        message_draft_schema, message_locator_schema, message_ref_schema, tool_context_schema,
    };
}

pub use contracts::{ToolCall, ToolKind, ToolRegistry, ToolResult, ToolRisk, ToolSpec};
pub use protocol::*;

#[derive(Clone)]
pub struct ToolBundle {
    pub registry: ToolRegistry,
    pub tool_server_handle: ToolServerHandle,
}

impl ToolBundle {
    pub fn new(registry: ToolRegistry, tool_server_handle: ToolServerHandle) -> Self {
        Self {
            registry,
            tool_server_handle,
        }
    }

    pub fn empty() -> Self {
        Self::new(ToolRegistry::new(), ToolServer::new().run())
    }
}

impl std::fmt::Debug for ToolBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolBundle")
            .field("registry", &self.registry)
            .field("tool_server_handle", &"<hidden>")
            .finish()
    }
}
