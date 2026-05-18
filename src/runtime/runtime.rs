use std::{error::Error, io};

use crate::{
    app::{App, ReplyPlan},
    config::{Config, RuntimeMode},
    platforms::{PlatformMessage, Platforms, TelegramReplyScript, TelegramRuntime},
};
use teloxide::prelude::Requester;

#[derive(Clone, Debug)]
pub struct RuntimeSummary {
    pub runtime_mode: RuntimeMode,
    pub app_description: String,
    pub platforms_description: String,
    pub telegram_description: String,
    pub placeholder_edit_flow: bool,
}

#[derive(Debug)]
pub struct RuntimeController {
    config: Config,
    app: App,
    platforms: Platforms,
    telegram: TelegramRuntime,
}

impl RuntimeController {
    pub fn new(config: Config) -> Self {
        let app = App::new(config.clone());
        let platforms = Platforms::new();
        let telegram = TelegramRuntime::from_config(&config);

        Self {
            config,
            app,
            platforms,
            telegram,
        }
    }

    pub fn summary(&self) -> RuntimeSummary {
        RuntimeSummary {
            runtime_mode: self.config.runtime_mode.clone(),
            app_description: self.app.describe(),
            platforms_description: self.platforms.describe(),
            telegram_description: self.telegram.describe(),
            placeholder_edit_flow: self.telegram.supports_placeholder_edit_flow(),
        }
    }

    pub async fn plan_telegram_reply(&self, message: &PlatformMessage) -> TelegramReplyScript {
        let reply_plan: ReplyPlan = self.app.plan_message(message).await;
        self.telegram.build_reply_script(&reply_plan)
    }

    pub async fn run(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let token =
            self.config.telegram_bot_token.clone().ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "missing Telegram bot token")
            })?;
        let bot = teloxide::Bot::new(token);
        let app = self.app.clone();
        let telegram = self.telegram.clone();

        teloxide::repl(
            bot,
            move |bot: teloxide::Bot, msg: teloxide::types::Message| {
                let app = app.clone();
                let telegram = telegram.clone();

                async move {
                    if msg.from.as_ref().map(|user| user.is_bot).unwrap_or(false) {
                        return Ok(());
                    }

                    let inbound = telegram.inbound_from_message(&msg);
                    let platform_message = telegram.normalize_inbound(inbound);
                    let placeholder = bot
                        .send_message(msg.chat.id, telegram.placeholder_text().to_string())
                        .await?;
                    let reply_plan = app.plan_message(&platform_message).await;

                    let script = telegram.build_reply_script(&reply_plan);
                    if script.edit_in_place {
                        match bot
                            .edit_message_text(
                                msg.chat.id,
                                placeholder.id,
                                script.final_text.clone(),
                            )
                            .await
                        {
                            Ok(_) => {}
                            Err(err) => {
                                let fallback_text =
                                    format!("{}\n\n(编辑失败：{err})", script.final_text);
                                let _ = bot.send_message(msg.chat.id, fallback_text).await?;
                            }
                        }
                    } else {
                        let _ = bot.send_message(msg.chat.id, script.final_text).await?;
                    }

                    Ok(())
                }
            },
        )
        .await;

        Ok(())
    }
}

pub async fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = Config::load()?;
    let runtime = RuntimeController::new(config);
    runtime.run().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ProviderConfig, ProviderKind, RuntimeMode};

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
        let runtime = RuntimeController::new(config);
        let summary = runtime.summary();

        assert!(summary.placeholder_edit_flow);
        assert!(summary.app_description.contains("provider="));
    }
}
