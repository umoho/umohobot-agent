use tracing::{debug, warn};

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
        warn!(
            user_id = %ctx.user_id,
            room_id = %ctx.room_id,
            tool_name = %ctx.tool_name,
            request_chars = ctx.request_summary.chars().count(),
            "tool call requires confirmation"
        );
        PolicyDecision::NeedConfirmation {
            reason: format!("工具 `{}` 的执行层尚未接入", ctx.tool_name),
        }
    }

    pub fn decide_quota(&self, quota: &QuotaSnapshot) -> PolicyDecision {
        if matches!(quota.remaining_tokens, Some(0)) {
            warn!(
                prompt_tokens = quota.prompt_tokens,
                completion_tokens = quota.completion_tokens,
                tool_calls = quota.tool_calls,
                "quota denied"
            );
            PolicyDecision::Deny {
                reason: "剩余额度为 0".to_string(),
            }
        } else {
            debug!(
                prompt_tokens = quota.prompt_tokens,
                completion_tokens = quota.completion_tokens,
                tool_calls = quota.tool_calls,
                remaining_tokens = ?quota.remaining_tokens,
                "quota allowed"
            );
            PolicyDecision::Allow
        }
    }
}

pub mod policy {
    pub use super::{PolicyDecision, PolicyEngine, QuotaSnapshot, ToolCallContext};
}
