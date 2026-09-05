use std::collections::HashMap;

use async_trait::async_trait;
use futures::StreamExt;
use zakhar::config::{Capability, Config, Level};
use zakhar::levels::Resolved;
use zakhar::provider::{ChatStreamEvent, DeltaStream, Provider, Registry};
use zakhar::provider::types::Config as ProviderConfig;
use zakhar::types::ChatRequest;

fn cfg() -> Config {
    let mut providers = HashMap::new();
    providers.insert(
        "zai".to_string(),
        ProviderConfig {
            api_type: "openai".to_string(),
            base_url: "https://api.z.ai/v1".to_string(),
            api_key: String::new(),
            default_model: "glm-4.7-flash".to_string(),
            models: vec!["glm-4.7-flash".to_string(), "glm-4.5-flash".to_string()],
            user_agent: String::new(),
        },
    );
    providers.insert(
        "opencode".to_string(),
        ProviderConfig {
            api_type: "openai".to_string(),
            base_url: "https://opencode.ai/v1".to_string(),
            api_key: String::new(),
            default_model: "big-pickle".to_string(),
            models: vec!["big-pickle".to_string()],
            user_agent: String::new(),
        },
    );

    let mut levels = HashMap::new();
    levels.insert(
        "heavy".to_string(),
        Level {
            provider: None,
            model: Some("glm-4.7-flash".to_string()),
            fallback: vec!["opencode/big-pickle".to_string()],
        },
    );

    let mut capabilities = HashMap::new();
    capabilities.insert(
        "code".to_string(),
        Capability {
            provider: None,
            model: Some("glm-4.7-flash".to_string()),
            hints: Vec::new(),
            fallback: vec!["opencode/big-pickle".to_string()],
        },
    );

    Config {
        providers,
        agents: HashMap::new(),
        levels,
        capabilities,
        default_provider: Some("zai".to_string()),
        default_model: Some("glm-4.7-flash".to_string()),
        ui: Default::default(),
    }
}

struct Failing {
    prefix: &'static str,
}

#[async_trait]
impl Provider for Failing {
    fn id(&self) -> &str {
        self.prefix
    }

    fn list_models(&self) -> Vec<String> {
        vec!["m".to_string()]
    }

    async fn chat_stream(&self, _request: ChatRequest) -> anyhow::Result<DeltaStream> {
        anyhow::bail!("{} is overloaded", self.prefix)
    }
}

struct Talking {
    name: &'static str,
    text: &'static str,
}

#[async_trait]
impl Provider for Talking {
    fn id(&self) -> &str {
        self.name
    }

    fn list_models(&self) -> Vec<String> {
        vec![self.name.to_string()]
    }

    async fn chat_stream(&self, _request: ChatRequest) -> anyhow::Result<DeltaStream> {
        let events = vec![
            Ok(ChatStreamEvent::Text(self.text.to_string())),
            Ok(ChatStreamEvent::Done),
        ];
        Ok(Box::pin(futures::stream::iter(events)))
    }
}

fn request() -> ChatRequest {
    ChatRequest {
        model: "whatever".to_string(),
        messages: vec![zakhar::types::Message::user("hi")],
        temperature: None,
        max_tokens: None,
        stream: Some(true),
        tools: None,
    }
}

#[test]
fn chain_primary_then_explicit_then_rest() {
    let c = cfg();
    let routes = zakhar::fallback::chain(
        &c,
        Resolved {
            provider: "zai".to_string(),
            model: "glm-4.7-flash".to_string(),
        },
        &["opencode/big-pickle".to_string()],
    );
    assert_eq!(routes.len(), 2);
    assert_eq!(routes[0].provider, "zai");
    assert_eq!(routes[0].model, "glm-4.7-flash");
    assert_eq!(routes[1].provider, "opencode");
    assert_eq!(routes[1].model, "big-pickle");
}

