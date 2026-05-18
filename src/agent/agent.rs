use std::env;

use crate::{
    agent::context::PromptContext,
    config::{Config, ProviderKind},
    logging::sanitize_for_log,
    storage::TurnStatus,
    tools::ToolRegistry,
};
use rig::{
    client::CompletionClient,
    client::Nothing,
    completion::Prompt,
    providers::{ollama, openai, openrouter},
};
use thiserror::Error;
use tracing::{debug, error, info};

const OPENROUTER_API_BASE_URL: &str = "https://openrouter.ai/api/v1";

#[derive(Clone, Debug)]
pub struct AgentRequest {
    pub prompt: PromptContext,
}

impl AgentRequest {
    pub fn new(prompt: PromptContext) -> Self {
        Self { prompt }
    }

    pub fn prompt_text(&self) -> &str {
        self.prompt.prompt_text()
    }
}

#[derive(Clone, Debug)]
pub struct AgentResponse {
    pub final_text: String,
    pub notes: Vec<String>,
    pub status: TurnStatus,
    pub error_code: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AgentRuntime {
    provider_kind: ProviderKind,
    provider_backend: ProviderBackend,
    model: String,
    user_provider_enabled: bool,
    max_response_chars: usize,
    base_url: String,
    client: AgentClient,
}

#[derive(Debug, Error)]
pub enum AgentRuntimeError {
    #[error("provider.kind={kind:?} requires provider.api_key_ref")]
    MissingApiKeyRef { kind: ProviderKind },
    #[error("provider.api_key_ref={api_key_ref} is not set or is empty")]
    MissingApiKeyEnv { api_key_ref: String },
    #[error("provider.kind={kind:?} requires provider.base_url")]
    MissingBaseUrl { kind: ProviderKind },
    #[error("failed to build {backend} client: {message}")]
    ClientBuild {
        backend: &'static str,
        message: String,
    },
}

#[derive(Clone, Debug)]
enum ProviderBackend {
    Ollama,
    OpenAICompatible,
    OpenRouter,
}

impl ProviderBackend {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::OpenAICompatible => "openai_compatible",
            Self::OpenRouter => "openrouter",
        }
    }
}

#[derive(Clone, Debug)]
enum AgentClient {
    Ollama(ollama::Client),
    OpenAICompatible(openai::CompletionsClient),
    OpenRouter(openrouter::Client),
}

impl AgentRuntime {
    pub fn new(config: &Config) -> Self {
        Self::try_new(config)
            .unwrap_or_else(|err| panic!("failed to initialize agent runtime: {err}"))
    }

    pub fn try_new(config: &Config) -> Result<Self, AgentRuntimeError> {
        let provider_kind = config.default_provider.kind;
        let base_url =
            Self::resolve_base_url(provider_kind, config.default_provider.base_url.as_deref())?;
        let (client, provider_backend) = Self::build_client(
            provider_kind,
            &base_url,
            config.default_provider.api_key_ref.as_deref(),
        )?;

        let runtime = Self {
            provider_kind,
            provider_backend,
            model: config.default_provider.model.clone(),
            user_provider_enabled: config.allow_user_provider,
            max_response_chars: config.max_response_chars,
            base_url,
            client,
        };

        info!(
            provider = %runtime.provider_backend.as_str(),
            provider_kind = %runtime.provider_kind.as_str(),
            model = %runtime.model,
            base_url = %runtime.base_url,
            user_provider_enabled = runtime.user_provider_enabled,
            max_response_chars = runtime.max_response_chars,
            "agent runtime initialized"
        );

        Ok(runtime)
    }

    pub fn describe(&self) -> String {
        format!(
            "provider={}:kind={}:model={}:user_provider={}",
            self.provider_backend.as_str(),
            self.provider_kind.as_str(),
            self.model,
            self.user_provider_enabled
        )
    }

