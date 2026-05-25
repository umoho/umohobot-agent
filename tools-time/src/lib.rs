use std::sync::Arc;

use agent::{AgentError, AgentRuntime};
use rig_core::completion::CompletionModel;
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

mod config;
mod now;
mod timer;

pub use config::TimerConfig;
pub use now::TimeNowTool;
pub use timer::{TimerCancelTool, TimerListTool, TimerSetTool};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Timer too short: minimum {min_secs}s, got {actual_secs}s")]
    TimerTooShort { min_secs: u64, actual_secs: u64 },

    #[error("Timer too long: maximum {max_secs}s, got {actual_secs}s")]
    TimerTooLong { max_secs: u64, actual_secs: u64 },

    #[error("Timer limit reached: max {max} timers per thread")]
    TimerLimitReached { max: usize },

    #[error("No matching timer found")]
    TimerNotFound,

    #[error("Not in agent context")]
    NotInAgentContext,

    #[error("{0}")]
    AgentError(#[from] AgentError),
}

#[derive(Debug, Clone)]
pub struct TimerExpiry {
    pub thread_id: Uuid,
    pub task: String,
}

pub async fn register_time_tools(
    runtime: Arc<AgentRuntime<impl CompletionModel + 'static>>,
    config: TimerConfig,
    expiry_tx: UnboundedSender<TimerExpiry>,
) -> Result<(), AgentError> {
    let state = Arc::new(timer::TimerState {
        timers: Default::default(),
        config,
        expiry_tx,
    });

    runtime.register_tool(TimeNowTool).await?;
    runtime
        .register_tool(TimerSetTool {
            state: state.clone(),
        })
        .await?;
    runtime
        .register_tool(TimerListTool {
            state: state.clone(),
        })
        .await?;
    runtime.register_tool(TimerCancelTool { state }).await?;

    Ok(())
}
