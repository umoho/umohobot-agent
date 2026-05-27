use std::sync::Arc;

use rig_core::client::CompletionClient;
use rig_core::providers::openai;

use crate::pool::{Account, ModelPool};
use crate::runtime::AgentRuntime;
use crate::storage::Storage;

pub struct AgentBuilder {
    preamble: Option<String>,
    max_turns: Option<usize>,
    storage: Option<Arc<dyn Storage>>,
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self {
            preamble: None,
            max_turns: Some(10),
            storage: None,
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

    pub fn max_turns(mut self, turns: usize) -> Self {
        self.max_turns = Some(turns);
        self
    }

    pub fn storage(mut self, storage: Arc<dyn Storage>) -> Self {
        self.storage = Some(storage);
        self
    }

    pub fn build(
        self,
        account: &Account,
        model_pool: Arc<ModelPool>,
    ) -> Result<AgentRuntime, rig_core::http_client::Error> {
        let max_turns = self.max_turns.unwrap_or(10);

        let mut client_builder = openai::Client::builder().api_key(&account.api_key);
        client_builder = client_builder.base_url(&account.base_url);
        let client = client_builder.build()?;

        let mut agent_builder = client.completions_api().agent(&account.model);
        if let Some(preamble) = &self.preamble {
            agent_builder = agent_builder.preamble(preamble);
        }
        let agent = agent_builder.default_max_turns(max_turns).build();

        let mut runtime = AgentRuntime::new(
            agent,
            account.capabilities.clone(),
            model_pool,
            Arc::new(account.clone()),
            max_turns,
        );
        if let Some(storage) = self.storage {
            runtime.storage = Some(storage);
        }
        Ok(runtime)
    }
}
