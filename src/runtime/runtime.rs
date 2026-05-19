use std::{error::Error, io, sync::Arc};

use crate::{
    agent::AgentRuntimeError,
    app::{App, TurnOutcome},
    config::{Config, RuntimeMode},
    platforms::{Platforms, TelegramRuntime},
    storage::TurnStatus,
};
use teloxide::payloads::SendMessageSetters;
use teloxide::prelude::Requester;
use teloxide::types::{ChatId, MessageId, ThreadId};
use tracing::{debug, error, info, warn};

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
    telegram: TelegramRuntime,
}

impl RuntimeController {
    pub fn new(config: Config) -> Self {
        Self::try_new(config).unwrap_or_else(|err| panic!("failed to initialize runtime: {err}"))
    }

    pub fn try_new(config: Config) -> Result<Self, AgentRuntimeError> {
        let app = App::try_new(config.clone())?;
        let platforms = Platforms::new();
        let telegram = TelegramRuntime::from_config(&config);

        Ok(Self {
            config,
            app,
            platforms,
            telegram,
        })
    }

    pub fn summary(&self) -> RuntimeSummary {
        RuntimeSummary {
            runtime_mode: self.config.runtime_mode.clone(),
            app_description: self.app.describe(),
            platforms_description: self.platforms.describe(),
            telegram_description: self.telegram.describe(),
        }
    }

    async fn handle_telegram_message(
        &self,
        bot: &teloxide::Bot,
        msg: &teloxide::types::Message,
    ) -> Result<(), teloxide::RequestError> {
        if msg.from.as_ref().map(|user| user.is_bot).unwrap_or(false) {
            debug!(
                chat_id = %msg.chat.id,
                message_id = %msg.id,
                "ignoring bot-authored telegram message"
            );
            return Ok(());
        }

        let inbound = self.telegram.inbound_from_message(msg);
        let platform_message = self.telegram.normalize_inbound(inbound);
        let thread_id = platform_message.thread_id.as_deref();
        info!(
            platform = %platform_message.platform.as_str(),
            chat_id = %platform_message.room_id,
            thread_id = %platform_message.thread_id.as_deref().unwrap_or("none"),
            message_id = %platform_message.message_id,
            sender_id = %platform_message.sender_id,
            kind = %platform_message.kind.as_str(),
            text_present = platform_message.text().is_some(),
            reply_present = platform_message.reply.is_some(),
            attachments = platform_message.attachments.len(),
            is_mention = platform_message.is_mention,
            "received telegram message"
        );

        let prepared = match self.app.prepare_turn(&platform_message).await {
            Ok(prepared) => prepared,
            Err(err) => {
                error!(
                    error = %err,
                    platform = %platform_message.platform.as_str(),
                    room_id = %platform_message.room_id,
                    thread_id = %platform_message.thread_id.as_deref().unwrap_or("none"),
                    message_id = %platform_message.message_id,
                    "failed to prepare turn"
                );
                let error_text = format!("系统暂时无法记录这条消息：{err}");
                self.send_final_message(bot, msg.chat.id, thread_id, &error_text)
                    .await?;
                return Ok(());
            }
        };

        let started = match self.app.start_turn(&prepared).await {
            Ok(started) => started,
            Err(err) => {
                error!(
                    error = %err,
                    thread_id = %prepared.observation.thread.id,
                    trigger_event_id = %prepared.observation.event.id,
                    "failed to start turn"
                );
                let error_text = format!("系统暂时无法开始处理：{err}");
                self.send_final_message(bot, msg.chat.id, thread_id, &error_text)
                    .await?;
                return Ok(());
            }
        };

        let outcome = self.app.respond_turn(&started).await;
        let delivery_result = self
            .send_final_message(bot, msg.chat.id, thread_id, &outcome.final_text)
            .await;
        let (final_message_id, delivery_error) = match delivery_result {
            Ok(final_message_id) => (Some(final_message_id), None),
            Err(err) => (None, Some(err)),
        };

        let final_status = Self::resolve_final_turn_status(&outcome, delivery_error.is_some());
        let final_error_code =
            Self::resolve_final_turn_error_code(&outcome, delivery_error.is_some());

        if let Err(err) = self
            .app
            .finish_turn(
                &started,
                &outcome,
                final_status,
                final_message_id,
                final_error_code,
            )
            .await
        {
            return Err(self.storage_error_to_request_error(err));
        }

        if let Some(err) = delivery_error {
            return Err(err);
        }

        Ok(())
    }

