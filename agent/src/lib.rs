mod thread;

pub use thread::Thread;

pub use rig_core::OneOrMany;
pub use rig_core::completion::CompletionModel;
pub use rig_core::completion::Message;
pub use rig_core::completion::ToolDefinition;
pub use rig_core::completion::Usage;
pub use rig_core::completion::message::{
    DocumentSourceKind, Image, ImageDetail, ImageMediaType, UserContent,
};

use chrono::Utc;
use rig_core::agent::Agent;
use rig_core::client::CompletionClient;
use rig_core::completion::{self, AssistantContent, Prompt};
use rig_core::providers::openai;
use rig_core::tool::{ToolDyn, ToolError};
use rig_core::wasm_compat::WasmBoxedFuture;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::AbortHandle;
use tracing::{debug, trace};
use uuid::Uuid;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type ThreadStore = Arc<RwLock<HashMap<Uuid, Thread>>>;

tokio::task_local! {
    pub static CURRENT_PARENT_THREAD_ID: Uuid;
}

// ── Capabilities ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Vision,
    Audio,
}

impl std::str::FromStr for Capability {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "vision" => Ok(Capability::Vision),
            "audio" => Ok(Capability::Audio),
            _ => Err(format!("Unknown capability: {s}")),
        }
    }
}

// ── Errors ──

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("Thread not found: {0}")]
    ThreadNotFound(Uuid),
    #[error("Model error: {0}")]
    ModelError(#[from] completion::PromptError),
    #[error("Subagent error: {0}")]
    Subagent(String),
    #[error("Tool server error: {0}")]
    ToolServer(#[from] rig_core::tool::server::ToolServerError),
    #[error("Not in agent context")]
    NotInContext,
}

impl From<String> for AgentError {
    fn from(s: String) -> Self {
        AgentError::Subagent(s)
    }
}

// ── DynTool: cloneable ToolDyn wrapper ──

#[derive(Clone)]
pub struct DynTool(pub Arc<dyn ToolDyn + 'static>);

impl ToolDyn for DynTool {
    fn name(&self) -> String {
        self.0.name()
    }

    fn definition<'a>(&'a self, prompt: String) -> WasmBoxedFuture<'a, ToolDefinition> {
        Box::pin(async move { self.0.definition(prompt).await })
    }

    fn call<'a>(&'a self, args: String) -> WasmBoxedFuture<'a, Result<String, ToolError>> {
        Box::pin(async move { self.0.call(args).await })
    }
}

// ── ModelConfig ──

#[derive(Clone)]
pub struct ModelConfig {
    pub model: String,
    pub base_url: Option<String>,
    pub api_key: String,
    pub max_turns: usize,
}

// ── Subagent types ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentStatus {
    Idle,
    Running,
    Completed,
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

// ── AgentHandle trait ──

pub trait AgentHandle: Send + Sync {
    fn run_turn<'a>(
        &'a self,
        thread_id: Uuid,
        messages: Vec<Message>,
    ) -> BoxFuture<'a, Result<(String, Usage), AgentError>>;
    fn get_or_create_thread<'a>(&'a self, id: Uuid) -> BoxFuture<'a, Thread>;
    fn append_system_message<'a>(&'a self, thread_id: Uuid, text: &'a str) -> BoxFuture<'a, ()>;
    fn set_system_message<'a>(&'a self, thread_id: Uuid, text: &'a str) -> BoxFuture<'a, ()>;
    fn compact_thread<'a>(
        &'a self,
        old_thread_id: Uuid,
        new_thread_id: Uuid,
        compact_prompt: &'a str,
    ) -> BoxFuture<'a, Result<String, AgentError>>;
    fn capabilities(&self) -> &[Capability];
}

// ── AgentRuntime ──

pub struct AgentRuntime<M: CompletionModel> {
    agent: Agent<M>,
    threads: ThreadStore,
    capabilities: Vec<Capability>,
    tool_registry: Arc<RwLock<HashMap<String, DynTool>>>,
    subagents: Arc<RwLock<HashMap<(Uuid, String), SubagentEntry<M>>>>,
    pub model_config: ModelConfig,
}

