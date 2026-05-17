use crate::{
    agent::{AgentRequest, AgentRuntime},
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

#[derive(Debug)]
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
        let storage = Storage::new();
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
            "bot={} provider={} platforms={}",
            self.config.bot_name,
            self.agent.describe(),
            self.platforms.describe()
        )
    }

    pub fn plan_message(&self, message: &PlatformMessage) -> ReplyPlan {
        let response = self.agent.respond(
            &AgentRequest {
                input: message.text.clone(),
            },
            &self.tools,
        );
        let quota_decision = self.policy.decide_quota(&QuotaSnapshot::default());

        self.storage.record_conversation(ConversationRecord {
            conversation_id: format!("{}:{}", message.platform.as_str(), message.room_id),
            platform: message.platform.as_str().to_string(),
            room_id: message.room_id.clone(),
            last_message_id: None,
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
            PolicyDecision::Allow => response.final_text,
            PolicyDecision::Deny { reason } | PolicyDecision::NeedConfirmation { reason } => reason,
        };

        ReplyPlan {
            platform: message.platform.clone(),
            room_id: message.room_id.clone(),
            placeholder_text: self.config.placeholder_text.clone(),
            final_text,
            notes: response.notes,
        }
    }

    pub fn run(&self) {
        let _ = self.describe();
        let _ = self.storage.is_ready();
        let _ = self.policy.default_decision();
        let _ = self.tools.is_empty();
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = Config::load();
    let app = App::new(config);
    app.run();
    Ok(())
}
