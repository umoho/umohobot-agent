use rig_core::agent::Agent;
use rig_core::completion::CompletionModel;
use serde::Deserialize;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::AbortHandle;
use uuid::Uuid;

use crate::Capability;

#[derive(Debug, Deserialize)]
pub struct ConfigFile {
    #[serde(rename = "default-model")]
    pub default_model: String,
    #[serde(rename = "model-accounts")]
    pub model_accounts: Vec<ModelAccountEntry>,
}

#[derive(Debug, Deserialize)]
pub struct ModelAccountEntry {
    pub provider: String,
    pub model: String,
    #[serde(rename = "api-key")]
    pub api_key: Option<String>,
    #[serde(rename = "api-key-raw")]
    pub api_key_raw: Option<String>,
    #[serde(rename = "base-url")]
    pub base_url: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
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
    pub capabilities: Vec<Capability>,
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
            capabilities: self.capabilities.clone(),
            status: self.status.clone(),
            task_abort: self.task_abort.clone(),
            results: self.results.clone(),
        }
    }
}
