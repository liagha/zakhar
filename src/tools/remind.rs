use serde_json::{json, Value};

use crate::handler::Handler;
use crate::reminder;
use crate::types::Tool;

pub struct Remind;

impl Handler for Remind {
    fn spec(&self) -> Tool {
        Tool {
            tool_type: "function".to_string(),
            function: crate::types::Function {
                name: "remind".to_string(),
                description: "Schedule a reminder. First call the time tool to learn the current time, then \
                              compute the RFC3339 UTC due timestamp from natural language \
                              (e.g. '11AM', 'in 30 min', 'every day'). Stores it so a background \
                              daemon fires it automatically."
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "message": { "type": "string", "description": "What to remind about" },
                        "due_at": { "type": "string", "description": "RFC3339 UTC due timestamp computed from the user's phrase and the current time" },
                        "recurring": { "type": "string", "description": "Optional interval (e.g. 'daily', 'weekly', 'every hour') or null for one-shot" }
                    },
                    "required": ["message", "due_at"]
                }),
            },
        }
    }

    fn run(&self, args: &Value) -> anyhow::Result<String> {
        let message = args["message"].as_str().unwrap_or("").to_string();
        let due_at = args["due_at"].as_str().unwrap_or("").to_string();
        let recurring = args.get("recurring").and_then(|v| v.as_str()).map(String::from);

        if message.is_empty() || due_at.is_empty() {
            return Err(anyhow::anyhow!("remind needs 'message' and 'due_at'"));
        }

        let r = reminder::add(message, due_at, recurring)?;
        crate::cli::daemon::ensure_daemon();
        Ok(format!(
            "reminder set: {}\n  '{}' due at {}{}",
            r.id,
            r.message,
            r.due_at,
            r.recurring
                .map(|x| format!(" (recurring: {x})"))
                .unwrap_or_default()
        ))
    }
}
