use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tracing::{debug, info};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolKind {
    Custom(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolRisk {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolSpec {
    pub kind: ToolKind,
    pub name: String,
    pub description: String,
    pub risk: ToolRisk,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub input: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ToolResult {
    pub name: String,
    pub output: String,
    pub sources: Vec<String>,
    pub latency_ms: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, ToolSpec>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, spec: ToolSpec) {
        info!(tool_name = %spec.name, risk = ?spec.risk, "tool registered");
        self.tools.insert(spec.name.clone(), spec);
    }

    pub fn get(&self, name: &str) -> Option<&ToolSpec> {
        debug!(tool_name = %name, exists = self.tools.contains_key(name), "tool lookup");
        self.tools.get(name)
    }

    pub fn count(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.tools.keys().map(String::as_str)
    }
}
