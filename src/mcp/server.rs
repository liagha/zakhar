//! MCP server: `zakhar mcp` serves a fixed allowlist of read-only and
//! knowledge tools plus read-only resources and a digest prompt over stdio,
//! so any MCP client can drive them. Interactive and stdout-writing tools are
//! never exposed.

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

const RESOURCES: &[(&str, &str, &str)] = &[
    (
        "zakhar://memory/knowledge",
        "saved knowledge",
        "notes learned across sessions",
    ),
    (
        "zakhar://memory/events",
        "recent events",
        "recent episodic log",
    ),
    ("zakhar://sessions", "saved sessions", "list of saved sessions"),
];

fn resource_text(uri: &str) -> Option<String> {
    match uri {
        "zakhar://memory/knowledge" => Some(crate::memory::knowledge::block(20)),
        "zakhar://memory/events" => Some(crate::memory::episodic::block(30)),
        "zakhar://sessions" => Some(crate::session::list_formatted()),
        _ => None,
    }
}

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
            "capabilities": {
                "tools": { "listChanged": false },
                "resources": { "subscribe": false },
                "prompts": {},
            },
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
        "resources/list" => respond(json!({
            "resources": RESOURCES
                .iter()
                .map(|(uri, name, description)| json!({
                    "uri": uri,
                    "name": name,
                    "description": description,
                    "mimeType": "text/plain",
                }))
                .collect::<Vec<_>>(),
        })),
        "resources/read" => {
            let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
            if uri.is_empty() {
                return respond_err(-32602, "missing uri".to_string());
            }
            match resource_text(uri) {
                Some(text) => respond(json!({
                    "content": [ { "uri": uri, "mimeType": "text/plain", "text": text } ],
                })),
                None => respond_err(-32002, format!("resource not found: {uri}")),
            }
        }
        "prompts/list" => respond(json!({
            "prompts": [{
                "name": "daily",
                "description": "daily digest of knowledge, recent events, and saved sessions",
                "arguments": [],
            }],
        })),
        "prompts/get" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            if name != "daily" {
                return respond_err(-32602, format!("no such prompt: {name}"));
            }
            respond(json!({
                "description": "daily digest of knowledge, recent events, and saved sessions",
                "messages": [
                    {
                        "role": "system",
                        "content": {
                            "type": "text",
                            "text": "You write concise, human-ready daily digests.",
                        },
                    },
                    {
                        "role": "user",
                        "content": {
                            "type": "text",
                            "text": format!(
                                "knowledge:\n{}\nrecent events:\n{}\nsaved sessions:\n{}",
                                crate::memory::knowledge::block(20),
                                crate::memory::episodic::block(30),
                                crate::session::list_formatted(),
                            ),
                        },
                    },
                ],
            }))
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

    fn tmp() -> (tempfile::TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = crate::memory::lock();
        let dir = tempfile::tempdir().unwrap();
        crate::paths::set_home(dir.path().join(".zakhar"));
        (dir, guard)
    }

    #[test]
    fn resources_list_advertises() {
        let (_dir, _g) = tmp();
        let msg = json!({ "jsonrpc": "2.0", "id": 6, "method": "resources/list", "params": {} });
        let out = handle(&msg, &server()).unwrap();
        let uris: Vec<&str> = out["result"]["resources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["uri"].as_str().unwrap())
            .collect();
        for uri in ["zakhar://memory/knowledge", "zakhar://memory/events", "zakhar://sessions"] {
            assert!(uris.contains(&uri), "missing {uri}: {uris:?}");
        }
    }

    #[test]
    fn resources_read_returns_blocks() {
        let (dir, _g) = tmp();
        crate::memory::knowledge::set_path(dir.path().join("memory/knowledge.jsonl"));
        crate::memory::set_path(dir.path().join("memory/episodic.jsonl"));
        crate::memory::knowledge::save_pair("router ip", "192.168.1.1", "test").unwrap();
        crate::memory::episodic::append("note", "started the server").unwrap();
        let invoke = server();
        for (uri, needle) in [
            ("zakhar://memory/knowledge", "router ip"),
            ("zakhar://memory/events", "started the server"),
        ] {
            let msg = json!({
                "jsonrpc": "2.0",
                "id": 7,
                "method": "resources/read",
                "params": { "uri": uri },
            });
            let out = handle(&msg, &invoke).unwrap();
            let text = out["result"]["content"][0]["text"].as_str().unwrap();
            assert!(text.contains(needle), "{uri}: {text}");
        }
    }

    #[test]
    fn resources_read_sessions_and_missing() {
        let (_dir, _g) = tmp();
        let invoke = server();
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 8,
            "method": "resources/read",
            "params": { "uri": "zakhar://nope" },
        });
        let out = handle(&msg, &invoke).unwrap();
        assert_eq!(out["error"]["code"], -32002);
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 9,
            "method": "resources/read",
            "params": {},
        });
        let out = handle(&msg, &invoke).unwrap();
        assert_eq!(out["error"]["code"], -32602);
    }

    #[test]
    fn prompts_list_and_get() {
        let (_dir, _g) = tmp();
        let invoke = server();
        let list = json!({ "jsonrpc": "2.0", "id": 10, "method": "prompts/list", "params": {} });
        let out = handle(&list, &invoke).unwrap();
        let names: Vec<&str> = out["result"]["prompts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"daily"), "got: {names:?}");
        let get = json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "prompts/get",
            "params": { "name": "daily" },
        });
        let out = handle(&get, &invoke).unwrap();
        assert!(!out["result"]["messages"].as_array().unwrap().is_empty());
        assert!(out["result"]["messages"][1]["content"]["text"]
            .as_str()
            .unwrap()
            .contains("recent events:"));
        let bad = json!({
            "jsonrpc": "2.0",
            "id": 12,
            "method": "prompts/get",
            "params": { "name": "nope" },
        });
        let out = handle(&bad, &invoke).unwrap();
        assert_eq!(out["error"]["code"], -32602);
    }

    #[test]
    fn initialize_advertises_resources_and_prompts() {
        let (_dir, _g) = tmp();
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 13,
            "method": "initialize",
            "params": { "protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {} }
        });
        let out = handle(&msg, &server()).unwrap();
        assert!(out["result"]["capabilities"]["resources"]
            .as_object()
            .is_some());
        assert!(out["result"]["capabilities"]["prompts"].as_object().is_some());
    }
}
