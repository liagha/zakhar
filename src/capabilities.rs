//! Capability-aware routing. A *capability* is a task type a model is good at
//! (e.g. "code", "vision", "summary") mapped in config to a (provider, model).
//! Callers either already know their task type (chat/shout → "code", daemon →
//! "summary") or ask `detect` to pick one by matching keyword hints against
//! the request text.
//!
//! ```toml
//! [capabilities.code]
//! provider = "zai"
//! model = "glm-4.7-flash"
//!
//! [capabilities.vision]
//! provider = "zai"
//! model = "glm-4.6v-flash"
//! hints = ["image", "photo", "screenshot", ".png", ".jpg"]
//! ```
//!
//! A capability may omit provider or model — the missing field falls back to
//! `default_provider` / `default_model`. An unknown capability name degrades
//! to the given fallback level (e.g. "heavy"), so a config with no
//! capabilities section behaves exactly as before.

use crate::config::Config;
use crate::levels::Resolved;

pub fn resolve(cfg: &Config, name: &str, fallback_level: &str) -> Resolved {
    match cfg.capabilities.get(name) {
        Some(cap) => Resolved {
            provider: cap
                .provider
                .clone()
                .or_else(|| cfg.default_provider.clone())
                .or_else(|| cfg.providers.keys().next().cloned())
                .unwrap_or_default(),
            model: cap
                .model
                .clone()
                .or_else(|| cfg.default_model.clone())
                .unwrap_or_default(),
        },
        None => crate::levels::resolve(cfg, fallback_level),
    }
}

/// Pick a capability by matching hint keywords against request text.
/// Returns the matching capability name, or "code" when nothing matches.
pub fn detect(cfg: &Config, text: &str) -> String {
    let lower = text.to_lowercase();
    let mut names: Vec<(&String, &Vec<String>)> = cfg
        .capabilities
        .iter()
        .filter(|(_, c)| !c.hints.is_empty())
        .map(|(name, c)| (name, &c.hints))
        .collect();
    names.sort_by(|a, b| a.0.cmp(b.0));
    for (name, hints) in &names {
        let hinted = hints.iter().any(|h| lower.contains(&h.to_lowercase()));
        if hinted {
            return (*name).clone();
        }
    }
    "code".to_string()
}