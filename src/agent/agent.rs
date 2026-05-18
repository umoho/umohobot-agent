use std::env;

use crate::{
    config::{Config, ProviderKind},
    logging::sanitize_for_log,
    platforms::PlatformMessage,
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

const DEFAULT_PROMPT_PREAMBLE: &str = "你是一个 Telegram AI Bot。输入可能包含正文、附件、回复和@提及等结构化上下文。请结合这些信息使用简洁中文回答，不要输出推理过程。";
const OPENROUTER_API_BASE_URL: &str = "https://openrouter.ai/api/v1";

#[derive(Clone, Debug)]
pub struct AgentRequest {
    pub message: PlatformMessage,
}

impl AgentRequest {
    pub fn new(message: PlatformMessage) -> Self {
        Self { message }
    }

    pub fn prompt_text(&self) -> String {
        self.message.prompt_text()
    }
}

#[derive(Clone, Debug)]
pub struct AgentResponse {
    pub final_text: String,
    pub notes: Vec<String>,
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
        let agent = client
            .agent(self.model.as_str())
            .preamble(DEFAULT_PROMPT_PREAMBLE)
            .build();

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
            format!("input_kind={}", request.message.kind.as_str()),
            format!("input_body_kind={}", request.message.body.kind_label()),
            format!("input_entities={}", request.message.body_entities().len()),
            format!("input_attachments={}", request.message.attachments.len()),
            format!("input_reply={}", request.message.reply.is_some()),
            format!("input_mention={}", request.message.is_mention),
            format!("input_text_present={}", request.message.text().is_some()),
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
    use crate::config::{ProviderConfig, ProviderKind, RuntimeMode};
    use crate::platforms::{
        AttachmentInfo, AttachmentKind, MessageBody, PlatformKind, PlatformMessage,
        PlatformMessageKind, ReplyMetadata,
    };

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
    fn agent_request_prompt_text_preserves_plain_text_messages() {
        let request = AgentRequest::new(PlatformMessage {
            platform: PlatformKind::Telegram,
            room_id: "room".to_string(),
            thread_id: None,
            message_id: "msg".to_string(),
            sender_id: "user".to_string(),
            kind: PlatformMessageKind::Text,
            body: MessageBody::Text {
                text: "你好".to_string(),
                entities: vec![],
            },
            attachments: vec![],
            reply: None,
            is_mention: false,
        });

        assert_eq!(request.prompt_text(), "你好");
    }

    #[test]
    fn agent_request_prompt_text_includes_structured_context() {
        let request = AgentRequest::new(PlatformMessage {
            platform: PlatformKind::Telegram,
            room_id: "room".to_string(),
            thread_id: None,
            message_id: "msg".to_string(),
            sender_id: "user".to_string(),
            kind: PlatformMessageKind::Photo,
            body: MessageBody::Caption {
                text: "看这张图".to_string(),
                entities: vec![],
            },
            attachments: vec![AttachmentInfo {
                kind: AttachmentKind::Image,
                file_id: Some("file-id".to_string()),
                file_unique_id: Some("file-unique-id".to_string()),
                file_name: None,
                mime_type: None,
                url: None,
                width: Some(640),
                height: Some(480),
                size_bytes: Some(1_024),
            }],
            reply: Some(ReplyMetadata {
                message_id: "reply-1".to_string(),
                sender_id: "user-2".to_string(),
                kind: PlatformMessageKind::Text,
                body: MessageBody::Text {
                    text: "原始消息".to_string(),
                    entities: vec![],
                },
                attachments: vec![],
            }),
            is_mention: true,
        });

        let prompt = request.prompt_text();

        assert!(prompt.contains("看这张图"));
        assert!(prompt.contains("message_kind=photo"));
        assert!(prompt.contains("attachments=image"));
        assert!(prompt.contains("reply=message_id=reply-1"));
        assert!(prompt.contains("bot_mentioned=true"));
    }

    #[test]
    fn strip_think_sections_prefers_final_answer_after_think_block() {
        assert_eq!(
            strip_think_sections("<think>reasoning</think>\n\n最终答案"),
            "最终答案"
        );
    }
}
