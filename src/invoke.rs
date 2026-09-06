use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

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
