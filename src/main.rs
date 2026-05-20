use std::sync::Arc;

use agent::AgentBuilder;
use telegram_host::TelegramHost;
use tools_telegram::register_telegram_tools;
use tracing::info;
use trigger_telegram::{TelegramTrigger, TriggerConfig};

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

    #[arg(long, env = "OPENAI_API_KEY")]
    openai_api_key: String,

    #[arg(long, env = "OPENAI_BASE_URL")]
    openai_base_url: Option<String>,

    #[arg(long, default_value = "gpt-4o-mini")]
    model: String,

    #[arg(long, default_value = "You are a helpful Telegram bot.")]
    system_prompt: String,

    #[arg(long, default_value = "300")]
    idle_timeout_seconds: u64,

    #[arg(long, default_value = "100")]
    max_thread_length: usize,

    #[arg(long, default_value = "10")]
    max_turns: usize,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    setup_tracing();

    let cli = <Cli as clap::Parser>::parse();

    info!(
        model = %cli.model,
        idle_timeout = cli.idle_timeout_seconds,
        max_thread_length = cli.max_thread_length,
        max_turns = cli.max_turns,
        "starting umohobot"
    );

    let telegram_host = TelegramHost::new(&cli.telegram_token);

    let mut agent_builder = AgentBuilder::new()
        .model(&cli.model)
        .api_key(&cli.openai_api_key)
        .max_turns(cli.max_turns);

    if let Some(base_url) = &cli.openai_base_url {
        agent_builder = agent_builder.base_url(base_url);
    }

    let agent_runtime = agent_builder.build()?;

    register_telegram_tools(
        &agent_runtime.agent().tool_server_handle,
        telegram_host.clone(),
    )
    .await?;

    let config = TriggerConfig {
        idle_timeout: chrono::Duration::seconds(cli.idle_timeout_seconds as i64),
        max_thread_length: cli.max_thread_length,
        system_prompt: cli.system_prompt,
    };

    let trigger = TelegramTrigger::new(telegram_host, Arc::new(agent_runtime), config);

    trigger.start().await
}
