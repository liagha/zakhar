//! Scripted provider for offline verification of the mobile turn engine. It
//! never touches the network: on the first call it emits one `time` tool call,
//! and once the caller feeds the tool result back into the history it replies
//! with a fixed final answer. Enough to exercise the full text/tool/approval
//! cycle without any live model.

use async_trait::async_trait;

use crate::types::{ChatRequest, Role};

use super::{ChatStreamEvent, DeltaStream, Provider, ToolCallPart};

pub struct Script {
    pub name: String,
    pub answer: String,
}

#[async_trait]
impl Provider for Script {
    fn id(&self) -> &str {
        "script"
    }

    fn list_models(&self) -> Vec<String> {
        vec![self.name.clone()]
    }

    async fn chat_stream(&self, request: ChatRequest) -> anyhow::Result<DeltaStream> {
        let tool_seen = request.messages.iter().any(|m| m.role == Role::Tool);
        let events = if tool_seen {
            vec![
                Ok(ChatStreamEvent::Text(self.answer.clone())),
                Ok(ChatStreamEvent::Done),
            ]
        } else {
            vec![
                Ok(ChatStreamEvent::ToolCall(ToolCallPart {
                    index: 0,
                    id: Some("mock_call_1".to_string()),
                    name: Some("time".to_string()),
                    arguments: Some("{}".to_string()),
                })),
                Ok(ChatStreamEvent::Done),
            ]
        };
        Ok(Box::pin(futures::stream::iter(events)))
    }
}