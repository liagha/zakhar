use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;

use crate::types::{ChatRequest, Message};

pub type DeltaStream = Pin<Box<dyn Stream<Item = anyhow::Result<ChatStreamEvent>> + Send>>;

#[derive(Debug, Clone)]
pub enum ChatStreamEvent {
    Text(String),
    Reasoning(String),
    ToolCall(ToolCallPart),
    #[allow(dead_code)]
    Message(Message),
    Done,
}

#[derive(Debug, Clone)]
pub struct ToolCallPart {
    pub index: usize,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    fn list_models(&self) -> Vec<String>;
    async fn chat_stream(&self, request: ChatRequest) -> anyhow::Result<DeltaStream>;
}

pub struct Registry {
    providers: Vec<Box<dyn Provider>>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    pub fn register(&mut self, provider: Box<dyn Provider>) {
        self.providers.push(provider);
    }

    pub fn get(&self, id: &str) -> Option<&dyn Provider> {
        self.providers
            .iter()
            .find(|p| p.id() == id)
            .map(|p| p.as_ref())
    }

    pub fn ids(&self) -> Vec<String> {
        self.providers.iter().map(|p| p.id().to_string()).collect()
    }
}

pub mod types;
pub mod openai;
