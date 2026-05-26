use std::sync::Arc;

use agent::{AgentBuilder, ConfigFile, ModelPool};
use data_buffer::DataBuffer;
use telegram_host::{MessageCache, TelegramHost};
use tools_image::ocr::{ImageOcrTool, ocrs::OcrsBackend};
use tools_subagent::register_subagent_tools;
use tools_telegram::{TelegramDownloadTool, register_telegram_tools};
use tools_time::{TimerConfig, TimerExpiry, register_time_tools};
use tools_web::{WebFetchTool, WebFindTool, WebScrapeTool};
use tracing::info;
use trigger_telegram::{TelegramTrigger, TriggerConfig};
use uuid::Uuid;

fn setup_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,umohobot=debug".into()),
        )
        .init();
}

#[derive(clap::Parser)]
#[command(name = "umohobot")]
struct Cli {
    #[arg(long, env = "TELEGRAM_BOT_TOKEN")]
    telegram_token: String,

    #[arg(long, default_value = "config.toml")]
    config: String,

    #[arg(long, default_value = "You are a helpful Telegram bot.")]
    system_prompt: String,

    #[arg(long, default_value = "300")]
    idle_timeout_seconds: u64,

    #[arg(long, default_value = "100")]
    max_thread_length: usize,

    #[arg(long, default_value = "10")]
    max_turns: usize,

    #[arg(long)]
    compact_prompt: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    setup_tracing();

    let cli = <Cli as clap::Parser>::parse();

    let config_content = std::fs::read_to_string(&cli.config)
        .map_err(|e| format!("Failed to read config file '{}': {}", cli.config, e))?;
    let config_file: ConfigFile = toml::from_str(&config_content)
        .map_err(|e| format!("Failed to parse config file: {}", e))?;

    let model_pool = Arc::new(ModelPool::from_entries(config_file.model_accounts)?);

    let (default_provider, default_model_name) = config_file
        .default_model
        .split_once('/')
        .map(|(p, m)| (p.to_string(), m.to_string()))
        .unwrap_or_else(|| {
            panic!(
                "default-model must be in 'provider/model' format, got '{}'",
                config_file.default_model
            )
        });

    let default_account = model_pool
        .allocate(Uuid::new_v4(), &default_provider, &default_model_name)
        .await;

    let agent_runtime = Arc::new(
        AgentBuilder::new()
            .max_turns(cli.max_turns)
            .build(&default_account, model_pool.clone())?,
    );

    info!(
        model = %agent_runtime.parent_account.model,
        provider = %agent_runtime.parent_account.provider,
        idle_timeout = cli.idle_timeout_seconds,
        max_thread_length = cli.max_thread_length,
        max_turns = cli.max_turns,
        "starting umohobot"
    );

    let telegram_host = TelegramHost::new(&cli.telegram_token);

    let cache = MessageCache::new(200);

    register_telegram_tools(agent_runtime.as_ref(), telegram_host.clone(), cache.clone()).await?;

    agent_runtime.register_tool(WebScrapeTool).await?;
    agent_runtime.register_tool(WebFetchTool).await?;
    agent_runtime.register_tool(WebFindTool).await?;

    let data_buffer = DataBuffer::new();
    let ocr_backend = Box::new(OcrsBackend::new().await?);
    agent_runtime
        .register_tool(ImageOcrTool::new(ocr_backend, data_buffer.clone()))
        .await?;

    agent_runtime
        .register_tool(TelegramDownloadTool {
            host: telegram_host.clone(),
            buffer: data_buffer,
        })
        .await?;

    register_subagent_tools(agent_runtime.clone()).await?;

    let (expiry_tx, expiry_rx) = tokio::sync::mpsc::unbounded_channel::<TimerExpiry>();
    register_time_tools(agent_runtime.clone(), TimerConfig::default(), expiry_tx).await?;

    let config = TriggerConfig {
        idle_timeout: chrono::Duration::seconds(cli.idle_timeout_seconds as i64),
        max_thread_length: cli.max_thread_length,
        system_prompt: cli.system_prompt,
        compact_prompt: cli.compact_prompt.unwrap_or_default(),
        available_models: model_pool.available_models(),
    };

    let trigger = TelegramTrigger::new(
        telegram_host,
        agent_runtime.clone(),
        config,
        cache,
        Some(expiry_rx),
    );

    trigger.start().await
}
