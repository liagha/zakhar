use std::collections::HashMap;
use zakhar::config::{Config, Level};
use zakhar::provider::types::Config as ProviderConfig;

fn cfg() -> Config {
    let mut providers = HashMap::new();
    providers.insert(
        "zai".to_string(),
        ProviderConfig {
            api_type: "openai".to_string(),
            base_url: "https://api.z.ai/v1".to_string(),
            api_key: String::new(),
            default_model: "glm-4.7-flash".to_string(),
            models: vec![
                "glm-4.7-flash".to_string(),
                "glm-4.5-flash".to_string(),
                "glm-4.6v-flash".to_string(),
            ],
            user_agent: String::new(),
        },
    );

    let mut levels = HashMap::new();
    levels.insert(
        "light".to_string(),
        Level {
            provider: None,
            model: Some("glm-4.5-flash".to_string()),
            fallback: Vec::new(),
        },
    );
    levels.insert(
        "heavy".to_string(),
        Level {
            provider: None,
            model: Some("glm-4.7-flash".to_string()),
            fallback: Vec::new(),
        },
    );

    Config {
        providers,
        agents: HashMap::new(),
        levels,
        capabilities: HashMap::new(),
        default_provider: Some("zai".to_string()),
        default_model: Some("glm-4.7-flash".to_string()),
        ui: Default::default(),
        mcp: Default::default(),
    }
}

#[test]
fn light_level_uses_light_model() {
    let r = zakhar::levels::resolve(&cfg(), "light");
    assert_eq!(r.provider, "zai");
    assert_eq!(r.model, "glm-4.5-flash");
}

#[test]
fn heavy_level_uses_heavy_model() {
    let r = zakhar::levels::resolve(&cfg(), "heavy");
    assert_eq!(r.provider, "zai");
    assert_eq!(r.model, "glm-4.7-flash");
}

#[test]
fn unknown_level_falls_back_to_defaults() {
    let r = zakhar::levels::resolve(&cfg(), "turbo");
    assert_eq!(r.provider, "zai");
    assert_eq!(r.model, "glm-4.7-flash");
}

#[test]
fn empty_level_inherits_defaults() {
    let c = cfg();
    let r = zakhar::levels::resolve(&c, "light");
    assert_eq!(r.provider, "zai");
}