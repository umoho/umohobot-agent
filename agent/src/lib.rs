mod builder;
mod capability;
mod dyn_tool;
mod error;
mod handle;
mod pool;
mod run_turn;
mod runtime;
pub mod storage;
mod thread;
mod types;

pub use builder::AgentBuilder;
pub use capability::Capability;
pub use dyn_tool::DynTool;
pub use error::AgentError;
pub use handle::AgentHandle;
pub use pool::{Account, ModelInfo, ModelPool};
pub use rig_core::completion::Usage;
pub(crate) use run_turn::run_turn_inner;
pub use runtime::AgentRuntime;
pub use storage::{Storage, StorageResult, file::FileStorage};
pub use thread::Thread;
pub use types::{ConfigFile, CreatedSubagent, ModelAccountEntry, SubagentEntry, SubagentStatus};

pub use rig_core::OneOrMany;
pub use rig_core::completion::AssistantContent;
pub use rig_core::completion::CompletionModel;
pub use rig_core::completion::Message;
pub use rig_core::completion::ToolDefinition;
pub use rig_core::completion::message::{
    DocumentSourceKind, Image, ImageDetail, ImageMediaType, UserContent,
};

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type ThreadStore = Arc<RwLock<HashMap<Uuid, Thread>>>;

tokio::task_local! {
    pub static CURRENT_PARENT_THREAD_ID: Uuid;
}
