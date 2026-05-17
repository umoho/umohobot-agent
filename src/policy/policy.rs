#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Deny { reason: String },
    NeedConfirmation { reason: String },
}

#[derive(Clone, Debug)]
pub struct ToolCallContext {
    pub user_id: String,
    pub room_id: String,
    pub tool_name: String,
    pub request_summary: String,
}

#[derive(Clone, Debug, Default)]
pub struct QuotaSnapshot {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub tool_calls: u64,
    pub remaining_tokens: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct PolicyEngine;

impl PolicyEngine {
    pub fn new() -> Self {
        Self
    }

    pub fn default_decision(&self) -> PolicyDecision {
        PolicyDecision::Allow
    }

    pub fn decide_tool_call(&self, ctx: &ToolCallContext) -> PolicyDecision {
        match ctx.tool_name.as_str() {
            "calculator" | "web_search" => PolicyDecision::Allow,
            other => PolicyDecision::NeedConfirmation {
                reason: format!("工具 `{other}` 尚未纳入默认放行范围"),
            },
        }
    }

    pub fn decide_quota(&self, quota: &QuotaSnapshot) -> PolicyDecision {
        if matches!(quota.remaining_tokens, Some(0)) {
            PolicyDecision::Deny {
                reason: "剩余额度为 0".to_string(),
            }
        } else {
            PolicyDecision::Allow
        }
    }
}
