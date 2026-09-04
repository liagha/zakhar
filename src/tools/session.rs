use serde_json::{json, Value};

use crate::handler::Handler;
use crate::types::{Function, Tool};

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

pub struct SessionTool;

impl Handler for SessionTool {
    fn spec(&self) -> Tool {
        def("session", "Manage chat sessions. action='list' shows saved sessions; action='current' shows this session's info; action='load' resumes a previous session by id (prefix match ok). Use to switch between conversations.", json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["list", "current", "load"], "description": "What to do" },
                "id": { "type": "string", "description": "Session id or prefix (for action=load)" }
            },
            "required": ["action"]
        }))
    }

    fn run(&self, args: &Value) -> anyhow::Result<String> {
        match args["action"].as_str().unwrap_or("") {
            "list" => Ok(crate::session::list_formatted()),
            "current" => Ok("session management is handled by the chat loop. use action=list to see saved sessions, or action=load to switch.".to_string()),
            "load" => {
                let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
                if id.is_empty() {
                    anyhow::bail!("missing id");
                }
                let sessions = crate::session::list();
                let matched = sessions.iter().find(|s| s.id.starts_with(id));
                match matched {
                    Some(s) => {
                        crate::invoke::resume_session(s.id.clone());
                        Ok(format!("resuming session {} (created {}, {} messages)", &s.id[..8], s.created_at, s.message_count))
                    }
                    None => Ok(format!("no session matches '{id}'")),
                }
            }
            a => anyhow::bail!("unknown action {a}"),
        }
    }
}
