use serde_json::{json, Value};

use crate::handler::Handler;
use crate::types::Tool;

pub struct Time;

impl Handler for Time {
    fn spec(&self) -> Tool {
        Tool {
            tool_type: "function".to_string(),
            function: crate::types::Function {
                name: "time".to_string(),
                description: "Get the current UTC time. Use it to compute due timestamps for \
                              reminders or to interpret relative times like '11AM' or 'in 30 min'."
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                }),
            },
        }
    }

    fn run(&self, _args: &Value) -> anyhow::Result<String> {
        Ok(chrono::Utc::now().to_rfc3339())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_returns_rfc3339() {
        let out = Time.run(&json!({})).unwrap();
        let parsed = chrono::DateTime::parse_from_rfc3339(&out);
        assert!(parsed.is_ok(), "expected RFC3339, got {out}");
    }
}