mod thread;

pub use thread::Thread;

pub use rig_core::OneOrMany;
pub use rig_core::completion::Message;
pub use rig_core::completion::message::{
    DocumentSourceKind, Image, ImageDetail, ImageMediaType, UserContent,
};

use chrono::Utc;
use rig_core::agent::Agent;
use rig_core::client::CompletionClient;
use rig_core::completion::{self, AssistantContent, Chat, CompletionModel};
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
        user_message: Message,
    ) -> BoxFuture<'a, Result<String, AgentError>>;
    fn get_or_create_thread<'a>(&'a self, id: Uuid) -> BoxFuture<'a, Thread>;
    fn append_system_message<'a>(&'a self, thread_id: Uuid, text: &'a str) -> BoxFuture<'a, ()>;
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
        user_message: Message,
    ) -> BoxFuture<'a, Result<String, AgentError>> {
        Box::pin(async move {
            let mut messages = {
                let threads = self.threads.read().await;
                let thread = threads
                    .get(&thread_id)
                    .ok_or(AgentError::ThreadNotFound(thread_id))?;
                thread.messages.clone()
            };

            let messages_len = messages.len();

            trace!(thread_id = %thread_id, messages = ?messages, "run_turn: messages before chat");

            let response = {
                let max_attempts = 3;
                let mut attempt = 0u32;
                loop {
                    match self.agent.chat(user_message.clone(), &mut messages).await {
                        Ok(resp) => break resp,
                        Err(e) => {
                            if attempt < max_attempts - 1 && is_rate_limited(&e) {
                                messages.truncate(messages_len);
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

            let reasoning = extract_reasoning(&messages[messages_len..]);

            {
                let mut threads = self.threads.write().await;
                if let Some(thread) = threads.get_mut(&thread_id) {
                    thread.messages = messages;
                    thread.last_activity = Utc::now();
                }
            }

            debug!(thread_id = %thread_id, response_len = response.len(), "run_turn completed");
            if let Some(r) = &reasoning {
                trace!(thread_id = %thread_id, reasoning = %r, "run_turn: model reasoning");
            }
            Ok(response)
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
