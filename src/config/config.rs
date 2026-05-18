use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMode {
    Telegram,
    Discord,
    Matrix,
    Local,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum ProviderKind {
    #[serde(rename = "ollama")]
    Ollama,
    #[serde(rename = "openai_compatible")]
    OpenAICompatible,
    #[serde(rename = "openai")]
    OpenAI,
    #[serde(rename = "custom")]
    Custom,
}

#[derive(Clone, Debug)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub base_url: Option<String>,
    pub model: String,
    pub api_key_ref: Option<String>,
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
    pub data_dir: Option<PathBuf>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("无法读取配置文件 {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("无法解析配置文件 {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error(
        "配置文件 {path} 缺少必要字段: {missing}。Telegram token 需要通过环境变量 TELEGRAM_BOT_TOKEN 或 TELOXIDE_TOKEN 提供。请复制 config.example.toml 为 config.toml 并填写。"
    )]
    Missing { path: PathBuf, missing: String },
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct FileConfig {
    app: AppFileConfig,
    provider: ProviderFileConfig,
    storage: StorageFileConfig,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct AppFileConfig {
    bot_name: Option<String>,
    runtime_mode: Option<RuntimeMode>,
    allow_user_provider: Option<bool>,
    max_response_chars: Option<usize>,
    message_edit_throttle_ms: Option<u64>,
    placeholder_text: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct ProviderFileConfig {
    kind: Option<ProviderKind>,
    base_url: Option<String>,
    model: Option<String>,
    api_key_ref: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
struct StorageFileConfig {
    data_dir: Option<PathBuf>,
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let (config_path, explicit) = Self::resolve_config_path();
        let file = Self::load_file(&config_path, explicit)?;

        Self::from_sources(config_path, file)
    }

    fn from_sources(config_path: PathBuf, file: FileConfig) -> Result<Self, ConfigError> {
        let bot_name = env_string("BOT_NAME")
            .or(file.app.bot_name)
            .unwrap_or_else(|| "umohobot".to_string());
        let runtime_mode = env_runtime_mode()
            .or(file.app.runtime_mode)
            .unwrap_or(RuntimeMode::Telegram);
        let telegram_bot_token =
            env_string("TELEGRAM_BOT_TOKEN").or_else(|| env_string("TELOXIDE_TOKEN"));
        let allow_user_provider = env_bool("ALLOW_USER_PROVIDER")
            .or(file.app.allow_user_provider)
            .unwrap_or(false);
        let max_response_chars = env_usize("MAX_RESPONSE_CHARS")
            .or(file.app.max_response_chars)
            .unwrap_or(4_000);
        let message_edit_throttle_ms = env_u64("MESSAGE_EDIT_THROTTLE_MS")
            .or(file.app.message_edit_throttle_ms)
            .unwrap_or(750);
        let placeholder_text = env_string("PLACEHOLDER_TEXT")
            .or(file.app.placeholder_text)
            .unwrap_or_else(|| "正在处理...".to_string());
        let provider_kind = env_provider_kind()
            .or_else(|| env_legacy_provider_kind())
            .or(file.provider.kind);
        let provider_model = env_string("DEFAULT_MODEL").or(file.provider.model);
        let provider_base_url = env_string("DEFAULT_BASE_URL")
            .or(file.provider.base_url)
            .or_else(|| {
                provider_kind
                    .and_then(ProviderKind::default_base_url)
                    .map(str::to_string)
            });
        let api_key_ref = env_string("DEFAULT_API_KEY_REF")
            .or_else(|| env_string("DEFAULT_API_KEY_ENV"))
            .or(file.provider.api_key_ref);
        let data_dir = env_string("DATA_DIR")
            .map(PathBuf::from)
            .or(file.storage.data_dir);

        let mut missing = Vec::new();
        if provider_kind.is_none() {
            missing.push("provider.kind");
        }
        if provider_model
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
        {
            missing.push("provider.model");
        }
        if provider_kind
            .map(ProviderKind::requires_api_key_ref)
            .unwrap_or(false)
            && api_key_ref
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
        {
            missing.push("provider.api_key_ref");
        }
        if provider_kind
            .map(ProviderKind::requires_explicit_base_url)
            .unwrap_or(false)
            && provider_base_url
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
        {
            missing.push("provider.base_url");
        }
        if matches!(runtime_mode, RuntimeMode::Telegram) && telegram_bot_token.is_none() {
            missing.push("telegram.bot_token");
        }

        if !missing.is_empty() {
            return Err(ConfigError::Missing {
                path: config_path,
                missing: missing.join(", "),
            });
        }

        Ok(Self {
            bot_name,
            runtime_mode,
            telegram_bot_token,
            default_provider: ProviderConfig {
                kind: provider_kind.expect("validated above"),
                base_url: provider_base_url,
                model: provider_model.expect("validated above"),
                api_key_ref,
            },
            allow_user_provider,
            max_response_chars,
            message_edit_throttle_ms,
            placeholder_text,
            data_dir,
        })
    }

    fn resolve_config_path() -> (PathBuf, bool) {
        env::var("UMOHOBOT_CONFIG")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .map(|path| (path, true))
            .unwrap_or_else(|| (PathBuf::from("config.toml"), false))
    }

    fn load_file(path: &Path, explicit: bool) -> Result<FileConfig, ConfigError> {
        if !path.exists() {
            if explicit {
                return Err(ConfigError::Read {
                    path: path.to_path_buf(),
                    source: io::Error::new(
                        io::ErrorKind::NotFound,
                        "通过 UMOHOBOT_CONFIG 指定的配置文件不存在",
                    ),
                });
            }
            return Ok(FileConfig::default());
        }

        let raw = fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        toml::from_str(&raw).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })
    }
}

impl ProviderKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Ollama => "ollama",
            Self::OpenAICompatible => "openai_compatible",
            Self::OpenAI => "openai",
            Self::Custom => "custom",
        }
    }

    pub(crate) fn default_base_url(self) -> Option<&'static str> {
        match self {
            Self::Ollama => Some("http://127.0.0.1:11434"),
            Self::OpenAI => Some("https://api.openai.com/v1"),
            Self::OpenAICompatible | Self::Custom => None,
        }
    }

    pub(crate) fn requires_api_key_ref(self) -> bool {
        !matches!(self, Self::Ollama)
    }

    pub(crate) fn requires_explicit_base_url(self) -> bool {
        matches!(self, Self::OpenAICompatible | Self::Custom)
    }

    fn from_env(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "ollama" => Some(Self::Ollama),
            "openai_compatible" | "openai-compatible" | "compat" => Some(Self::OpenAICompatible),
            "openai" => Some(Self::OpenAI),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

impl RuntimeMode {
    fn from_env(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "telegram" => Some(Self::Telegram),
            "discord" => Some(Self::Discord),
            "matrix" => Some(Self::Matrix),
            "local" => Some(Self::Local),
            _ => None,
        }
    }
}

