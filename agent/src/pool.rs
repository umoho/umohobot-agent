use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::Capability;
use crate::types::ModelAccountEntry;

pub type ProviderName = String;
pub type ModelName = String;
pub type AccountId = usize;
pub type ThreadId = Uuid;

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub provider: ProviderName,
    pub model: ModelName,
    pub capabilities: Vec<Capability>,
}

#[derive(Debug, Clone)]
pub struct Account {
    pub id: AccountId,
    pub provider: ProviderName,
    pub model: ModelName,
    pub api_key: String,
    pub base_url: String,
    pub capabilities: Vec<Capability>,
}

fn guess_base_url(provider: &str) -> Option<&'static str> {
    match provider {
        "openai" => Some("https://api.openai.com/v1"),
        "deepseek" => Some("https://api.deepseek.com/v1"),
        "openrouter" => Some("https://openrouter.ai/api/v1"),
        "groq" => Some("https://api.groq.com/openai/v1"),
        "together" => Some("https://api.together.xyz/v1"),
        "mistral" => Some("https://api.mistral.ai/v1"),
        _ => None,
    }
}

fn parse_capabilities(raw: &[String]) -> Result<Vec<Capability>, String> {
    raw.iter()
        .map(|s| s.parse::<Capability>().map_err(|e| format!("{e}")))
        .collect()
}

pub struct ModelPool {
    accounts: HashMap<(ProviderName, ModelName), Vec<Arc<Account>>>,
    known_models: Vec<(ProviderName, ModelName, Vec<Capability>)>,
    assignments: RwLock<HashMap<(ThreadId, ProviderName, ModelName), Arc<Account>>>,
    usage_counts: RwLock<HashMap<(ProviderName, ModelName), Vec<usize>>>,
}

impl ModelPool {
    pub fn from_entries(entries: Vec<ModelAccountEntry>) -> Result<Self, String> {
        let mut accounts: HashMap<(ProviderName, ModelName), Vec<Arc<Account>>> = HashMap::new();
        let mut known_models: Vec<(ProviderName, ModelName, Vec<Capability>)> = Vec::new();

        for (idx, entry) in entries.into_iter().enumerate() {
            let api_key = match (entry.api_key, entry.api_key_raw) {
                (Some(env_key), None) => std::env::var(&env_key).map_err(|_| {
                    format!(
                        "Environment variable '{}' not set for {}/{} (entry #{})",
                        env_key, entry.provider, entry.model, idx
                    )
                })?,
                (None, Some(raw)) => raw,
                (Some(_), Some(_)) => {
                    return Err(format!(
                        "Cannot specify both api-key and api-key-raw for {}/{} (entry #{})",
                        entry.provider, entry.model, idx
                    ));
                }
                (None, None) => {
                    return Err(format!(
                        "Missing api-key or api-key-raw for {}/{} (entry #{})",
                        entry.provider, entry.model, idx
                    ));
                }
            };

            let base_url = match &entry.base_url {
                Some(url) => url.clone(),
                None => guess_base_url(&entry.provider)
                    .ok_or_else(|| {
                        format!(
                            "Unknown provider '{}' for {}/{} (entry #{}), provide base-url",
                            entry.provider, entry.provider, entry.model, idx
                        )
                    })?
                    .to_string(),
            };

            let capabilities = parse_capabilities(&entry.capabilities)?;

            let account_id = idx;
            let account = Arc::new(Account {
                id: account_id,
                provider: entry.provider.clone(),
                model: entry.model.clone(),
                api_key,
                base_url,
                capabilities: capabilities.clone(),
            });

            let key = (entry.provider.clone(), entry.model.clone());
            accounts.entry(key).or_default().push(account);

            // Track unique (provider, model, capabilities) for display
            if !known_models
                .iter()
                .any(|(p, m, _)| p == &entry.provider && m == &entry.model)
            {
                known_models.push((entry.provider, entry.model, capabilities));
            }
        }

        // Initialize usage counts
        let mut usage_counts = HashMap::new();
        for (key, accs) in &accounts {
            usage_counts.insert(key.clone(), vec![0usize; accs.len()]);
        }

        Ok(Self {
            accounts,
            known_models,
            assignments: RwLock::new(HashMap::new()),
            usage_counts: RwLock::new(usage_counts),
        })
    }

    pub async fn allocate(&self, thread_id: ThreadId, provider: &str, model: &str) -> Arc<Account> {
        let key = (provider.to_string(), model.to_string());
        let assign_key = (thread_id, provider.to_string(), model.to_string());

        {
            let assignments = self.assignments.read().await;
            if let Some(account) = assignments.get(&assign_key) {
                return account.clone();
            }
        }

        let accounts = self.accounts.get(&key).expect("account must exist");
        let mut usage_counts = self.usage_counts.write().await;
        let counts = usage_counts.get_mut(&key).expect("usage counts must exist");

        let min_idx = counts
            .iter()
            .enumerate()
            .min_by_key(|&(_, c)| *c)
            .map(|(i, _)| i)
            .unwrap_or(0);

        counts[min_idx] += 1;
        let account = accounts[min_idx].clone();

        let mut assignments = self.assignments.write().await;
        assignments.insert(assign_key, account.clone());

        account
    }

    pub fn resolve_model(
        &self,
        model: Option<&str>,
        default_provider: &str,
        default_model: &str,
    ) -> Result<(ProviderName, ModelName), String> {
        let spec = match model {
            Some(s) => s,
            None => return Ok((default_provider.to_string(), default_model.to_string())),
        };

        if let Some(slash_pos) = spec.find('/') {
            let provider = spec[..slash_pos].to_string();
            let model_name = spec[slash_pos + 1..].to_string();
            if self
                .accounts
                .contains_key(&(provider.clone(), model_name.clone()))
            {
                Ok((provider, model_name))
            } else {
                Err(format!(
                    "Model '{}' not found in pool for provider '{}'",
                    model_name, provider
                ))
            }
        } else {
            let matches: Vec<_> = self
                .accounts
                .keys()
                .filter(|(_, m)| m == spec)
                .map(|(p, _)| p.clone())
                .collect();
            match matches.len() {
                0 => Err(format!("Model '{}' not found in pool", spec)),
                1 => Ok((matches[0].clone(), spec.to_string())),
                _ => Err(format!(
                    "Model '{}' is ambiguous, available from: {}. Specify as 'provider/{}'",
                    spec,
                    matches.join(", "),
                    spec
                )),
            }
        }
    }

    pub fn available_models(&self) -> Vec<ModelInfo> {
        let mut models: Vec<ModelInfo> = self
            .known_models
            .iter()
            .map(|(provider, model, caps)| ModelInfo {
                provider: provider.clone(),
                model: model.clone(),
                capabilities: caps.clone(),
            })
            .collect();
        models.sort_by(|a, b| a.provider.cmp(&b.provider).then(a.model.cmp(&b.model)));
        models
    }
}
