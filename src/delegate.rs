use std::collections::HashMap;

use colored::Colorize;
use serde_json::{json, Value};

use crate::agent::Runner;
use crate::config::Config;
use crate::hooks;
use crate::invoke::Invoke;
use crate::provider::Provider;
use crate::slash;
use crate::types::{Message, Tool, ToolCall};

const MAX_DEPTH: usize = 3;
const MAX_TURNS: usize = 8;
const MAX_RETRY: usize = 2;

fn exec_with_retry(invoke: &Invoke, name: &str, args: &Value) -> String {
    let out = invoke.exec(name, args);
    if !out.starts_with("error:") || name != "edit" {
        return out;
    }
    let mut attempts = 0;
    let mut current = out;
    while current.starts_with("error:") && attempts < MAX_RETRY {
        match edit_retry(args, &current) {
            EditRetry::Ok(out) => return out,
            EditRetry::Retry => {
                println!("{} {}", "↺ retry".bold(), "old_string now matches".dimmed());
                current = invoke.exec(name, args);
            }
            EditRetry::Boosted(out) => {
                println!("{} {}", "↻ context".bold(), "attached file content to error".dimmed());
                return out;
            }
        }
        attempts += 1;
    }
    current
}

enum EditRetry {
    Ok(String),
    Retry,
    Boosted(String),
}

/// Decide how to recover from a failed edit call.
fn edit_retry(args: &Value, err: &str) -> EditRetry {
    let Some(path) = args.get("path").and_then(|v| v.as_str()) else {
        return EditRetry::Ok(err.to_string());
    };
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return EditRetry::Ok(err.to_string()),
    };
    let old = args.get("old_string").and_then(|v| v.as_str()).unwrap_or("");
    if content.contains(old) {
        return EditRetry::Retry;
    }
    let snippet: String = content.chars().take(700).collect();
    let boosted = format!("{err}\n--- actual content of {path} (first 700 chars) ---\n{snippet}\n---");
    EditRetry::Boosted(boosted)
}

pub fn tool_def(cfg: &Config) -> Tool {
    let agents: Vec<String> = cfg.agents.keys().cloned().collect();
    let agent_list = if agents.is_empty() {
        "none configured".to_string()
    } else {
        agents.join(", ")
    };
    let mut props = serde_json::Map::new();
    props.insert(
        "agent".to_string(),
        json!({
            "type": "string",
            "description": format!("Agent name to delegate to. Available: {}", agent_list)
        }),
    );
    if !agents.is_empty()
        && let Some(v) = props.get_mut("agent")
            && let Some(obj) = v.as_object_mut() {
                obj.insert("enum".to_string(), json!(agents));
            }
    props.insert(
        "task".to_string(),
        json!({
            "type": "string",
            "description": "Task description for the sub-agent. Be specific and self-contained."
        }),
    );
    Tool::function(
        "delegate",
        format!(
            "Delegate a sub-task to a specialist agent. Available agents: {}. Use when a task is better handled by a specialist. The sub-agent will run autonomously with its own tools and return a result. Multiple delegates in one turn run in parallel.",
            agent_list
        ),
        json!({
            "type": "object",
            "properties": props,
            "required": ["agent", "task"]
        }),
    )
}

pub fn handoff_tool_def(cfg: &Config) -> Tool {
    let agents: Vec<String> = cfg.agents.keys().cloned().collect();
    let agent_list = if agents.is_empty() {
        "none configured".to_string()
    } else {
        agents.join(", ")
    };
    let mut props = serde_json::Map::new();
    props.insert(
        "agent".to_string(),
        json!({
            "type": "string",
            "description": format!("Agent to hand off to. Available: {}", agent_list)
        }),
    );
    if !agents.is_empty()
        && let Some(v) = props.get_mut("agent")
            && let Some(obj) = v.as_object_mut() {
                obj.insert("enum".to_string(), json!(agents));
            }
    props.insert(
        "task".to_string(),
        json!({
            "type": "string",
            "description": "Task/context to hand off. The receiving agent takes over the conversation and its final answer is returned as the handoff result."
        }),
    );
    Tool::function(
        "handoff",
        format!(
            "Hand off the conversation to another agent (pipeline). Available: {}. The target agent runs to completion and its answer becomes the final result. Use for sequential pipelines e.g. explorer -> coder -> reviewer. Prefer delegate for parallel sub-tasks.",
            agent_list
        ),
        json!({
            "type": "object",
            "properties": props,
            "required": ["agent", "task"]
        }),
    )
}

