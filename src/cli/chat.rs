use std::collections::HashMap;

use futures::StreamExt;

use crate::agent::Runner;
use crate::config::Config;
use crate::delegate;
use crate::hooks;
use crate::invoke::Invoke;
use crate::registry;
use crate::session::Session;
use crate::slash;
use crate::types::ToolCall;
use crate::ui::Ui;

#[allow(clippy::too_many_arguments)]
pub async fn chat(
    provider: Option<String>,
    model: Option<String>,
    agent: Option<String>,
    invoke_flag: bool,
    auto_approve: bool,
    plan_mode: bool,
    simple: bool,
    seed: String,
) -> anyhow::Result<()> {
    let cfg = Config::load()?;
    let registry = registry::build(&cfg);
    let mut ui = Ui::new(simple);

    let provider_id = provider.unwrap_or_else(|| registry::default_provider(&cfg));
    let p = registry
        .get(&provider_id)
        .ok_or_else(|| anyhow::anyhow!("unknown provider: {provider_id}"))?;

    let agent_cfg = model
        .as_ref()
        .and(None)
        .or(agent.as_ref().and_then(|name| cfg.agents.get(name)));

    let model = model
        .or(agent_cfg.map(|a| a.model.clone()))
        .unwrap_or_else(|| p.list_models().first().cloned().unwrap_or_default());

    let invoke = if invoke_flag {
        Some(Invoke::new())
    } else {
        None
    };

    let mut session = Session::new();
    let mut runner = Runner::new(p, model.clone(), agent_cfg);

    if let Some(mem) = crate::memory::load() {
        runner.push(crate::types::Message::system(format!("Project memory:\n{mem}")));
    }
    if plan_mode {
        runner.push(crate::types::Message::system(
            "PLAN MODE: read-only. Do not use write/edit/bash to modify files. Use todowrite to plan, ask_user to clarify, and delegate/handoff to specialists. When plan is complete, summarize without making edits.".to_string(),
        ));
        ui.note("⚑ plan mode: read-only");
    }

    if let Some(inv) = &invoke {
        let allowed = agent_cfg.map(|a| a.tools.as_slice()).unwrap_or(&[]);
        let mut tools = if allowed.is_empty() {
            inv.definitions()
        } else {
            inv.filtered_definitions(allowed)
        };
        let delegate_allowed = allowed.is_empty() || allowed.contains(&"delegate".to_string());
        let handoff_allowed = allowed.is_empty() || allowed.contains(&"handoff".to_string());
        let slash_allowed = allowed.is_empty() || allowed.contains(&"slash".to_string());
        if !cfg.agents.is_empty() {
            if delegate_allowed {
                tools.push(delegate::tool_def(&cfg));
            }
            if handoff_allowed {
                tools.push(delegate::handoff_tool_def(&cfg));
            }
        }
        if slash_allowed {
            tools.push(slash::tool_def());
        }
        if plan_mode {
            tools.retain(|t| crate::invoke::READONLY_TOOLS.contains(&t.function.name.as_str()));
            ui.note(format!("plan mode: tools filtered to {} readonly", tools.len()).as_str());
        }
        runner.set_tools(tools);
    }

    for msg in &session.messages {
        runner.push(msg.clone());
    }

    if plan_mode {
        ui.note(format!("zakhar [{provider_id}/{model}] plan ⚑  ctrl+d to exit").as_str());
    } else {
        ui.note(format!("zakhar [{provider_id}/{model}]  ctrl+d to exit").as_str());
    }

    let mut allow_all = false;
    let mut pending: Vec<String> = if seed.trim().is_empty() {
        Vec::new()
    } else {
        vec![seed.trim().to_string()]
    };
    let mut line = String::new();
    loop {
        ui.prompt();
        let text = if !pending.is_empty() {
            pending.remove(0)
        } else {
            line.clear();
            let read = std::io::stdin().read_line(&mut line)?;
            if read == 0 {
                break;
            }
            line.trim().to_string()
        };
        if text.is_empty() {
            continue;
        }
        if let Some(out) = slash::handle_user(&text, &mut session, &mut runner) {
            ui.note(out.as_str());
            continue;
        }

        let user_msg = crate::types::Message::user(text);
        runner.push(user_msg.clone());
        session.messages.push(user_msg);

        loop {
            ui.status("…");
            let mut stream = match runner.stream().await {
                Ok(s) => s,
                Err(e) => {
                    ui.err(format!("failed to start stream: {e}").as_str());
                    return Err(e);
                }
            };
            let mut full = String::new();
            let mut saw_reasoning = false;
            let mut had_reasoning = false;
            let mut tool_parts: HashMap<usize, ToolCallPartAccum> = HashMap::new();
            let mut events_seen = 0usize;

            while let Some(event) = stream.next().await {
                let event = match event {
                    Ok(ev) => ev,
                    Err(e) => {
                        ui.err(format!("stream error: {e}").as_str());
                        return Err(e);
                    }
                };
                match event {
                    crate::provider::ChatStreamEvent::Reasoning(t) => {
                        events_seen += 1;
                        saw_reasoning = true;
                        ui.reasoning(&t);
                    }
                    crate::provider::ChatStreamEvent::Text(t) => {
                        events_seen += 1;
                        if saw_reasoning && !had_reasoning {
                            had_reasoning = true;
                        }
                        full.push_str(&t);
                        ui.text(&t);
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
            if events_seen == 0 {
                ui.note("stream ended with no content");
            }

            let tool_calls: Vec<ToolCall> = tool_parts
                .into_iter()
                .collect::<Vec<_>>()
                .into_iter()
                .filter_map(|(_, acc)| {
                    let args: serde_json::Value =
                        serde_json::from_str(&acc.arguments).unwrap_or(
                            serde_json::Value::Object(serde_json::Map::new()),
                        );
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

            if tool_calls.is_empty() || invoke.is_none() {
                runner.push(crate::types::Message::assistant(full.clone(), None));
                session
                    .messages
                    .push(crate::types::Message::assistant(full, None));
                ui.end();
                break;
            }

            runner.push(crate::types::Message::assistant(
                full.clone(),
                Some(tool_calls.clone()),
            ));
            session.messages.push(crate::types::Message::assistant(
                full,
                Some(tool_calls.clone()),
            ));

            if !tool_calls.is_empty() {
                ui.note(
                    format!(
                        "→ {}: {}",
                        tool_calls.len(),
                        tool_calls
                            .iter()
                            .map(|tc| format!("{}({})", tc.name, compact_args(&tc.arguments)))
                            .collect::<Vec<_>>()
                            .join(" · ")
                    )
                    .as_str(),
                );
            }
            ui.end();

            let inv = invoke.as_ref().unwrap();
            let mut denied = false;
            let mut outputs: HashMap<String, String> = HashMap::new();
            let mut delegate_futures: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = String>>>> =
                Vec::new();
            let mut delegate_ids: Vec<String> = Vec::new();
            let mut delegate_kinds: Vec<String> = Vec::new();

            for tc in &tool_calls {
                let approved = if allow_all || auto_approve {
                    true
                } else {
                    ui.note(format!("invoke:{}({}) — approve? [y/n/a(all)]", tc.name, compact_args(&tc.arguments)).as_str());
                    let mut resp = String::new();
                    std::io::stdin().read_line(&mut resp)?;
                    let resp = resp.trim().to_lowercase();
                    match resp.as_str() {
                        "a" | "all" => {
                            allow_all = true;
                            true
                        }
                        "y" | "yes" | "" => true,
                        _ => false,
                    }
                };

                if !approved {
                    runner.push(crate::types::Message::tool(
                        tc.id.clone(),
                        "tool call denied by user".to_string(),
                    ));
                    session.messages.push(crate::types::Message::tool(
                        tc.id.clone(),
                        "tool call denied by user".to_string(),
                    ));
                    denied = true;
                    break;
                }

                if let Err(e) = hooks::run_pre(&tc.name, &tc.arguments) {
                    ui.err(format!("pre-hook blocked {}: {e}", tc.name).as_str());
                    outputs.insert(tc.id.clone(), format!("blocked by pre-hook: {e}"));
                    continue;
                }

                if tc.name == "delegate" || tc.name == "handoff" {
                    let agent = tc
                        .arguments
                        .get("agent")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let task = tc
                        .arguments
                        .get("task")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let kind = tc.name.clone();
                    if agent.is_empty() || task.is_empty() {
                        outputs.insert(
                            tc.id.clone(),
                            format!("error: {kind} requires 'agent' and 'task', got {}", tc.arguments),
                        );
                    } else {
                        let cfg_clone = cfg.clone();
                        let prov_copy: &dyn crate::provider::Provider = p;
                        let agent_c = agent.clone();
                        let task_c = task.clone();
                        let plan_copy = plan_mode;
                        delegate_ids.push(tc.id.clone());
                        delegate_kinds.push(kind.clone());
                        delegate_futures.push(Box::pin(async move {
                            let res = delegate::run(prov_copy, &cfg_clone, &agent_c, &task_c, 0, plan_copy).await;
                            hooks::run_post(&kind, &serde_json::json!({"agent": agent_c, "task": task_c}), &res);
                            res
                        }));
                    }
                } else if tc.name == "slash" {
                    let cmd = tc
                        .arguments
                        .get("command")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let args = tc
                        .arguments
                        .get("args")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let out = slash::handle_ai(cmd, args, &mut session, &mut runner);
                    ui.note(
                        format!(
                            "✓ slash {cmd} {}",
                            out.lines().next().unwrap_or("").chars().take(80).collect::<String>()
                        )
                        .as_str(),
                    );
                    hooks::run_post(&tc.name, &tc.arguments, &out);
                    outputs.insert(tc.id.clone(), out);
                } else {
                    let out = inv.exec(&tc.name, &tc.arguments);
                    let preview: String = out.chars().take(500).collect();
                    if out.len() > 500 {
                        ui.note(format!("✓ {}({} bytes): {} …", tc.name, out.len(), preview).as_str());
                    } else {
                        ui.note(format!("✓ {}: {}", tc.name, preview).as_str());
                    }
                    hooks::run_post(&tc.name, &tc.arguments, &out);
                    outputs.insert(tc.id.clone(), out);
                }
            }

            if denied {
                ui.err("tool calls denied, ending turn");
                break;
            }

            if !delegate_futures.is_empty() {
                let has_handoff = delegate_kinds.iter().any(|k| k == "handoff");
                ui.note(
                    format!(
                        "→ running {} delegate/handoff(s) in parallel …",
                        delegate_futures.len()
                    )
                    .as_str(),
                );
                let results = futures::future::join_all(delegate_futures).await;
                for ((id, kind), res) in delegate_ids.into_iter().zip(delegate_kinds).zip(results) {
                    ui.note(format!("✓ {kind} {id} ({})", pretty_bytes(res.len())).as_str());
                    outputs.insert(id, res);
                }
                if has_handoff {
                    ui.note("↪ handoff complete, pipeline will continue");
                }
            }

            for tc in &tool_calls {
                let out = outputs
                    .remove(&tc.id)
                    .unwrap_or_else(|| "error: missing output".to_string());
                runner.push(crate::types::Message::tool(tc.id.clone(), out));
                session.messages.push(crate::types::Message::tool(
                    tc.id.clone(),
                    "(tool result in context)".to_string(),
                ));
            }

            ui.note("↻ feeding tool results back, continuing loop …");
        }
        session.save()?;
        ui.ok("turn complete");
    }
    Ok(())
}

fn compact_args(args: &serde_json::Value) -> String {
    match args {
        serde_json::Value::Object(map) => {
            let parts: Vec<String> = map
                .iter()
                .map(|(k, v)| {
                    let val = match v {
                        serde_json::Value::String(s) => {
                            if s.len() > 40 {
                                format!("\"{}...\"", &s[..37])
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
        other => other.to_string(),
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

#[cfg(test)]
mod tests {
    use super::compact_args;
    use serde_json::json;

    #[test]
    fn compact_args_object() {
        let args = json!({"command": "echo hello", "dir": "/tmp"});
        let s = compact_args(&args);
        assert!(s.contains("command=\"echo hello\""));
        assert!(s.contains("dir=\"/tmp\""));
    }

    #[test]
    fn compact_args_truncates() {
        let long = "a".repeat(50);
        let args = json!({"command": long});
        let s = compact_args(&args);
        assert!(s.contains("..."));
        assert!(!s.contains(&long));
    }
}

#[derive(Default)]
struct ToolCallPartAccum {
    id: String,
    name: String,
    arguments: String,
}
