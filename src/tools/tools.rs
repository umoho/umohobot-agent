use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ToolKind {
    Calculator,
    WebSearch,
    Custom(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolRisk {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug)]
pub struct ToolSpec {
    pub kind: ToolKind,
    pub name: String,
    pub description: String,
    pub risk: ToolRisk,
}

#[derive(Clone, Debug)]
pub struct ToolCall {
    pub name: String,
    pub input: String,
}

#[derive(Clone, Debug, Default)]
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

#[derive(Clone, Debug, Default)]
pub struct CalculatorTool;

#[derive(Clone, Debug, Default)]
pub struct WebSearchTool;

impl ToolRegistry {
    pub fn new() -> Self {
        let mut registry = Self::default();
        registry.register(CalculatorTool::spec());
        registry.register(WebSearchTool::spec());
        registry
    }

    pub fn register(&mut self, spec: ToolSpec) {
        self.tools.insert(spec.name.clone(), spec);
    }

    pub fn get(&self, name: &str) -> Option<&ToolSpec> {
        self.tools.get(name)
    }

    pub fn count(&self) -> usize {
        self.tools.len()
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.tools.keys().map(String::as_str)
    }
}

impl CalculatorTool {
    pub fn spec() -> ToolSpec {
        ToolSpec {
            kind: ToolKind::Calculator,
            name: "calculator".to_string(),
            description: "执行数学计算、公式求值和单位换算".to_string(),
            risk: ToolRisk::Low,
        }
    }
}

impl WebSearchTool {
    pub fn spec() -> ToolSpec {
        ToolSpec {
            kind: ToolKind::WebSearch,
            name: "web_search".to_string(),
            description: "用于资料查询、事实核验和带来源回答".to_string(),
            risk: ToolRisk::Low,
        }
    }
}