fn env_string(key: &str) -> Option<String> {
    env::var(key).ok().filter(|value| !value.trim().is_empty())
}

fn env_bool(key: &str) -> Option<bool> {
    env::var(key)
        .ok()
        .and_then(|value| match value.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
}

fn env_usize(key: &str) -> Option<usize> {
    env::var(key).ok().and_then(|value| value.parse().ok())
}

fn env_u64(key: &str) -> Option<u64> {
    env::var(key).ok().and_then(|value| value.parse().ok())
}

fn env_runtime_mode() -> Option<RuntimeMode> {
    env_string("RUNTIME_MODE").and_then(|value| RuntimeMode::from_env(&value))
}

fn env_provider_kind() -> Option<ProviderKind> {
    env_string("DEFAULT_PROVIDER_KIND").and_then(|value| ProviderKind::from_env(&value))
}

fn env_legacy_provider_kind() -> Option<ProviderKind> {
    env_string("DEFAULT_PROVIDER").and_then(|value| ProviderKind::from_env(&value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_kind_default_base_urls_match_expected_backends() {
        assert_eq!(
            ProviderKind::Ollama.default_base_url(),
            Some("http://127.0.0.1:11434")
        );
        assert_eq!(
            ProviderKind::OpenAI.default_base_url(),
            Some("https://api.openai.com/v1")
        );
        assert_eq!(ProviderKind::OpenAICompatible.default_base_url(), None);
        assert_eq!(ProviderKind::Custom.default_base_url(), None);
    }

    #[test]
    fn provider_kind_requirement_flags_match_expected_backends() {
        assert!(!ProviderKind::Ollama.requires_api_key_ref());
        assert!(ProviderKind::OpenAI.requires_api_key_ref());
        assert!(ProviderKind::OpenAICompatible.requires_api_key_ref());
        assert!(ProviderKind::Custom.requires_api_key_ref());

        assert!(!ProviderKind::Ollama.requires_explicit_base_url());
        assert!(!ProviderKind::OpenAI.requires_explicit_base_url());
        assert!(ProviderKind::OpenAICompatible.requires_explicit_base_url());
        assert!(ProviderKind::Custom.requires_explicit_base_url());
    }
}
