use crate::{
    agent::AgentRuntime, config::Config, platforms::Platforms, policy::PolicyEngine,
    storage::Storage, tools::ToolRegistry,
};

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
            "bot={} provider={} platform_count={}",
            self.config.bot_name,
            self.agent.describe(),
            self.platforms.supported_kinds().len()
        )
    }

    pub fn run(&self) {
        let _ = self.describe();
        let _ = self.storage.is_ready();
        let _ = self.policy.default_decision();
        let _ = self.tools.names().collect::<Vec<_>>();
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = Config::load();
    let app = App::new(config);
    app.run();
    Ok(())
}