pub async fn run(
    provider: &dyn Provider,
    cfg: &Config,
    agent_name: &str,
    task: &str,
    depth: usize,
    plan: bool,
) -> String {
    if depth >= MAX_DEPTH {
        return format!("error: max delegation depth ({MAX_DEPTH}) reached at agent '{agent_name}'");
    }
    let agent_cfg = match cfg.agents.get(agent_name) {
        Some(a) => a,
        None => {
            let available: Vec<String> = cfg.agents.keys().cloned().collect();
            return format!(
                "error: unknown agent '{}'. Available: {}",
                agent_name,
                if available.is_empty() { "none".to_string() } else { available.join(", ") }
            );
        }
    };

    let model = if !agent_cfg.model.is_empty() {
        agent_cfg.model.clone()
    } else {
        let cap = crate::capabilities::detect(cfg, task);
        let r = crate::capabilities::resolve(cfg, &cap, "heavy");
        if !r.model.is_empty() {
            r.model
        } else if let Some(m) = &cfg.default_model {
            m.clone()
        } else {
            provider.list_models().first().cloned().unwrap_or_default()
        }
    };

    let prefix = format!("{}", "  ▸".dimmed());
    println!("{prefix} {agent_name}: \"{}\"", truncate(task, 80));
    std::io::Write::flush(&mut std::io::stdout()).ok();

    let mut runner = Runner::new(provider, model.clone(), Some(agent_cfg));
    for (label, text) in crate::memory::load_blocks() {
        runner.push(Message::system(format!("{label}:\n{text}")));
    }
    if plan {
        runner.push(Message::system(
            "PLAN MODE: read-only. Do not use write/edit/bash to modify files. Use todo to plan, ask to clarify, and delegate/handoff to specialists."
                .to_string(),
        ));
    }
    let mut invoke = Invoke::new();
    let _ = invoke.mount_servers(cfg);
    let allowed = agent_cfg.tools.as_slice();
    let mut tools = if allowed.is_empty() {
        invoke.definitions()
    } else {
        invoke.filtered_definitions(allowed)
    };
    let delegate_allowed = allowed.is_empty() || allowed.contains(&"delegate".to_string());
    let handoff_allowed = allowed.is_empty() || allowed.contains(&"handoff".to_string());
    let slash_allowed = allowed.is_empty() || allowed.contains(&"slash".to_string());
    if depth + 1 < MAX_DEPTH && !cfg.agents.is_empty() {
        if delegate_allowed {
            tools.push(tool_def(cfg));
        }
        if handoff_allowed {
            tools.push(handoff_tool_def(cfg));
        }
    }
    if slash_allowed {
        tools.push(slash::tool_def());
    }
    if plan {
        tools.retain(|t| crate::invoke::READONLY.contains(&t.function.name.as_str()));
        println!("{prefix} plan mode: tools filtered to {} readonly", tools.len());
    }
    runner.set_tools(tools);
    runner.push(Message::user(task.to_string()));

    let mut turns = 0;
    while turns < MAX_TURNS {
        turns += 1;

        let stream = match runner.stream().await {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("{prefix} ✗ {agent_name} failed: {e}");
                println!("{}", msg.dimmed());
                std::io::Write::flush(&mut std::io::stdout()).ok();
                return msg;
            }
        };

        use futures::StreamExt;
        let mut full = String::new();
        let mut tool_parts: HashMap<usize, ToolCallPartAccum> = HashMap::new();
        let mut events_seen = 0usize;

        {
            let mut stream = stream;
            while let Some(event) = stream.next().await {
                let event = match event {
                    Ok(ev) => ev,
                    Err(e) => {
                        println!("{}", format!("{prefix} ✗ stream error: {e}").dimmed());
                        std::io::Write::flush(&mut std::io::stdout()).ok();
                        return format!("{prefix} stream error: {e}");
                    }
                };
                match event {
                    crate::provider::ChatStreamEvent::Reasoning(t) => {
                        events_seen += 1;
                        print!("{}", t.dimmed().italic());
                        std::io::Write::flush(&mut std::io::stdout()).ok();
                    }
                    crate::provider::ChatStreamEvent::Text(t) => {
                        events_seen += 1;
                        print!("{t}");
                        std::io::Write::flush(&mut std::io::stdout()).ok();
                        full.push_str(&t);
                    }
                    crate::provider::ChatStreamEvent::ToolCall(part) => {
                        events_seen += 1;
                        let entry = tool_parts.entry(part.index).or_default();
                        if let Some(id) = part.id {
                            entry.id = id;
                        }
                        if let Some(name) = part.name {
                            entry.name = name;
                        }
                        if let Some(args) = part.arguments {
                            entry.arguments.push_str(&args);
                        }
                    }
                    _ => {}
                }
            }
        }

        if events_seen == 0 {
            println!("{}", format!("{prefix} … {agent_name} stream ended with no content").dimmed());
        }
        std::io::Write::flush(&mut std::io::stdout()).ok();

        let tool_calls: Vec<ToolCall> = tool_parts
            .into_iter()
            .collect::<Vec<_>>()
            .into_iter()
            .filter_map(|(_, acc)| {
                let args: Value = serde_json::from_str(&acc.arguments)
                    .unwrap_or(Value::Object(serde_json::Map::new()));
                if acc.name.is_empty() {
                    return None;
                }
                Some(ToolCall {
                    id: acc.id,
                    name: acc.name,
                    arguments: args,
                })
            })
            .collect();

        if tool_calls.is_empty() {
            println!("{}", format!("{prefix} ✓ {agent_name} done ({} chars)", full.len()).dimmed());
            std::io::Write::flush(&mut std::io::stdout()).ok();
            runner.push(Message::assistant(full.clone(), None));
            return full;
        }

        println!(
            "{}",
            format!(
                "{prefix} → {}: {}",
                tool_calls.len(),
                tool_calls
                    .iter()
                    .map(|tc| format!("{}({})", tc.name, compact_args(&tc.arguments)))
                    .collect::<Vec<_>>()
                    .join(" · ")
            )
            .dimmed()
        );
        std::io::Write::flush(&mut std::io::stdout()).ok();

        runner.push(Message::assistant(full.clone(), Some(tool_calls.clone())));

        let mut outputs: HashMap<String, String> = HashMap::new();
        let mut delegate_futures: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = String>>>> =
            Vec::new();
        let mut delegate_ids: Vec<String> = Vec::new();

        for tc in &tool_calls {
            if let Err(e) = hooks::run_pre(&tc.name, &tc.arguments) {
                println!("{}", format!("{prefix} ✗ pre-hook blocked {}: {e}", tc.name).dimmed());
                std::io::Write::flush(&mut std::io::stdout()).ok();
                outputs.insert(tc.id.clone(), format!("blocked by pre-hook: {e}"));
                continue;
            }
            if tc.name == "delegate" || tc.name == "handoff" {
                let sub_agent = tc
                    .arguments
                    .get("agent")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let sub_task = tc
                    .arguments
                    .get("task")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let kind = tc.name.clone();
                if sub_agent.is_empty() || sub_task.is_empty() {
                    outputs.insert(
                        tc.id.clone(),
                        format!("error: {kind} requires 'agent' and 'task', got {}", tc.arguments),
                    );
                } else {
                    println!(
                        "{prefix} → {kind} → {sub_agent}: \"{}\"",
                        truncate(&sub_task, 80)
                    );
                    std::io::Write::flush(&mut std::io::stdout()).ok();
                    let cfg_clone = cfg.clone();
                    let prov_copy: &dyn Provider = provider;
                    let depth_next = depth + 1;
                    let plan_copy = plan;
                    let id_clone = tc.id.clone();
                    let kind_clone = kind.clone();
                    let args_clone = tc.arguments.clone();
                    delegate_ids.push(id_clone);
                    delegate_futures.push(Box::pin(async move {
                        let res = run(prov_copy, &cfg_clone, &sub_agent, &sub_task, depth_next, plan_copy).await;
                        hooks::run_post(&kind_clone, &args_clone, &res);
                        res
                    }));
                }
            } else if tc.name == "slash" {
                let cmd = tc.arguments.get("command").and_then(|v| v.as_str()).unwrap_or("");
                let args = tc.arguments.get("args").and_then(|v| v.as_str()).unwrap_or("");
                println!("{prefix} → slash {cmd} {args} …");
                std::io::Write::flush(&mut std::io::stdout()).ok();
                let mut mirror = crate::session::Session::new();
                mirror.messages = runner.messages().clone();
                let out = slash::handle_ai(cmd, args, &mut mirror, &mut runner);
                println!(
                    "{prefix} ← slash {cmd} done: {}",
                    out.lines().next().unwrap_or("").chars().take(60).collect::<String>()
                );
                std::io::Write::flush(&mut std::io::stdout()).ok();
                hooks::run_post(&tc.name, &tc.arguments, &out);
                outputs.insert(tc.id.clone(), out);
            } else {
                println!("{}", format!("{prefix} → invoke({}) …", tc.name).dimmed());
                std::io::Write::flush(&mut std::io::stdout()).ok();
                let out = exec_with_retry(&invoke, &tc.name, &tc.arguments);
                let preview = truncate(&out, 300);
                println!("{}", format!("{prefix} ✓ {}({})", tc.name, preview).dimmed());
                std::io::Write::flush(&mut std::io::stdout()).ok();
                hooks::run_post(&tc.name, &tc.arguments, &out);
                outputs.insert(tc.id.clone(), out);
            }
        }

        if !delegate_futures.is_empty() {
            println!(
                "{}",
                format!(
                    "{prefix} → running {} delegate/handoff(s) in parallel …",
                    delegate_futures.len()
                )
                .dimmed()
            );
            std::io::Write::flush(&mut std::io::stdout()).ok();
            let results = futures::future::join_all(delegate_futures).await;
            for (id, res) in delegate_ids.into_iter().zip(results) {
                println!("{}", format!("{prefix} ✓ {id} ({})", pretty_bytes(res.len())).dimmed());
                std::io::Write::flush(&mut std::io::stdout()).ok();
                outputs.insert(id, res);
            }
        }

        for tc in &tool_calls {
            let out = outputs
                .remove(&tc.id)
                .unwrap_or_else(|| "error: missing output".to_string());
            if tc.name == "handoff" {
                let src = tc.arguments.get("agent").and_then(|v| v.as_str()).unwrap_or("?");
                println!("{}", format!("{prefix} ↪ handoff from {src} complete").dimmed());
                std::io::Write::flush(&mut std::io::stdout()).ok();
            }
            runner.push(Message::tool(tc.id.clone(), out));
        }
    }

    format!("{prefix} ✗ max turns ({MAX_TURNS}) reached without final answer")
}

