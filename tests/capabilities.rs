use std::collections::HashMap;
use zakhar::config::{Capability, Config, Level};
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
        "heavy".to_string(),
        Level {
            provider: None,
            model: Some("glm-4.7-flash".to_string()),
            fallback: Vec::new(),
        },
    );
    levels.insert(
        "light".to_string(),
        Level {
            provider: None,
            model: Some("glm-4.5-flash".to_string()),
            fallback: Vec::new(),
        },
    );

    let mut capabilities = HashMap::new();
    capabilities.insert(
        "code".to_string(),
        Capability {
            provider: None,
            model: Some("glm-4.7-flash".to_string()),
            hints: vec!["refactor".to_string(), "compile".to_string(), ".rs".to_string()],
            fallback: Vec::new(),
        },
    );
    capabilities.insert(
        "vision".to_string(),
        Capability {
            provider: None,
            model: Some("glm-4.6v-flash".to_string()),
            hints: vec!["image".to_string(), "photo".to_string(), "screenshot".to_string()],
            fallback: Vec::new(),
        },
    );

    Config {
        providers,
        agents: HashMap::new(),
        levels,
        capabilities,
        default_provider: Some("zai".to_string()),
        default_model: Some("glm-4.7-flash".to_string()),
    }
}

#[test]
fn known_capability_resolves_its_model() {
    let r = zakhar::capabilities::resolve(&cfg(), "code", "heavy");
    assert_eq!(r.provider, "zai");
    assert_eq!(r.model, "glm-4.7-flash");
}

#[test]
fn vision_capability_resolves_vision_model() {
    let r = zakhar::capabilities::resolve(&cfg(), "vision", "heavy");
    assert_eq!(r.provider, "zai");
    assert_eq!(r.model, "glm-4.6v-flash");
}

#[test]
fn unknown_capability_falls_back_to_level() {
    let r = zakhar::capabilities::resolve(&cfg(), "nope", "light");
    assert_eq!(r.model, "glm-4.5-flash");
}

#[test]
fn detects_capability_from_text() {
    assert_eq!(zakhar::capabilities::detect(&cfg(), "make the image smaller"), "vision");
    assert_eq!(zakhar::capabilities::detect(&cfg(), "refactor this to async"), "code");
}

#[test]
fn detect_defaults_to_code() {
    assert_eq!(zakhar::capabilities::detect(&cfg(), "list the files"), "code");
}