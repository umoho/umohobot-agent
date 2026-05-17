use crate::{config::Config, tools::ToolRegistry};

#[derive(Clone, Debug)]
pub struct AgentRequest {
    pub input: String,
}

#[derive(Clone, Debug)]
pub struct AgentResponse {
    pub draft: String,
    pub tool_calls: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct AgentRuntime {
    provider_label: String,
    model: String,
    user_provider_enabled: bool,
}

impl AgentRuntime {
    pub fn new(config: &Config) -> Self {
        Self {
            provider_label: format!("{:?}", config.default_provider.kind),
            model: config.default_provider.model.clone(),
            user_provider_enabled: config.allow_user_provider,
        }
    }

    pub fn describe(&self) -> String {
        format!(
            "{}:{}:user_provider={}",
            self.provider_label, self.model, self.user_provider_enabled
        )
    }

    pub fn respond(&self, request: &AgentRequest, tools: &ToolRegistry) -> AgentResponse {
        let tool_names = tools.names().collect::<Vec<_>>().join(", ");
        AgentResponse {
            draft: format!("skeleton agent received: {}", request.input),
            tool_calls: if tool_names.is_empty() {
                Vec::new()
            } else {
                vec![format!("available_tools={tool_names}")]
            },
        }
    }
}
