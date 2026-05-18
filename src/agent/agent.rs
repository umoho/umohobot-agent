use crate::{config::Config, tools::ToolRegistry};
use rig::{
    client::{CompletionClient, Nothing},
    completion::Prompt,
    providers::ollama,
};

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
    base_url: String,
    client: ollama::Client,
}

impl AgentRuntime {
    pub fn new(config: &Config) -> Self {
        let base_url = config
            .default_provider
            .base_url
            .clone()
            .unwrap_or_else(|| "http://127.0.0.1:11434".to_string());
        let base_url = normalize_ollama_base_url(&base_url);
        let client = ollama::Client::builder()
            .api_key(Nothing)
            .base_url(&base_url)
            .build()
            .unwrap_or_else(|_| {
                ollama::Client::new(Nothing).unwrap_or_else(|err| {
                    panic!("failed to build Ollama client: {err}");
                })
            });

        Self {
            provider_label: format!("rig::{:?}", config.default_provider.kind),
            model: config.default_provider.model.clone(),
            user_provider_enabled: config.allow_user_provider,
            max_response_chars: config.max_response_chars,
            base_url,
            client,
        }
    }

    pub fn describe(&self) -> String {
        format!(
            "{}:{}:user_provider={}",
            self.provider_label, self.model, self.user_provider_enabled
        )
    }

    pub async fn respond(&self, request: &AgentRequest, tools: &ToolRegistry) -> AgentResponse {
        if request.input.trim().is_empty() {
            return AgentResponse {
                final_text: "当前没有收到有效输入，骨架层暂不生成模型回复。".to_string(),
                notes: self.notes(tools, Some("empty_input")),
            };
        }

        match self.query_ollama(&request.input).await {
            Ok(response) => {
                let cleaned = strip_think_sections(&response);
                let final_text = cleaned;

                AgentResponse {
                    final_text: self.truncate_response(final_text),
                    notes: self.notes(tools, Some("rig_prompt")),
                }
            }
            Err(err) => AgentResponse {
                final_text: format!("rig Ollama 请求失败：{err}"),
                notes: self.notes(tools, Some("ollama_error")),
            },
        }
    }

    fn truncate_response(&self, text: String) -> String {
        text.chars().take(self.max_response_chars).collect()
    }

    async fn query_ollama(&self, input: &str) -> Result<String, String> {
        let agent = self
            .client
            .agent(self.model.as_str())
            .preamble("你是一个 Telegram AI Bot。请使用简洁中文回答，不要输出推理过程。")
            .build();

        agent.prompt(input).await.map_err(|err| err.to_string())
    }

    fn notes(&self, tools: &ToolRegistry, done_reason: Option<&str>) -> Vec<String> {
        let mut notes = vec![
            format!("provider={}", self.provider_label),
            format!("model={}", self.model),
            format!("base_url={}", self.base_url),
            format!("user_provider_enabled={}", self.user_provider_enabled),
            format!("registered_tools={}", tools.count()),
        ];
        if let Some(done_reason) = done_reason {
            notes.push(format!("done_reason={done_reason}"));
        }
        notes
    }
}

fn normalize_ollama_base_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    trimmed.strip_suffix("/api").unwrap_or(trimmed).to_string()
}

fn strip_think_sections(text: &str) -> String {
    if let Some(close_index) = text.find("</think>") {
        let after = text[close_index + "</think>".len()..].trim();
        if !after.is_empty() {
            return after.to_string();
        }
    }

    text.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_ollama_base_url_strips_api_suffix() {
        assert_eq!(
            normalize_ollama_base_url("http://localhost:11434"),
            "http://localhost:11434"
        );
        assert_eq!(
            normalize_ollama_base_url("http://localhost:11434/api"),
            "http://localhost:11434"
        );
    }

    #[test]
    fn strip_think_sections_prefers_final_answer_after_think_block() {
        assert_eq!(
            strip_think_sections("<think>reasoning</think>\n\n最终答案"),
            "最终答案"
        );
    }
}