impl<M: CompletionModel + 'static> AgentRuntime<M> {
    pub fn new(
        agent: Agent<M>,
        threads: ThreadStore,
        capabilities: Vec<Capability>,
        model_config: ModelConfig,
    ) -> Self {
        Self {
            agent,
            threads,
            capabilities,
            tool_registry: Arc::new(RwLock::new(HashMap::new())),
            subagents: Arc::new(RwLock::new(HashMap::new())),
            model_config,
        }
    }

    pub fn agent(&self) -> &Agent<M> {
        &self.agent
    }

    // ── Tool registration ──

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

    // ── Thread management ──

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

    // ── Subagent methods ──

    pub async fn subagent_create(
        &self,
        name: &str,
        system_prompt: Option<&str>,
        tools: &str,
    ) -> Result<String, AgentError> {
        let parent_id = CURRENT_PARENT_THREAD_ID
            .try_with(|id| *id)
            .map_err(|_| AgentError::NotInContext)?;

        let token = Uuid::new_v4().to_string();
        let thread_id = Uuid::new_v4();

        // Parse whitelist
        let allowed = if tools.is_empty() {
            Vec::new()
        } else if tools == "all" {
            let reg = self.tool_registry.read().await;
            reg.keys().cloned().collect()
        } else {
            tools.split(',').map(|t| t.trim().to_string()).collect()
        };

        // Build filtered handle
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

        // Clone parent agent and swap handle
        let mut sub_agent: Agent<M> = self.agent.clone();
        sub_agent.tool_server_handle = filtered;

        // Create thread
        self.get_or_create_thread(thread_id).await;
        if let Some(sp) = system_prompt {
            self.set_system_message(thread_id, sp).await;
        }

        // Link parent → child
        {
            let mut threads = self.threads.write().await;
            if let Some(parent) = threads.get_mut(&parent_id) {
                parent.sub_thread_ids.push(thread_id);
            }
            if let Some(child) = threads.get_mut(&thread_id) {
                child.parent_thread_id = Some(parent_id);
            }
        }

        // Store entry
        let entry = SubagentEntry {
            name: name.to_string(),
            token: token.clone(),
            thread_id,
            agent: sub_agent,
            status: Arc::new(RwLock::new(SubagentStatus::Idle)),
            task_abort: Arc::new(tokio::sync::Mutex::new(None)),
            results: Arc::new(RwLock::new(VecDeque::new())),
        };
        self.subagents
            .write()
            .await
            .insert((parent_id, name.to_string()), entry);

        Ok(format!("subagent '{name}' created, token={token}"))
    }

    pub async fn subagent_ask(
        &self,
        token: &str,
        name: &str,
        task: &str,
    ) -> Result<String, AgentError> {
        let parent_id = CURRENT_PARENT_THREAD_ID
            .try_with(|id| *id)
            .map_err(|_| AgentError::NotInContext)?;

        let entry = {
            let map = self.subagents.read().await;
            map.get(&(parent_id, name.to_string()))
                .cloned()
                .ok_or_else(|| AgentError::Subagent(format!("subagent '{name}' not found")))?
        };

        if entry.token != token {
            return Err(AgentError::Subagent("invalid token".into()));
        }

        {
            let mut status = entry.status.write().await;
            if *status == SubagentStatus::Running {
                return Err(AgentError::Subagent(format!(
                    "subagent '{name}' is running, use subagent_stop or wait"
                )));
            }
            *status = SubagentStatus::Running;
        }

        let sub_agent = entry.agent.clone();
        let threads = self.threads.clone();
        let sub_thread_id = entry.thread_id;
        let results = entry.results.clone();
        let status_arc = entry.status.clone();
        let abort_holder = entry.task_abort.clone();
        let task_owned = task.to_string();

        let handle = tokio::spawn(async move {
            let result = run_turn_inner(
                &sub_agent,
                &threads,
                sub_thread_id,
                vec![Message::user(task_owned)],
            )
            .await;

            match result {
                Ok((text, _)) => {
                    results.write().await.push_back(text);
                }
                Err(e) => {
                    results.write().await.push_back(format!("Error: {e}"));
                }
            }
            *status_arc.write().await = SubagentStatus::Completed;
            *abort_holder.lock().await = None;
        });

        *entry.task_abort.lock().await = Some(handle.abort_handle());

        Ok("task submitted".into())
    }

    pub async fn subagent_stop(&self, token: &str, name: &str) -> Result<String, AgentError> {
        let parent_id = CURRENT_PARENT_THREAD_ID
            .try_with(|id| *id)
            .map_err(|_| AgentError::NotInContext)?;

        let entry = {
            let map = self.subagents.read().await;
            map.get(&(parent_id, name.to_string()))
                .cloned()
                .ok_or_else(|| AgentError::Subagent(format!("subagent '{name}' not found")))?
        };

        if entry.token != token {
            return Err(AgentError::Subagent("invalid token".into()));
        }

        if let Some(abort) = entry.task_abort.lock().await.take() {
            abort.abort();
        }
        *entry.status.write().await = SubagentStatus::Idle;

        Ok(format!("subagent '{name}' stopped"))
    }

    pub async fn subagent_status(&self, token: &str, name: &str) -> Result<String, AgentError> {
        let parent_id = CURRENT_PARENT_THREAD_ID
            .try_with(|id| *id)
            .map_err(|_| AgentError::NotInContext)?;

        let entry = {
            let map = self.subagents.read().await;
            map.get(&(parent_id, name.to_string()))
                .cloned()
                .ok_or_else(|| AgentError::Subagent(format!("subagent '{name}' not found")))?
        };

        if entry.token != token {
            return Err(AgentError::Subagent("invalid token".into()));
        }

        let status = *entry.status.read().await;
        let available = entry.results.read().await.len();
        Ok(match status {
            SubagentStatus::Idle => "idle".into(),
            SubagentStatus::Running => "running".into(),
            SubagentStatus::Completed => format!("completed ({available} available)"),
        })
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
            map.get(&(parent_id, name.to_string()))
                .cloned()
                .ok_or_else(|| AgentError::Subagent(format!("subagent '{name}' not found")))?
        };

        if entry.token != token {
            return Err(AgentError::Subagent("invalid token".into()));
        }

        let results = entry.results.read().await;
        let idx = index.unwrap_or(-1);
        let len = results.len();

        if len == 0 {
            return Err(AgentError::Subagent("no results available".into()));
        }

        let actual = if idx >= 0 {
            idx as usize
        } else {
            (len as i32 + idx) as usize
        };

        if actual >= len {
            return Err(AgentError::Subagent(format!(
                "index {idx} out of range, {len} result(s) available"
            )));
        }

        Ok(results.get(actual).cloned().unwrap_or_default())
    }

    pub async fn subagent_destroy(&self, token: &str, name: &str) -> Result<String, AgentError> {
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
            None => return Err(AgentError::Subagent(format!("subagent '{name}' not found"))),
        };

        if entry.token != token {
            return Err(AgentError::Subagent("invalid token".into()));
        }

        if let Some(abort) = entry.task_abort.lock().await.take() {
            abort.abort();
        }

        self.close_thread(entry.thread_id).await;
        self.subagents.write().await.remove(&key);

        Ok(format!("subagent '{name}' destroyed"))
    }
}

