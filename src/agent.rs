use crate::config::Agent as AgentCfg;
use crate::provider::{DeltaStream, Provider};
use crate::types::{ChatRequest, Message, Tool};

pub struct Runner<'a> {
    provider: &'a dyn Provider,
    model: String,
    agent: Option<&'a AgentCfg>,
    messages: Vec<Message>,
    tools: Vec<Tool>,
}

impl<'a> Runner<'a> {
    pub fn new(provider: &'a dyn Provider, model: String, agent: Option<&'a AgentCfg>) -> Self {
        let mut messages = Vec::new();
        if let Some(agent) = agent
            && !agent.prompt.is_empty() {
                messages.push(Message::system(agent.prompt.clone()));
            }
        Self {
            provider,
            model,
            agent,
            messages,
            tools: Vec::new(),
        }
    }

    pub fn set_tools(&mut self, tools: Vec<Tool>) {
        self.tools = tools;
    }

    pub fn push(&mut self, message: Message) {
        self.messages.push(message);
    }

    pub fn request(&self) -> ChatRequest {
        ChatRequest {
            model: self.model.clone(),
            messages: self.messages.clone(),
            temperature: self.agent.and_then(|a| a.temperature),
            max_tokens: None,
            stream: Some(true),
            tools: if self.tools.is_empty() { None } else { Some(self.tools.clone()) },
        }
    }

    pub async fn stream(&self) -> anyhow::Result<DeltaStream> {
        self.provider.chat_stream(self.request()).await
    }
}
