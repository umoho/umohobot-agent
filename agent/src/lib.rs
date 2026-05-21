mod thread;

pub use thread::Thread;

pub use rig_core::OneOrMany;
pub use rig_core::completion::Message;
pub use rig_core::completion::Usage;
pub use rig_core::completion::message::{
    DocumentSourceKind, Image, ImageDetail, ImageMediaType, UserContent,
};

use chrono::Utc;
use rig_core::agent::Agent;
use rig_core::client::CompletionClient;
use rig_core::completion::{self, AssistantContent, CompletionModel, Prompt};
use rig_core::providers::openai;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, trace};
use uuid::Uuid;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type ThreadStore = Arc<RwLock<HashMap<Uuid, Thread>>>;

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

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("Thread not found: {0}")]
    ThreadNotFound(Uuid),
    #[error("Model error: {0}")]
    ModelError(#[from] completion::PromptError),
}

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
        thread_id: Uuid,
        compact_prompt: &'a str,
    ) -> BoxFuture<'a, Result<String, AgentError>>;
    fn capabilities(&self) -> &[Capability];
}

pub struct AgentRuntime<M: CompletionModel> {
    agent: Agent<M>,
    threads: ThreadStore,
    capabilities: Vec<Capability>,
}

impl<M: CompletionModel + 'static> AgentRuntime<M> {
    pub fn new(agent: Agent<M>, threads: ThreadStore, capabilities: Vec<Capability>) -> Self {
        Self {
            agent,
            threads,
            capabilities,
        }
    }

    pub fn agent(&self) -> &Agent<M> {
        &self.agent
    }
}

impl<M: CompletionModel + 'static> AgentHandle for AgentRuntime<M> {
    fn run_turn<'a>(
        &'a self,
        thread_id: Uuid,
        messages: Vec<Message>,
    ) -> BoxFuture<'a, Result<(String, Usage), AgentError>> {
        Box::pin(async move {
            let mut history = {
                let threads = self.threads.read().await;
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
                    match self
                        .agent
                        .prompt(prompt.clone())
                        .with_history(history.clone())
                        .extended_details()
                        .await
                    {
                        Ok(resp) => break resp,
                        Err(e) => {
                            if attempt < max_attempts - 1 && is_rate_limited(&e) {
                                let delay =
                                    std::time::Duration::from_millis(1000 * (attempt as u64 + 1));
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
                let mut threads = self.threads.write().await;
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
        thread_id: Uuid,
        compact_prompt: &'a str,
    ) -> BoxFuture<'a, Result<String, AgentError>> {
        Box::pin(async move {
            let messages = {
                let threads = self.threads.read().await;
                let thread = threads
                    .get(&thread_id)
                    .ok_or(AgentError::ThreadNotFound(thread_id))?;
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

            Ok(response.output)
        })
    }

    fn capabilities(&self) -> &[Capability] {
        &self.capabilities
    }
}

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
        let mut client_builder =
            openai::Client::builder().api_key(self.api_key.unwrap_or_else(|| {
                std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set")
            }));
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

        Ok(AgentRuntime::new(
            agent,
            Arc::new(RwLock::new(HashMap::new())),
            self.capabilities,
        ))
    }
}

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
