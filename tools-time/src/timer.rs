use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use chrono_english::{Dialect, parse_date_string};
use rig_core::completion::ToolDefinition;
use rig_core::tool::Tool;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::RwLock;
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use crate::{Error, TimerConfig, TimerExpiry};

#[derive(Debug, Clone)]
pub(crate) struct TimerEntry {
    pub id: Uuid,
    pub target_time: DateTime<Utc>,
    pub task: String,
    pub thread_id: Uuid,
    #[allow(dead_code)]
    pub created_at: DateTime<Utc>,
}

pub(crate) struct TimerState {
    pub timers: Arc<RwLock<HashMap<Uuid, TimerEntry>>>,
    pub config: TimerConfig,
    pub expiry_tx: UnboundedSender<TimerExpiry>,
}

// ── timer_set ──

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerSetArgs {
    pub datetime: String,
    pub task: String,
}

pub struct TimerSetTool {
    pub(crate) state: Arc<TimerState>,
}

impl TimerSetTool {
    fn parse_datetime(&self, input: &str) -> Result<DateTime<Utc>, Error> {
        if let Ok(secs) = input.parse::<u64>() {
            return Ok(Utc::now() + Duration::seconds(secs as i64));
        }

        if let Ok(dt) = parse_date_string(input, Utc::now(), Dialect::Uk) {
            return Ok(dt);
        }

        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(input) {
            return Ok(dt.with_timezone(&Utc));
        }

        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(input, "%Y-%m-%dT%H:%M:%S") {
            return Ok(dt.and_utc());
        }

        if let Ok(dt) = chrono::NaiveDate::parse_from_str(input, "%Y-%m-%d") {
            return Ok(dt.and_hms_opt(0, 0, 0).unwrap().and_utc());
        }

        Err(Error::ParseError(format!(
            "cannot parse datetime: '{input}'. Try natural language like 'in 5 minutes', \
            ISO 8601 like '2026-06-01T10:00:00Z', or just a number of seconds"
        )))
    }
}

impl Tool for TimerSetTool {
    const NAME: &'static str = "time_timer_set";

    type Error = Error;
    type Args = TimerSetArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: "Set a one-shot timer. When the time is reached, the agent will be triggered with the task description. \
                datetime supports: natural language (\"in 5 minutes\", \"next Monday 9am\", \"tomorrow at 3pm\"), \
                ISO 8601 (\"2026-06-01T10:00:00Z\"), or a plain number as seconds from now (\"1800\" = 30 min).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "datetime": {
                        "type": "string",
                        "description": "When to trigger. Natural language (\"in 5 minutes\", \"next Monday 9am\"), ISO 8601, or seconds from now (\"1800\")"
                    },
                    "task": {
                        "type": "string",
                        "description": "Task description — what to do when the timer fires. The agent will be notified with this text."
                    }
                },
                "required": ["datetime", "task"]
            }),
        }
    }

    async fn call(&self, args: TimerSetArgs) -> Result<String, Error> {
        let thread_id = agent::CURRENT_PARENT_THREAD_ID
            .try_with(|id| *id)
            .map_err(|_| Error::NotInAgentContext)?;

        let target_time = self.parse_datetime(&args.datetime)?;
        let now = Utc::now();
        let delay = target_time - now;

        if delay < Duration::seconds(self.state.config.min_delay_secs as i64) {
            return Err(Error::TimerTooShort {
                min_secs: self.state.config.min_delay_secs,
                actual_secs: delay.num_seconds().max(0) as u64,
            });
        }

        if delay > Duration::seconds(self.state.config.max_delay_secs as i64) {
            return Err(Error::TimerTooLong {
                max_secs: self.state.config.max_delay_secs,
                actual_secs: delay.num_seconds() as u64,
            });
        }

        {
            let timers = self.state.timers.read().await;
            let thread_count = timers.values().filter(|t| t.thread_id == thread_id).count();
            if thread_count >= self.state.config.max_per_thread {
                return Err(Error::TimerLimitReached {
                    max: self.state.config.max_per_thread,
                });
            }
        }

        let id = Uuid::new_v4();
        let entry = TimerEntry {
            id,
            target_time,
            task: args.task.clone(),
            thread_id,
            created_at: now,
        };

        self.state.timers.write().await.insert(id, entry.clone());

        let state = self.state.clone();
        let delay_std = delay.to_std().unwrap_or(StdDuration::from_secs(0));
        tokio::spawn(async move {
            tokio::time::sleep(delay_std).await;

            let _ = state.timers.write().await.remove(&id);

            let expiry = TimerExpiry {
                thread_id,
                task: args.task,
            };
            let _ = state.expiry_tx.send(expiry);
        });

        Ok(json!({
            "id": id.to_string(),
            "targetTime": target_time.to_rfc3339(),
            "task": entry.task,
            "delaySeconds": delay.num_seconds(),
        })
        .to_string())
    }
}

