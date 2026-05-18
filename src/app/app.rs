use crate::{
    agent::{AgentRequest, AgentResponse, AgentRuntime, AgentRuntimeError},
    config::Config,
    platforms::{PlatformKind, PlatformMessage, Platforms},
    policy::{PolicyDecision, PolicyEngine, QuotaSnapshot},
    storage::{InboundMessageRecord, Storage, ThreadScope, UsageLedgerRecord, UsageScope},
    tools::ToolRegistry,
};
use serde_json::json;

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
pub struct App {
    config: Config,
    storage: Storage,
    policy: PolicyEngine,
    tools: ToolRegistry,
    agent: AgentRuntime,
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
        let platforms = Platforms::new();

        Ok(Self {
            config,
            storage,
            policy,
            tools,
            agent,
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

    pub async fn plan_message(&self, message: &PlatformMessage) -> ReplyPlan {
        let thread_scope = ThreadScope::from_platform_message(message);
        let thread_key = thread_scope.thread_key();
        let mut notes = structured_message_notes(message);

        match self
            .storage
            .observe_message(InboundMessageRecord {
                scope: thread_scope.clone(),
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
                lease_until: None,
            })
            .await
        {
            Ok(observation) => {
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
            }
            Err(err) => {
                notes.push(format!("storage_observe_error={err}"));
            }
        }

        let AgentResponse {
            final_text: agent_text,
            notes: mut agent_notes,
        } = self
            .agent
            .respond(&AgentRequest::new(message.clone()), &self.tools)
            .await;
        notes.append(&mut agent_notes);

        let quota_decision = self.policy.decide_quota(&QuotaSnapshot::default());
        notes.push(format!(
            "quota_decision={}",
            quota_decision_label(&quota_decision)
        ));

        if let Err(err) = self
            .storage
            .record_usage(UsageLedgerRecord {
                scope: UsageScope::thread(thread_key.to_string()),
                provider: self.agent.describe(),
                turn_id: None,
                prompt_tokens: 0,
                completion_tokens: 0,
                tool_calls: 0,
                estimated: true,
                created_at: None,
            })
            .await
        {
            notes.push(format!("storage_usage_error={err}"));
        }

        let final_text = match quota_decision {
            PolicyDecision::Allow => agent_text,
            PolicyDecision::Deny { reason } | PolicyDecision::NeedConfirmation { reason } => reason,
        };

        ReplyPlan {
            platform: message.platform,
            room_id: message.room_id.clone(),
            thread_id: message.thread_id.clone(),
            placeholder_text: self.config.placeholder_text.clone(),
            final_text,
            notes,
        }
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
    use crate::config::{ProviderConfig, ProviderKind, RuntimeMode};
    use crate::platforms::{
        AttachmentInfo, AttachmentKind, MessageBody, PlatformKind, PlatformMessage,
        PlatformMessageKind, ReplyMetadata,
    };

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
}
