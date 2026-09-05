use serde_json::{json, Value};

use crate::handler::Handler;
use crate::types::Tool;

pub struct Compact;

impl Handler for Compact {
    fn spec(&self) -> Tool {
        Tool {
            tool_type: "function".to_string(),
            function: crate::types::Function {
                name: "compact".to_string(),
                description: "Compress episodic memory: the oldest events are moved out of the \
                              active window into NOTES.md, where a background agent distills \
                              them into prose. Call when the session has accumulated many \
                              events."
                    .to_string(),
                parameters: json!({ "type": "object", "properties": {} }),
            },
        }
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