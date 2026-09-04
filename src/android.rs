//! Android JNI bridge for the embedded zakhar core.
//!
//! Compiled only when the `jni` feature is enabled (see `Cargo.toml`). The
//! Kotlin `ZakharCore` module drives the conversation engine on-device.
//!
//! Turns run on a background thread so the UI stays responsive. The thread
//! pushes JSON events into a per-session queue that Kotlin drains with
//! repeated `poll` calls: text and reasoning deltas, tool start/results, and
//! approval requests. A turn can pause on a per-tool approval and resume when
//! the caller resolves it, then completes with a `done` event.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jstring};
use jni::JNIEnv;
use serde_json::json;

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

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_zakhar_mobile_ZakharCore_nativeStartChatSession(
    mut env: JNIEnv,
    _class: JClass,
    provider_json: JString,
    messages_json: JString,
    auto_approve: jboolean,
) -> jstring {
    let provider = env.get_string(&provider_json).ok().map(|s| s.into());
    let messages = env.get_string(&messages_json).ok().map(|s| s.into());
    respond(&mut env, &spawn(provider, messages, auto_approve != 0))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_zakhar_mobile_ZakharCore_nativePollEvents(
    mut env: JNIEnv,
    _class: JClass,
    session: JString,
    timeout_ms: i64,
) -> jstring {
    let id: String = env.get_string(&session).ok().map(|s| s.into()).unwrap_or_default();
    respond(&mut env, &poll(&id, timeout_ms))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_zakhar_mobile_ZakharCore_nativeResolveApproval(
    mut env: JNIEnv,
    _class: JClass,
    session: JString,
    approved: jboolean,
) -> jstring {
    let id: String = env.get_string(&session).ok().map(|s| s.into()).unwrap_or_default();
    if let Some(s) = get_session(&id) {
        s.resolve(approved != 0);
    }
    respond(&mut env, "ok")
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_zakhar_mobile_ZakharCore_nativeCancelSession(
    mut env: JNIEnv,
    _class: JClass,
    session: JString,
) -> jstring {
    let id: String = env.get_string(&session).ok().map(|s| s.into()).unwrap_or_default();
    if let Some(s) = get_session(&id) {
        s.cancel();
    }
    respond(&mut env, "ok")
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_zakhar_mobile_ZakharCore_nativeDropSession(
    mut env: JNIEnv,
    _class: JClass,
    session: JString,
) -> jstring {
    let id: String = env.get_string(&session).ok().map(|s| s.into()).unwrap_or_default();
    drop_session(&id);
    respond(&mut env, "ok")
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_zakhar_mobile_ZakharCore_nativeContextKeys(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    respond(&mut env, &crate::tools::context_keys())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_zakhar_mobile_ZakharCore_nativeRecentEvents(
    mut env: JNIEnv,
    _class: JClass,
    n: jni::sys::jint,
) -> jstring {
    respond(&mut env, &crate::memory::episodic::recent_json(n as usize))
}

fn respond(env: &mut JNIEnv, payload: &str) -> jstring {
    let output = env.new_string(payload).unwrap_or_else(|_| env.new_string("{}").unwrap());
    output.into_raw()
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

fn spawn(provider_json: Option<String>, messages_json: Option<String>, auto_approve: bool) -> String {
    let provider_json = match provider_json {
        Some(s) if !s.trim().is_empty() => s,
        _ => return json_err("missing provider config"),
    };
    let messages_json = match messages_json {
        Some(s) if !s.trim().is_empty() => s,
        _ => return json_err("missing messages"),
    };
    let id = format!(
        "s{:x}",
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos()
    );
    let session = Arc::new(Session::new());
    sessions().lock().unwrap().insert(id.clone(), session.clone());
    let s2 = session.clone();
    std::thread::spawn(move || run_turn(s2, provider_json, messages_json, auto_approve));
    id
}

fn poll(id: &str, timeout_ms: i64) -> String {
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

fn run_turn(
    session: Arc<Session>,
    provider_json: String,
    messages_json: String,
    auto_approve: bool,
) {
    let turn = async {
        let pcfg: crate::provider::types::Config = match serde_json::from_str(&provider_json) {
            Ok(c) => c,
            Err(e) => return Err(anyhow::anyhow!("bad provider config: {e}")),
        };
        if pcfg.base_url.trim().is_empty() {
            return Err(anyhow::anyhow!("missing provider base url"));
        }
        let provider: Box<dyn crate::provider::Provider> =
            Box::new(crate::provider::openai::OpenAI::new("app", &pcfg));
        let model = pcfg.default_model.clone();

        let inv = crate::invoke::Invoke::new();
        let tools = inv.definitions();

        let mut runner = crate::agent::Runner::new(provider.as_ref(), model.clone(), None);
        runner.set_tools(tools);

        for (label, text) in crate::memory::load_blocks() {
            runner.push(crate::types::Message::system(format!("{label}:\n{text}")));
        }
        let ctx = crate::tools::context_index();
        if ctx != "no saved context" {
            runner.push(crate::types::Message::system(format!(
                "Saved context (fetch values with the context tool as needed):\n{ctx}"
            )));
        }

        let history: Vec<crate::types::Message> = match serde_json::from_str(&messages_json) {
            Ok(h) => h,
            Err(e) => return Err(anyhow::anyhow!("bad messages: {e}")),
        };
        for msg in &history {
            runner.push(msg.clone());
        }
        if let Some(last) = history.last()
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
                let mut tools = serde_json::Value::Array(tool_events.clone());
                let _ = &mut tools;
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

fn json_err(msg: &str) -> String {
    json!({ "error": msg }).to_string()
}
