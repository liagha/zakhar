use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use chrono::Utc;
use serde_json::{json, Value};

use crate::handler::Handler;
use crate::types::{Function, Tool};

const HALF_LIFE_HOURS: f64 = 168.0;
const STALE_DAYS: f64 = 30.0;

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
    #[serde(default)]
    accessed_at: String,
    #[serde(default)]
    access_count: u64,
    #[serde(default)]
    source: String,
}

impl Entry {
    fn new(value: String) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            value,
            updated: now.clone(),
            accessed_at: now,
            access_count: 0,
            source: String::new(),
        }
    }

    fn bump_access(&mut self) {
        self.accessed_at = Utc::now().to_rfc3339();
        self.access_count += 1;
    }
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
    let tmp = dir.join(format!(".context.{}.tmp", std::process::id()));
    std::fs::write(&tmp, serde_json::to_string_pretty(store)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// Proof of origin for one fact: where it came from and when it last lived.
pub struct Meta {
    pub value: String,
    pub updated: String,
    pub accessed_at: String,
    pub access_count: u64,
    pub source: String,
}

pub fn meta(key: &str) -> Option<Meta> {
    let store = load();
    let e = store.entries.get(key)?;
    Some(Meta {
        value: e.value.clone(),
        updated: e.updated.clone(),
        accessed_at: e.accessed_at.clone(),
        access_count: e.access_count,
        source: e.source.clone(),
    })
}

/// Keys whose last access is older than `within_days` — candidates for pruning.
pub fn stale(within_days: u64) -> Vec<String> {
    let mut out: Vec<String> = load()
        .entries
        .into_iter()
        .filter(|(_, e)| age_days(&e.accessed_at).is_some_and(|d| d > within_days as f64))
        .map(|(k, _)| k)
        .collect();
    out.sort();
    out
}

pub fn index() -> String {
    let store = load();
    if store.entries.is_empty() {
        return "no saved context".to_string();
    }
    let mut out = String::from("saved context keys:\n");
    for (key, entry) in &store.entries {
        let preview: String = entry.value.chars().take(60).collect();
        let ellipsis = if entry.value.chars().count() > 60 { "…" } else { "" };
        let access_info = if entry.access_count > 0 {
            format!(" (accessed {}×, last {})", entry.access_count, entry.accessed_at)
        } else {
            String::new()
        };
        let source_info = if entry.source.is_empty() {
            String::new()
        } else {
            format!(" [source: {}]", entry.source)
        };
        let stale_mark = if age_days(&entry.accessed_at).is_some_and(|d| d > STALE_DAYS) {
            " (stale)"
        } else {
            ""
        };
        out.push_str(&format!(
            "- {key}: {preview}{ellipsis} (updated {}){stale_mark}{}{}\n",
            entry.updated, access_info, source_info
        ));
    }
    out
}

pub fn keys() -> Vec<String> {
    load().entries.into_keys().collect()
}

pub fn context_keys() -> String {
    serde_json::to_string(&keys()).unwrap_or_else(|_| "[]".to_string())
}

pub fn remove(key: &str) -> Option<String> {
    let mut store = load();
    let removed = store.entries.remove(key)?;
    save(&store).ok()?;
    Some(removed.value)
}

fn age_hours(ts: &str) -> Option<f64> {
    let parsed = chrono::DateTime::parse_from_rfc3339(ts).ok()?;
    let age = Utc::now().signed_duration_since(parsed);
    Some(age.num_seconds() as f64 / 3600.0)
}

fn age_days(ts: &str) -> Option<f64> {
    age_hours(ts).map(|h| h / 24.0)
}

/// Rank entries by keyword relevance, boosted by recency (7-day half-life)
/// and importance (access frequency). Returns `(key, value, source)`.
pub fn recall(query: &str, top: usize) -> Vec<(String, String, String)> {
    let lower = query.to_lowercase();
    let terms: Vec<&str> = lower
        .split_whitespace()
        .filter(|t| t.chars().count() > 1)
        .collect();
    if terms.is_empty() {
        return Vec::new();
    }
    let store = load();
    let mut scored: Vec<(f64, String, String, String)> = store
        .entries
        .iter()
        .map(|(key, entry)| {
            let hay = format!("{key} {}", entry.value).to_lowercase();
            let keyword_score = terms.iter().filter(|t| hay.contains(**t)).count() as f64;
            let recency = age_hours(&entry.accessed_at)
                .map_or(1.0, |hours| 0.5f64.powf(hours / HALF_LIFE_HOURS));
            let freq = ((entry.access_count + 1) as f64).ln() / 5.0;
            let combined = keyword_score * (1.0 + 0.3 * recency + 0.2 * freq);
            (combined, key.clone(), entry.value.clone(), entry.source.clone())
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored
        .into_iter()
        .filter(|(s, _, _, _)| *s > 0.0)
        .take(top)
        .map(|(_, k, v, s)| (k, v, s))
        .collect()
}

pub struct Context;
impl Handler for Context {
    fn spec(&self) -> Tool {
        def("context", "Persistent key-value memory for this project, shared across sessions. action='save' stores a value under a key (upsert); action='get' returns a key's value; action='list' shows all keys with previews (plus source and stale markers); action='drop' removes a key; action='recall' ranks entries by keyword relevance, boosted by recency and access importance (pass query + optional top). Use to remember decisions, plans, and facts between runs. The keys are auto-loaded into context at startup, so get/list. Call save when you produce something worth remembering.", json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["save", "get", "list", "drop", "recall"], "description": "What to do with context" },
                "key": { "type": "string", "description": "Name of the entry (for save/get/drop)" },
                "value": { "type": "string", "description": "Content to store (only for action=save)" },
                "source": { "type": "string", "description": "Provenance of the fact (for action=save; optional, defaults to 'conversation' and sticks on later updates)" },
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
                let given = args.get("source").and_then(|v| v.as_str());
                if key.is_empty() {
                    anyhow::bail!("missing key");
                }
                let mut store = load();
                let is_update = store.entries.contains_key(key);
                let mut entry = Entry::new(value.to_string());
                entry.source = given.unwrap_or("conversation").to_string();
                if is_update {
                    if let Some(existing) = store.entries.get(key) {
                        entry.access_count = existing.access_count;
                        entry.accessed_at = existing.accessed_at.clone();
                        if given.is_none() && !existing.source.is_empty() {
                            entry.source = existing.source.clone();
                        }
                    }
                    entry.bump_access();
                }
                store.entries.insert(key.to_string(), entry);
                save(&store)?;
                Ok(format!("saved context key '{key}' ({} bytes)", value.len()))
            }
            "get" => {
                let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("");
                if key.is_empty() {
                    anyhow::bail!("missing key");
                }
                let mut store = load();
                match store.entries.get_mut(key) {
                    Some(entry) => {
                        entry.bump_access();
                        let val = entry.value.clone();
                        save(&store).ok();
                        Ok(val)
                    }
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
                for (key, value, source) in hits {
                    if source.is_empty() {
                        out.push_str(&format!("- {key}: {value}\n"));
                    } else {
                        out.push_str(&format!("- {key}: {value} (source: {source})\n"));
                    }
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

    #[test]
    fn recall_ranks_frequent_access_higher() {
        let _g = tmp_store();
        let tool = Context;
        tool.run(&json!({"action": "save", "key": "hot", "value": "build the watch tool"})).unwrap();
        tool.run(&json!({"action": "save", "key": "cold", "value": "build the watch tool"})).unwrap();
        for _ in 0..9 {
            tool.run(&json!({"action": "get", "key": "hot"})).unwrap();
        }
        let hits = recall("build tool", 2);
        assert_eq!(hits[0].0, "hot", "importance must outrank equal keywords");
        assert_eq!(hits[1].0, "cold");
    }

    #[test]
    fn stale_surfaces_old_keys() {
        let _g = tmp_store();
        let tool = Context;
        tool.run(&json!({"action": "save", "key": "old", "value": "x"})).unwrap();
        tool.run(&json!({"action": "save", "key": "fresh", "value": "y"})).unwrap();
        let mut store = load();
        let past = (Utc::now() - chrono::Duration::days(90)).to_rfc3339();
        store.entries.get_mut("old").unwrap().accessed_at = past;
        save(&store).unwrap();
        let stale = stale(30);
        assert_eq!(stale, vec!["old".to_string()]);
        assert!(index().contains("(stale)"), "got: {}", index());
        assert!(!index().contains("fresh (stale)"));
    }

    #[test]
    fn save_defaults_and_preserves_source() {
        let _g = tmp_store();
        let tool = Context;
        tool.run(&json!({"action": "save", "key": "k", "value": "v1", "source": "file:README.md"})).unwrap();
        assert_eq!(meta("k").unwrap().source, "file:README.md");
        tool.run(&json!({"action": "save", "key": "k", "value": "v2"})).unwrap();
        let m = meta("k").unwrap();
        assert_eq!(m.value, "v2");
        assert_eq!(m.source, "file:README.md", "first provenance must stick");
        tool.run(&json!({"action": "save", "key": "n", "value": "v"})).unwrap();
        assert_eq!(meta("n").unwrap().source, "conversation");
    }

    #[test]
    fn recall_notes_source() {
        let _g = tmp_store();
        let tool = Context;
        tool.run(&json!({"action": "save", "key": "plan", "value": "build tool", "source": "file:todo.md"})).unwrap();
        let out = tool.run(&json!({"action": "recall", "query": "build"})).unwrap();
        assert!(out.contains("file:todo.md"), "got: {out}");
    }
}