use std::env;

use crate::{
    config::{Config, ProviderKind},
    logging::sanitize_for_log,
    platforms::{PlatformKind, ReplyHandle},
    storage::TurnStatus,
    tools::ToolRegistry,
    tools::protocol::{ChatBatchResult, MessageRef},
};
use rig::{
    OneOrMany,
    client::CompletionClient,
    client::Nothing,
    completion::{AssistantContent, Completion, Message as RigMessage, Usage as RigUsage},
    providers::{ollama, openai, openrouter},
    tool::server::{ToolServer, ToolServerHandle},
};
use thiserror::Error;
use tracing::{debug, error, info, warn};

const OPENROUTER_API_BASE_URL: &str = "https://openrouter.ai/api/v1";

pub use crate::agent::context::AgentRequest;

#[derive(Clone, Debug)]
pub struct AgentResponse {
    pub final_text: String,
    pub final_message: Option<ReplyHandle>,
    pub notes: Vec<String>,
    pub status: TurnStatus,
    pub error_code: Option<String>,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub tool_calls: u64,
    pub estimated_usage: bool,
}

#[derive(Clone)]
pub struct AgentRuntime {
    provider_kind: ProviderKind,
    provider_backend: ProviderBackend,
    model: String,
    user_provider_enabled: bool,
    max_response_chars: usize,
    tool_loop_max_turns: usize,
    base_url: String,
    client: AgentClient,
    tool_server_handle: ToolServerHandle,
}

