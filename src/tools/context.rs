use serde_json::{json, Value};

use crate::handler::Handler;
use crate::memory::knowledge;
use crate::types::Tool;

pub(crate) fn def(name: &str, description: &str, parameters: Value) -> Tool {
    Tool {
        tool_type: "function".to_string(),
        function: crate::types::Function {
            name: name.to_string(),
            description: description.to_string(),
            parameters,
        },
    }
}

pub struct Context;

impl Handler for Context {
    fn spec(&self) -> Tool {
        def("context", "Persistent memory for this project, shared across sessions and backed \
            by consolidated knowledge. action='save' stores a fact under a key (upsert — re-saving \
            the same key updates its value); action='get' returns a key's value by its key or item \
            id; action='list' shows everything remembered with salience and stale markers; \
            action='drop' removes an entry. Strong recall lives in the remember tool; this is for \
            simple structured persistence, not searching. Use save when you produce something worth \
            remembering. Keys and their gist are auto-loaded into context at startup.", json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["save", "get", "list", "drop"], "description": "What to do with memory" },
                "key": { "type": "string", "description": "Name of the entry (for save/get/drop)" },
                "value": { "type": "string", "description": "Content to store (only for action=save)" },
                "source": { "type": "string", "description": "Provenance of the fact (for action=save; optional, defaults to 'conversation' and sticks on later updates)" }
            },
            "required": ["action"]
        }))
    }
    fn run(&self, args: &Value) -> anyhow::Result<String> {
        match args["action"].as_str().unwrap_or("") {
            "save" => {
                let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("");
                let value = args.get("value").and_then(|v| v.as_str()).unwrap_or("");
                let source = args.get("source").and_then(|v| v.as_str()).unwrap_or("");
                if key.is_empty() {
                    anyhow::bail!("missing key");
                }
                knowledge::save_pair(key, value, source)?;
                Ok(format!("saved context key '{key}' ({} bytes)", value.len()))
            }
            "get" => {
                let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("");
                if key.is_empty() {
                    anyhow::bail!("missing key");
                }
                let item = knowledge::find(key).or_else(|| {
                    knowledge::load()
                        .into_iter()
                        .find(|i| i.id.starts_with(key))
                });
                match item {
                    Some(item) => {
                        let _ = knowledge::touch(&item.id);
                        Ok(item.detail.unwrap_or(item.summary))
                    }
                    None => Ok(format!(
                        "context lookup: no key '{key}' saved yet. Treat this as 'not in stored memory' — do NOT say you have nothing or ask for the value if the user just provided it; use what the user said in the conversation."
                    )),
                }
            }
            "list" => {
                let store = knowledge::load();
                if store.is_empty() {
                    return Ok("no saved context".to_string());
                }
                let mut out = String::from("saved context keys:\n");
                for item in store {
                    let id8: String = item.id.chars().take(8).collect();
                    let preview: String = item.detail.as_ref().map(|d| d.chars().take(60).collect::<String>()).unwrap_or_default();
                    let ellipsis = if item.detail.as_ref().is_some_and(|d| d.chars().count() > 60) { "…" } else { "" };
                    let stale = knowledge::stale(30).contains(&item.summary);
                    let stale_mark = if stale { " (stale)" } else { "" };
                    let open = if item.open { " (open)" } else { "" };
                    let src = if item.origin.is_empty() { "?" } else { &item.origin };
                    out.push_str(&format!(
                        "- {} ({id8}, {salience:.2}, src: {src}){stale_mark}{open}\n",
                        item.summary,
                        salience = item.salience
                    ));
                    if !preview.is_empty() {
                        out.push_str(&format!("    {preview}{ellipsis}\n"));
                    }
                }
                Ok(out)
            }
            "drop" => {
                let key = args.get("key").and_then(|v| v.as_str()).unwrap_or("");
                if key.is_empty() {
                    anyhow::bail!("missing key");
                }
                match knowledge::remove(key)? {
                    Some(item) => Ok(format!("dropped context key '{}'", item.summary)),
                    None => Ok(format!("no context key '{key}'")),
                }
            }
            a => anyhow::bail!("unknown action {a}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmp_store() -> std::sync::MutexGuard<'static, ()> {
        let guard = crate::memory::lock();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.keep().join("knowledge.jsonl");
        crate::memory::knowledge::set_path(path.clone());
        let _ = std::fs::remove_file(&path);
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
    fn get_by_id_works() {
        let _g = tmp_store();
        let tool = Context;
        tool.run(&json!({"action": "save", "key": "k", "value": "v1"})).unwrap();
        let listed = tool.run(&json!({"action": "list"})).unwrap();
        let id8 = listed
            .lines()
            .find(|l| l.starts_with("- "))
            .and_then(|l| l.split('(').nth(1))
            .and_then(|rest| rest.split(',').next())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        assert!(!id8.is_empty(), "list produced: {listed}");
        assert_eq!(tool.run(&json!({"action": "get", "key": id8})).unwrap(), "v1");
    }

    #[test]
    fn save_defaults_and_preserves_source() {
        let _g = tmp_store();
        let tool = Context;
        tool.run(&json!({"action": "save", "key": "k", "value": "v1", "source": "file:README.md"})).unwrap();
        tool.run(&json!({"action": "save", "key": "k", "value": "v2"})).unwrap();
        let store = crate::memory::knowledge::load();
        let item = store.iter().find(|i| i.summary == "k").unwrap();
        assert_eq!(item.detail.as_deref(), Some("v2"));
        assert_eq!(item.origin, "file:README.md", "first provenance must stick");
    }
}