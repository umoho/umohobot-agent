use crate::{
    agent::{
        AgentRequest, AgentResponse, AgentRuntime, AgentRuntimeError, PromptContext,
        PromptContextBuilder,
    },
    config::Config,
    logging::sanitize_for_log,
    platforms::{PlatformKind, PlatformMessage, Platforms},
    policy::{PolicyDecision, PolicyEngine, QuotaSnapshot},
    storage::{
        EventRecord, InboundMessageRecord, MessageObservation, Storage, StorageError,
        SummaryRecord, ThreadRecord, ThreadScope, TurnFinish, TurnRecord, TurnStart, TurnStatus,
        UsageLedgerRecord, UsageScope,
    },
    tools::ToolRegistry,
};
use chrono::{Duration, Utc};
use serde_json::json;
use tracing::{debug, info, warn};

#[derive(Clone, Debug)]
pub struct ReplyPlan {
    pub platform: PlatformKind,
    pub room_id: String,
    pub thread_id: Option<String>,
    pub placeholder_text: String,
    pub final_text: String,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct PreparedTurn {
    pub message: PlatformMessage,
    pub observation: MessageObservation,
    pub prompt: PromptContext,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct StartedTurn {
    pub prepared: PreparedTurn,
    pub turn: TurnRecord,
}

#[derive(Clone, Debug)]
pub struct TurnOutcome {
    pub final_text: String,
    pub notes: Vec<String>,
    pub status: TurnStatus,
    pub error_code: Option<String>,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub tool_calls: u64,
    pub estimated_usage: bool,
}

#[derive(Clone, Debug)]
pub struct App {
    config: Config,
    storage: Storage,
    policy: PolicyEngine,
    tools: ToolRegistry,
    agent: AgentRuntime,
    prompt_builder: PromptContextBuilder,
    platforms: Platforms,
}

impl App {
    pub fn new(config: Config) -> Self {
        Self::try_new(config).unwrap_or_else(|err| panic!("failed to initialize app: {err}"))
    }

    pub fn try_new(config: Config) -> Result<Self, AgentRuntimeError> {
        let storage = Storage::new(config.data_dir.clone());
        let policy = PolicyEngine::new();
        let tools = ToolRegistry::new();
        let agent = AgentRuntime::try_new(&config)?;
        let prompt_builder = PromptContextBuilder::from_config(&config);
        let platforms = Platforms::new();

        Ok(Self {
            config,
            storage,
            policy,
            tools,
            agent,
            prompt_builder,
            platforms,
        })
    }

    pub fn describe(&self) -> String {
        format!(
            "bot={} provider={} platforms={} placeholder_edit_flow={}",
            self.config.bot_name,
            self.agent.describe(),
            self.platforms.describe(),
            self.platforms.supports_placeholder_edit_flow()
        )
    }

    pub async fn prepare_turn(
        &self,
        message: &PlatformMessage,
    ) -> Result<PreparedTurn, StorageError> {
        let thread_scope = ThreadScope::from_platform_message(message);
        let mut notes = structured_message_notes(message);

        debug!(
            platform = %message.platform.as_str(),
            room_id = %message.room_id,
            thread_id = %message.thread_id.as_deref().unwrap_or("none"),
            message_id = %message.message_id,
            sender_id = %message.sender_id,
            kind = %message.kind.as_str(),
            text_present = message.text().is_some(),
            attachments = message.attachments.len(),
            "preparing turn"
        );

        let observation = self
            .storage
            .observe_message(InboundMessageRecord {
                scope: thread_scope,
                platform_message_id: message.message_id.clone(),
                sender_id: message.sender_id.clone(),
                sender_name: None,
                reply_to_platform_message_id: message
                    .reply
                    .as_ref()
                    .map(|reply| reply.message_id.clone()),
                content: platform_message_content(message),
                visible_to_model: !matches!(
                    message.kind,
                    crate::platforms::PlatformMessageKind::Service
                ),
                lease_until: Some(
                    Utc::now()
                        + Duration::seconds(self.config.prompt.thread_idle_timeout_secs as i64),
                ),
            })
            .await?;

        debug!(
            thread_id = observation.thread.id,
            thread_key = %observation.thread.thread_key(),
            thread_state = %observation.thread.state.as_str(),
            summary_cursor = observation.thread.summary_cursor,
            turn_count = observation.thread.turn_count,
            event_id = observation.event.id,
            event_seq = observation.event.seq,
            was_new_thread = observation.was_new_thread,
            "message recorded in storage"
        );
        if observation.was_new_thread {
            info!(
                thread_id = observation.thread.id,
                thread_key = %observation.thread.thread_key(),
                "new thread created"
            );
        } else {
            debug!(
                thread_id = observation.thread.id,
                thread_key = %observation.thread.thread_key(),
                "existing thread refreshed"
            );
        }
        notes.push(format!("thread_id={}", observation.thread.id));
        notes.push(format!("thread_key={}", observation.thread.thread_key()));
        notes.push(format!(
            "thread_state={}",
            observation.thread.state.as_str()
        ));
        notes.push(format!(
            "thread_turn_count={}",
            observation.thread.turn_count
        ));
        notes.push(format!(
            "thread_summary_cursor={}",
            observation.thread.summary_cursor
        ));
        notes.push(format!("thread_new={}", observation.was_new_thread));
        notes.push(format!("thread_event_id={}", observation.event.id));
        notes.push(format!("thread_event_seq={}", observation.event.seq));

        let summary = self
            .resolve_thread_summary(&observation.thread, observation.event.seq)
            .await?;
        let recent_events = self
            .load_recent_events(
                observation.thread.id,
                summary
                    .as_ref()
                    .map(|summary| {
                        if summary.thread_id == observation.thread.id {
                            summary.upto_seq
                        } else {
                            0
                        }
                    })
                    .unwrap_or(0),
                observation.event.seq.saturating_sub(1),
            )
            .await?;
        let prompt = self.prompt_builder.build(
            &observation.thread,
            summary.as_ref(),
            &recent_events,
            message,
            &observation.event,
            &self.tools,
        );

        notes.push(format!("prompt_version={}", prompt.prompt_version));
        notes.push(format!(
            "prompt_estimated_tokens={}",
            prompt.estimated_tokens
        ));
        notes.push(format!("prompt_sections={}", prompt.sections.len()));
        notes.push(format!(
            "prompt_recent_events={}",
            prompt.recent_event_count
        ));
        notes.push(format!(
            "prompt_recent_events_trimmed={}",
            prompt.trimmed_recent_event_count
        ));
        notes.push(format!("prompt_summary_present={}", summary.is_some()));

        Ok(PreparedTurn {
            message: message.clone(),
            observation,
            prompt,
            notes,
        })
    }

    pub async fn start_turn(
        &self,
        prepared: &PreparedTurn,
        placeholder_message_id: String,
    ) -> Result<StartedTurn, StorageError> {
        let lease_until =
            Some(Utc::now() + Duration::seconds(self.config.prompt.turn_lease_secs as i64));
        let turn = self
            .storage
            .start_turn(TurnStart {
                thread_id: prepared.observation.thread.id,
                trigger_event_id: prepared.observation.event.id,
                provider: self.agent.provider_name().to_string(),
                model: self.agent.model_name().to_string(),
                prompt_version: prepared.prompt.prompt_version,
                context_hash: None,
                placeholder_message_id: Some(placeholder_message_id),
                lease_until,
                started_at: None,
            })
            .await?;

        Ok(StartedTurn {
            prepared: prepared.clone(),
            turn,
        })
    }

    pub async fn respond_turn(&self, started: &StartedTurn) -> TurnOutcome {
        let AgentResponse {
            final_text: agent_text,
            notes: mut agent_notes,
            status: agent_status,
            error_code: agent_error_code,
        } = self
            .agent
            .respond(
                &AgentRequest::new(started.prepared.prompt.clone()),
                &self.tools,
            )
            .await;
        let mut notes = Vec::with_capacity(agent_notes.len() + 4);
        notes.append(&mut agent_notes);
        notes.push(format!("turn_status={}", agent_status.as_str()));
        if let Some(error_code) = agent_error_code.as_deref() {
            notes.push(format!("turn_error_code={error_code}"));
        }
        debug!(
            provider = %self.agent.describe(),
            response_chars = agent_text.chars().count(),
            status = %agent_status.as_str(),
            "agent response ready"
        );

        let quota_decision = self.policy.decide_quota(&QuotaSnapshot::default());
        match &quota_decision {
            PolicyDecision::Allow => {
                debug!(thread_key = %started.prepared.observation.thread.thread_key(), "quota allowed");
            }
            PolicyDecision::Deny { reason } => {
                warn!(
                    thread_key = %started.prepared.observation.thread.thread_key(),
                    user_error = %sanitize_for_log(reason),
                    "quota denied"
                );
            }
            PolicyDecision::NeedConfirmation { reason } => {
                warn!(
                    thread_key = %started.prepared.observation.thread.thread_key(),
                    user_error = %sanitize_for_log(reason),
                    "quota confirmation required"
                );
            }
        }
        notes.push(format!(
            "quota_decision={}",
            quota_decision_label(&quota_decision)
        ));

        let provider = self.agent.provider_name().to_string();
        if let Err(err) = self
            .storage
            .record_usage(UsageLedgerRecord {
                scope: UsageScope::thread(
                    started.prepared.observation.thread.thread_key().to_string(),
                ),
                provider,
                turn_id: Some(started.turn.id),
                prompt_tokens: 0,
                completion_tokens: 0,
                tool_calls: 0,
                estimated: true,
                created_at: None,
            })
            .await
        {
            warn!(
                error = %err,
                thread_key = %started.prepared.observation.thread.thread_key(),
                provider = %self.agent.provider_name(),
                turn_id = started.turn.id,
                "failed to record usage"
            );
            notes.push(format!("storage_usage_error={err}"));
        } else {
            debug!(
                thread_key = %started.prepared.observation.thread.thread_key(),
                provider = %self.agent.provider_name(),
                turn_id = started.turn.id,
                "usage recorded"
            );
        }

        let final_text = match quota_decision {
            PolicyDecision::Allow => agent_text,
            PolicyDecision::Deny { reason } | PolicyDecision::NeedConfirmation { reason } => reason,
        };

        debug!(
            thread_key = %started.prepared.observation.thread.thread_key(),
            turn_id = started.turn.id,
            final_text_chars = final_text.chars().count(),
            "turn response prepared"
        );

        TurnOutcome {
            final_text,
            notes,
            status: agent_status,
            error_code: agent_error_code,
            prompt_tokens: 0,
            completion_tokens: 0,
            tool_calls: 0,
            estimated_usage: true,
        }
    }

    pub async fn finish_turn(
        &self,
        started: &StartedTurn,
        outcome: &TurnOutcome,
        status: TurnStatus,
        final_message_id: Option<String>,
        error_code: Option<String>,
    ) -> Result<TurnRecord, StorageError> {
        self.storage
            .finish_turn(TurnFinish {
                turn_id: started.turn.id,
                status,
                final_message_id,
                prompt_tokens: outcome.prompt_tokens,
                completion_tokens: outcome.completion_tokens,
                tool_calls: outcome.tool_calls,
                estimated_usage: outcome.estimated_usage,
                error_code,
                ended_at: None,
            })
            .await
    }

    async fn resolve_thread_summary(
        &self,
        thread: &ThreadRecord,
        current_seq: i64,
    ) -> Result<Option<SummaryRecord>, StorageError> {
        let mut current_thread = Some(thread.clone());
        while let Some(thread) = current_thread {
            if let Some(summary) = self
                .storage
                .load_latest_summary_before_seq(thread.id, current_seq.saturating_sub(1))
                .await?
            {
                return Ok(Some(summary));
            }

            current_thread = match thread.parent_thread_id {
                Some(parent_thread_id) => self.storage.load_thread(parent_thread_id).await?,
                None => None,
            };
        }

        Ok(None)
    }

    async fn load_recent_events(
        &self,
        thread_id: i64,
        after_seq_exclusive: i64,
        before_seq_exclusive: i64,
    ) -> Result<Vec<EventRecord>, StorageError> {
        self.storage
            .load_recent_visible_events(thread_id, after_seq_exclusive, before_seq_exclusive)
            .await
    }

    pub fn run(&self) {
        let _ = self.describe();
        let _ = self.storage.is_ready();
        let _ = self.policy.default_decision();
        let _ = self.tools.is_empty();
    }
}

pub async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    crate::runtime::run().await
}

fn structured_message_notes(message: &PlatformMessage) -> Vec<String> {
    vec![
        format!("input_platform={}", message.platform.as_str()),
        format!("input_room_id={}", message.room_id),
        format!(
            "input_thread_id={}",
            message.thread_id.as_deref().unwrap_or("none")
        ),
        format!("input_message_kind={}", message.kind.as_str()),
        format!("input_body_kind={}", message.body.kind_label()),
        format!("input_text_present={}", message.text().is_some()),
        format!("input_entities={}", message.body_entities().len()),
        format!("input_attachments={}", message.attachments.len()),
        format!("input_reply={}", message.reply.is_some()),
        format!("input_mention={}", message.is_mention),
    ]
}

fn quota_decision_label(decision: &PolicyDecision) -> &'static str {
    match decision {
        PolicyDecision::Allow => "allow",
        PolicyDecision::Deny { .. } => "deny",
        PolicyDecision::NeedConfirmation { .. } => "need_confirmation",
    }
}