// ── AgentHandle impl ──

impl<M: CompletionModel + 'static> AgentHandle for AgentRuntime<M> {
    fn run_turn<'a>(
        &'a self,
        thread_id: Uuid,
        messages: Vec<Message>,
    ) -> BoxFuture<'a, Result<(String, Usage), AgentError>> {
        Box::pin(async move {
            CURRENT_PARENT_THREAD_ID
                .scope(
                    thread_id,
                    run_turn_inner(&self.agent, &self.threads, thread_id, messages),
                )
                .await
        })
    }

    fn get_or_create_thread<'a>(&'a self, id: Uuid) -> BoxFuture<'a, Thread> {
        Box::pin(async move {
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
        })
    }

    fn append_system_message<'a>(&'a self, thread_id: Uuid, text: &'a str) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let mut threads = self.threads.write().await;
            if let Some(thread) = threads.get_mut(&thread_id) {
                thread.messages.push(Message::system(text));
            }
        })
    }

    fn set_system_message<'a>(&'a self, thread_id: Uuid, text: &'a str) -> BoxFuture<'a, ()> {
        Box::pin(async move {
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
        })
    }

    fn compact_thread<'a>(
        &'a self,
        old_thread_id: Uuid,
        new_thread_id: Uuid,
        compact_prompt: &'a str,
    ) -> BoxFuture<'a, Result<String, AgentError>> {
        Box::pin(async move {
            let messages = {
                let threads = self.threads.read().await;
                let thread = threads
                    .get(&old_thread_id)
                    .ok_or(AgentError::ThreadNotFound(old_thread_id))?;
                thread.messages.clone()
            };

            let conversation_text = messages
                .iter()
                .filter_map(|msg| format_message(msg))
                .collect::<Vec<_>>()
                .join("\n");

            let response = self
                .agent
                .prompt(Message::User {
                    content: OneOrMany::one(UserContent::text(conversation_text)),
                })
                .with_history(vec![Message::system(compact_prompt)])
                .extended_details()
                .await?;

            // Link old → new thread
            {
                let mut threads = self.threads.write().await;
                if let Some(old) = threads.get_mut(&old_thread_id) {
                    old.close();
                }
                if let Some(new) = threads.get_mut(&new_thread_id) {
                    new.prev_thread_id = Some(old_thread_id);
                }
            }

            Ok(response.output)
        })
    }

    fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }
}

