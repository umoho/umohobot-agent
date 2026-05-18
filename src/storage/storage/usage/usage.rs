use chrono::{DateTime, Utc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsageScopeKind {
    User,
    Group,
    Session,
    Thread,
    Provider,
}

impl UsageScopeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Group => "group",
            Self::Session => "session",
            Self::Thread => "thread",
            Self::Provider => "provider",
        }
    }

    pub fn from_db(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "group" => Some(Self::Group),
            "session" => Some(Self::Session),
            "thread" => Some(Self::Thread),
            "provider" => Some(Self::Provider),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsageScope {
    pub kind: UsageScopeKind,
    pub scope_id: String,
}

impl UsageScope {
    pub fn new(kind: UsageScopeKind, scope_id: impl Into<String>) -> Self {
        Self {
            kind,
            scope_id: scope_id.into(),
        }
    }

    pub fn user(scope_id: impl Into<String>) -> Self {
        Self::new(UsageScopeKind::User, scope_id)
    }

    pub fn group(scope_id: impl Into<String>) -> Self {
        Self::new(UsageScopeKind::Group, scope_id)
    }

    pub fn session(scope_id: impl Into<String>) -> Self {
        Self::new(UsageScopeKind::Session, scope_id)
    }

    pub fn thread(scope_id: impl Into<String>) -> Self {
        Self::new(UsageScopeKind::Thread, scope_id)
    }

    pub fn provider(scope_id: impl Into<String>) -> Self {
        Self::new(UsageScopeKind::Provider, scope_id)
    }

    pub fn key(&self) -> String {
        format!("{}:{}", self.kind.as_str(), self.scope_id)
    }
}

#[derive(Clone, Debug)]
pub struct UsageLedgerRecord {
    pub scope: UsageScope,
    pub provider: String,
    pub turn_id: Option<i64>,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub tool_calls: u64,
    pub estimated: bool,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug)]
pub struct UsageTotalsRecord {
    pub scope: UsageScope,
    pub provider: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub tool_calls: u64,
    pub estimated_prompt_tokens: u64,
    pub estimated_completion_tokens: u64,
    pub estimated_tool_calls: u64,
    pub updated_at: DateTime<Utc>,
}
