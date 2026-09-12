//! Tool invocation core: a registry of `Handler`s the model can call. Also
//! carries process-wide permission and model-seed state for the tool loop.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

use crate::config::Config;
use crate::handler::Handler;
use crate::types::Tool;

static PERMIT: AtomicBool = AtomicBool::new(false);
static MODELS: OnceLock<Vec<String>> = OnceLock::new();
static CHAT: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static RESUME: OnceLock<Mutex<Option<String>>> = OnceLock::new();

pub fn seed_models(models: Vec<String>) {
    let _ = MODELS.set(models);
}

pub fn permitted() -> bool {
    PERMIT.load(Ordering::SeqCst)
}

pub fn grant() {
    PERMIT.store(true, Ordering::SeqCst);
}

pub fn open(message: String) {
    let cell = CHAT.get_or_init(|| Mutex::new(None));
    *cell.lock().unwrap() = Some(message);
}

pub fn chat_message() -> Option<String> {
    let cell = CHAT.get_or_init(|| Mutex::new(None));
    cell.lock().unwrap().take()
}

pub fn resume_session(id: String) {
    let cell = RESUME.get_or_init(|| Mutex::new(None));
    *cell.lock().unwrap() = Some(id);
}

pub fn take_resume_session() -> Option<String> {
    let cell = RESUME.get_or_init(|| Mutex::new(None));
    cell.lock().unwrap().take()
}

pub fn models() -> anyhow::Result<String> {
    match MODELS.get() {
        Some(m) if !m.is_empty() => Ok(format!("available models:\n{}", m.join("\n"))),
        _ => Ok("no models available".to_string()),
    }
}

pub const READONLY: &[&str] = &[
    "read", "glob", "grep", "ask", "todo", "task", "skill", "control", "context", "remember",
    "slash", "delegate", "handoff", "session", "search", "fetch", "calc", "clipboard", "env",
    "json", "ps", "regex",
];

pub const PARALLEL: &[&str] = &[
    "read", "glob", "grep", "search", "fetch", "calc", "clipboard", "env", "json", "ps", "regex",
];

pub struct Invoke {
    tools: HashMap<String, Box<dyn Handler>>,
}

impl Default for Invoke {
    fn default() -> Self {
        Self::new()
    }
}

impl Invoke {
    pub fn new() -> Self {
        let mut tools = HashMap::new();
        for handler in crate::tools::all() {
            let name = handler.spec().function.name.clone();
            tools.insert(name, handler);
        }
        Self { tools }
    }

    pub fn definitions(&self) -> Vec<Tool> {
        self.tools.values().map(|t| t.spec()).collect()
    }

    pub fn mount(&mut self, name: String, handler: Box<dyn Handler>) {
        self.tools.insert(name, handler);
    }

    pub fn mount_servers(&mut self, cfg: &Config) -> Vec<String> {
        let mut labels = Vec::new();
        for (server_key, server) in &cfg.mcp.servers {
            match crate::mcp::client::connect(server_key, server) {
                Ok(client) => {
                    let prefix = crate::mcp::sanitize(server_key);
                    for tool in client.tools().to_vec() {
                        let label = format!("{prefix}__{}", crate::mcp::sanitize(&tool.name));
                        let handler = crate::mcp::client::RemoteHandler::new(
                            label.clone(),
                            server_key,
                            client.clone(),
                            tool,
                        );
                        self.tools.insert(label, Box::new(handler));
                    }
                    labels.push(format!("{server_key} ({} tool(s))", client.tools().len()));
                }
                Err(e) => eprintln!("[mcp] {server_key}: {e}"),
            }
        }
        labels
    }

    pub fn filtered_definitions(&self, allowed: &[String]) -> Vec<Tool> {
        if allowed.is_empty() {
            return self.definitions();
        }
        self.tools
            .values()
            .filter(|t| allowed.contains(&t.spec().function.name))
            .map(|t| t.spec())
            .collect()
    }