// ── AgentBuilder ──

pub struct AgentBuilder {
    preamble: Option<String>,
    model: String,
    base_url: Option<String>,
    api_key: Option<String>,
    max_turns: Option<usize>,
    capabilities: Vec<Capability>,
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self {
            preamble: None,
            model: "gpt-4o-mini".into(),
            base_url: None,
            api_key: None,
            max_turns: Some(10),
            capabilities: Vec::new(),
        }
    }

    pub fn preamble(mut self, preamble: &str) -> Self {
        self.preamble = Some(preamble.into());
        self
    }

    pub fn append_preamble(mut self, doc: &str) -> Self {
        self.preamble = Some(format!("{}\n{}", self.preamble.unwrap_or_default(), doc));
        self
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    pub fn max_turns(mut self, turns: usize) -> Self {
        self.max_turns = Some(turns);
        self
    }

    pub fn capabilities(mut self, caps: Vec<Capability>) -> Self {
        self.capabilities = caps;
        self
    }

    pub fn build(self) -> Result<AgentRuntime<impl CompletionModel>, rig_core::http_client::Error> {
        let api_key = self
            .api_key
            .unwrap_or_else(|| std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set"));

        let mut client_builder = openai::Client::builder().api_key(&api_key);
        if let Some(base_url) = &self.base_url {
            client_builder = client_builder.base_url(base_url);
        }
        let client = client_builder.build()?;

        let mut agent_builder = client.completions_api().agent(&self.model);
        if let Some(preamble) = &self.preamble {
            agent_builder = agent_builder.preamble(preamble);
        }
        let agent = agent_builder
            .default_max_turns(self.max_turns.unwrap_or(10))
            .build();

        let model_config = ModelConfig {
            model: self.model,
            base_url: self.base_url,
            api_key,
            max_turns: self.max_turns.unwrap_or(10),
        };

        Ok(AgentRuntime::new(
            agent,
            Arc::new(RwLock::new(HashMap::new())),
            self.capabilities,
            model_config,
        ))
    }
}

// ── Core run_turn logic (extracted for subagent reuse) ──

async fn run_turn_inner<M: CompletionModel + 'static>(
    agent: &Agent<M>,
    threads: &ThreadStore,
    thread_id: Uuid,
    messages: Vec<Message>,
) -> Result<(String, Usage), AgentError> {
    let mut history = {
        let threads = threads.read().await;
        let thread = threads
            .get(&thread_id)
            .ok_or(AgentError::ThreadNotFound(thread_id))?;
        thread.messages.clone()
    };

    if messages.is_empty() {
        return Err(AgentError::ModelError(
            completion::PromptError::PromptCancelled {
                chat_history: vec![],
                reason: "empty batch".into(),
            },
        ));
    }

    let history_len = history.len();

    let (prepend, prompt) = messages.split_at(messages.len() - 1);
    for msg in prepend {
        history.push(msg.clone());
    }
    let prompt = prompt[0].clone();

    trace!(thread_id = %thread_id, batch_size = messages.len(), "run_turn");

    let response = {
        let max_attempts = 3;
        let mut attempt = 0u32;
        loop {
            match agent
                .prompt(prompt.clone())
                .with_history(history.clone())
                .extended_details()
                .await
            {
                Ok(resp) => break resp,
                Err(e) => {
                    if attempt < max_attempts - 1 && is_rate_limited(&e) {
                        let delay = std::time::Duration::from_millis(1000 * (attempt as u64 + 1));
                        tokio::time::sleep(delay).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(AgentError::ModelError(e));
                }
            }
        }
    };

    if let Some(new_msgs) = response.messages {
        history.extend(new_msgs);
    }

    let reasoning = extract_reasoning(&history[history_len..]);

    {
        let mut threads = threads.write().await;
        if let Some(thread) = threads.get_mut(&thread_id) {
            thread.messages = history;
            thread.last_activity = Utc::now();
        }
    }

    let output = &response.output;
    debug!(thread_id = %thread_id, response_len = output.len(), "run_turn completed");
    if let Some(r) = &reasoning {
        trace!(thread_id = %thread_id, reasoning = %r, "run_turn: model reasoning");
    }
    Ok((response.output, response.usage))
}

// ── Helpers ──

fn extract_reasoning(messages: &[Message]) -> Option<String> {
    for msg in messages.iter().rev() {
        if let Message::Assistant { content, .. } = msg {
            for c in content.iter() {
                if let AssistantContent::Reasoning(reasoning) = c {
                    let text = reasoning.display_text();
                    if !text.is_empty() {
                        return Some(text);
                    }
                }
            }
        }
    }
    None
}

fn format_message(msg: &Message) -> Option<String> {
    match msg {
        Message::System { content } => Some(format!("System: {content}")),
        Message::User { content } => {
            let texts: Vec<String> = content
                .iter()
                .filter_map(|c| match c {
                    UserContent::Text(t) => Some(t.text.clone()),
                    _ => None,
                })
                .collect();
            if texts.is_empty() {
                None
            } else {
                Some(format!("User: {}", texts.join(" ")))
            }
        }
        Message::Assistant { content, .. } => {
            let texts: Vec<String> = content
                .iter()
                .filter_map(|c| match c {
                    AssistantContent::Text(t) => Some(t.text.clone()),
                    AssistantContent::Reasoning(r) => Some(r.display_text()),
                    AssistantContent::ToolCall(tc) => {
                        Some(format!("[tool_call: {}]", tc.function.name))
                    }
                    AssistantContent::Image(_) => None,
                })
                .collect();
            if texts.is_empty() {
                None
            } else {
                Some(format!("Assistant: {}", texts.join(" ")))
            }
        }
    }
}

fn is_rate_limited(err: &completion::PromptError) -> bool {
    match err {
        completion::PromptError::CompletionError(inner) => match inner {
            completion::CompletionError::HttpError(http_err) => match http_err {
                rig_core::http_client::Error::InvalidStatusCode(s) => s.as_u16() == 429,
                rig_core::http_client::Error::InvalidStatusCodeWithMessage(s, _) => {
                    s.as_u16() == 429
                }
                _ => false,
            },
            _ => false,
        },
        _ => false,
    }
}
