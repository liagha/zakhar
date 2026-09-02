use std::collections::HashMap;

use serde_json::{json, Value};

use crate::agent::Runner;
use crate::config::Config;
use crate::invoke::Invoke;
use crate::provider::Provider;
use crate::types::{Function, Message, Tool, ToolCall};

const MAX_DEPTH: usize = 3;
const MAX_TURNS: usize = 8;

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
    Tool {
        tool_type: "function".to_string(),
        function: Function {
            name: "delegate".to_string(),
            description: format!(
                "Delegate a sub-task to a specialist agent. Available agents: {}. Use when a task is better handled by a specialist. The sub-agent will run autonomously with its own tools and return a result. Multiple delegates in one turn run in parallel.",
                agent_list
            ),
            parameters: json!({
                "type": "object",
                "properties": props,
                "required": ["agent", "task"]
            }),
        },
    }
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
    Tool {
        tool_type: "function".to_string(),
        function: Function {
            name: "handoff".to_string(),
            description: format!(
                "Hand off the conversation to another agent (pipeline). Available: {}. The target agent runs to completion and its answer becomes the final result. Use for sequential pipelines e.g. explorer -> coder -> reviewer. Prefer delegate for parallel sub-tasks.",
                agent_list
            ),
            parameters: json!({
                "type": "object",
                "properties": props,
                "required": ["agent", "task"]
            }),
        },
    }
}

