use crate::{
    agent::{AgentRequest, AgentResponse, AgentRuntime},
    config::Config,
    platforms::{PlatformKind, PlatformMessage, Platforms},
    policy::{PolicyDecision, PolicyEngine, QuotaSnapshot},
    storage::{ConversationRecord, Storage, UsageRecord},
    tools::ToolRegistry,
};

#[derive(Clone, Debug)]
pub struct ReplyPlan {
    pub platform: PlatformKind,
    pub room_id: String,
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
        let storage = Storage::new(config.data_dir.clone());
        let policy = PolicyEngine::new();
        let tools = ToolRegistry::new();
        let agent = AgentRuntime::new(&config);
        let platforms = Platforms::new();

        Self {
            config,
            storage,
            policy,
            tools,
            agent,
            platforms,
        }
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
        let AgentResponse {
            final_text: agent_text,
            mut notes,
        } = self
            .agent
            .respond(&AgentRequest::new(message.clone()), &self.tools)
            .await;
        notes.extend(structured_message_notes(message));

        let quota_decision = self.policy.decide_quota(&QuotaSnapshot::default());
        notes.push(format!(
            "quota_decision={}",
            quota_decision_label(&quota_decision)
        ));

        self.storage.record_conversation(ConversationRecord {
            conversation_id: format!("{}:{}", message.platform.as_str(), message.room_id),
            platform: message.platform.as_str().to_string(),
            room_id: message.room_id.clone(),
            last_message_id: Some(message.message_id.clone()),
        });
        self.storage.record_usage(UsageRecord {
            scope: message.room_id.clone(),
            provider: self.agent.describe(),
            prompt_tokens: 0,
            completion_tokens: 0,
            tool_calls: 0,
            estimated: true,
        });

        let final_text = match quota_decision {
            PolicyDecision::Allow => agent_text,
            PolicyDecision::Deny { reason } | PolicyDecision::NeedConfirmation { reason } => reason,
        };

        ReplyPlan {
            platform: message.platform,
            room_id: message.room_id.clone(),
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
