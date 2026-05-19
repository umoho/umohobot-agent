use rig::completion::ToolDefinition;
use rig::tool::Tool;

use crate::tools::protocol::{ChatBatchRequest, ChatBatchResult, chat_batch_request_schema};

#[derive(Clone, Debug, Default)]
pub struct ChatBatchTool;

#[derive(Debug, thiserror::Error)]
pub enum ChatBatchError {
    #[error("chat batch request must contain at least one operation")]
    EmptyBatch,
}

impl ChatBatchTool {
    pub fn new() -> Self {
        Self
    }
}

impl Tool for ChatBatchTool {
    const NAME: &'static str = "chat.batch";

    type Error = ChatBatchError;
    type Args = ChatBatchRequest;
    type Output = ChatBatchResult;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Execute a sequential batch of outbox actions for the current chat thread. The batch is host-side, model-friendly, and best-effort typing stops are allowed.".to_string(),
            parameters: chat_batch_request_schema(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        if args.operations.is_empty() {
            return Err(ChatBatchError::EmptyBatch);
        }

        Ok(ChatBatchResult::planned(args))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::protocol::{ChatOp, MessageDraft, MessageLocator, ToolContext};

    #[tokio::test]
    async fn tool_definition_mentions_chat_batch() {
        let tool = ChatBatchTool::new();
        let definition = tool.definition(String::new()).await;

        assert_eq!(definition.name, "chat.batch");
        let params = definition.parameters.to_string();
        assert!(params.contains("\"context\""));
        assert!(params.contains("\"operations\""));
    }

    #[tokio::test]
    async fn call_returns_planned_batch_shape() {
        let tool = ChatBatchTool::new();
        let result = tool
            .call(ChatBatchRequest {
                context: ToolContext::new("telegram", "telegram:1", "1"),
                operations: vec![
                    ChatOp::Send {
                        draft: MessageDraft::new("hello"),
                    },
                    ChatOp::StopTyping {
                        target: MessageLocator::new("1"),
                    },
                ],
                best_effort: true,
            })
            .await
            .unwrap();

        assert_eq!(result.requested_count, 2);
        assert_eq!(result.planned_count, 2);
        assert!(!result.partial_failure);
        assert_eq!(result.outcomes.len(), 2);
        assert_eq!(
            result.outcomes[0].status,
            crate::tools::protocol::ChatOpStatus::Applied
        );
        assert!(result.outcomes[0].message_ref.is_some());
        assert_eq!(
            result.outcomes[1].status,
            crate::tools::protocol::ChatOpStatus::BestEffort
        );
        assert!(result.final_message.is_some());
        assert_eq!(result.final_visible_text.as_deref(), Some("hello"));
    }
}
