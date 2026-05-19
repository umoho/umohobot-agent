use std::error::Error;

use crate::{
    app::App,
    config::{Config, RuntimeMode},
    platforms::Platforms,
};
use tracing::info;

#[derive(Clone, Debug)]
pub struct RuntimeSummary {
    pub runtime_mode: RuntimeMode,
    pub app_description: String,
    pub platforms_description: String,
    pub telegram_description: String,
}

#[derive(Clone, Debug)]
pub struct RuntimeController {
    config: Config,
    app: App,
    platforms: Platforms,
}

impl RuntimeController {
    pub fn new(config: Config) -> Self {
        Self::try_new(config).unwrap_or_else(|err| panic!("failed to initialize runtime: {err}"))
    }

    pub fn try_new(config: Config) -> Result<Self, crate::agent::AgentRuntimeError> {
        let app = App::try_new(config.clone())?;
        let platforms = Platforms::new();

        Ok(Self {
            config,
            app,
            platforms,
        })
    }

    pub fn summary(&self) -> RuntimeSummary {
        RuntimeSummary {
            runtime_mode: self.config.runtime_mode.clone(),
            app_description: self.app.describe(),
            platforms_description: self.platforms.describe(),
            telegram_description: "telegram host moved to tools-telegram".to_string(),
        }
    }

    pub async fn run(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let summary = self.summary();
        info!(
            runtime_mode = ?summary.runtime_mode,
            app = %summary.app_description,
            platforms = %summary.platforms_description,
            telegram = %summary.telegram_description,
            "core agent runtime initialized"
        );
        Ok(())
    }
}

pub async fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = Config::load()?;
    info!(
        runtime_mode = ?config.runtime_mode,
        bot_name = %config.bot_name,
        data_dir = ?config.data_dir,
        "configuration loaded"
    );
    let runtime = RuntimeController::try_new(config)?;
    runtime.run().await?;
    Ok(())
}
