use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures::Stream;

use crate::types::{ChatRequest, Message};

pub type DeltaStream = Pin<Box<dyn Stream<Item = anyhow::Result<ChatStreamEvent>> + Send>>;

pub mod mock;

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
    providers: Vec<Arc<dyn Provider>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    #[allow(clippy::new_without_default)]
    pub fn register(&mut self, provider: Box<dyn Provider>) {
        self.providers.push(Arc::from(provider));
    }

    pub fn get(&self, id: &str) -> Option<&dyn Provider> {
        self.providers
            .iter()
            .find(|p| p.id() == id)
            .map(|p| p.as_ref())
    }

    pub fn arc(&self, id: &str) -> Option<Arc<dyn Provider>> {
        self.providers
            .iter()
            .find(|p| p.id() == id)
            .cloned()
    }

    pub fn ids(&self) -> Vec<String> {
        self.providers.iter().map(|p| p.id().to_string()).collect()
    }
}

pub mod types;
pub mod openai;