impl std::fmt::Debug for AgentRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentRuntime")
            .field("provider_kind", &self.provider_kind)
            .field("provider_backend", &self.provider_backend)
            .field("model", &self.model)
            .field("user_provider_enabled", &self.user_provider_enabled)
            .field("max_response_chars", &self.max_response_chars)
            .field("tool_loop_max_turns", &self.tool_loop_max_turns)
            .field("base_url", &self.base_url)
            .finish()
    }
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
        Self::try_new_with_tool_server(config, ToolServer::new().run())
    }

    pub fn try_new_with_tool_server(
        config: &Config,
        tool_server_handle: ToolServerHandle,
    ) -> Result<Self, AgentRuntimeError> {
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
            tool_loop_max_turns: config.tool_loop_max_turns,
            base_url,
            client,
            tool_server_handle,
        };

        info!(
            provider = %runtime.provider_backend.as_str(),
            provider_kind = %runtime.provider_kind.as_str(),
            model = %runtime.model,
            base_url = %runtime.base_url,
            user_provider_enabled = runtime.user_provider_enabled,
            max_response_chars = runtime.max_response_chars,
            tool_loop_max_turns = runtime.tool_loop_max_turns,
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
                final_message: None,
                notes: self.notes(tools, request, Some("empty_input"), None),
                status: TurnStatus::Completed,
                error_code: None,
                prompt_tokens: 0,
                completion_tokens: 0,
                tool_calls: 0,
                estimated_usage: true,
            };
        }

        info!(
            provider = %self.provider_backend.as_str(),
            model = %self.model,
            input_chars = prompt_text.chars().count(),
            history_messages = request.chat_history.len(),
            tool_count = tools.count(),
            tool_loop_max_turns = self.tool_loop_max_turns,
            "agent request started"
        );

        let response = match &self.client {
            AgentClient::Ollama(client) => self.query_client(client, request, tools).await,
            AgentClient::OpenAICompatible(client) => {
                self.query_client(client, request, tools).await
            }
            AgentClient::OpenRouter(client) => self.query_client(client, request, tools).await,
        };

        match response {
            Ok(response) => response,
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
                final_message: None,
                notes: self.notes(tools, request, Some("provider_error"), None),
                status: TurnStatus::Failed,
                error_code: Some("provider_error".to_string()),
                prompt_tokens: 0,
                completion_tokens: 0,
                tool_calls: 0,
                estimated_usage: true,
            },
        }
    }

    fn truncate_response(&self, text: String) -> String {
        text.chars().take(self.max_response_chars).collect()
    }

    async fn query_client<C>(
        &self,
        client: &C,
        request: &AgentRequest,
        tools: &ToolRegistry,
    ) -> Result<AgentResponse, String>
    where
        C: CompletionClient,
    {
        let agent = client
            .agent(self.model.as_str())
            .tool_server_handle(self.tool_server_handle.clone())
            .default_max_turns(self.tool_loop_max_turns)
            .build();

        let mut conversation = request.chat_history.clone();
        conversation.push(request.prompt.clone());

        let mut usage = RigUsage::default();
        let mut tool_calls_total = 0u64;
        let mut turn_index = 0usize;
        let mut recovered_final_message: Option<ReplyHandle> = None;
        let mut recovered_final_text: Option<String> = None;

        loop {
            if turn_index > self.tool_loop_max_turns {
                let message = format!("模型超过工具循环深度上限：{}", self.tool_loop_max_turns);
                warn!(
                    provider = %self.provider_backend.as_str(),
                    model = %self.model,
                    turn_index,
                    tool_loop_max_turns = self.tool_loop_max_turns,
                    "agent tool loop exhausted"
                );
                return Ok(self.protocol_error_response(
                    request,
                    tools,
                    Some("tool_loop_exhausted"),
                    &message,
                    usage,
                    tool_calls_total,
                ));
            }

            let prompt = conversation
                .last()
                .cloned()
                .expect("conversation must always contain the current prompt");
            let history = conversation[..conversation.len() - 1].to_vec();

            let completion_request = agent
                .completion(prompt.clone(), history)
                .await
                .map_err(|err| err.to_string())?
                .additional_params_opt(request.additional_params.clone());

            let response = completion_request
                .send()
                .await
                .map_err(|err| err.to_string())?;

            usage += response.usage;
            let tool_calls = assistant_tool_calls(&response.choice);
            let response_text = completion_choice_text(&response.choice);

            if tool_calls.is_empty() {
                let final_text = recovered_final_text
                    .clone()
                    .unwrap_or_else(|| response_text.clone());

                if recovered_final_text.is_none() && tools.count() > 0 {
                    let message = "模型返回了纯文本，但当前启用了工具优先协议".to_string();
                    warn!(
                        provider = %self.provider_backend.as_str(),
                        model = %self.model,
                        "agent violated tool-first protocol"
                    );
                    return Ok(self.protocol_error_response(
                        request,
                        tools,
                        Some("protocol_error"),
                        &message,
                        usage,
                        tool_calls_total,
                    ));
                }

                if final_text.trim().is_empty() {
                    let message = "模型返回了空白结果".to_string();
                    warn!(
                        provider = %self.provider_backend.as_str(),
                        model = %self.model,
                        "agent produced empty response"
                    );
                    return Ok(self.protocol_error_response(
                        request,
                        tools,
                        Some("empty_response"),
                        &message,
                        usage,
                        tool_calls_total,
                    ));
                }

                let final_text = self.truncate_response(final_text);
                let response_chars = final_text.chars().count();
                let truncated = response_chars > self.max_response_chars;

                info!(
                    provider = %self.provider_backend.as_str(),
                    model = %self.model,
                    response_chars,
                    truncated,
                    tool_calls = tool_calls_total,
                    "agent request completed"
                );

                return Ok(AgentResponse {
                    final_text,
                    final_message: recovered_final_message.clone(),
                    notes: self.notes(tools, request, Some("rig_completion"), Some(&usage)),
                    status: TurnStatus::Completed,
                    error_code: None,
                    prompt_tokens: usage.input_tokens,
                    completion_tokens: usage.output_tokens,
                    tool_calls: tool_calls_total,
                    estimated_usage: usage.total_tokens == 0,
                });
            }

            tool_calls_total += tool_calls.len() as u64;

            conversation.push(RigMessage::Assistant {
                id: response.message_id.clone(),
                content: response.choice.clone(),
            });

            let mut tool_results = Vec::with_capacity(tool_calls.len());
            for tool_call in tool_calls {
                let tool_name = tool_call.function.name.clone();
                let arguments = match serde_json::to_string(&tool_call.function.arguments) {
                    Ok(arguments) => arguments,
                    Err(err) => {
                        let message = format!("工具参数序列化失败：{err}");
                        warn!(
                            provider = %self.provider_backend.as_str(),
                            model = %self.model,
                            tool_name = %tool_name,
                            error = %sanitize_for_log(&message),
                            "tool arguments could not be serialized"
                        );
                        return Ok(self.protocol_error_response(
                            request,
                            tools,
                            Some("tool_error"),
                            &message,
                            usage,
                            tool_calls_total,
                        ));
                    }
                };
                let output = match self
                    .tool_server_handle
                    .call_tool(&tool_name, &arguments)
                    .await
                {
                    Ok(output) => output,
                    Err(err) => {
                        let message = format!("工具执行失败：{err}");
                        warn!(
                            provider = %self.provider_backend.as_str(),
                            model = %self.model,
                            tool_name = %tool_name,
                            error = %sanitize_for_log(&message),
                            "tool execution failed"
                        );
                        return Ok(self.protocol_error_response(
                            request,
                            tools,
                            Some("tool_error"),
                            &message,
                            usage,
                            tool_calls_total,
                        ));
                    }
                };

                if tool_name == "chat.batch" {
                    if let Some((final_message, final_visible_text)) =
                        chat_batch_final_output(&output)
                    {
                        if final_message.is_some() {
                            recovered_final_message = final_message;
                        }
                        if final_visible_text.is_some() {
                            recovered_final_text = final_visible_text;
                        }
                    }
                }

                let tool_result = match tool_call.call_id.clone() {
                    Some(call_id) => RigMessage::tool_result_with_call_id(
                        tool_call.id.clone(),
                        Some(call_id),
                        output,
                    ),
                    None => RigMessage::tool_result(tool_call.id.clone(), output),
                };
                tool_results.push(tool_result);
                debug!(
                    provider = %self.provider_backend.as_str(),
                    model = %self.model,
                    tool_name = %tool_name,
                    tool_call_id = %tool_call.id,
                    "tool call executed"
                );
            }

            conversation.extend(tool_results);
            turn_index += 1;
        }
    }

    fn notes(
        &self,
        tools: &ToolRegistry,
        request: &AgentRequest,
        done_reason: Option<&str>,
        usage: Option<&RigUsage>,
    ) -> Vec<String> {
        let prompt_text = request.prompt_text();
        let mut notes = vec![
            format!("provider={}", self.provider_backend.as_str()),
            format!("provider_kind={}", self.provider_kind.as_str()),
            format!("model={}", self.model),
            format!("base_url={}", self.base_url),
            format!("user_provider_enabled={}", self.user_provider_enabled),
            format!("registered_tools={}", tools.count()),
            format!("tool_loop_max_turns={}", self.tool_loop_max_turns),
            format!("prompt_version={}", request.prompt_version),
            format!("prompt_estimated_tokens={}", request.estimated_tokens),
            format!("prompt_recent_events={}", request.recent_event_count),
            format!(
                "prompt_recent_events_trimmed={}",
                request.trimmed_recent_event_count
            ),
            format!("prompt_summary_present={}", request.summary_present),
            format!("prompt_history_messages={}", request.chat_history.len()),
            format!("prompt_text_chars={}", prompt_text.chars().count()),
            format!("prompt_tool_count={}", request.tool_count),
            format!("thread_id={}", request.thread_id),
            format!("thread_key={}", request.thread_key),
            format!("thread_state={}", request.thread_state),
            format!("input_role=user"),
            format!("input_text_present={}", !prompt_text.trim().is_empty()),
        ];
        if let Some(usage) = usage {
            notes.push(format!("usage_prompt_tokens={}", usage.input_tokens));
            notes.push(format!("usage_completion_tokens={}", usage.output_tokens));
            notes.push(format!("usage_total_tokens={}", usage.total_tokens));
            notes.push(format!(
                "usage_cached_input_tokens={}",
                usage.cached_input_tokens
            ));
            notes.push(format!("usage_estimated={}", usage.total_tokens == 0));
        }
        if let Some(done_reason) = done_reason {
            notes.push(format!("done_reason={done_reason}"));
        }
        notes
    }

    fn protocol_error_response(
        &self,
        request: &AgentRequest,
        tools: &ToolRegistry,
        error_code: Option<&str>,
        message: &str,
        usage: RigUsage,
        tool_calls: u64,
    ) -> AgentResponse {
        let final_text = self.truncate_response(message.to_string());
        AgentResponse {
            final_text,
            final_message: None,
            notes: self.notes(tools, request, error_code, Some(&usage)),
            status: TurnStatus::Failed,
            error_code: error_code.map(str::to_string),
            prompt_tokens: usage.input_tokens,
            completion_tokens: usage.output_tokens,
            tool_calls,
            estimated_usage: usage.total_tokens == 0,
        }
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

fn completion_choice_text(choice: &OneOrMany<AssistantContent>) -> String {
    choice
        .iter()
        .filter_map(|content| match content {
            AssistantContent::Text(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn assistant_tool_calls(choice: &OneOrMany<AssistantContent>) -> Vec<rig::message::ToolCall> {
    choice
        .iter()
        .filter_map(|content| match content {
            AssistantContent::ToolCall(tool_call) => Some(tool_call.clone()),
            _ => None,
        })
        .collect()
}

fn reply_handle_from_message_ref(message_ref: &MessageRef) -> Option<ReplyHandle> {
    Some(ReplyHandle {
        platform: PlatformKind::from_str(message_ref.platform.as_str())?,
        room_id: message_ref.room_id.clone(),
        message_id: message_ref.message_id.clone(),
    })
}

fn chat_batch_final_output(output: &str) -> Option<(Option<ReplyHandle>, Option<String>)> {
    let result: ChatBatchResult = serde_json::from_str(output).ok()?;
    let final_message = result
        .final_message
        .as_ref()
        .and_then(reply_handle_from_message_ref);
    let final_visible_text = result.final_visible_text.clone();

    if final_message.is_none() && final_visible_text.is_none() {
        return None;
    }

    Some((final_message, final_visible_text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::context::AgentRequest;
    use crate::config::{PromptConfig, ProviderConfig, ProviderKind, RuntimeMode};
    use crate::tools::protocol::{ChatBatchResult, MessageRef, ToolContext};
    use rig::OneOrMany;
    use rig::message::{AssistantContent, Reasoning};

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
            placeholder_text: "正在处理...".to_string(),
            message_edit_throttle_ms: 750,
            tool_loop_max_turns: 3,
            default_provider: ProviderConfig {
                kind: ProviderKind::OpenAICompatible,
                base_url: Some("https://openrouter.ai/api/v1".to_string()),
                model: "openai/gpt-4o-mini".to_string(),
                api_key_ref: Some(api_key_ref.to_string()),
            },
            allow_user_provider: false,
            max_response_chars: 4_000,
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
    fn agent_request_prompt_text_uses_structured_message() {
        let request = agent_request("rendered prompt");

        assert_eq!(request.prompt_text(), "rendered prompt");
    }

    #[test]
    fn completion_choice_text_prefers_visible_answer_over_reasoning() {
        let choice = OneOrMany::many(vec![
            AssistantContent::Reasoning(Reasoning::new("reasoning")),
            AssistantContent::text("最终答案"),
        ])
        .unwrap();

        assert_eq!(completion_choice_text(&choice), "最终答案");
    }

    #[test]
    fn chat_batch_output_recovers_final_message_and_text() {
        let result = ChatBatchResult {
            context: ToolContext::new("telegram", "telegram:room", "room-1"),
            requested_count: 1,
            planned_count: 1,
            rejected_count: 0,
            partial_failure: false,
            final_message: Some(MessageRef::new("telegram", "room-1", "msg-2")),
            final_visible_text: Some("hello".to_string()),
            outcomes: vec![],
            warnings: vec![],
        };

        let output = serde_json::to_string(&result).expect("chat batch result should serialize");
        let (final_message, final_visible_text) =
            chat_batch_final_output(&output).expect("chat batch output should parse");

        assert_eq!(final_visible_text.as_deref(), Some("hello"));
        let final_message = final_message.expect("final message should be recovered");
        assert_eq!(final_message.platform.as_str(), "telegram");
        assert_eq!(final_message.room_id, "room-1");
        assert_eq!(final_message.message_id, "msg-2");
    }

    fn agent_request(prompt_text: &str) -> AgentRequest {
        AgentRequest {
            prompt_version: 1,
            thread_id: 1,
            thread_key: "telegram:room".to_string(),
            thread_state: "active".to_string(),
            summary_present: false,
            prompt: rig::message::Message::user(prompt_text),
            chat_history: vec![],
            additional_params: None,
            estimated_tokens: 1,
            recent_event_count: 0,
            trimmed_recent_event_count: 0,
            trimmed_summary_chars: 0,
            trimmed_current_turn_chars: 0,
            tool_count: 0,
        }
    }
}