fn platform_message_content(message: &PlatformMessage) -> serde_json::Value {
    json!({
        "platform": message.platform.as_str(),
        "room_id": message.room_id.as_str(),
        "thread_id": message.thread_id.as_deref(),
        "message_id": message.message_id.as_str(),
        "sender_id": message.sender_id.as_str(),
        "kind": message.kind.as_str(),
        "body_kind": message.body.kind_label(),
        "text": message.text(),
        "entities": message
            .body_entities()
            .iter()
            .map(|entity| json!({
                "kind": format!("{:?}", entity.kind.clone()),
                "text": entity.text.as_str(),
            }))
            .collect::<Vec<_>>(),
        "attachments": message
            .attachments
            .iter()
            .map(|attachment| json!({
                "kind": attachment.kind.as_str(),
                "file_id": attachment.file_id.as_deref(),
                "file_unique_id": attachment.file_unique_id.as_deref(),
                "file_name": attachment.file_name.as_deref(),
                "mime_type": attachment.mime_type.as_deref(),
                "url": attachment.url.as_deref(),
                "width": attachment.width,
                "height": attachment.height,
                "size_bytes": attachment.size_bytes,
            }))
            .collect::<Vec<_>>(),
        "reply": message.reply.as_ref().map(|reply| json!({
            "message_id": reply.message_id.as_str(),
            "sender_id": reply.sender_id.as_str(),
            "kind": reply.kind.as_str(),
            "body_kind": reply.body.kind_label(),
            "text": reply.body.text(),
            "attachments": reply
                .attachments
                .iter()
                .map(|attachment| json!({
                    "kind": attachment.kind.as_str(),
                    "file_id": attachment.file_id.as_deref(),
                    "file_unique_id": attachment.file_unique_id.as_deref(),
                    "file_name": attachment.file_name.as_deref(),
                    "mime_type": attachment.mime_type.as_deref(),
                    "url": attachment.url.as_deref(),
                    "width": attachment.width,
                    "height": attachment.height,
                    "size_bytes": attachment.size_bytes,
                }))
                .collect::<Vec<_>>(),
        })),
        "is_mention": message.is_mention,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PromptConfig, ProviderConfig, ProviderKind, RuntimeMode};
    use crate::platforms::{
        AttachmentInfo, AttachmentKind, MessageBody, PlatformKind, PlatformMessage,
        PlatformMessageKind, ReplyMetadata,
    };
    use crate::storage::ThreadScope;
    use std::path::PathBuf;

    fn unique_data_dir(prefix: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX_EPOCH")
            .as_nanos();
        std::env::temp_dir()
            .join("umohobot-tests")
            .join(format!("{prefix}-{nonce}"))
            .join("state")
    }

    fn test_config(data_dir: PathBuf) -> Config {
        Config {
            bot_name: "bot".to_string(),
            runtime_mode: RuntimeMode::Local,
            telegram_bot_token: None,
            default_provider: ProviderConfig {
                kind: ProviderKind::Ollama,
                base_url: Some("http://127.0.0.1:11434".to_string()),
                model: "llama3.1".to_string(),
                api_key_ref: None,
            },
            allow_user_provider: false,
            max_response_chars: 4_000,
            message_edit_throttle_ms: 750,
            placeholder_text: "正在处理...".to_string(),
            prompt: PromptConfig::default(),
            data_dir: Some(data_dir),
        }
    }

    fn test_message() -> PlatformMessage {
        PlatformMessage {
            platform: PlatformKind::Telegram,
            room_id: "room".to_string(),
            thread_id: None,
            message_id: "msg".to_string(),
            sender_id: "user".to_string(),
            kind: PlatformMessageKind::Text,
            body: MessageBody::Text {
                text: "hello".to_string(),
                entities: vec![],
            },
            attachments: vec![],
            reply: None,
            is_mention: false,
        }
    }

    #[test]
    fn runtime_summary_reports_placeholder_edit_flow() {
        let config = Config {
            bot_name: "bot".to_string(),
            runtime_mode: RuntimeMode::Telegram,
            telegram_bot_token: None,
            default_provider: ProviderConfig {
                kind: ProviderKind::Ollama,
                base_url: None,
                model: "llama3.1".to_string(),
                api_key_ref: None,
            },
            allow_user_provider: false,
            max_response_chars: 4_000,
            message_edit_throttle_ms: 750,
            placeholder_text: "正在处理...".to_string(),
            prompt: PromptConfig::default(),
            data_dir: None,
        };
        let runtime = App::new(config);
        let summary = runtime.describe();

        assert!(summary.contains("provider="));
        assert!(summary.contains("placeholder_edit_flow=true"));
    }

    #[test]
    fn structured_message_notes_include_message_metadata() {
        let message = PlatformMessage {
            platform: PlatformKind::Telegram,
            room_id: "room".to_string(),
            thread_id: None,
            message_id: "msg".to_string(),
            sender_id: "user".to_string(),
            kind: PlatformMessageKind::Photo,
            body: MessageBody::Caption {
                text: "看看".to_string(),
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
        };

        let notes = structured_message_notes(&message);

        assert!(notes.iter().any(|note| note == "input_platform=telegram"));
        assert!(notes.iter().any(|note| note == "input_message_kind=photo"));
        assert!(notes.iter().any(|note| note == "input_body_kind=caption"));
        assert!(notes.iter().any(|note| note == "input_attachments=1"));
        assert!(notes.iter().any(|note| note == "input_reply=true"));
        assert!(notes.iter().any(|note| note == "input_mention=true"));
    }

    #[tokio::test]
    async fn app_turn_lifecycle_records_completed_turn() {
        let app = App::new(test_config(unique_data_dir("app-turn-completed")));
        let message = test_message();

        let prepared = app.prepare_turn(&message).await.expect("prepare turn");
        assert_eq!(prepared.observation.thread.turn_count, 0);

        let started = app
            .start_turn(&prepared, "placeholder-1".to_string())
            .await
            .expect("start turn");
        assert_eq!(
            started.turn.placeholder_message_id.as_deref(),
            Some("placeholder-1")
        );
        assert_eq!(started.turn.status, TurnStatus::Running);

        let active = app
            .storage
            .load_active_thread(&ThreadScope::from_platform_message(&message))
            .await
            .expect("load active thread")
            .expect("active thread");
        assert_eq!(active.turn_count, 1);
        assert!(active.lease_until.is_some());

        let outcome = TurnOutcome {
            final_text: "done".to_string(),
            notes: vec![],
            status: TurnStatus::Completed,
            error_code: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            tool_calls: 0,
            estimated_usage: true,
        };

        let finished = app
            .finish_turn(
                &started,
                &outcome,
                TurnStatus::Completed,
                Some("placeholder-1".to_string()),
                None,
            )
            .await
            .expect("finish turn");

        assert_eq!(finished.status, TurnStatus::Completed);
        assert_eq!(finished.final_message_id.as_deref(), Some("placeholder-1"));

        let active_after = app
            .storage
            .load_active_thread(&ThreadScope::from_platform_message(&message))
            .await
            .expect("load active thread after finish")
            .expect("active thread after finish");
        assert_eq!(active_after.turn_count, 1);
        assert!(active_after.lease_until.is_some());
    }

    #[tokio::test]
    async fn app_turn_lifecycle_records_failed_turn() {
        let app = App::new(test_config(unique_data_dir("app-turn-failed")));
        let message = test_message();

        let prepared = app.prepare_turn(&message).await.expect("prepare turn");
        let started = app
            .start_turn(&prepared, "placeholder-1".to_string())
            .await
            .expect("start turn");

        let outcome = TurnOutcome {
            final_text: "rig 请求失败：boom".to_string(),
            notes: vec![],
            status: TurnStatus::Failed,
            error_code: Some("provider_error".to_string()),
            prompt_tokens: 0,
            completion_tokens: 0,
            tool_calls: 0,
            estimated_usage: true,
        };

        let finished = app
            .finish_turn(
                &started,
                &outcome,
                TurnStatus::Failed,
                None,
                Some("provider_error".to_string()),
            )
            .await
            .expect("finish turn");

        assert_eq!(finished.status, TurnStatus::Failed);
        assert!(finished.final_message_id.is_none());
        assert_eq!(finished.error_code.as_deref(), Some("provider_error"));

        let active_after = app
            .storage
            .load_active_thread(&ThreadScope::from_platform_message(&message))
            .await
            .expect("load active thread after finish")
            .expect("active thread after finish");
        assert!(active_after.lease_until.is_some());
        assert_eq!(active_after.turn_count, 1);
    }
}