    pub fn exec(&self, name: &str, args: &Value) -> String {
        let handler = match self.tools.get(name) {
            Some(h) => h,
            None => return format!("error: unknown tool: {name}"),
        };
        let revert = if name == "write" || name == "edit" {
            args.get("path")
                .and_then(|v| v.as_str())
                .and_then(crate::ledger::snapshot)
        } else {
            None
        };
        let outcome = match handler.run(args) {
            Ok(v) => v,
            Err(e) => format!("error: {e}"),
        };
        if let Err(e) = crate::ledger::record(name, args, &outcome, revert) {
            return format!("{outcome}\n[ledger] {e}");
        }
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmp() -> (tempfile::TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = crate::memory::lock();
        let dir = tempfile::tempdir().unwrap();
        crate::paths::set_home(dir.path().join(".zakhar"));
        (dir, guard)
    }

    struct Stub {
        name: String,
    }

    impl Handler for Stub {
        fn spec(&self) -> Tool {
            Tool::function(&self.name, "stub handler", json!({"type": "object"}))
        }

        fn run(&self, _args: &Value) -> anyhow::Result<String> {
            Ok(self.name.clone())
        }
    }

    #[test]
    fn missing_tool_reports_error() {
        let (_dir, _g) = tmp();
        let out = Invoke::new().exec("nope", &json!({}));
        assert_eq!(out, "error: unknown tool: nope");
    }

    #[test]
    fn read_records_ledger_entry() {
        let (dir, _g) = tmp();
        let path = dir.path().join("note.txt");
        std::fs::write(&path, "hello").unwrap();
        let before = crate::ledger::read().len();
        let invoke = Invoke::new();
        let out = invoke.exec("read", &json!({"path": path}));
        assert!(out.contains("hello"), "got: {out}");
        assert!(!out.starts_with("error:"));
        let entries = crate::ledger::read();
        assert!(entries.len() > before, "no new ledger entry");
        let last = entries.last().unwrap();
        assert_eq!(last.tool, "read");
        assert_eq!(last.outcome, out);
    }

    #[test]
    fn failing_handler_reports_error_and_records() {
        let (_dir, _g) = tmp();
        let before = crate::ledger::read().len();
        let invoke = Invoke::new();
        let out = invoke.exec("read", &json!({"path": "/no/such/file"}));
        assert!(out.starts_with("error:"), "got: {out}");
        let entries = crate::ledger::read();
        assert!(entries.len() > before, "no new ledger entry");
        assert_eq!(entries.last().unwrap().tool, "read");
        assert_eq!(entries.last().unwrap().outcome, out);
    }

    #[test]
    fn write_snapshots_and_records() {
        let (dir, _g) = tmp();
        let path = dir.path().join("sub/notes.txt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "v1").unwrap();
        let invoke = Invoke::new();
        let before = crate::ledger::read().len();
        let out = invoke.exec("write", &json!({"path": path, "content": "v2"}));
        assert!(out.starts_with("wrote"), "got: {out}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "v2");
        let entries = crate::ledger::read();
        assert_eq!(entries.len(), before + 1);
        assert!(entries.last().unwrap().revert.is_some(), "write must snapshot");
        let restore = crate::ledger::undo(1).unwrap();
        assert!(restore.contains("reverted 1"), "got: {restore}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "v1");
    }

    #[test]
    fn definitions_follow_allowed_filter() {
        let invoke = Invoke::new();
        let all = invoke.definitions();
        assert!(all.len() > 10);
        assert_eq!(invoke.filtered_definitions(&[]).len(), all.len());
        let only = invoke.filtered_definitions(&["read".to_string()]);
        assert_eq!(only.len(), 1);
        assert_eq!(only[0].function.name, "read");
    }

    #[test]
    fn mount_adds_handler() {
        let mut invoke = Invoke::new();
        invoke.mount("stub".to_string(), Box::new(Stub { name: "stub".to_string() }));
        assert_eq!(invoke.exec("stub", &json!({})), "stub");
        assert!(invoke
            .definitions()
            .iter()
            .any(|t| t.function.name == "stub"));
    }

    #[test]
    fn mount_servers_dead_binary_returns_empty() {
        let cfg = Config {
            mcp: crate::config::Mcp {
                servers: std::collections::HashMap::from([(
                    "dead".to_string(),
                    crate::config::Server {
                        command: "/definitely/missing/binary".to_string(),
                        args: vec![],
                        env: std::collections::HashMap::new(),
                    },
                )]),
            },
            ..Default::default()
        };
        assert_eq!(Invoke::new().mount_servers(&cfg).len(), 0);
    }
}
