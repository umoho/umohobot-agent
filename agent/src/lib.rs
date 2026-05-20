mod thread;

pub use thread::Thread;

use chrono::Utc;
use rig_core::agent::Agent;
use rig_core::client::CompletionClient;
use rig_core::completion::{self, Chat, CompletionModel, Message};
use rig_core::providers::openai;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;
use uuid::Uuid;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type ThreadStore = Arc<RwLock<HashMap<Uuid, Thread>>>;

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
        user_message: &'a str,
    ) -> BoxFuture<'a, Result<String, AgentError>>;
    fn get_or_create_thread<'a>(
        &'a self,
        id: Uuid,
        system_prompt: &'a str,
    ) -> BoxFuture<'a, Thread>;
    fn system_prompt(&self) -> &str;
}

pub struct AgentRuntime<M: CompletionModel> {
    agent: Agent<M>,
    threads: ThreadStore,
    system_prompt: String,
}

impl<M: CompletionModel + 'static> AgentRuntime<M> {
    pub fn new(agent: Agent<M>, threads: ThreadStore, system_prompt: impl Into<String>) -> Self {
        Self {
            agent,
            threads,
            system_prompt: system_prompt.into(),
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
        user_message: &'a str,
    ) -> BoxFuture<'a, Result<String, AgentError>> {
        Box::pin(async move {
            let mut messages = {
                let threads = self.threads.read().await;
                let thread = threads
                    .get(&thread_id)
                    .ok_or(AgentError::ThreadNotFound(thread_id))?;
                thread.messages.clone()
            };

            let prompt = Message::user(user_message);
            let response = self.agent.chat(prompt, &mut messages).await?;

            {
                let mut threads = self.threads.write().await;
                if let Some(thread) = threads.get_mut(&thread_id) {
                    thread.messages = messages;
                    thread.last_activity = Utc::now();
                }
            }

            debug!(thread_id = %thread_id, "run_turn completed");
            Ok(response)
        })
    }

    fn get_or_create_thread<'a>(
        &'a self,
        id: Uuid,
        system_prompt: &'a str,
    ) -> BoxFuture<'a, Thread> {
        Box::pin(async move {
            let threads = self.threads.read().await;
            if let Some(thread) = threads.get(&id) {
                return thread.clone();
            }
            drop(threads);

            let mut threads = self.threads.write().await;
            let thread = Thread::new(system_prompt);
            let clone = thread.clone();
            threads.insert(id, thread);
            clone
        })
    }

    fn system_prompt(&self) -> &str {
        &self.system_prompt
    }
}

pub struct AgentBuilder {
    system_prompt: String,
    model: String,
    base_url: Option<String>,
    api_key: Option<String>,
    max_turns: Option<usize>,
}

impl AgentBuilder {
    pub fn new(system_prompt: impl Into<String>) -> Self {
        Self {
            system_prompt: system_prompt.into(),
            model: "gpt-4o-mini".into(),
            base_url: None,
            api_key: None,
            max_turns: Some(10),
        }
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

    pub fn build(self) -> Result<AgentRuntime<impl CompletionModel>, rig_core::http_client::Error> {
        let mut client_builder =
            openai::Client::builder().api_key(self.api_key.unwrap_or_else(|| {
                std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set")
            }));
        if let Some(base_url) = &self.base_url {
            client_builder = client_builder.base_url(base_url);
        }
        let client = client_builder.build()?;

        let agent = client
            .agent(&self.model)
            .without_preamble()
            .default_max_turns(self.max_turns.unwrap_or(10))
            .build();

        Ok(AgentRuntime::new(
            agent,
            Arc::new(RwLock::new(HashMap::new())),
            &self.system_prompt,
        ))
    }
}
