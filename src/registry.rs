use crate::config::Config;
use crate::provider::openai::OpenAI;
use crate::provider::{Registry, types::Config as ProviderConfig};

pub fn build(cfg: &Config) -> Registry {
    let mut registry = Registry::new();
    for (id, pcfg) in &cfg.providers {
        let id = id.clone();
        let mut pcfg = pcfg.clone();
        pcfg.api_key = resolve_key(&pcfg);
        let provider: Box<dyn crate::provider::Provider> = match pcfg.api_type.as_str() {
            "anthropic" => {
                tracing::warn!("anthropic provider not implemented; using openai for {id}");
                Box::new(OpenAI::new(&id, &pcfg))
            }
            _ => Box::new(OpenAI::new(&id, &pcfg)),
        };
        registry.register(provider);
    }
    registry
}

pub fn default_provider(cfg: &Config) -> String {
    cfg.default_provider
        .clone()
        .unwrap_or_else(|| cfg.providers.keys().next().cloned().unwrap_or_default())
}

fn resolve_key(cfg: &ProviderConfig) -> String {
    let key = cfg.api_key.trim();
    if let Some(env) = key.strip_prefix("{env:")
        && let Some(end) = env.strip_suffix('}') {
            return std::env::var(end).unwrap_or_default();
        }
    key.to_string()
}
