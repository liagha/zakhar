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

pub fn keys() -> Vec<String> {
    load().entries.into_keys().collect()
}

pub fn value(key: &str) -> Option<String> {
    load().entries.get(key).map(|e| e.value.clone())
}

pub fn remove(key: &str) -> Option<String> {
    let mut store = load();
    let removed = store.entries.remove(key)?;
    save(&store).ok()?;
    Some(removed.value)
}

pub fn recall(query: &str, top: usize) -> Vec<(String, String)> {
    let lower = query.to_lowercase();
    let terms: Vec<&str> = lower
        .split_whitespace()
        .filter(|t| t.chars().count() > 1)
        .collect();
    if terms.is_empty() {
        return Vec::new();
    }
    let store = load();
    let mut scored: Vec<(usize, String, String)> = store
        .entries
        .iter()
        .map(|(key, entry)| {
            let hay = format!("{key} {}", entry.value).to_lowercase();
            let score = terms.iter().filter(|t| hay.contains(**t)).count();
            (score, key.clone(), entry.value.clone())
        })
        .collect();
    scored.sort_by_key(|(s, _, _)| std::cmp::Reverse(*s));
    scored
        .into_iter()
        .filter(|(s, _, _)| *s > 0)
        .take(top)
        .map(|(_, k, v)| (k, v))
        .collect()
}

pub struct Context;
impl Handler for Context {
    fn spec(&self) -> Tool {
        def("context", "Persistent key-value memory for this project, shared across sessions. action='save' stores a value under a key (upsert); action='get' returns a key's value; action='list' shows all keys with previews; action='drop' removes a key; action='recall' ranks entries by keyword relevance to a query (pass query + optional top). Use to remember decisions, plans, and facts between runs. The keys are auto-loaded into context at startup, so get/list. Call save when you produce something worth remembering.", json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["save", "get", "list", "drop", "recall"], "description": "What to do with context" },
                "key": { "type": "string", "description": "Name of the entry (for save/get/drop)" },
                "value": { "type": "string", "description": "Content to store (only for action=save)" },
                "query": { "type": "string", "description": "Search text (only for action=recall)" },
                "top": { "type": "integer", "description": "Max results for recall (default 5)" }
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
                    None => Ok(format!("context lookup: no key '{key}' saved yet. Treat this as 'not in stored memory' — do NOT say you have nothing or ask for the value if the user just provided it; use what the user said in the conversation.")),
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
            "recall" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                if query.is_empty() {
                    anyhow::bail!("missing query");
                }
                let top = args.get("top").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
                let hits = recall(query, top);
                if hits.is_empty() {
                    return Ok(format!("no context entries match '{query}'"));
                }
                let mut out = String::new();
                for (key, value) in hits {
                    out.push_str(&format!("- {key}: {value}\n"));
                }
                Ok(out)
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
        assert!(out.contains("no key 'x'"), "got: {out}");
    }

    #[test]
    fn recall_ranks_by_keyword() {
        let _g = tmp_store();
        let tool = Context;
        tool.run(&json!({"action": "save", "key": "plan", "value": "build the watch tool"})).unwrap();
        tool.run(&json!({"action": "save", "key": "food", "value": "like pizza and pasta"})).unwrap();
        let out = tool.run(&json!({"action": "recall", "query": "tool"})).unwrap();
        assert!(out.contains("plan"), "got: {out}");
        assert!(!out.contains("food"), "got: {out}");
    }
}
