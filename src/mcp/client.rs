//! MCP client: connects to an external server, runs the initialize/tools/list
//! handshake over newline-delimited JSON-RPC, and exposes each remote tool as
//! a zakhar `Handler`. Connections are cached per server and serialized so a
//! single child process serves every mount site.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};

use serde_json::{json, Value};

use crate::config::Server;
use crate::handler::Handler;
use crate::mcp::{CALL_TIMEOUT, PROTOCOL_VERSION};
use crate::types::Tool;

static CACHE: OnceLock<Mutex<HashMap<String, Arc<Client>>>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct RemoteTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub struct Client {
    child: Mutex<Child>,
    stdin: Mutex<Option<ChildStdin>>,
    responses: Mutex<mpsc::Receiver<(u64, Result<Value, String>)>>,
    tools: Vec<RemoteTool>,
    next_id: AtomicU64,
    gate: Mutex<()>,
}

pub fn connect(server_key: &str, cfg: &Server) -> anyhow::Result<Arc<Client>> {
    let cell = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(existing) = cell.lock().unwrap().get(server_key) {
        return Ok(existing.clone());
    }
    let client = Arc::new(spawn(cfg)?);
    client.request(
        "initialize",
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "zakhar", "version": env!("CARGO_PKG_VERSION") }
        }),
    )?;
    client.notify("notifications/initialized", json!({}))?;
    let listed = client.request("tools/list", json!({}))?;
    let raw = listed
        .get("tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut tools = Vec::new();
    for item in raw {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        tools.push(RemoteTool {
            name,
            description: item
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            input_schema: item
                .get("inputSchema")
                .cloned()
                .unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
        });
    }
    let mut client = client;
    if let Some(inner) = Arc::get_mut(&mut client) {
        inner.tools = tools;
    }
    cell.lock().unwrap().insert(server_key.to_string(), client.clone());
    Ok(client)
}

fn spawn(cfg: &Server) -> anyhow::Result<Client> {
    let mut child = Command::new(&cfg.command)
        .args(&cfg.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("server has no stdout"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("server has no stdin"))?;
    let (tx, rx) = mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("mcp-reader".to_string())
        .spawn(move || {
            let mut reader = std::io::BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let trimmed = line.trim_end();
                        if trimmed.is_empty() {
                            continue;
                        }
                        if let Ok(msg) = serde_json::from_str::<Value>(trimmed)
                            && let Some(id) = msg.get("id").and_then(Value::as_u64)
                        {
                            let outcome = msg
                                .get("result")
                                .cloned()
                                .map(Ok)
                                .unwrap_or_else(|| {
                                    let code = msg["error"]["code"].as_i64().unwrap_or(-1);
                                    let message = msg["error"]["message"]
                                        .as_str()
                                        .unwrap_or("mcp error")
                                        .to_string();
                                    Err(format!("mcp error {code}: {message}"))
                                });
                            let _ = tx.send((id, outcome));
                        }
                    }
                }
            }
        });
    Ok(Client {
        child: Mutex::new(child),
        stdin: Mutex::new(Some(stdin)),
        responses: Mutex::new(rx),
        tools: Vec::new(),
        next_id: AtomicU64::new(1),
        gate: Mutex::new(()),
    })
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Ok(mut input) = self.stdin.lock() {
            input.take();
        }
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Client {
    pub fn request(&self, method: &str, params: Value) -> anyhow::Result<Value> {
        let _gate = self.gate.lock().unwrap();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        {
            let mut guard = self.stdin.lock().unwrap();
            let input = guard
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("mcp transport closed"))?;
            let mut locked = &mut *input;
            serde_json::to_writer(&mut locked, &msg)?;
            locked.write_all(b"\n")?;
            locked.flush()?;
        }
        let deadline = std::time::Instant::now() + CALL_TIMEOUT;
        loop {
            let now = std::time::Instant::now();
            if now >= deadline {
                return Err(anyhow::anyhow!("timeout waiting for mcp {method}"));
            }
            let received = self.responses.lock().unwrap().recv_timeout(deadline - now);
            let (rid, outcome) = match received {
                Ok(pair) => pair,
                Err(_) => return Err(anyhow::anyhow!("mcp {method}: read end closed")),
            };
            if rid == id {
                return outcome.map_err(|e| anyhow::anyhow!(e));
            }
        }
    }

    pub fn notify(&self, method: &str, params: Value) -> anyhow::Result<()> {
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        let mut guard = self.stdin.lock().unwrap();
        let input = guard
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("mcp transport closed"))?;
        let mut locked = &mut *input;
        serde_json::to_writer(&mut locked, &msg)?;
        locked.write_all(b"\n")?;
        locked.flush()?;
        Ok(())
    }

    pub fn tools(&self) -> &[RemoteTool] {
        &self.tools
    }

    pub fn call(&self, name: &str, args: &Value) -> anyhow::Result<String> {
        let out = self.request(
            "tools/call",
            json!({ "name": name, "arguments": args }),
        )?;
        Ok(flatten(&out))
    }
}

pub struct RemoteHandler {
    client: Arc<Client>,
    label: String,
    tool: RemoteTool,
}

impl RemoteHandler {
    pub fn new(
        label: String,
        server_key: &str,
        client: Arc<Client>,
        tool: RemoteTool,
    ) -> Self {
        Self {
            client,
            label,
            tool: RemoteTool {
                description: format!(
                    "MCP tool `{}` on server `{}`.\n{}",
                    tool.name, server_key, tool.description
                ),
                ..tool
            },
        }
    }
}

impl Handler for RemoteHandler {
    fn spec(&self) -> Tool {
        Tool::function(
            &self.label,
            &self.tool.description,
            self.tool.input_schema.clone(),
        )
    }

    fn run(&self, args: &Value) -> anyhow::Result<String> {
        self.client.call(&self.tool.name, args)
    }
}

pub fn flatten(res: &Value) -> String {
    let is_error = res.get("isError").and_then(Value::as_bool).unwrap_or(false);
    let content = res
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut parts: Vec<String> = content
        .iter()
        .filter_map(|part| {
            if part.get("type").and_then(Value::as_str) == Some("text") {
                part.get("text").and_then(Value::as_str).map(ToOwned::to_owned)
            } else {
                None
            }
        })
        .collect();
    if !content.is_empty() && parts.is_empty() {
        parts.push(format!("[{}] non-text result part(s)", content.len()));
    }
    let mut text = match res.get("structuredContent") {
        Some(v) if parts.is_empty() => v.to_string(),
        _ => parts.join("\n"),
    };
    if is_error && !text.starts_with("error:") {
        text = format!("error: {text}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_text_and_error_flag() {
        let res = json!({
            "content": [ { "type": "text", "text": "ok" } ],
            "isError": false,
        });
        assert_eq!(flatten(&res), "ok");
        let bad = json!({
            "content": [ { "type": "text", "text": "boom" } ],
            "isError": true,
        });
        assert_eq!(flatten(&bad), "error: boom");
    }

    #[test]
    fn flatten_non_text_parts() {
        let res = json!({
            "content": [ { "type": "image", "data": "xyz", "mimeType": "image/png" } ],
        });
        assert_eq!(flatten(&res), "[1] non-text result part(s)");
    }
}
