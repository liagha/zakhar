//! Mobile turn engine, embedded by the Android JNI bridge and the desktop
//! `zakhar mobile` subcommand. Owns nothing platform-specific: a turn is a
//! session that streams events (text, reasoning, tool calls, approvals) into a
//! queue the caller drains with `poll`. Registration and approvals resolve
//! through the same JSON protocol everywhere.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;

use crate::provider::Provider;

static SESSIONS: OnceLock<Mutex<HashMap<String, Arc<Session>>>> = OnceLock::new();

struct Session {
    events: Mutex<VecDeque<String>>,
    pending: Mutex<Approval>,
    pending_cond: Condvar,
    done: AtomicBool,
    cancel: AtomicBool,
}

struct Approval {
    requested: bool,
    decision: Option<bool>,
}

impl Session {
    fn new() -> Self {
        Session {
            events: Mutex::new(VecDeque::new()),
            pending: Mutex::new(Approval { requested: false, decision: None }),
            pending_cond: Condvar::new(),
            done: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
        }
    }

    fn push(&self, event: String) {
        self.events.lock().unwrap().push_back(event);
        self.pending_cond.notify_all();
    }

    fn finish(&self, event: String) {
        self.push(event);
        self.done.store(true, Ordering::SeqCst);
        self.pending_cond.notify_all();
    }

    fn request_approval(&self, index: usize, name: &str, args: &serde_json::Value) -> bool {
        let msg = json!({
            "type": "tool_approval",
            "index": index,
            "name": name,
            "args": args,
        })
        .to_string();
        self.push(msg);
        let mut pending = self.pending.lock().unwrap();
        pending.requested = true;
        pending.decision = None;
        while pending.decision.is_none() {
            if self.cancel.load(Ordering::SeqCst) {
                pending.decision = Some(false);
                break;
            }
            pending = self.pending_cond.wait(pending).unwrap();
        }
        pending.decision == Some(true)
    }

    fn resolve(&self, decision: bool) {
        let mut pending = self.pending.lock().unwrap();
        pending.decision = Some(decision);
        drop(pending);
        self.pending_cond.notify_all();
    }

    fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        let mut pending = self.pending.lock().unwrap();
        if pending.decision.is_none() {
            pending.decision = Some(false);
        }
        drop(pending);
        self.pending_cond.notify_all();
    }
}

pub fn start(provider: Box<dyn Provider>, messages_json: &str, auto_approve: bool) -> String {
    let messages_json = messages_json.trim();
    if messages_json.is_empty() {
        return json_err("missing messages");
    }
    let id = format!(
        "s{:x}",
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos()
    );
    let session = Arc::new(Session::new());
    sessions().lock().unwrap().insert(id.clone(), session.clone());
    let s2 = session.clone();
    let messages = messages_json.to_string();
    std::thread::spawn(move || run_turn(s2, provider, messages, auto_approve));
    id
}

pub fn poll(id: &str, timeout_ms: i64) -> String {
    let session = match get_session(id) {
        Some(s) => s,
        None => return json_err("no such session"),
    };
    let timeout = Duration::from_millis(timeout_ms.max(0) as u64);
    let mut events = session.events.lock().unwrap();
    if events.is_empty() && !session.done.load(Ordering::SeqCst) {
        let (guard, _) = session.pending_cond.wait_timeout(events, timeout).unwrap();
        events = guard;
    }
    let batch: Vec<String> = events.drain(..).collect();
    if batch.is_empty() && session.cancel.load(Ordering::SeqCst) {
        return json!([{ "type": "cancelled" }]).to_string();
    }
    json!({ "events": batch }).to_string()
}

pub fn approve(id: &str, decision: bool) -> String {
    if let Some(s) = get_session(id) {
        s.resolve(decision);
    }
    "ok".to_string()
}

pub fn cancel(id: &str) -> String {
    if let Some(s) = get_session(id) {
        s.cancel();
    }
    "ok".to_string()
}

pub fn discard(id: &str) -> String {
    drop_session(id);
    "ok".to_string()
}

pub fn keys() -> String {
    crate::tools::context_keys()
}

pub fn recent(n: usize) -> String {
    crate::memory::episodic::recent_json(n)
}

