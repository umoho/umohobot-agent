#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("Thread not found: {0}")]
    ThreadNotFound(uuid::Uuid),
    #[error("Model error: {0}")]
    ModelError(#[from] rig_core::completion::PromptError),
    #[error("Subagent '{name}' not found")]
    SubagentNotFound { name: String },
    #[error("Invalid subagent token")]
    SubagentInvalidToken,
    #[error("Subagent '{name}' is already running")]
    SubagentAlreadyRunning { name: String },
    #[error("No subagent results available")]
    SubagentNoResults,
    #[error("Index {index} out of range, {available} result(s) available")]
    SubagentIndexOutOfRange { index: i32, available: usize },
    #[error("Tool server error: {0}")]
    ToolServer(#[from] rig_core::tool::server::ToolServerError),
    #[error("Not in agent context")]
    NotInContext,
}
