use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use rig_core::client::CompletionClient;
use rig_core::completion::CompletionModel;
use rig_core::providers::openai;

use crate::capability::Capability;
use crate::runtime::AgentRuntime;
use crate::types::ModelConfig;

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
