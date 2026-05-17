use std::{env, path::PathBuf};

#[derive(Clone, Debug)]
pub struct UserProfile {
    pub user_id: String,
    pub display_name: Option<String>,
    pub default_provider: Option<String>,
}

#[derive(Clone, Debug)]
pub struct UsageRecord {
    pub scope: String,
    pub provider: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub tool_calls: u64,
    pub estimated: bool,
}

#[derive(Clone, Debug)]
pub struct ProviderBinding {
    pub owner_scope: String,
    pub provider_name: String,
    pub base_url: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Storage {
    pub data_dir: Option<PathBuf>,
}

impl Storage {
    pub fn new() -> Self {
        Self {
            data_dir: env::var("DATA_DIR").ok().map(PathBuf::from),
        }
    }

    pub fn is_ready(&self) -> bool {
        true
    }

    pub fn data_dir(&self) -> Option<&PathBuf> {
        self.data_dir.as_ref()
    }

    pub fn record_usage(&self, _record: UsageRecord) {}

    pub fn record_provider_binding(&self, _binding: ProviderBinding) {}
}