pub async fn run(
    provider: &dyn Provider,
    cfg: &Config,
    agent_name: &str,
    task: &str,
    depth: usize,
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
    } else if let Some(m) = &cfg.default_model {
        m.clone()
    } else {
        provider.list_models().first().cloned().unwrap_or_default()
    };

    let prefix = format!("[zakhar:delegate:{agent_name}:{depth}]");
    println!("{prefix} → spawning agent '{agent_name}' model={model} task=\"{}\"", truncate(task, 80));
    std::io::Write::flush(&mut std::io::stdout()).ok();

    let mut runner = Runner::new(provider, model.clone(), Some(agent_cfg));
    let invoke = Invoke::new();
    let allowed = agent_cfg.tools.as_slice();
    let mut tools = if allowed.is_empty() {
        invoke.definitions()
    } else {
        invoke.filtered_definitions(allowed)
    };
    let delegate_allowed = allowed.is_empty() || allowed.contains(&"delegate".to_string());
    let handoff_allowed = allowed.is_empty() || allowed.contains(&"handoff".to_string());
    if depth + 1 < MAX_DEPTH && !cfg.agents.is_empty() {
        if delegate_allowed {
            tools.push(tool_def(cfg));
        }
        if handoff_allowed {
            tools.push(handoff_tool_def(cfg));
        }
    }
    runner.set_tools(tools);
    runner.push(Message::user(task.to_string()));

    let mut turns = 0;
    while turns < MAX_TURNS {
        turns += 1;
        println!("{prefix} → turn {turns}/{MAX_TURNS} sending request …");
        std::io::Write::flush(&mut std::io::stdout()).ok();

        let stream = match runner.stream().await {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("{prefix} ✗ delegate stream failed: {e}");
                println!("{msg}");
                std::io::Write::flush(&mut std::io::stdout()).ok();
                return msg;
            }
        };

        use futures::StreamExt;
        let mut full = String::new();
        let mut saw_reasoning = false;
        let mut had_reasoning = false;
        let mut tool_parts: HashMap<usize, ToolCallPartAccum> = HashMap::new();
        let mut events_seen = 0usize;

        {
            let mut stream = stream;
            while let Some(event) = stream.next().await {
                let event = match event {
                    Ok(ev) => ev,
                    Err(e) => {
                        println!("{prefix} ✗ stream error: {e}");
                        std::io::Write::flush(&mut std::io::stdout()).ok();
                        return format!("{prefix} stream error: {e}");
                    }
                };
                match event {
                    crate::provider::ChatStreamEvent::Reasoning(t) => {
                        if events_seen == 0 {
                            println!("{prefix} … receiving (reasoning) …");
                            std::io::Write::flush(&mut std::io::stdout()).ok();
                        }
                        events_seen += 1;
                        saw_reasoning = true;
                        print!("{}", t);
                        std::io::Write::flush(&mut std::io::stdout()).ok();
                    }
                    crate::provider::ChatStreamEvent::Text(t) => {
                        if events_seen == 0 {
                            println!("{prefix} … receiving (content) …");
                            std::io::Write::flush(&mut std::io::stdout()).ok();
                        }
                        events_seen += 1;
                        if saw_reasoning && !had_reasoning {
                            had_reasoning = true;
                            println!();
                        }
                        print!("{t}");
                        std::io::Write::flush(&mut std::io::stdout()).ok();
                        full.push_str(&t);
                    }
                    crate::provider::ChatStreamEvent::ToolCall(part) => {
                        if events_seen == 0 {
                            println!("{prefix} … receiving (tool_call) …");
                            std::io::Write::flush(&mut std::io::stdout()).ok();
                        }
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
            println!("\n{prefix} … stream ended with no content");
        } else {
            println!("\n{prefix} ✓ stream done ({} events, {} chars)", events_seen, full.len());
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
            println!("{prefix} ✓ delegate '{agent_name}' complete ({} chars)", full.len());
            std::io::Write::flush(&mut std::io::stdout()).ok();
            runner.push(Message::assistant(full.clone(), None));
            return full;
        }

        println!(
            "{prefix} → {} tool call(s): {}",
            tool_calls.len(),
            tool_calls
                .iter()
                .map(|tc| format!("{}({})", tc.name, compact_args(&tc.arguments)))
                .collect::<Vec<_>>()
                .join(", ")
        );
        std::io::Write::flush(&mut std::io::stdout()).ok();

        runner.push(Message::assistant(full.clone(), Some(tool_calls.clone())));

        let mut outputs: HashMap<String, String> = HashMap::new();
        let mut delegate_futures: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = String>>>> =
            Vec::new();
        let mut delegate_ids: Vec<String> = Vec::new();

        for tc in &tool_calls {
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
                    let id_clone = tc.id.clone();
                    delegate_ids.push(id_clone);
                    delegate_futures.push(Box::pin(async move {
                        run(prov_copy, &cfg_clone, &sub_agent, &sub_task, depth_next).await
                    }));
                }
            } else {
                println!("{prefix} → invoke({}) …", tc.name);
                std::io::Write::flush(&mut std::io::stdout()).ok();
                let out = invoke.exec(&tc.name, &tc.arguments);
                let preview = truncate(&out, 300);
                println!("{prefix} ← invoke({}) done: {preview}", tc.name);
                std::io::Write::flush(&mut std::io::stdout()).ok();
                outputs.insert(tc.id.clone(), out);
            }
        }

        if !delegate_futures.is_empty() {
            println!(
                "{prefix} → running {} delegate/handoff(s) in parallel …",
                delegate_futures.len()
            );
            std::io::Write::flush(&mut std::io::stdout()).ok();
            let results = futures::future::join_all(delegate_futures).await;
            for (id, res) in delegate_ids.into_iter().zip(results) {
                println!("{prefix} ← delegate {id} done ({} bytes)", res.len());
                std::io::Write::flush(&mut std::io::stdout()).ok();
                outputs.insert(id, res);
            }
        }

        for tc in &tool_calls {
            let out = outputs
                .remove(&tc.id)
                .unwrap_or_else(|| "error: missing output".to_string());
            // handoff semantics: if was handoff, its output becomes the pipeline result
            // we still push as tool result; the outer loop will feed it back, but we log it
            if tc.name == "handoff" {
                println!("{prefix} ↪ handoff from {} complete", tc.arguments.get("agent").and_then(|v| v.as_str()).unwrap_or("?"));
                std::io::Write::flush(&mut std::io::stdout()).ok();
            }
            runner.push(Message::tool(tc.id.clone(), out));
        }
    }

    format!("{prefix} error: max turns ({MAX_TURNS}) reached without final answer")
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

#[derive(Default)]
struct ToolCallPartAccum {
    id: String,
    name: String,
    arguments: String,
}
