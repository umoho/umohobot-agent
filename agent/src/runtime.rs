use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use rig_core::OneOrMany;
use rig_core::agent::Agent;
use rig_core::client::CompletionClient;
use rig_core::completion::Message;
use rig_core::completion::message::UserContent;
use rig_core::providers::openai;
use rig_core::tool::ToolDyn;

use crate::Capability;
use crate::dyn_tool::DynTool;
use crate::error::AgentError;
use crate::pool::{Account, ModelPool};
use crate::thread::Thread;
use crate::types::{CreatedSubagent, SubagentEntry, SubagentStatus};
use crate::{CURRENT_PARENT_THREAD_ID, ThreadStore, run_turn_inner};

type CompletionModel = openai::CompletionModel;

pub struct AgentRuntime {
    pub(crate) agent: Agent<CompletionModel>,
    pub(crate) threads: ThreadStore,
    pub(crate) capabilities: Vec<crate::Capability>,
    pub(crate) tool_registry: Arc<RwLock<HashMap<String, DynTool>>>,
    pub(crate) subagents: Arc<RwLock<HashMap<(Uuid, String), SubagentEntry<CompletionModel>>>>,
    pub(crate) model_pool: Arc<ModelPool>,
    pub parent_account: Arc<Account>,
    pub(crate) max_turns: usize,
}

impl AgentRuntime {
    pub fn new(
        agent: Agent<CompletionModel>,
        capabilities: Vec<crate::Capability>,
        model_pool: Arc<ModelPool>,
        parent_account: Arc<Account>,
        max_turns: usize,
    ) -> Self {
        Self {
            agent,
            threads: Arc::new(RwLock::new(HashMap::new())),
            capabilities,
            tool_registry: Arc::new(RwLock::new(HashMap::new())),
            subagents: Arc::new(RwLock::new(HashMap::new())),
            model_pool,
            parent_account,
            max_turns,
        }
    }

    pub fn agent(&self) -> &Agent<CompletionModel> {
        &self.agent
    }