    async fn send_final_message(
        &self,
        bot: &teloxide::Bot,
        chat_id: ChatId,
        thread_id: Option<&str>,
        final_text: &str,
    ) -> Result<String, teloxide::RequestError> {
        let mut send_request = bot.send_message(chat_id, final_text.to_string());
        if let Some(thread_id) = parse_thread_id(thread_id) {
            send_request = send_request.message_thread_id(thread_id);
        }
        match send_request.await {
            Ok(message) => {
                info!(
                    chat_id = %chat_id,
                    final_message_id = %message.id,
                    "final message sent"
                );
                Ok(message.id.to_string())
            }
            Err(err) => {
                error!(
                    error = %err,
                    chat_id = %chat_id,
                    "failed to send final message"
                );
                Err(err)
            }
        }
    }

    fn resolve_final_turn_status(outcome: &TurnOutcome, delivery_failed: bool) -> TurnStatus {
        if matches!(outcome.status, TurnStatus::Failed) || delivery_failed {
            TurnStatus::Failed
        } else {
            TurnStatus::Completed
        }
    }

    fn resolve_final_turn_error_code(
        outcome: &TurnOutcome,
        delivery_failed: bool,
    ) -> Option<String> {
        if matches!(outcome.status, TurnStatus::Failed) {
            outcome.error_code.clone().or_else(|| {
                if delivery_failed {
                    Some("telegram_delivery_failed".to_string())
                } else {
                    None
                }
            })
        } else if delivery_failed {
            Some("telegram_delivery_failed".to_string())
        } else {
            None
        }
    }

    fn storage_error_to_request_error(
        &self,
        err: crate::storage::StorageError,
    ) -> teloxide::RequestError {
        teloxide::RequestError::Io(Arc::new(io::Error::new(
            io::ErrorKind::Other,
            err.to_string(),
        )))
    }

    pub async fn run(&self) -> Result<(), teloxide::RequestError> {
        let token = self.config.telegram_bot_token.clone().ok_or_else(|| {
            let error = io::Error::new(io::ErrorKind::NotFound, "missing Telegram bot token");
            error!(error = %error, "missing telegram bot token");
            teloxide::RequestError::Io(Arc::new(error))
        })?;
        info!(
            runtime_mode = ?self.config.runtime_mode,
            bot_name = %self.config.bot_name,
            provider = %self.app.describe(),
            platforms = %self.platforms.describe(),
            telegram_description = %self.telegram.describe(),
            data_dir = ?self.config.data_dir,
            telegram_token_present = self.config.telegram_bot_token.is_some(),
            "runtime starting"
        );
        let bot = teloxide::Bot::new(token);
        let runtime = self.clone();

        teloxide::repl(
            bot,
            move |bot: teloxide::Bot, msg: teloxide::types::Message| {
                let runtime = runtime.clone();

                async move { runtime.handle_telegram_message(&bot, &msg).await }
            },
        )
        .await;

        Ok(())
    }
}

pub async fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    let config = Config::load()?;
    info!(
        runtime_mode = ?config.runtime_mode,
        bot_name = %config.bot_name,
        provider_kind = %config.default_provider.kind.as_str(),
        model = %config.default_provider.model,
        allow_user_provider = config.allow_user_provider,
        data_dir = ?config.data_dir,
        telegram_token_present = config.telegram_bot_token.is_some(),
        "configuration loaded"
    );
    let runtime = RuntimeController::new(config);
    runtime.run().await?;
    Ok(())
}

fn parse_thread_id(value: Option<&str>) -> Option<ThreadId> {
    value.and_then(|value| match value.parse::<i32>() {
        Ok(id) => Some(ThreadId(MessageId(id))),
        Err(err) => {
            warn!(thread_id = %value, error = %err, "failed to parse telegram thread id");
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PromptConfig, ProviderConfig, ProviderKind, RuntimeMode};

    #[test]
    fn runtime_summary_reports_platforms_and_provider() {
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
            prompt: PromptConfig::default(),
            data_dir: None,
        };
        let runtime = RuntimeController::new(config);
        let summary = runtime.summary();

        assert!(summary.app_description.contains("provider="));
        assert!(summary.platforms_description.contains("telegram"));
        assert!(summary.telegram_description.contains("telegram("));
    }
}