fn compact_args(args: &Value) -> String {
    match args {
        Value::Object(map) => {
            let parts: Vec<String> = map
                .iter()
                .map(|(k, v)| {
                    let val = match v {
                        Value::String(s) => {
                            if s.len() > 30 {
                                format!("\"{}...\"", &s[..27])
                            } else {
                                format!("\"{s}\"")
                            }
                        }
                        other => other.to_string(),
                    };
                    format!("{k}={val}")
                })
                .collect();
            parts.join(", ")
        }
        other => truncate(&other.to_string(), 60),
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}...", &s[..n])
    }
}

fn pretty_bytes(n: usize) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    }
}

#[derive(Default)]
struct ToolCallPartAccum {
    id: String,
    name: String,
    arguments: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn edit_retry_matches_retries() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.txt");
        std::fs::write(&p, "hello world").unwrap();
        let args = json!({"path": p.to_str().unwrap(), "old_string": "hello", "new_string": "hi"});
        assert!(matches!(edit_retry(&args, "error: x"), EditRetry::Retry));
    }

    #[test]
    fn edit_retry_mismatch_boosts() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.txt");
        std::fs::write(&p, "actual content").unwrap();
        let args = json!({"path": p.to_str().unwrap(), "old_string": "something else", "new_string": "x"});
        match edit_retry(&args, "error: old_string not found") {
            EditRetry::Boosted(out) => assert!(out.contains("actual content")),
            _ => panic!("expected boosted"),
        }
    }

    #[test]
    fn edit_retry_missing_file_passthrough() {
        let args = json!({"path": "/no/such/path.txt", "old_string": "a", "new_string": "b"});
        match edit_retry(&args, "error: no such file") {
            EditRetry::Ok(out) => assert!(out.starts_with("error:")),
            _ => panic!("expected passthrough"),
        }
    }
}