// ── timer_list ──

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerListArgs {
    pub id: Option<String>,
    pub task: Option<String>,
}

pub struct TimerListTool {
    pub(crate) state: Arc<TimerState>,
}

impl Tool for TimerListTool {
    const NAME: &'static str = "time_timer_list";

    type Error = Error;
    type Args = TimerListArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description:
                "List active timers. Optionally filter by id (UUID) or task (keyword match). \
                Returns timer id, target time, task description, and remaining seconds."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Filter by timer UUID (optional)"
                    },
                    "task": {
                        "type": "string",
                        "description": "Filter by task keyword match (optional)"
                    }
                }
            }),
        }
    }

    async fn call(&self, args: TimerListArgs) -> Result<String, Error> {
        let timers = self.state.timers.read().await;
        let now = Utc::now();

        let mut results: Vec<serde_json::Value> = timers
            .values()
            .filter(|t| {
                if let Some(ref id_str) = args.id {
                    if let Ok(uid) = Uuid::parse_str(id_str) {
                        if t.id != uid {
                            return false;
                        }
                    }
                }
                if let Some(ref msg) = args.task {
                    if !t.task.to_lowercase().contains(&msg.to_lowercase()) {
                        return false;
                    }
                }
                true
            })
            .map(|t| {
                let remaining = (t.target_time - now).num_seconds().max(0);
                json!({
                    "id": t.id.to_string(),
                    "targetTime": t.target_time.to_rfc3339(),
                    "task": t.task,
                    "remainingSeconds": remaining,
                })
            })
            .collect();

        results.sort_by(|a, b| {
            a["remainingSeconds"]
                .as_i64()
                .unwrap_or(0)
                .cmp(&b["remainingSeconds"].as_i64().unwrap_or(0))
        });

        Ok(serde_json::to_string_pretty(&results).unwrap_or_else(|_| "[]".into()))
    }
}

// ── timer_cancel ──

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerCancelArgs {
    pub id: Option<String>,
    pub task: Option<String>,
}

pub struct TimerCancelTool {
    pub(crate) state: Arc<TimerState>,
}

impl Tool for TimerCancelTool {
    const NAME: &'static str = "time_timer_cancel";

    type Error = Error;
    type Args = TimerCancelArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: "Cancel one or more active timers. Provide the timer UUID, or a keyword to match against task descriptions. \
                At least one of id or task must be provided.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "UUID of the timer to cancel (optional)"
                    },
                    "task": {
                        "type": "string",
                        "description": "Keyword to match against task descriptions (optional)"
                    }
                }
            }),
        }
    }

    async fn call(&self, args: TimerCancelArgs) -> Result<String, Error> {
        if args.id.is_none() && args.task.is_none() {
            return Err(Error::ParseError(
                "at least one of 'id' or 'task' must be provided".into(),
            ));
        }

        let mut timers = self.state.timers.write().await;
        let mut cancelled = Vec::new();

        timers.retain(|&id, t| {
            let match_id = args.id.as_ref().map_or(true, |id_str| {
                Uuid::parse_str(id_str).map_or(false, |uid| uid == id)
            });
            let match_task = args.task.as_ref().map_or(true, |kw| {
                t.task.to_lowercase().contains(&kw.to_lowercase())
            });
            let should_remove = match_id && match_task;
            if should_remove {
                cancelled.push(json!({
                    "id": id.to_string(),
                    "targetTime": t.target_time.to_rfc3339(),
                    "task": t.task,
                }));
            }
            !should_remove
        });

        if cancelled.is_empty() {
            return Err(Error::TimerNotFound);
        }

        Ok(json!({
            "cancelled": cancelled,
        })
        .to_string())
    }
}