    pub fn provider_name(&self) -> &'static str {
        self.provider_backend.as_str()
    }

    pub fn model_name(&self) -> &str {
        &self.model
    }

    pub async fn respond(&self, request: &AgentRequest, tools: &ToolRegistry) -> AgentResponse {
        let prompt_text = request.prompt_text();
        if prompt_text.trim().is_empty() {
            debug!(
                provider = %self.provider_backend.as_str(),
                model = %self.model,
                "agent request skipped because prompt was empty"
            );
            return AgentResponse {
                final_text: "当前没有收到有效输入，骨架层暂不生成模型回复。".to_string(),
                notes: self.notes(tools, request, Some("empty_input")),
                status: TurnStatus::Completed,
                error_code: None,
            };
        }

        info!(
            provider = %self.provider_backend.as_str(),
            model = %self.model,
            input_chars = prompt_text.chars().count(),
            tool_count = tools.count(),
            "agent request started"
        );

        let response = match &self.client {
            AgentClient::Ollama(client) => self.query_client(client, &prompt_text).await,
            AgentClient::OpenAICompatible(client) => self.query_client(client, &prompt_text).await,
            AgentClient::OpenRouter(client) => self.query_client(client, &prompt_text).await,
        };

        match response {
            Ok(response) => {
                let cleaned = strip_think_sections(&response);
                let response_chars = cleaned.chars().count();
                let truncated = response_chars > self.max_response_chars;

                info!(
                    provider = %self.provider_backend.as_str(),
                    model = %self.model,
                    response_chars,
                    truncated,
                    "agent request completed"
                );

                AgentResponse {
                    final_text: self.truncate_response(cleaned),
                    notes: self.notes(tools, request, Some("rig_prompt")),
                    status: TurnStatus::Completed,
                    error_code: None,
                }
            }
            Err(err) => AgentResponse {
                final_text: {
                    let user_message = format!("rig 请求失败：{err}");
                    error!(
                        provider = %self.provider_backend.as_str(),
                        model = %self.model,
                        error = %sanitize_for_log(&err),
                        user_message = %sanitize_for_log(&user_message),
                        "agent request failed"
                    );
                    user_message
                },
                notes: self.notes(tools, request, Some("provider_error")),
                status: TurnStatus::Failed,
                error_code: Some("provider_error".to_string()),
            },
        }
    }

    fn truncate_response(&self, text: String) -> String {
        text.chars().take(self.max_response_chars).collect()
    }

    async fn query_client<C>(&self, client: &C, input: &str) -> Result<String, String>
    where
        C: CompletionClient,
    {
        let agent = client.agent(self.model.as_str()).build();

        agent.prompt(input).await.map_err(|err| err.to_string())
    }

    fn notes(
        &self,
        tools: &ToolRegistry,
        request: &AgentRequest,
        done_reason: Option<&str>,
    ) -> Vec<String> {
        let mut notes = vec![
            format!("provider={}", self.provider_backend.as_str()),
            format!("provider_kind={}", self.provider_kind.as_str()),
            format!("model={}", self.model),
            format!("base_url={}", self.base_url),
            format!("user_provider_enabled={}", self.user_provider_enabled),
            format!("registered_tools={}", tools.count()),
            format!("prompt_version={}", request.prompt.prompt_version),
            format!(
                "prompt_estimated_tokens={}",
                request.prompt.estimated_tokens
            ),
            format!("prompt_sections={}", request.prompt.sections.len()),
            format!("prompt_recent_events={}", request.prompt.recent_event_count),
            format!(
                "prompt_recent_events_trimmed={}",
                request.prompt.trimmed_recent_event_count
            ),
            format!("prompt_summary_present={}", request.prompt.summary_present),
            format!("thread_id={}", request.prompt.thread_id),
            format!("thread_key={}", request.prompt.thread_key),
            format!("thread_state={}", request.prompt.thread_state),
            format!("input_kind={}", request.prompt.current_turn.kind.as_str()),
            format!("input_body_kind={}", request.prompt.current_turn.body_kind),
            format!(
                "input_entities={}",
                request.prompt.current_turn.entity_count
            ),
            format!(
                "input_attachments={}",
                request.prompt.current_turn.attachment_count
            ),
            format!("input_reply={}", request.prompt.current_turn.reply_present),
            format!("input_mention={}", request.prompt.current_turn.mention),
            format!(
                "input_text_present={}",
                request.prompt.current_turn.text_present
            ),
            format!(
                "input_speaker={}",
                request.prompt.current_turn.sender.label()
            ),
        ];
        if let Some(done_reason) = done_reason {
            notes.push(format!("done_reason={done_reason}"));
        }
        notes
    }

    fn build_client(
        provider_kind: ProviderKind,
        base_url: &str,
        api_key_ref: Option<&str>,
    ) -> Result<(AgentClient, ProviderBackend), AgentRuntimeError> {
        if matches!(provider_kind, ProviderKind::Ollama) {
            let client = ollama::Client::builder()
                .api_key(Nothing)
                .base_url(base_url)
                .build()
                .map_err(|message| AgentRuntimeError::ClientBuild {
                    backend: "ollama",
                    message: message.to_string(),
                })?;

            return Ok((AgentClient::Ollama(client), ProviderBackend::Ollama));
        }

        let api_key_ref = api_key_ref.ok_or(AgentRuntimeError::MissingApiKeyRef {
            kind: provider_kind,
        })?;
        let api_key = read_api_key(api_key_ref)?;

        if is_openrouter_base_url(base_url) {
            let client = openrouter::Client::builder()
                .api_key(api_key)
                .base_url(base_url)
                .build()
                .map_err(|message| AgentRuntimeError::ClientBuild {
                    backend: "openrouter",
                    message: message.to_string(),
                })?;

            Ok((AgentClient::OpenRouter(client), ProviderBackend::OpenRouter))
        } else {
            let client = openai::CompletionsClient::builder()
                .api_key(api_key)
                .base_url(base_url)
                .build()
                .map_err(|message| AgentRuntimeError::ClientBuild {
                    backend: "openai_compatible",
                    message: message.to_string(),
                })?;

            Ok((
                AgentClient::OpenAICompatible(client),
                ProviderBackend::OpenAICompatible,
            ))
        }
    }

    fn resolve_base_url(
        provider_kind: ProviderKind,
        explicit_base_url: Option<&str>,
    ) -> Result<String, AgentRuntimeError> {
        let normalized = explicit_base_url
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(normalize_remote_base_url)
            .or_else(|| {
                provider_kind
                    .default_base_url()
                    .map(normalize_remote_base_url)
            })
            .ok_or(AgentRuntimeError::MissingBaseUrl {
                kind: provider_kind,
            })?;

        Ok(match provider_kind {
            ProviderKind::Ollama => normalize_ollama_base_url(&normalized),
            _ => normalized,
        })
    }
}

