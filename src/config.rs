use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::provider::types::Config as ProviderConfig;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
    #[serde(default)]
    pub agents: HashMap<String, Agent>,
    #[serde(default)]
    pub levels: HashMap<String, Level>,
    #[serde(default)]
    pub default_provider: Option<String>,
    #[serde(default)]
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Level {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub model: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub tools: Vec<String>,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let path = config_path()?;
        if path.exists() {
            let text = std::fs::read_to_string(&path)?;
            Ok(toml::from_str(&text)?)
        } else {
            Ok(Self::default())
        }
    }
}

fn config_path() -> anyhow::Result<PathBuf> {
    Ok(crate::paths::config_path())
}
