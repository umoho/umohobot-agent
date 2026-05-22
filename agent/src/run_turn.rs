use chrono::Utc;
use rig_core::agent::Agent;
use rig_core::completion::message::UserContent;
use rig_core::completion::{self, AssistantContent, CompletionModel, Message, Prompt, Usage};
use tracing::{debug, trace};
use uuid::Uuid;

use crate::ThreadStore;
use crate::error::AgentError;

pub(crate) async fn run_turn_inner<M: CompletionModel + 'static>(
    agent: &Agent<M>,
    threads: &ThreadStore,
    thread_id: Uuid,
    messages: Vec<Message>,
) -> Result<(String, Usage), AgentError> {
    let mut history = {
        let threads = threads.read().await;
        let thread = threads
            .get(&thread_id)
            .ok_or(AgentError::ThreadNotFound(thread_id))?;
        thread.messages.clone()
    };

    if messages.is_empty() {
        return Err(AgentError::ModelError(
            completion::PromptError::PromptCancelled {
                chat_history: vec![],
                reason: "empty batch".into(),
            },
        ));
    }

    let history_len = history.len();

    let (prepend, prompt) = messages.split_at(messages.len() - 1);
    for msg in prepend {
        history.push(msg.clone());
    }
    let prompt = prompt[0].clone();

    trace!(thread_id = %thread_id, batch_size = messages.len(), "run_turn");

    let response = {
        let max_attempts = 3;
        let mut attempt = 0u32;
        loop {
            match agent
                .prompt(prompt.clone())
                .with_history(history.clone())
                .extended_details()
                .await
            {
                Ok(resp) => break resp,
                Err(e) => {
                    if attempt < max_attempts - 1 && is_rate_limited(&e) {
                        let delay = std::time::Duration::from_millis(1000 * (attempt as u64 + 1));
                        tokio::time::sleep(delay).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(AgentError::ModelError(e));
                }
            }
        }
    };

    if let Some(new_msgs) = response.messages {
        history.extend(new_msgs);
    }

    let reasoning = extract_reasoning(&history[history_len..]);

    {
        let mut threads = threads.write().await;
        if let Some(thread) = threads.get_mut(&thread_id) {
            thread.messages = history;
            thread.last_activity = Utc::now();
        }
    }

    let output = &response.output;
    debug!(thread_id = %thread_id, response_len = output.len(), "run_turn completed");
    if let Some(r) = &reasoning {
        trace!(thread_id = %thread_id, reasoning = %r, "run_turn: model reasoning");
    }
    Ok((response.output, response.usage))
}

fn extract_reasoning(messages: &[Message]) -> Option<String> {
    for msg in messages.iter().rev() {
        if let Message::Assistant { content, .. } = msg {
            for c in content.iter() {
                if let AssistantContent::Reasoning(reasoning) = c {
                    let text = reasoning.display_text();
                    if !text.is_empty() {
                        return Some(text);
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn format_message(msg: &Message) -> Option<String> {
    match msg {
        Message::System { content } => Some(format!("System: {content}")),
        Message::User { content } => {
            let texts: Vec<String> = content
                .iter()
                .filter_map(|c| match c {
                    UserContent::Text(t) => Some(t.text.clone()),
                    _ => None,
                })
                .collect();
            if texts.is_empty() {
                None
            } else {
                Some(format!("User: {}", texts.join(" ")))
            }
        }
        Message::Assistant { content, .. } => {
            let texts: Vec<String> = content
                .iter()
                .filter_map(|c| match c {
                    AssistantContent::Text(t) => Some(t.text.clone()),
                    AssistantContent::Reasoning(r) => Some(r.display_text()),
                    AssistantContent::ToolCall(tc) => {
                        Some(format!("[tool_call: {}]", tc.function.name))
                    }
                    AssistantContent::Image(_) => None,
                })
                .collect();
            if texts.is_empty() {
                None
            } else {
                Some(format!("Assistant: {}", texts.join(" ")))
            }
        }
    }
}

fn is_rate_limited(err: &completion::PromptError) -> bool {
    match err {
        completion::PromptError::CompletionError(inner) => match inner {
            completion::CompletionError::HttpError(http_err) => match http_err {
                rig_core::http_client::Error::InvalidStatusCode(s) => s.as_u16() == 429,
                rig_core::http_client::Error::InvalidStatusCodeWithMessage(s, _) => {
                    s.as_u16() == 429
                }
                _ => false,
            },
            _ => false,
        },
        _ => false,
    }
}