fn read_api_key(api_key_ref: &str) -> Result<String, AgentRuntimeError> {
    let api_key = env::var(api_key_ref).map_err(|_| AgentRuntimeError::MissingApiKeyEnv {
        api_key_ref: api_key_ref.to_string(),
    })?;

    if api_key.trim().is_empty() {
        return Err(AgentRuntimeError::MissingApiKeyEnv {
            api_key_ref: api_key_ref.to_string(),
        });
    }

    Ok(api_key)
}

fn normalize_remote_base_url(base_url: &str) -> String {
    base_url.trim().trim_end_matches('/').to_string()
}

fn normalize_ollama_base_url(base_url: &str) -> String {
    let trimmed = normalize_remote_base_url(base_url);
    trimmed.strip_suffix("/api").unwrap_or(&trimmed).to_string()
}

fn is_openrouter_base_url(base_url: &str) -> bool {
    normalize_remote_base_url(base_url) == OPENROUTER_API_BASE_URL
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
    use crate::agent::context::{
        PromptContext, PromptCurrentTurn, PromptSection, PromptSectionKind, PromptSpeaker,
    };
    use crate::config::{PromptConfig, ProviderConfig, ProviderKind, RuntimeMode};
    use crate::platforms::{PlatformKind, PlatformMessageKind};

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
    fn normalize_remote_base_url_trims_trailing_slashes() {
        assert_eq!(
            normalize_remote_base_url("https://openrouter.ai/api/v1/"),
            "https://openrouter.ai/api/v1"
        );
    }

    #[test]
    fn openrouter_detection_accepts_trailing_slashes() {
        assert!(is_openrouter_base_url("https://openrouter.ai/api/v1/"));
        assert!(!is_openrouter_base_url("https://example.com/v1"));
    }

    #[test]
    fn resolve_base_url_defaults_for_ollama_and_openai() {
        assert_eq!(
            AgentRuntime::resolve_base_url(ProviderKind::Ollama, None).unwrap(),
            "http://127.0.0.1:11434"
        );
        assert_eq!(
            AgentRuntime::resolve_base_url(ProviderKind::OpenAI, None).unwrap(),
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn resolve_base_url_requires_explicit_base_url_for_openai_compatible() {
        assert!(matches!(
            AgentRuntime::resolve_base_url(ProviderKind::OpenAICompatible, None),
            Err(AgentRuntimeError::MissingBaseUrl {
                kind: ProviderKind::OpenAICompatible
            })
        ));
    }

    #[test]
    fn agent_runtime_try_new_builds_openrouter_client_from_env_ref() {
        let api_key_ref = "UMOHOBOT_TEST_OPENROUTER_API_KEY";
        unsafe {
            std::env::set_var(api_key_ref, "dummy-openrouter-key");
        }

        let config = Config {
            bot_name: "bot".to_string(),
            runtime_mode: RuntimeMode::Local,
            telegram_bot_token: None,
            default_provider: ProviderConfig {
                kind: ProviderKind::OpenAICompatible,
                base_url: Some("https://openrouter.ai/api/v1".to_string()),
                model: "openai/gpt-4o-mini".to_string(),
                api_key_ref: Some(api_key_ref.to_string()),
            },
            allow_user_provider: false,
            max_response_chars: 4_000,
            message_edit_throttle_ms: 750,
            placeholder_text: "正在处理...".to_string(),
            prompt: PromptConfig::default(),
            data_dir: None,
        };

        let runtime = AgentRuntime::try_new(&config).expect("openrouter client should build");

        assert!(runtime.describe().contains("provider=openrouter"));
        assert!(runtime.describe().contains("kind=openai_compatible"));

        unsafe {
            std::env::remove_var(api_key_ref);
        }
    }

    #[test]
    fn agent_request_prompt_text_uses_rendered_prompt_context() {
        let request = AgentRequest::new(prompt_context("rendered prompt"));

        assert_eq!(request.prompt_text(), "rendered prompt");
    }

    #[test]
    fn strip_think_sections_prefers_final_answer_after_think_block() {
        assert_eq!(
            strip_think_sections("<think>reasoning</think>\n\n最终答案"),
            "最终答案"
        );
    }

    fn prompt_context(rendered_prompt_text: &str) -> PromptContext {
        PromptContext {
            prompt_version: 1,
            thread_id: 1,
            thread_key: "telegram:room".to_string(),
            thread_state: "active".to_string(),
            summary_present: false,
            current_turn: PromptCurrentTurn {
                event_id: 1,
                message_id: "msg".to_string(),
                seq: 1,
                sender: PromptSpeaker::new("user", Some("Alice".to_string())),
                platform: PlatformKind::Telegram.as_str().to_string(),
                room_id: "room".to_string(),
                thread_id: None,
                kind: PlatformMessageKind::Text.as_str().to_string(),
                body_kind: "text".to_string(),
                text_present: true,
                text_chars: 5,
                entity_count: 0,
                attachment_count: 0,
                reply_present: false,
                reply_message_id: None,
                reply_sender_id: None,
                mention: false,
                body_text: "hello".to_string(),
                rendered_text: "hello".to_string(),
                estimated_tokens: 1,
                trimmed: false,
            },
            recent_events: vec![],
            tools: vec![],
            sections: vec![PromptSection::new(
                PromptSectionKind::SystemRules,
                "system rules",
                false,
            )],
            rendered_prompt: rendered_prompt_text.to_string(),
            estimated_tokens: rendered_prompt_text.chars().count(),
            recent_event_count: 0,
            trimmed_recent_event_count: 0,
            trimmed_summary_chars: 0,
            trimmed_current_turn_chars: 0,
            trimmed_tool_catalog_chars: 0,
            trimmed_response_policy_chars: 0,
            trimmed_system_rules_chars: 0,
        }
    }
}
