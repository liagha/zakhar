use serde_json::{json, Value};

use crate::handler::Handler;
use crate::types::Tool;

pub struct SessionTool;

impl Handler for SessionTool {
    fn spec(&self) -> Tool {
        Tool::function("session", "Manage chat sessions. action='list' shows saved sessions; action='current' shows this session's info; action='load' resumes a previous session by id (prefix match ok, missing id resumes the newest); action='diff' compares two sessions by id (prefix match ok) showing asks, tools, and final answers.", json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["list", "current", "load", "diff"], "description": "What to do" },
                "id": { "type": "string", "description": "Session id or prefix (for action=load; missing = newest session)" },
                "a": { "type": "string", "description": "First session id or prefix (for action=diff)" },
                "b": { "type": "string", "description": "Second session id or prefix (for action=diff)" }
            },
            "required": ["action"]
        }))
    }

    fn run(&self, args: &Value) -> anyhow::Result<String> {
        match args["action"].as_str().unwrap_or("") {
            "list" => Ok(crate::session::list_formatted()),
            "current" => Ok("chat holds the current session; you are already inside it. use action=list to see saved sessions, or action=load to switch.".to_string()),
            "load" => {
                let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let target = if id.is_empty() {
                    match crate::session::last() {
                        Some(sid) => sid,
                        None => return Ok("no saved sessions to resume".to_string()),
                    }
                } else {
                    match crate::session::find(id) {
                        Some(sid) => sid,
                        None => return Ok(format!("no session matches '{id}'")),
                    }
                };
                let sessions = crate::session::list();
                match sessions.iter().find(|s| s.id == target) {
                    Some(s) => {
                        crate::invoke::resume_session(s.id.clone());
                        Ok(format!(
                            "resuming session {} (created {}, {} messages)",
                            &s.id[..8], s.created_at, s.message_count
                        ))
                    }
                    None => Ok("no saved sessions to resume".to_string()),
                }
            }
            "diff" => {
                let a = args.get("a").and_then(|v| v.as_str()).unwrap_or("");
                let b = args.get("b").and_then(|v| v.as_str()).unwrap_or("");
                if a.is_empty() || b.is_empty() {
                    anyhow::bail!("missing a or b");
                }
                Ok(crate::session::diff(a, b)?)
            }
            a => anyhow::bail!("unknown action {a}"),
        }
    }
}
