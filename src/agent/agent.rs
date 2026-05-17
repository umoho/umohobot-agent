use crate::{config::Config, tools::ToolRegistry};

#[derive(Clone, Debug)]
pub struct AgentRequest {
    pub input: String,
}

#[derive(Clone, Debug)]
pub struct AgentResponse {
    pub final_text: String,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct AgentRuntime {
    provider_label: String,
    model: String,
    user_provider_enabled: bool,
    max_response_chars: usize,
}

impl AgentRuntime {
    pub fn new(config: &Config) -> Self {
        Self {
            provider_label: format!("{:?}", config.default_provider.kind),
            model: config.default_provider.model.clone(),
            user_provider_enabled: config.allow_user_provider,
            max_response_chars: config.max_response_chars,
        }
    }

    pub fn describe(&self) -> String {
        format!(
            "{}:{}:user_provider={}",
            self.provider_label, self.model, self.user_provider_enabled
        )
    }

    pub fn respond(&self, request: &AgentRequest, tools: &ToolRegistry) -> AgentResponse {
        let mut final_text = if request.input.trim().is_empty() {
            "当前没有收到有效输入，骨架层暂不生成模型回复。".to_string()
        } else {
            format!(
                "已收到消息：{}\n\n这是 Telegram AI Bot 的骨架响应，后续会接入模型、工具和权限层。",
                request.input
            )
        };

        final_text = self.truncate_response(final_text);

        AgentResponse {
            final_text,
            notes: vec![
                format!("provider={}", self.provider_label),
                format!("model={}", self.model),
                format!("user_provider_enabled={}", self.user_provider_enabled),
                format!("registered_tools={}", tools.count()),
            ],
        }
    }

    fn truncate_response(&self, text: String) -> String {
        text.chars().take(self.max_response_chars).collect()
    }
}