    pub async fn register_tool<T: ToolDyn + 'static>(&self, tool: T) -> Result<(), AgentError> {
        let name = tool.name();
        let dyn_tool: Arc<dyn ToolDyn + 'static> = Arc::new(tool);
        self.agent
            .tool_server_handle
            .add_tool(DynTool(dyn_tool.clone()))
            .await?;
        self.tool_registry
            .write()
            .await
            .insert(name, DynTool(dyn_tool));
        Ok(())
    }

    pub async fn close_thread(&self, thread_id: Uuid) {
        let sub_ids = {
            let threads = self.threads.read().await;
            threads
                .get(&thread_id)
                .map(|t| t.sub_thread_ids.clone())
                .unwrap_or_default()
        };
        for sub_id in sub_ids {
            Box::pin(self.close_thread(sub_id)).await;
        }
        let mut threads = self.threads.write().await;
        if let Some(thread) = threads.get_mut(&thread_id) {
            thread.close();
        }
    }

    pub async fn subagent_create(
        &self,
        name: &str,
        system_prompt: Option<&str>,
        tools: &str,
        model: Option<&str>,
    ) -> Result<CreatedSubagent, AgentError> {
        let parent_id = CURRENT_PARENT_THREAD_ID
            .try_with(|id| *id)
            .map_err(|_| AgentError::NotInContext)?;

        let (provider, model_name) = self
            .model_pool
            .resolve_model(
                model,
                &self.parent_account.provider,
                &self.parent_account.model,
            )
            .map_err(AgentError::ModelResolution)?;

        let account = self
            .model_pool
            .allocate(parent_id, &provider, &model_name)
            .await;

        let token = Uuid::new_v4().to_string();
        let thread_id = Uuid::new_v4();

        let allowed = if tools.is_empty() {
            Vec::new()
        } else if tools == "all" {
            let reg = self.tool_registry.read().await;
            reg.keys().cloned().collect()
        } else {
            tools.split(',').map(|t| t.trim().to_string()).collect()
        };

        let filtered = {
            let handle = rig_core::tool::server::ToolServer::new().run();
            let reg = self.tool_registry.read().await;
            for (tname, dtool) in reg.iter() {
                if tname.starts_with("subagent_") {
                    continue;
                }
                if allowed.is_empty()
                    || allowed
                        .iter()
                        .any(|a| tname == a || tname.starts_with(&format!("{}_", a)))
                {
                    handle.add_tool(dtool.clone()).await?;
                }
            }
            handle
        };

        let client = openai::Client::builder()
            .api_key(&account.api_key)
            .base_url(&account.base_url)
            .build()?;

        let mut sub_agent: Agent<CompletionModel> = client
            .completions_api()
            .agent(&account.model)
            .default_max_turns(self.max_turns)
            .build();
        sub_agent.tool_server_handle = filtered;

        self.get_or_create_thread(thread_id).await;
        if let Some(sp) = system_prompt {
            self.set_system_message(thread_id, sp).await;
        }

        {
            let mut threads = self.threads.write().await;
            if let Some(parent) = threads.get_mut(&parent_id) {
                parent.sub_thread_ids.push(thread_id);
            }
            if let Some(child) = threads.get_mut(&thread_id) {
                child.parent_thread_id = Some(parent_id);
            }
        }

        let entry = SubagentEntry {
            name: name.to_string(),
            token: token.clone(),
            thread_id,
            agent: sub_agent,
            capabilities: account.capabilities.clone(),
            status: Arc::new(RwLock::new(SubagentStatus::Idle)),
            task_abort: Arc::new(tokio::sync::Mutex::new(None)),
            results: Arc::new(RwLock::new(VecDeque::new())),
        };
        self.subagents
            .write()
            .await
            .insert((parent_id, name.to_string()), entry);

        Ok(CreatedSubagent {
            name: name.to_string(),
            token,
        })
    }

    pub async fn subagent_ask(
        &self,
        token: &str,
        name: &str,
        content: Vec<UserContent>,
    ) -> Result<(), AgentError> {
        let parent_id = CURRENT_PARENT_THREAD_ID
            .try_with(|id| *id)
            .map_err(|_| AgentError::NotInContext)?;

        let entry = {
            let map = self.subagents.read().await;
            map.get(&(parent_id, name.to_string())).cloned().ok_or(
                AgentError::SubagentNotFound {
                    name: name.to_string(),
                },
            )?
        };

        if entry.token != token {
            return Err(AgentError::SubagentInvalidToken);
        }

        // Check capabilities match content modalities
        for item in &content {
            match item {
                UserContent::Image(_) => {
                    if !entry.capabilities.contains(&Capability::Image) {
                        return Err(AgentError::CapabilityMismatch {
                            name: name.to_string(),
                            required: Capability::Image,
                            actual: entry.capabilities.clone(),
                        });
                    }
                }
                UserContent::Audio(_) => {
                    if !entry.capabilities.contains(&Capability::Audio) {
                        return Err(AgentError::CapabilityMismatch {
                            name: name.to_string(),
                            required: Capability::Audio,
                            actual: entry.capabilities.clone(),
                        });
                    }
                }
                _ => {}
            }
        }

        {
            let mut status = entry.status.write().await;
            if *status == SubagentStatus::Running {
                return Err(AgentError::SubagentAlreadyRunning {
                    name: name.to_string(),
                });
            }
            *status = SubagentStatus::Running;
        }

        let sub_agent = entry.agent.clone();
        let threads = self.threads.clone();
        let sub_thread_id = entry.thread_id;
        let results = entry.results.clone();
        let status_arc = entry.status.clone();
        let abort_holder = entry.task_abort.clone();

        let message = Message::User {
            content: OneOrMany::many(content).map_err(|_| {
                AgentError::ModelError(rig_core::completion::PromptError::PromptCancelled {
                    chat_history: vec![],
                    reason: "empty content".into(),
                })
            })?,
        };

        let handle = tokio::spawn(async move {
            let result = run_turn_inner(&sub_agent, &threads, sub_thread_id, vec![message]).await;

            match result {
                Ok((text, _)) => {
                    results.write().await.push_back(text);
                }
                Err(e) => {
                    results.write().await.push_back(format!("Error: {e}"));
                }
            }
            let count = results.read().await.len();
            *status_arc.write().await = SubagentStatus::Completed(count);
            *abort_holder.lock().await = None;
        });

        *entry.task_abort.lock().await = Some(handle.abort_handle());

        Ok(())
    }

    pub async fn subagent_stop(&self, token: &str, name: &str) -> Result<(), AgentError> {
        let parent_id = CURRENT_PARENT_THREAD_ID
            .try_with(|id| *id)
            .map_err(|_| AgentError::NotInContext)?;

        let entry = {
            let map = self.subagents.read().await;
            map.get(&(parent_id, name.to_string())).cloned().ok_or(
                AgentError::SubagentNotFound {
                    name: name.to_string(),
                },
            )?
        };

        if entry.token != token {
            return Err(AgentError::SubagentInvalidToken);
        }

        if let Some(abort) = entry.task_abort.lock().await.take() {
            abort.abort();
        }
        *entry.status.write().await = SubagentStatus::Idle;

        Ok(())
    }

    pub async fn subagent_status(
        &self,
        token: &str,
        name: &str,
    ) -> Result<SubagentStatus, AgentError> {
        let parent_id = CURRENT_PARENT_THREAD_ID
            .try_with(|id| *id)
            .map_err(|_| AgentError::NotInContext)?;

        let entry = {
            let map = self.subagents.read().await;
            map.get(&(parent_id, name.to_string())).cloned().ok_or(
                AgentError::SubagentNotFound {
                    name: name.to_string(),
                },
            )?
        };

        if entry.token != token {
            return Err(AgentError::SubagentInvalidToken);
        }

        let status = *entry.status.read().await;
        Ok(status)
    }

    pub async fn subagent_read(
        &self,
        token: &str,
        name: &str,
        index: Option<i32>,
    ) -> Result<String, AgentError> {
        let parent_id = CURRENT_PARENT_THREAD_ID
            .try_with(|id| *id)
            .map_err(|_| AgentError::NotInContext)?;

        let entry = {
            let map = self.subagents.read().await;
            map.get(&(parent_id, name.to_string())).cloned().ok_or(
                AgentError::SubagentNotFound {
                    name: name.to_string(),
                },
            )?
        };

        if entry.token != token {
            return Err(AgentError::SubagentInvalidToken);
        }

        let results = entry.results.read().await;
        let idx = index.unwrap_or(-1);
        let len = results.len();

        if len == 0 {
            return Err(AgentError::SubagentNoResults);
        }

        let actual = if idx >= 0 {
            idx as usize
        } else {
            (len as i32 + idx) as usize
        };

        if actual >= len {
            return Err(AgentError::SubagentIndexOutOfRange {
                index: idx,
                available: len,
            });
        }

        Ok(results.get(actual).cloned().unwrap_or_default())
    }

    pub async fn subagent_destroy(&self, token: &str, name: &str) -> Result<(), AgentError> {
        let parent_id = CURRENT_PARENT_THREAD_ID
            .try_with(|id| *id)
            .map_err(|_| AgentError::NotInContext)?;

        let key = (parent_id, name.to_string());
        let entry = {
            let map = self.subagents.read().await;
            map.get(&key).cloned()
        };

        let entry = match entry {
            Some(e) => e,
            None => {
                return Err(AgentError::SubagentNotFound {
                    name: name.to_string(),
                });
            }
        };

        if entry.token != token {
            return Err(AgentError::SubagentInvalidToken);
        }

        if let Some(abort) = entry.task_abort.lock().await.take() {
            abort.abort();
        }

        self.close_thread(entry.thread_id).await;
        self.subagents.write().await.remove(&key);

        Ok(())
    }

    pub(crate) async fn get_or_create_thread(&self, id: Uuid) -> Thread {
        let threads = self.threads.read().await;
        if let Some(thread) = threads.get(&id) {
            return thread.clone();
        }
        drop(threads);

        let mut threads = self.threads.write().await;
        let thread = Thread::new();
        let clone = thread.clone();
        threads.insert(id, thread);
        clone
    }

    pub(crate) async fn set_system_message(&self, thread_id: Uuid, text: &str) {
        let mut threads = self.threads.write().await;
        if let Some(thread) = threads.get_mut(&thread_id) {
            let system_idx = thread
                .messages
                .iter()
                .position(|m| matches!(m, Message::System { .. }));
            if let Some(idx) = system_idx {
                thread.messages[idx] = Message::system(text);
            } else {
                thread.messages.push(Message::system(text));
            }
        }
    }
}
