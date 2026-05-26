use std::sync::Arc;

use agent::{AgentError, AgentRuntime, SubagentStatus};
use base64::Engine;
use data_buffer::DataBuffer;
use rig_core::completion::ToolDefinition;
use rig_core::completion::message::{
    AudioMediaType, ImageDetail, ImageMediaType, MimeType, UserContent,
};
use rig_core::tool::Tool;
use serde::Deserialize;
use serde_json::json;

// ── Content part types ──

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ContentPart {
    Text {
        text: String,
    },
    Image {
        buffer_key: String,
        detail: Option<String>,
    },
    Audio {
        buffer_key: String,
    },
}

// ── Args ──

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentCreateArgs {
    pub name: String,
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub tools: String,
    pub model: Option<String>,
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
    pub content: Vec<ContentPart>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentReadArgs {
    pub token: String,
    pub name: String,
    pub index: Option<i32>,
}

// ── Tool structs ──

pub struct SubagentCreateTool {
    pub agent: Arc<AgentRuntime>,
}

impl Tool for SubagentCreateTool {
    const NAME: &'static str = "subagent_create";

    type Error = AgentError;
    type Args = SubagentCreateArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "subagent_create".into(),
            description: "Create a sub-agent that can run tasks independently in the background. Optionally specify a system_prompt, tool whitelist, and model. The sub-agent inherits the parent's model if omitted. Returns a token needed for subsequent operations.".into(),
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
                    },
                    "model": {
                        "type": "string",
                        "description": "Optional model for the sub-agent, e.g. \"gpt-4o-mini\" or \"openai/gpt-4o-mini\". Inherits parent's model if omitted. Use subagent_status to see available models."
                    }
                },
                "required": ["name"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<String, Self::Error> {
        let created = self
            .agent
            .subagent_create(
                &args.name,
                args.system_prompt.as_deref(),
                &args.tools,
                args.model.as_deref(),
            )
            .await?;
        Ok(format!(
            "subagent '{}' created, token={}",
            created.name, created.token
        ))
    }
}

pub struct SubagentAskTool {
    pub agent: Arc<AgentRuntime>,
    pub buffer: DataBuffer,
}

impl Tool for SubagentAskTool {
    const NAME: &'static str = "subagent_ask";

    type Error = AgentError;
    type Args = SubagentAskArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "subagent_ask".into(),
            description: "Send a task to a sub-agent. Supports text, images, and audio via the content array. Use subagent_stop to interrupt the current task.".into(),
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
                    "content": {
                        "type": "array",
                        "description": "Array of content parts. Each part has a 'type': 'text', 'image', or 'audio'.",
                        "items": {
                            "oneOf": [
                                {
                                    "type": "object",
                                    "properties": {
                                        "type": { "type": "string", "enum": ["text"] },
                                        "text": { "type": "string", "description": "Text content" }
                                    },
                                    "required": ["type", "text"]
                                },
                                {
                                    "type": "object",
                                    "properties": {
                                        "type": { "type": "string", "enum": ["image"] },
                                        "bufferKey": { "type": "string", "description": "Buffer key from telegram_download or other buffer tools" },
                                        "detail": { "type": "string", "enum": ["low", "high", "auto"], "description": "Optional image detail (default: auto)" }
                                    },
                                    "required": ["type", "bufferKey"]
                                },
                                {
                                    "type": "object",
                                    "properties": {
                                        "type": { "type": "string", "enum": ["audio"] },
                                        "bufferKey": { "type": "string", "description": "Buffer key from telegram_download or other buffer tools" }
                                    },
                                    "required": ["type", "bufferKey"]
                                }
                            ]
                        }
                    }
                },
                "required": ["token", "name", "content"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<String, Self::Error> {
        let mut content = Vec::new();

        for part in args.content {
            match part {
                ContentPart::Text { text } => {
                    content.push(UserContent::text(text));
                }
                ContentPart::Image { buffer_key, detail } => {
                    let bytes = self
                        .buffer
                        .get(&buffer_key)
                        .ok_or_else(|| AgentError::BufferKeyNotFound(buffer_key.clone()))?;

                    let mime = infer::get(&bytes)
                        .ok_or_else(|| AgentError::MediaTypeDetectionFailed(buffer_key.clone()))?
                        .mime_type()
                        .to_string();

                    let media_type = ImageMediaType::from_mime_type(&mime);
                    let detail = match detail.as_deref() {
                        Some("low") => ImageDetail::Low,
                        Some("high") => ImageDetail::High,
                        _ => ImageDetail::Auto,
                    };

                    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    content.push(UserContent::image_base64(b64, media_type, Some(detail)));
                }
                ContentPart::Audio { buffer_key } => {
                    let bytes = self
                        .buffer
                        .get(&buffer_key)
                        .ok_or_else(|| AgentError::BufferKeyNotFound(buffer_key.clone()))?;

                    let mime = infer::get(&bytes)
                        .ok_or_else(|| AgentError::MediaTypeDetectionFailed(buffer_key.clone()))?
                        .mime_type()
                        .to_string();

                    let media_type = AudioMediaType::from_mime_type(&mime);

                    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    content.push(UserContent::audio(b64, media_type));
                }
            }
        }

        self.agent
            .subagent_ask(&args.token, &args.name, content)
            .await?;
        Ok(format!("task submitted to subagent '{}'", args.name))
    }
}

pub struct SubagentStopTool {
    pub agent: Arc<AgentRuntime>,
}

impl Tool for SubagentStopTool {
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

pub struct SubagentStatusTool {
    pub agent: Arc<AgentRuntime>,
}

impl Tool for SubagentStatusTool {
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

pub struct SubagentReadTool {
    pub agent: Arc<AgentRuntime>,
}

impl Tool for SubagentReadTool {
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

pub struct SubagentDestroyTool {
    pub agent: Arc<AgentRuntime>,
}

impl Tool for SubagentDestroyTool {
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
    agent: Arc<AgentRuntime>,
    buffer: DataBuffer,
) -> Result<(), AgentError> {
    agent
        .register_tool(SubagentCreateTool {
            agent: agent.clone(),
        })
        .await?;
    agent
        .register_tool(SubagentAskTool {
            agent: agent.clone(),
            buffer,
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
