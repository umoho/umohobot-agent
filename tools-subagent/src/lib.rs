use std::sync::Arc;

use agent::{AgentError, AgentRuntime, SubagentStatus};
use rig_core::completion::CompletionModel;
use rig_core::completion::ToolDefinition;
use rig_core::tool::Tool;
use serde::Deserialize;
use serde_json::json;

// ── Args ──

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentCreateArgs {
    pub name: String,
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub tools: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentTokenNameArgs {
    pub token: String,
    pub name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentAskArgs {
    pub token: String,
    pub name: String,
    pub task: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentReadArgs {
    pub token: String,
    pub name: String,
    pub index: Option<i32>,
}

// ── Tool structs ──

pub struct SubagentCreateTool<M: CompletionModel> {
    pub agent: Arc<AgentRuntime<M>>,
}

impl<M: CompletionModel + 'static> Tool for SubagentCreateTool<M> {
    const NAME: &'static str = "subagent_create";

    type Error = AgentError;
    type Args = SubagentCreateArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "subagent_create".into(),
            description: "Create a sub-agent that can run tasks independently in the background. Optionally specify a system_prompt and tool whitelist (tools: \"all\", \"namespace_name\", or \"tool_name\"). The sub-agent inherits the parent's model. Returns a token needed for subsequent operations.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Unique name for the sub-agent within this conversation"
                    },
                    "systemPrompt": {
                        "type": "string",
                        "description": "Optional system prompt for the sub-agent"
                    },
                    "tools": {
                        "type": "string",
                        "description": "Tool whitelist: comma-separated tool names or namespaces (e.g. \"web_scrape,image_ocr\" or \"web\" or \"all\"). Empty means no tools."
                    }
                },
                "required": ["name"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<String, Self::Error> {
        let created = self
            .agent
            .subagent_create(&args.name, args.system_prompt.as_deref(), &args.tools)
            .await?;
        Ok(format!(
            "subagent '{}' created, token={}",
            created.name, created.token
        ))
    }
}

pub struct SubagentAskTool<M: CompletionModel> {
    pub agent: Arc<AgentRuntime<M>>,
}

impl<M: CompletionModel + 'static> Tool for SubagentAskTool<M> {
    const NAME: &'static str = "subagent_ask";

    type Error = AgentError;
    type Args = SubagentAskArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "subagent_ask".into(),
            description: "Send a task to a sub-agent. If the sub-agent is idle, it starts running. If it's already running, returns an error. Use subagent_stop to interrupt the current task.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "token": {
                        "type": "string",
                        "description": "Token returned by subagent_create"
                    },
                    "name": {
                        "type": "string",
                        "description": "Name of the sub-agent"
                    },
                    "task": {
                        "type": "string",
                        "description": "Task prompt for the sub-agent"
                    }
                },
                "required": ["token", "name", "task"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<String, Self::Error> {
        self.agent
            .subagent_ask(&args.token, &args.name, &args.task)
            .await?;
        Ok(format!("task submitted to subagent '{}'", args.name))
    }
}

pub struct SubagentStopTool<M: CompletionModel> {
    pub agent: Arc<AgentRuntime<M>>,
}

impl<M: CompletionModel + 'static> Tool for SubagentStopTool<M> {
    const NAME: &'static str = "subagent_stop";

    type Error = AgentError;
    type Args = SubagentTokenNameArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "subagent_stop".into(),
            description: "Interrupt the currently running task of a sub-agent. The sub-agent returns to idle state; previous results are preserved.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "token": {
                        "type": "string",
                        "description": "Token returned by subagent_create"
                    },
                    "name": {
                        "type": "string",
                        "description": "Name of the sub-agent"
                    }
                },
                "required": ["token", "name"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<String, Self::Error> {
        self.agent.subagent_stop(&args.token, &args.name).await?;
        Ok(format!("subagent '{}' stopped", args.name))
    }
}

pub struct SubagentStatusTool<M: CompletionModel> {
    pub agent: Arc<AgentRuntime<M>>,
}

impl<M: CompletionModel + 'static> Tool for SubagentStatusTool<M> {
    const NAME: &'static str = "subagent_status";

    type Error = AgentError;
    type Args = SubagentTokenNameArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "subagent_status".into(),
            description:
                "Check the status of a sub-agent: idle, running, or completed (with result count)."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "token": {
                        "type": "string",
                        "description": "Token returned by subagent_create"
                    },
                    "name": {
                        "type": "string",
                        "description": "Name of the sub-agent"
                    }
                },
                "required": ["token", "name"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<String, Self::Error> {
        let status = self.agent.subagent_status(&args.token, &args.name).await?;
        let s = match status {
            SubagentStatus::Idle => "idle".into(),
            SubagentStatus::Running => "running".into(),
            SubagentStatus::Completed(available) => {
                format!("completed ({available} available)")
            }
        };
        Ok(s)
    }
}

pub struct SubagentReadTool<M: CompletionModel> {
    pub agent: Arc<AgentRuntime<M>>,
}

impl<M: CompletionModel + 'static> Tool for SubagentReadTool<M> {
    const NAME: &'static str = "subagent_read";

    type Error = AgentError;
    type Args = SubagentReadArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "subagent_read".into(),
            description: "Read a result from a sub-agent. Defaults to the latest result. Use index to access specific results (0 = first, -1 = last, -2 = second-to-last). Results persist and can be read multiple times.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "token": {
                        "type": "string",
                        "description": "Token returned by subagent_create"
                    },
                    "name": {
                        "type": "string",
                        "description": "Name of the sub-agent"
                    },
                    "index": {
                        "type": "integer",
                        "description": "Result index: default -1 (latest), 0 = first, -2 = second-to-last"
                    }
                },
                "required": ["token", "name"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<String, Self::Error> {
        self.agent
            .subagent_read(&args.token, &args.name, args.index)
            .await
    }
}

pub struct SubagentDestroyTool<M: CompletionModel> {
    pub agent: Arc<AgentRuntime<M>>,
}

impl<M: CompletionModel + 'static> Tool for SubagentDestroyTool<M> {
    const NAME: &'static str = "subagent_destroy";

    type Error = AgentError;
    type Args = SubagentTokenNameArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "subagent_destroy".into(),
            description: "Permanently destroy a sub-agent. The current task is interrupted and all resources are freed. A destroyed sub-agent cannot be reused.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "token": {
                        "type": "string",
                        "description": "Token returned by subagent_create"
                    },
                    "name": {
                        "type": "string",
                        "description": "Name of the sub-agent"
                    }
                },
                "required": ["token", "name"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<String, Self::Error> {
        self.agent.subagent_destroy(&args.token, &args.name).await?;
        Ok(format!("subagent '{}' destroyed", args.name))
    }
}

// ── Registration ──

pub async fn register_subagent_tools(
    agent: Arc<AgentRuntime<impl CompletionModel + 'static>>,
) -> Result<(), AgentError> {
    agent
        .register_tool(SubagentCreateTool {
            agent: agent.clone(),
        })
        .await?;
    agent
        .register_tool(SubagentAskTool {
            agent: agent.clone(),
        })
        .await?;
    agent
        .register_tool(SubagentStopTool {
            agent: agent.clone(),
        })
        .await?;
    agent
        .register_tool(SubagentStatusTool {
            agent: agent.clone(),
        })
        .await?;
    agent
        .register_tool(SubagentReadTool {
            agent: agent.clone(),
        })
        .await?;
    agent
        .register_tool(SubagentDestroyTool {
            agent: agent.clone(),
        })
        .await?;
    Ok(())
}
