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
    pub capabilities: HashMap<String, Capability>,
    #[serde(default)]
    pub default_provider: Option<String>,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub ui: UiColors,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct UiColors {
    pub status: Option<String>,
    pub ok: Option<String>,
    pub err: Option<String>,
    pub note: Option<String>,
    pub summary: Option<String>,
    pub thought: Option<String>,
    pub tool_call: Option<String>,
    pub tool_result: Option<String>,
    pub preview: Option<String>,
    pub code: Option<String>,
    pub link: Option<String>,
    pub url: Option<String>,
    pub h1: Option<String>,
    pub h2: Option<String>,
    pub h3: Option<String>,
    pub list: Option<String>,
    pub quote: Option<String>,
    pub rule: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Level {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub fallback: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Capability {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub hints: Vec<String>,
    #[serde(default)]
    pub fallback: Vec<String>,
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

    pub fn palette(&self) -> crate::ui::palette::Palette {
        let mut map = HashMap::new();
        let ui = &self.ui;
        for (k, v) in [
            ("status", &ui.status),
            ("ok", &ui.ok),
            ("err", &ui.err),
            ("note", &ui.note),
            ("summary", &ui.summary),
            ("thought", &ui.thought),
            ("tool_call", &ui.tool_call),
            ("tool_result", &ui.tool_result),
            ("preview", &ui.preview),
            ("code", &ui.code),
            ("link", &ui.link),
            ("url", &ui.url),
            ("h1", &ui.h1),
            ("h2", &ui.h2),
            ("h3", &ui.h3),
            ("list", &ui.list),
            ("quote", &ui.quote),
            ("rule", &ui.rule),
        ] {
            if let Some(val) = v {
                map.insert(k.to_string(), val.clone());
            }
        }
        crate::ui::palette::Palette::from(&map)
    }
}

fn config_path() -> anyhow::Result<PathBuf> {
    Ok(crate::paths::config_path())
}
