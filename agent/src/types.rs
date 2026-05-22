use rig_core::agent::Agent;
use rig_core::completion::CompletionModel;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::AbortHandle;
use uuid::Uuid;

#[derive(Clone)]
pub struct ModelConfig {
    pub model: String,
    pub base_url: Option<String>,
    pub api_key: String,
    pub max_turns: usize,
}

#[derive(Debug, Clone)]
pub struct CreatedSubagent {
    pub name: String,
    pub token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentStatus {
    Idle,
    Running,
    Completed(usize),
}

pub struct SubagentEntry<M: CompletionModel> {
    pub name: String,
    pub token: String,
    pub thread_id: Uuid,
    pub agent: Agent<M>,
    pub status: Arc<RwLock<SubagentStatus>>,
    pub task_abort: Arc<tokio::sync::Mutex<Option<AbortHandle>>>,
    pub results: Arc<RwLock<VecDeque<String>>>,
}

impl<M: CompletionModel + 'static> Clone for SubagentEntry<M> {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            token: self.token.clone(),
            thread_id: self.thread_id,
            agent: self.agent.clone(),
            status: self.status.clone(),
            task_abort: self.task_abort.clone(),
            results: self.results.clone(),
        }
    }
}
