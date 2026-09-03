use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use chrono::Utc;
use serde_json::{json, Value};

use crate::handler::Handler;
use crate::types::{Function, Tool};

static OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

fn path() -> PathBuf {
    if let Some(over) = OVERRIDE.get()
        && let Some(p) = over.lock().unwrap().clone()
    {
        return p;
    }
    PathBuf::from(".zakhar/context.json")
}

#[cfg(test)]
fn set_path(p: PathBuf) {
    let cell = OVERRIDE.get_or_init(|| Mutex::new(None));
    *cell.lock().unwrap() = Some(p);
}

fn def(name: &str, description: &str, parameters: Value) -> Tool {
    Tool {
        tool_type: "function".to_string(),
        function: Function {
            name: name.to_string(),
            description: description.to_string(),
            parameters,
        },
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Entry {
    value: String,
    updated: String,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Store {
    entries: BTreeMap<String, Entry>,
}

fn load() -> Store {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(store: &Store) -> anyhow::Result<()> {
    let path = path();
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(dir)?;
    std::fs::write(path, serde_json::to_string_pretty(store)?)?;
    Ok(())
}

pub fn index() -> String {
    let store = load();
    if store.entries.is_empty() {
        return "no saved context".to_string();
    }
    let mut out = String::from("saved context keys:\n");
    for (key, entry) in &store.entries {
        let preview: String = entry.value.chars().take(60).collect();
        out.push_str(&format!("- {key}: {preview}{} (updated {})\n", if entry.value.chars().count() > 60 { "…" } else { "" }, entry.updated));
    }
    out
}

pub struct Context;
impl Handler for Context {
    fn spec(&self) -> Tool {
        def("context", "Persistent key-value memory for this project, shared across sessions. action='save' stores a value under a key (upsert); action='get' returns a key's value; action='list' shows all keys with previews; action='drop' removes a key. Use to remember decisions, plans, and facts between runs. The keys are auto-loaded into context at startup, so get/list. Call save when you produce something worth remembering.", json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["save", "get", "list", "drop"], "description": "What to do with context" },
                "key": { "type": "string", "description": "Name of the entry (for save/get/drop)" },
                "value": { "type": "string", "description": "Content to store (only for action=save)" }
            },
            "required": ["action"]
        }))
    }
    fn run(&self, args: &Value) -> anyhow::Result<String> {
        match args["action"].as_str().unwrap_or("") {
            "save" => {
                let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("");
                let value = args.get("value").and_then(|v| v.as_str()).unwrap_or("");
                if key.is_empty() {
                    anyhow::bail!("missing key");
                }
                let mut store = load();
                store.entries.insert(
                    key.to_string(),
                    Entry { value: value.to_string(), updated: Utc::now().to_rfc3339() },
                );
                save(&store)?;
                Ok(format!("saved context key '{key}' ({} bytes)", value.len()))
            }
            "get" => {
                let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("");
                if key.is_empty() {
                    anyhow::bail!("missing key");
                }
                let store = load();
                match store.entries.get(key) {
                    Some(entry) => Ok(entry.value.clone()),
                    None => Ok(format!("no context key '{key}'")),
                }
            }
            "list" => {
                let store = load();
                if store.entries.is_empty() {
                    return Ok("no saved context".to_string());
                }
                Ok(index())
            }
            "drop" => {
                let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("");
                if key.is_empty() {
                    anyhow::bail!("missing key");
                }
                let mut store = load();
                if store.entries.remove(key).is_some() {
                    save(&store)?;
                    Ok(format!("dropped context key '{key}'"))
                } else {
                    Ok(format!("no context key '{key}'"))
                }
            }
            a => anyhow::bail!("unknown action {a}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_store() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let guard = LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let tmp = std::env::temp_dir().join(format!("zakhar_ctx_{}", std::process::id()));
        set_path(tmp);
        let _ = std::fs::remove_file(path());
        guard
    }

    #[test]
    fn save_get_roundtrip() {
        let _g = tmp_store();
        let tool = Context;
        let args = json!({"action": "save", "key": "plan", "value": "build watch tool"});
        assert!(tool.run(&args).is_ok());
        let out = tool.run(&json!({"action": "get", "key": "plan"})).unwrap();
        assert_eq!(out, "build watch tool");
        let listed = tool.run(&json!({"action": "list"})).unwrap();
        assert!(listed.contains("plan"));
    }

    #[test]
    fn drop_removes_key() {
        let _g = tmp_store();
        let tool = Context;
        tool.run(&json!({"action": "save", "key": "x", "value": "1"})).unwrap();
        tool.run(&json!({"action": "drop", "key": "x"})).unwrap();
        let out = tool.run(&json!({"action": "get", "key": "x"})).unwrap();
        assert_eq!(out, "no context key 'x'");
    }
}