#[test]
fn chain_dedupes_primary_from_auto_added() {
    let c = cfg();
    let routes = zakhar::fallback::chain(
        &c,
        Resolved {
            provider: "zai".to_string(),
            model: "glm-4.5-flash".to_string(),
        },
        &[],
    );
    assert_eq!(routes.len(), 2);
    assert_eq!(routes[0].provider, "zai");
    assert_eq!(routes[1].provider, "opencode");
}

#[test]
fn chain_skips_unknown_candidate() {
    let c = cfg();
    let routes = zakhar::fallback::chain(
        &c,
        Resolved {
            provider: "zai".to_string(),
            model: "glm-4.7-flash".to_string(),
        },
        &["nope/404".to_string()],
    );
    assert_eq!(routes.len(), 2);
    assert!(routes.iter().all(|r| r.provider != "nope"));
}

#[tokio::test]
async fn fallback_auto_moves_to_next_on_failure() {
    let mut registry = Registry::new();
    registry.register(Box::new(Failing { prefix: "zai" }));
    registry.register(Box::new(Talking {
        name: "script",
        text: "hello from fallback",
    }));

    let routes = vec![
        Resolved {
            provider: "zai".to_string(),
            model: "glm-4.7-flash".to_string(),
        },
        Resolved {
            provider: "script".to_string(),
            model: "script".to_string(),
        },
    ];
    let fb = zakhar::fallback::build(&registry, &routes, zakhar::fallback::Decide::Auto).unwrap();
    let stream = fb.chat_stream(request()).await.unwrap();
    let events: Vec<ChatStreamEvent> = stream.map(|e| e.unwrap()).collect().await;
    let texts: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            ChatStreamEvent::Text(t) => Some(t.clone()),
            _ => None,
        })
        .collect();
    assert!(texts.contains(&"hello from fallback".to_string()));
}

#[tokio::test]
async fn fallback_off_propagates_error() {
    let mut registry = Registry::new();
    registry.register(Box::new(Failing { prefix: "zai" }));
    registry.register(Box::new(Talking {
        name: "script",
        text: "should not run",
    }));
    let routes = vec![
        Resolved {
            provider: "zai".to_string(),
            model: "glm-4.7-flash".to_string(),
        },
        Resolved {
            provider: "script".to_string(),
            model: "script".to_string(),
        },
    ];
    let fb = zakhar::fallback::build(&registry, &routes, zakhar::fallback::Decide::Off).unwrap();
    let err = match fb.chat_stream(request()).await {
        Ok(_) => panic!("expected error"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("zai is overloaded"));
}

#[tokio::test]
async fn fallback_returns_primary_first() {
    let mut registry = Registry::new();
    registry.register(Box::new(Talking {
        name: "glm",
        text: "primary ok",
    }));
    registry.register(Box::new(Talking {
        name: "big-pickle",
        text: "fallback ok",
    }));
    let routes = vec![
        Resolved {
            provider: "glm".to_string(),
            model: "glm".to_string(),
        },
        Resolved {
            provider: "big-pickle".to_string(),
            model: "big-pickle".to_string(),
        },
    ];
    let fb = zakhar::fallback::build(&registry, &routes, zakhar::fallback::Decide::Auto).unwrap();
    let stream = fb.chat_stream(request()).await.unwrap();
    let events: Vec<ChatStreamEvent> = stream.map(|e| e.unwrap()).collect().await;
    let texts: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            ChatStreamEvent::Text(t) => Some(t.clone()),
            _ => None,
        })
        .collect();
    assert!(texts.contains(&"primary ok".to_string()));
}

#[test]
fn capability_chain_uses_config_fallback() {
    let c = cfg();
    let routes = zakhar::capabilities::chain(&c, "code", "heavy");
    assert_eq!(routes.len(), 2);
    assert_eq!(routes[0].provider, "zai");
    assert_eq!(routes[1].provider, "opencode");
}