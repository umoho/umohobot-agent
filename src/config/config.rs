use std::env;

#[derive(Clone, Debug)]
pub enum RuntimeMode {
    Telegram,
    Discord,
    Matrix,
    Local,
}

#[derive(Clone, Debug)]
pub enum ProviderKind {
    Ollama,
    OpenAICompatible,
    OpenAI,
    Custom,
}

#[derive(Clone, Debug)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub base_url: Option<String>,
    pub model: String,
    pub api_key_env: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub bot_name: String,
    pub runtime_mode: RuntimeMode,
    pub telegram_bot_token: Option<String>,
    pub default_provider: ProviderConfig,
    pub allow_user_provider: bool,
    pub max_response_chars: usize,
    pub message_edit_throttle_ms: u64,
    pub placeholder_text: String,
}

impl Config {
    pub fn load() -> Self {
        let bot_name = env::var("BOT_NAME").unwrap_or_else(|_| "umohobot".to_string());
        let runtime_mode = match env::var("RUNTIME_MODE").ok().as_deref() {
            Some("discord") => RuntimeMode::Discord,
            Some("matrix") => RuntimeMode::Matrix,
            Some("local") => RuntimeMode::Local,
            _ => RuntimeMode::Telegram,
        };
        let default_provider = ProviderConfig::load();
        let allow_user_provider = env::var("ALLOW_USER_PROVIDER")
            .map(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false);
        let max_response_chars = env::var("MAX_RESPONSE_CHARS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(4_000);
        let message_edit_throttle_ms = env::var("MESSAGE_EDIT_THROTTLE_MS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(750);
        let placeholder_text =
            env::var("PLACEHOLDER_TEXT").unwrap_or_else(|_| "正在处理...".to_string());

        Self {
            bot_name,
            runtime_mode,
            telegram_bot_token: env::var("TELEGRAM_BOT_TOKEN")
                .ok()
                .or_else(|| env::var("TELOXIDE_TOKEN").ok()),
            default_provider,
            allow_user_provider,
            max_response_chars,
            message_edit_throttle_ms,
            placeholder_text,
        }
    }
}

impl ProviderConfig {
    fn load() -> Self {
        let kind = match env::var("DEFAULT_PROVIDER").ok().as_deref() {
            Some("openai") => ProviderKind::OpenAI,
            Some("compat") => ProviderKind::OpenAICompatible,
            Some("custom") => ProviderKind::Custom,
            _ => ProviderKind::Ollama,
        };
        let base_url = env::var("DEFAULT_BASE_URL").ok().or_else(|| {
            matches!(kind, ProviderKind::Ollama).then(|| "http://127.0.0.1:11434".to_string())
        });
        let model = env::var("DEFAULT_MODEL").unwrap_or_else(|_| {
            if matches!(kind, ProviderKind::Ollama) {
                "deepseek-r1:8b".to_string()
            } else {
                "llama3.1".to_string()
            }
        });
        let api_key_env = env::var("DEFAULT_API_KEY_ENV").ok();

        Self {
            kind,
            base_url,
            model,
            api_key_env,
        }
    }
}
