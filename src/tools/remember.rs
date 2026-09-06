use serde_json::{json, Value};

use crate::handler::Handler;
use crate::types::Tool;

pub struct Remember;

impl Handler for Remember {
    fn spec(&self) -> Tool {
        Tool::function(
            "remember",
            "Semantic memory recall. Given a natural-language query, returns the most relevant \
             items from persistent project memory (facts, decisions, preferences, open loops) \
             ranked by topical fit and freshness. Handles stemming and synonyms, and prioritises \
             open threads. Use this when you need to recall anything remembered earlier that a \
             plain key lookup cannot express. Use context for exact save/get by key.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "What to recall, in your own words (1-8 words)" },
                    "limit": { "type": "integer", "description": "Max items to return (default 5)" }
                },
                "required": ["query"]
            }),
        )
    }

    fn run(&self, args: &Value) -> anyhow::Result<String> {
        let query = args["query"].as_str().unwrap_or("").trim();
        if query.is_empty() {
            anyhow::bail!("missing query");
        }
        let limit = args["limit"].as_u64().unwrap_or(5) as usize;
        let store = crate::memory::knowledge::load();
        let hits = crate::memory::recall::remember(query, &store, limit);
        if hits.is_empty() {
            return Ok("no matching memories. Stay on what the user said in this conversation; do not invent prior context.".to_string());
        }
        let mut out = String::from("recalled:\n");
        for (i, hit) in hits.iter().enumerate() {
            let tag = if hit.loop_open { " (open loop)" } else { "" };
            let detail = hit
                .item
                .detail
                .as_ref()
                .map(|d| {
                    let preview: String = d.chars().take(120).collect();
                    let ellipsis = if d.chars().count() > 120 { "…" } else { "" };
                    format!(" — {preview}{ellipsis}")
                })
                .unwrap_or_default();
            let when: String = hit.item.updated.chars().take(10).collect();
            out.push_str(&format!(
                "[{}] {} · {} (salience {:.2}, since {when}){}{}\n",
                i + 1,
                hit.item.summary,
                hit.item.kind,
                hit.item.salience,
                tag,
                detail,
            ));
            let _ = crate::memory::knowledge::touch(&hit.item.id);
        }
        let _ = out.strip_suffix('\n');
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_store() -> std::sync::MutexGuard<'static, ()> {
        let guard = crate::memory::lock();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.keep().join("knowledge.jsonl");
        crate::memory::knowledge::set_path(path.clone());
        let _ = std::fs::remove_file(&path);
        guard
    }

    fn seed() -> String {
        crate::memory::knowledge::save_pair("build the rust engine", "using tokio", "t").unwrap().0
    }

    #[test]
    fn recalls_matching_item() {
        let _g = tmp_store();
        seed();
        let tool = Remember;
        let out = tool.run(&json!({"query": "compare heat flux"})).unwrap();
        assert!(out.contains("no matching memories"), "got: {out}");
        let out = tool.run(&json!({"query": "rust engine"})).unwrap();
        assert!(out.contains("rust engine"), "got: {out}");
        assert!(out.contains("recalled:"), "got: {out}");
    }

    #[test]
    fn touch_bumps_access_count() {
        let _g = tmp_store();
        let id = seed();
        let tool = Remember;
        tool.run(&json!({"query": "rust engine"})).unwrap();
        let item = crate::memory::knowledge::get(&id).unwrap();
        assert_eq!(item.access_count, 1);
    }

    #[test]
    fn missing_query_errors() {
        let _g = tmp_store();
        let tool = Remember;
        assert!(tool.run(&json!({})).is_err());
    }
}