fn run_turn(session: Arc<Session>, provider: Box<dyn Provider>, messages_json: String, auto_approve: bool) {
    let turn = async {
        let messages: Vec<crate::types::Message> = match serde_json::from_str(&messages_json) {
            Ok(m) => m,
            Err(e) => return Err(anyhow::anyhow!("bad messages: {e}")),
        };
        let model = provider.list_models().first().cloned().unwrap_or_default();

        let mut inv = crate::invoke::Invoke::new();
        let cfg = crate::config::Config::load().unwrap_or_default();
        let _ = inv.mount_servers(&cfg);
        let tools = inv.definitions();

        let mut runner = crate::agent::Runner::new(provider.as_ref(), model, None);
        runner.set_tools(tools);

        for (label, text) in crate::memory::load_blocks() {
            runner.push(crate::types::Message::system(format!("{label}:\n{text}")));
        }

        for msg in &messages {
            runner.push(msg.clone());
        }
        if let Some(last) = messages.last()
            && last.role == crate::types::Role::User
        {
            let _ = crate::memory::episodic::append("chat", &last.content);
        }

        let mut tool_events: Vec<serde_json::Value> = Vec::new();
        let mut tool_seq: usize = 0;

        loop {
            if session.cancel.load(Ordering::SeqCst) {
                return Err(anyhow::anyhow!("cancelled"));
            }
            let mut stream = match runner.stream().await {
                Ok(s) => s,
                Err(e) => return Err(e),
            };

            let mut full = String::new();
            let mut reasoning = String::new();
            let mut tool_parts: Vec<crate::provider::ToolCallPart> = Vec::new();

            use futures::StreamExt;
            while let Some(event) = stream.next().await {
                if session.cancel.load(Ordering::SeqCst) {
                    return Err(anyhow::anyhow!("cancelled"));
                }
                let event = match event {
                    Ok(ev) => ev,
                    Err(e) => return Err(e),
                };
                match event {
                    crate::provider::ChatStreamEvent::Text(t) => {
                        full.push_str(&t);
                        session.push(json!({ "type": "text", "data": t }).to_string());
                    }
                    crate::provider::ChatStreamEvent::Reasoning(t) => {
                        reasoning.push_str(&t);
                        session.push(json!({ "type": "reasoning", "data": t }).to_string());
                    }
                    crate::provider::ChatStreamEvent::ToolCall(part) => tool_parts.push(part),
                    _ => {}
                }
            }

            if session.cancel.load(Ordering::SeqCst) {
                return Err(anyhow::anyhow!("cancelled"));
            }
            let tool_calls = group_calls(tool_parts);

            if tool_calls.is_empty() {
                let _ = crate::memory::episodic::append("chat", &full);
                let msg =
                    json!({ "type": "done", "text": full, "reasoning": reasoning, "tools": tool_events })
                        .to_string();
                session.finish(msg);
                return Ok(());
            }

            for tc in &tool_calls {
                if session.cancel.load(Ordering::SeqCst) {
                    return Err(anyhow::anyhow!("cancelled"));
                }
                tool_seq += 1;
                let tool_id = tool_seq;
                let read_only = crate::invoke::READONLY.contains(&tc.name.as_str());
                let approved = if read_only || auto_approve {
                    true
                } else {
                    session.request_approval(tool_id, &tc.name, &tc.arguments)
                };
                let result = if approved {
                    inv.exec(&tc.name, &tc.arguments)
                } else {
                    "tool call denied by user".to_string()
                };
                session.push(
                    json!({
                        "type": "tool_result",
                        "index": tool_id,
                        "name": tc.name,
                        "approved": approved,
                        "result": result,
                    })
                    .to_string(),
                );
                tool_events.push(json!({
                    "name": tc.name,
                    "args": tc.arguments,
                    "result": result,
                }));
                runner.push(crate::types::Message::tool(tc.id.clone(), result.clone()));
            }
        }
    };

    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(r) => r,
        Err(e) => {
            session.finish(json!({ "type": "error", "message": format!("failed to start runtime: {e}") }).to_string());
            return;
        }
    };
    match rt.block_on(turn) {
        Ok(()) => {}
        Err(e) => {
            if session.cancel.load(Ordering::SeqCst) {
                session.finish(json!({ "type": "cancelled" }).to_string());
            } else {
                session.finish(json!({ "type": "error", "message": format!("{e}") }).to_string());
            }
        }
    }
}

fn group_calls(parts: Vec<crate::provider::ToolCallPart>) -> Vec<crate::types::ToolCall> {
    let mut grouped: HashMap<usize, (String, String, String)> = HashMap::new();
    let mut order: Vec<usize> = Vec::new();
    for part in parts {
        let e = grouped.entry(part.index).or_default();
        if !order.contains(&part.index) {
            order.push(part.index);
        }
        if let Some(id) = &part.id {
            e.0 = id.clone();
        }
        if let Some(name) = &part.name {
            e.1 = name.clone();
        }
        if let Some(args) = &part.arguments {
            e.2.push_str(args);
        }
    }
    order
        .into_iter()
        .filter_map(|index| {
            let (id, name, args) = grouped.remove(&index)?;
            if name.is_empty() {
                return None;
            }
            let arguments = serde_json::from_str(&args).unwrap_or_else(|_| json!({}));
            Some(crate::types::ToolCall { id, name, arguments })
        })
        .collect()
}

fn sessions() -> &'static Mutex<HashMap<String, Arc<Session>>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn get_session(id: &str) -> Option<Arc<Session>> {
    sessions().lock().unwrap().get(id).cloned()
}

fn drop_session(id: &str) {
    sessions().lock().unwrap().remove(id);
}

fn json_err(msg: &str) -> String {
    json!({ "error": msg }).to_string()
}