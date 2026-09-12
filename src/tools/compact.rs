use serde_json::{json, Value};

use crate::handler::Handler;
use crate::types::Tool;

pub struct Compact;

impl Handler for Compact {
    fn spec(&self) -> Tool {
        Tool::function(
            "compact",
            "Compress episodic memory: the oldest events are moved out of the active window into \
             NOTES.md, where a background agent distills them into prose. Call when the session \
             has accumulated many events.",
            json!({ "type": "object", "properties": {} }),
        )
    }

    fn run(&self, _args: &Value) -> anyhow::Result<String> {
        let archived = crate::memory::episodic::compact()?;
        if archived.is_empty() {
            return Ok("nothing to compact — memory is below the roll-up threshold".to_string());
        }
        let preview: Vec<String> = archived
            .iter()
            .rev()
            .take(3)
            .map(|e| e.text.clone())
            .collect();
        Ok(format!(
            "archived {} events; oldest kept in NOTES.md, background summary dispatched\nrecently archived:\n  - {}",
            archived.len(),
            preview.join("\n  - "),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> (tempfile::TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = crate::memory::lock();
        let dir = tempfile::tempdir().unwrap();
        crate::paths::set_home(dir.path().join(".zakhar"));
        crate::memory::set_path(dir.path().join("episodic.jsonl"));
        (dir, guard)
    }

    fn append_many(count: usize) {
        for i in 0..count {
            crate::memory::episodic::append("note", &format!("event {i}")).unwrap();
        }
    }

    #[test]
    fn archives_overflow_then_noop() {
        let (_dir, _g) = tmp();
        append_many(110);
        let out = Compact.run(&json!({})).unwrap();
        assert!(out.starts_with("archived 10 events"), "got: {out}");
        assert!(out.contains("event 9"), "got: {out}");
        let again = Compact.run(&json!({})).unwrap();
        assert!(again.starts_with("nothing to compact"), "got: {again}");
    }

    #[test]
    fn below_threshold_is_noop() {
        let (_dir, _g) = tmp();
        append_many(5);
        let out = Compact.run(&json!({})).unwrap();
        assert!(out.starts_with("nothing to compact"), "got: {out}");
    }
}