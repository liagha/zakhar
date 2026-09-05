//! Model tiering. Named levels map to a (provider, model) pair in config:
//!
//! ```toml
//! [levels.light]
//! provider = "zai"
//! model = "glm-4.5-flash"
//!
//! [levels.heavy]
//! provider = "zai"
//! model = "glm-4.7-flash"
//! ```
//!
//! Every caller resolves its tier through `resolve` instead of hardcoding a
//! model. Unset fields fall back to `default_provider` / `default_model`, and
//! an unknown level name degrades to those same defaults, so a config without
//! any `[levels]` section behaves exactly like it did before.

use crate::config::Config;

pub struct Resolved {
    pub provider: String,
    pub model: String,
}

pub fn resolve(cfg: &Config, name: &str) -> Resolved {
    match cfg.levels.get(name) {
        Some(level) => Resolved {
            provider: level
                .provider
                .clone()
                .or_else(|| cfg.default_provider.clone())
                .unwrap_or_default(),
            model: level
                .model
                .clone()
                .or_else(|| cfg.default_model.clone())
                .unwrap_or_default(),
        },
        None => Resolved {
            provider: cfg.default_provider.clone().unwrap_or_default(),
            model: cfg.default_model.clone().unwrap_or_default(),
        },
    }
}