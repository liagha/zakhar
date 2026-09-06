//! MCP server: `zakhar mcp` serves a fixed allowlist of read-only and
//! knowledge tools over stdio, so any MCP client can drive them. Interactive
//! and stdout-writing tools are never exposed.

use std::io::BufRead;
use std::sync::Mutex;

use serde_json::{json, Value};

use crate::invoke::Invoke;
use crate::mcp::PROTOCOL_VERSION;
use crate::types::Tool;

const ALLOWED: &[&str] = &[
    "read",
    "glob",
    "grep",
    "search",
    "fetch",
    "calc",
    "clipboard",
    "env",
    "json",
    "ps",
    "regex",
    "remember",
    "context",
    "session",
    "time",
];

fn tool_list(defs: &[Tool]) -> Vec<Value> {
    defs.iter()
        .filter(|t| ALLOWED.contains(&t.function.name.as_str()))
        .map(|t| {
            json!({
                "name": t.function.name,
                "description": t.function.description,
                "inputSchema": t.function.parameters,
            })
        })
        .collect()
}

pub fn run() -> anyhow::Result<()> {
    let invoke = Mutex::new(Invoke::new());
    let mut stdout = std::io::stdout();
    let stdin = std::io::stdin();
    let mut line = String::new();
    loop {
        line.clear();
        let read = {
            let mut input = stdin.lock();
            input.read_line(&mut line)
        };
        match read {
            Ok(0) => break,
            Err(e) => return Err(e.into()),
            Ok(_) => {
                let trimmed = line.trim_end();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(msg) = serde_json::from_str::<Value>(trimmed)
                    && let Some(reply) = handle(&msg, &invoke)
                {
                    crate::mcp::write_line(&mut stdout, &reply)?;
                }
            }
        }
    }
    Ok(())
}

pub fn handle(msg: &Value, invoke: &Mutex<Invoke>) -> Option<Value> {
    let id = match msg.get("id") {
        Some(Value::Number(n)) => Some(Value::Number(n.clone())),
        Some(v @ Value::String(_)) => Some(v.clone()),
        _ => None,
    };
    let method = msg.get("method").and_then(Value::as_str)?;
    let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
    let respond = |result: Value| -> Option<Value> {
        let id = id.clone()?;
        Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
    };
    let respond_err = |code: i64, message: String| -> Option<Value> {
        let id = id.clone()?;
        Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }))
    };
    match method {
        "initialize" => respond(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "zakhar", "version": env!("CARGO_PKG_VERSION") },
        })),
        "ping" => respond(json!({})),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            if !ALLOWED.contains(&name) {
                return respond_err(-32602, format!("tool not exposed by this server: {name}"));
            }
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let out = invoke.lock().unwrap().exec(name, &args);
            let is_error = out.starts_with("error:");
            respond(json!({
                "content": [ { "type": "text", "text": out } ],
                "isError": is_error,
            }))
        }
        "tools/list" | "tools/list_changed" => {
            respond(json!({ "tools": tool_list(&invoke.lock().unwrap().definitions()) }))
        }
        _ => respond_err(-32601, format!("method not found: {method}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Mutex<Invoke> {
        Mutex::new(Invoke::new())
    }

    #[test]
    fn initialize_handshake() {
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {} }
        });
        let out = handle(&msg, &server()).unwrap();
        assert_eq!(out["id"], 1);
        assert_eq!(out["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(out["result"]["serverInfo"]["name"], "zakhar");
    }

    #[test]
    fn list_is_allowlisted() {
        let msg = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} });
        let out = handle(&msg, &server()).unwrap();
        let names: Vec<&str> = out["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        for name in ["read", "fetch", "remember", "time"] {
            assert!(names.contains(&name), "missing {name}: {names:?}");
        }
        assert!(!names.contains(&"bash"), "bash leaked: {names:?}");
        assert!(!names.contains(&"write"), "write leaked: {names:?}");
        assert!(!names.contains(&"ask"), "ask leaked: {names:?}");
    }

    #[test]
    fn call_returns_is_error() {
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": { "name": "time", "arguments": {} }
        });
        let out = handle(&msg, &server()).unwrap();
        assert_eq!(out["id"], 3);
        assert_eq!(out["result"]["isError"], false);
        let text = out["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("utc:"), "time output: {text}");
    }

    #[test]
    fn call_rejects_hidden_tool() {
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": { "name": "bash", "arguments": { "command": "echo hi" } }
        });
        let out = handle(&msg, &server()).unwrap();
        assert_eq!(out["error"]["code"], -32602);
    }

    #[test]
    fn notifications_get_no_reply() {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        });
        assert!(handle(&msg, &server()).is_none());
    }

    #[test]
    fn unknown_method_errors() {
        let msg = json!({ "jsonrpc": "2.0", "id": 5, "method": "nope", "params": {} });
        let out = handle(&msg, &server()).unwrap();
        assert_eq!(out["error"]["code"], -32601);
    }
}
