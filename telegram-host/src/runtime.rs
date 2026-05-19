use std::{error::Error, io, sync::Arc};

use agent::{
    app::{App, TurnOutcome},
    config::{Config, RuntimeMode},
    platforms::{Platforms, ReplyHandle},
    storage::TurnStatus,
    tools::{ToolBundle, ToolRegistry},
};
use chrono::Duration;
use rig::tool::server::ToolServer;
use teloxide::types::ChatId;
use tracing::{debug, error, info, warn};

use tools_telegram::{ChatBatchTool, TelegramOutbox};
use trigger_telegram::{TelegramTrigger, TelegramTriggerResult};

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
    trigger: TelegramTrigger,
    outbox: TelegramOutbox,
}

impl RuntimeController {
    pub fn new(config: Config) -> Self {
        Self::try_new(config).unwrap_or_else(|err| panic!("failed to initialize runtime: {err}"))
    }

    pub fn try_new(config: Config) -> Result<Self, agent::agent::AgentRuntimeError> {
        let outbox = TelegramOutbox::new();
        let mut tools = ToolRegistry::new();
        tools.register(ChatBatchTool::spec());
        let tool_server_handle = ToolServer::new()
            .tool(ChatBatchTool::with_outbox(outbox.clone()))
            .run();
        let tool_bundle = ToolBundle::new(tools, tool_server_handle);
        let app = App::try_new_with_tool_bundle(config.clone(), tool_bundle)?;
        let trigger = TelegramTrigger::new(
            config.bot_name.clone(),
            Duration::seconds(config.prompt.thread_idle_timeout_secs as i64),
        );
        let platforms = Platforms::new();

        Ok(Self {
            config,
            app,
            platforms,
            trigger,
            outbox,
        })
    }

    pub fn summary(&self) -> RuntimeSummary {
        RuntimeSummary {
            runtime_mode: self.config.runtime_mode.clone(),
            app_description: self.app.describe(),
            platforms_description: self.platforms.describe(),
            telegram_description: format!(
                "telegram(trigger idle_timeout={}s)",
                self.config.prompt.thread_idle_timeout_secs
            ),
        }
    }

    async fn handle_telegram_message(
        &self,
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

        let TelegramTriggerResult {
            normalized,
            thread_scope,
            was_new_thread,
        } = self.trigger.trigger(msg);
        let platform_message = normalized.platform_message;
        let thread_scope = agent::storage::ThreadScope::new(thread_scope.to_string());
        let thread_id = platform_message.thread_id.as_deref();

        info!(
            platform = %platform_message.platform.as_str(),
            chat_id = %platform_message.room_id,
            thread_id = %platform_message.thread_id.as_deref().unwrap_or("none"),
            thread_key = %thread_scope.thread_key(),
            was_new_thread,
            message_id = %platform_message.message_id,
            sender_id = %platform_message.sender_id,
            kind = %platform_message.kind.as_str(),
            text_present = platform_message.text().is_some(),
            reply_present = platform_message.reply.is_some(),
            attachments = platform_message.attachments.len(),
            is_mention = platform_message.is_mention,
            "received telegram message"
        );

        let placeholder_message = match self
            .outbox
            .send_placeholder(
                msg.chat.id,
                thread_id,
                self.config.placeholder_text.as_str(),
            )
            .await
        {
            Ok(message) => Some(message),
            Err(err) => {
                warn!(
                    error = %err,
                    chat_id = %msg.chat.id,
                    thread_id = %thread_id.unwrap_or("none"),
                    "failed to send telegram placeholder"
                );
                None
            }
        };

        if let Err(err) = self.outbox.send_typing(msg.chat.id, thread_id).await {
            warn!(
                error = %err,
                chat_id = %msg.chat.id,
                thread_id = %thread_id.unwrap_or("none"),
                "failed to send telegram typing indicator"
            );
        }

        let prepared = match self
            .app
            .prepare_turn(&thread_scope, &platform_message, None)
            .await
        {
            Ok(prepared) => prepared,
            Err(err) => {
                error!(
                    error = %err,
                    thread_key = %thread_scope.thread_key(),
                    message_id = %platform_message.message_id,
                    "failed to prepare turn"
                );
                let error_text = format!("系统暂时无法记录这条消息：{err}");
                self.report_turn_error(msg.chat.id, placeholder_message.as_ref(), &error_text)
                    .await;
                return Ok(());
            }
        };

        let started = match self
            .app
            .start_turn(&prepared, placeholder_message.clone())
            .await
        {
            Ok(started) => started,
            Err(err) => {
                error!(
                    error = %err,
                    thread_id = %prepared.observation.thread.id,
                    trigger_event_id = %prepared.observation.event.id,
                    "failed to start turn"
                );
                let error_text = format!("系统暂时无法开始处理：{err}");
                self.report_turn_error(msg.chat.id, placeholder_message.as_ref(), &error_text)
                    .await;
                return Ok(());
            }
        };

        let outcome = self.app.respond_turn(&started).await;
        let final_message = outcome.final_message.clone();
        let final_status = Self::resolve_final_turn_status(&outcome);
        let final_error_code = Self::resolve_final_turn_error_code(&outcome);

        if let Err(err) = self
            .app
            .finish_turn(
                &started,
                &outcome,
                final_status,
                final_message,
                final_error_code,
            )
            .await
        {
            return Err(self.storage_error_to_request_error(err));
        }

        Ok(())
    }

    async fn report_turn_error(
        &self,
        chat_id: ChatId,
        existing_message: Option<&ReplyHandle>,
        error_text: &str,
    ) {
        if let Some(existing_message) = existing_message {
            if let Err(err) = self
                .outbox
                .edit_text(chat_id, existing_message.message_id.as_str(), error_text)
                .await
            {
                warn!(
                    error = %err,
                    chat_id = %chat_id,
                    message_id = %existing_message.message_id,
                    "failed to edit telegram placeholder with error"
                );
            }
            return;
        }

        warn!(
            chat_id = %chat_id,
            error_text = %error_text,
            "telegram placeholder unavailable for error reporting"
        );
    }

    fn resolve_final_turn_status(outcome: &TurnOutcome) -> TurnStatus {
        if matches!(outcome.status, TurnStatus::Failed) {
            TurnStatus::Failed
        } else {
            TurnStatus::Completed
        }
    }

    fn resolve_final_turn_error_code(outcome: &TurnOutcome) -> Option<String> {
        outcome.error_code.clone()
    }

    fn storage_error_to_request_error(
        &self,
        err: agent::storage::StorageError,
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
            telegram_description = %self.summary().telegram_description,
            data_dir = ?self.config.data_dir,
            telegram_token_present = self.config.telegram_bot_token.is_some(),
            "runtime starting"
        );
        let bot = teloxide::Bot::new(token);
        self.outbox.bind_bot(bot.clone());
        let runtime = self.clone();

        teloxide::repl(
            bot,
            move |_bot: teloxide::Bot, msg: teloxide::types::Message| {
                let runtime = runtime.clone();

                async move { runtime.handle_telegram_message(&msg).await }
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
        data_dir = ?config.data_dir,
        "configuration loaded"
    );
    let runtime = RuntimeController::try_new(config)?;
    runtime.run().await?;
    Ok(())
}

pub mod runtime {
    pub use super::{RuntimeController, RuntimeSummary, run};
}
