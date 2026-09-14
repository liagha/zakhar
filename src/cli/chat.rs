//! Interactive chat loop: prompt the user, stream model responses with
//! bounded retries, and execute approved tool calls (read-only ones in
//! parallel), feeding results back until the turn is complete.

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

const STREAM_ATTEMPTS: u32 = 3;
/// When resuming, only the tail of the prior conversation is fed to the model
/// so a long session stays inside the context window. The full history is kept
/// on disk and still saved on exit.
const RESUME_TAIL: usize = 40;

fn load_resumable(id: &str) -> Option<Session> {
    let full = crate::session::find(id)?;
    Session::load(&full).ok()
}

fn push_resumed(session: &Session, runner: &mut Runner<'_>, ui: &mut Ui<'_>) {
    runner.messages_mut().retain(|m| m.role == crate::types::Role::System);
    let tail = session.messages.iter().rev().take(RESUME_TAIL).cloned().collect::<Vec<_>>();
    for msg in tail.into_iter().rev() {
        runner.push(msg);
    }
    ui.note(format!(
        "↩ resumed session {} ({} messages, {} in context)",
        &session.id[..8],
        session.messages.len(),
        session.messages.len().min(RESUME_TAIL)
    ).as_str());
}

#[allow(clippy::too_many_arguments)]
pub async fn chat(
    provider: Option<String>,
    model: Option<String>,
    agent: Option<String>,
    invoke_flag: bool,
    auto_approve: bool,
    plan_mode: bool,
    simple: bool,
    resume: Option<String>,
    seed: String,
) -> anyhow::Result<()> {
    let cfg = Config::load()?;
    let registry = registry::build(&cfg);
    let pal = cfg.palette();
    let mut ui = Ui::new(simple, &pal);

    let heavy = crate::capabilities::resolve(&cfg, "code", "heavy");
    let provider_id = provider.unwrap_or(heavy.provider);

    let agent_cfg = model
        .as_ref()
        .and(None)
        .or(agent.as_ref().and_then(|name| cfg.agents.get(name)));

    let model = model
        .or(agent_cfg.map(|a| a.model.clone()))
        .or((!heavy.model.is_empty()).then_some(heavy.model.clone()))
        .unwrap_or_else(|| {
            registry
                .get(&provider_id)
                .map(|p| p.list_models().first().cloned().unwrap_or_default())
                .unwrap_or_default()
        });

    let primary = crate::levels::Resolved {
        provider: provider_id.clone(),
        model: model.clone(),
    };
    let explicit = cfg
        .capabilities
        .get("code")
        .map(|c| c.fallback.clone())
        .unwrap_or_default();
    let routes = crate::fallback::chain(&cfg, primary, &explicit);
    let decide = if auto_approve {
        crate::fallback::Decide::Auto
    } else {
        crate::fallback::Decide::Ask
    };
    let provider_box = crate::fallback::build(&registry, &routes, decide)?;
    let p: &dyn crate::provider::Provider = provider_box.as_ref();

    let mut invoke = if invoke_flag {
        Some(Invoke::new())
    } else {
        None
    };

    if let Some(inv) = &mut invoke {
        let mounted = inv.mount_servers(&cfg);
        if !mounted.is_empty() {
            ui.note(&format!("mcp: {}", mounted.join(", ")));
        }
    }
    let invoke = invoke.map(std::sync::Arc::new);

    let mut session = Session::new();
    let mut resumed = false;

    match resume.as_deref().filter(|id| !id.trim().is_empty()) {
        Some(id) => {
            session = match load_resumable(id) {
                Some(loaded) => loaded,
                None => return Err(anyhow::anyhow!("no session matches '{id}'")),
            };
            resumed = true;
        }
        // `--resume` with no id means the newest session.
        None if resume.is_some() => {
            if let Some(loaded) = crate::session::last().and_then(|id| Session::load(&id).ok()) {
                session = loaded;
                resumed = true;
            }
        }
        None => {}
    }

    let mut runner = Runner::new(p, model.clone(), agent_cfg);

    for (label, text) in crate::memory::load_blocks() {
        runner.push(crate::types::Message::system(format!("{label}:\n{text}")));
    }

    {
        let persisted = crate::tools::load_persisted_todos();
        if !persisted.is_empty() {
            runner.push(crate::types::Message::system(format!(
                "Persisted todos from previous session:\n{persisted}"
            )));
        }
    }
    if plan_mode {
        runner.push(crate::types::Message::system(
            "PLAN MODE: read-only. Do not use write/edit/bash to modify files. Use todo to plan, ask to clarify, and delegate/handoff to specialists. When plan is complete, summarize without making edits.".to_string(),
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
            tools.retain(|t| crate::invoke::READONLY.contains(&t.function.name.as_str()));
            ui.note(format!("plan mode: tools filtered to {} readonly", tools.len()).as_str());
        }
        runner.set_tools(tools);
    }

    if resumed {
        push_resumed(&session, &mut runner, &mut ui);
    } else {
        for msg in &session.messages {
            runner.push(msg.clone());
        }
    }

    if plan_mode {
        ui.note(format!("zakhar [{provider_id}/{model}] plan ⚑  esc to stop · ctrl+d to exit").as_str());
    } else {
        ui.note(format!("zakhar [{provider_id}/{model}]  esc to stop · ctrl+d to exit").as_str());
    }

    let mut allow_all = false;
    let mut pending: Vec<String> = if seed.trim().is_empty() {
        Vec::new()
    } else {
        vec![seed.trim().to_string()]
    };
    let mut line = String::new();
    loop {
        let text = if !pending.is_empty() {
            pending.remove(0)
        } else if crate::readline::available() {
            ui.clear_line();
            match crate::readline::readline("> ") {
                Some(t) => t,
                None => break,
            }
        } else {
            ui.prompt();
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
            if let Some(resume_id) = crate::invoke::take_resume_session() {
                let _ = session.save();
                match Session::load(&resume_id) {
                    Ok(loaded) => {
                        session = loaded;
                        push_resumed(&session, &mut runner, &mut ui);
                    }
                    Err(e) => ui.err(format!("failed to resume: {e}").as_str()),
                }
            }
            continue;
        }

        let user_msg = crate::types::Message::user(text);
        runner.push(user_msg.clone());
        session.messages.push(user_msg);

        let turn_start = std::time::Instant::now();
        let mut tool_count = 0usize;
        let mut stopped = false;

        let mut attempts = 0u32;
        loop {
            if attempts >= STREAM_ATTEMPTS {
                ui.err(format!("stream failed after {STREAM_ATTEMPTS} attempts, giving up").as_str());
                return Err(anyhow::anyhow!("stream failed after {STREAM_ATTEMPTS} attempts"));
            }
            attempts += 1;
            if attempts == 1 {
                ui.status("…");
            }
            let mut stream = match runner.stream().await {
                Ok(s) => s,
                Err(e) => {
                    ui.status(&format!("↻ retrying stream ({attempts}/{STREAM_ATTEMPTS}): {}", one_line(&format!("{e:#}"))));
                    continue;
                }
            };
            let watch = crate::term::Interrupt::armed();
            ui.reset_reasoning();
            let mut full = String::new();
            let mut tool_parts: HashMap<usize, ToolCallPartAccum> = HashMap::new();
            let mut events_seen = 0usize;
            let mut failed: Option<anyhow::Error> = None;

            while let Some(event) = stream.next().await {
                if watch.take_expand() {
                    ui.expand_reasoning();
                }
                if watch.is_set() {
                    break;
                }
                let event = match event {
                    Ok(ev) => ev,
                    Err(e) => {
                        failed = Some(e);
                        break;
                    }
                };
                match event {
                    crate::provider::ChatStreamEvent::Reasoning(t) => {
                        events_seen += 1;
                        ui.reasoning(&t);
                    }
                    crate::provider::ChatStreamEvent::Text(t) => {
                        events_seen += 1;
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
            let cancelled = watch.is_set();
            drop(watch);
            if cancelled {
                stopped = true;
                ui.note("⏹ stopped by esc");
                break;
            }
            if let Some(e) = failed {
                ui.status(&format!("↻ retrying stream ({attempts}/{STREAM_ATTEMPTS}): {}", one_line(&format!("{e:#}"))));
                continue;
            }
            attempts = 0;
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
            tool_count += tool_calls.len();

            if tool_calls.is_empty() || invoke.is_none() {
                if !full.trim().is_empty()
                    && let Err(e) = crate::memory::episodic::append("chat", &full)
                {
                    ui.note(&format!("[memory] failed to log event: {e}"));
                }
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
                let summary = tool_calls
                    .iter()
                    .map(|tc| format!("{}({})", tc.name, compact_args(&tc.arguments)))
                    .collect::<Vec<_>>()
                    .join(" · ");
                ui.tool_call(&summary);
            }
            ui.end();

            let inv = invoke.as_ref().unwrap();
            let mut denied = false;
            let mut outputs: HashMap<String, String> = HashMap::new();
            let mut delegate_futures: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = String>>>> =
                Vec::new();
            let mut delegate_ids: Vec<String> = Vec::new();
            let mut delegate_kinds: Vec<String> = Vec::new();
            let mut open: Vec<ToolCall> = Vec::new();
            let mut step: Vec<ToolCall> = Vec::new();

            for tc in &tool_calls {
                let approved = if allow_all || auto_approve {
                    true
                } else {
                    let ch = ui.confirm(&format!("{}({})", tc.name, compact_args(&tc.arguments)));
                    match ch {
                        'a' => {
                            allow_all = true;
                            true
                        }
                        'n' => false,
                        _ => true,
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
                } else if crate::invoke::PARALLEL.contains(&tc.name.as_str()) {
                    open.push(tc.clone());
                } else {
                    step.push(tc.clone());
                }
            }

            if denied {
                ui.err("tool calls denied, ending turn");
                break;
            }

            for tc in &step {
                if tc.name == "slash" {
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
                    let preview = out.lines().next().unwrap_or("").chars().take(80).collect::<String>();
                    ui.tool_result(&format!("slash {cmd}"), &preview, out.len());
                    hooks::run_post(&tc.name, &tc.arguments, &out);
                    outputs.insert(tc.id.clone(), out);
                } else if tc.name == "ask" {
                    ui.end();
                    let out = inv.exec("ask", &tc.arguments);
                    let preview: String = out.chars().take(500).collect();
                    ui.tool_result("ask", &preview, out.len());
                    hooks::run_post(&tc.name, &tc.arguments, &out);
                    outputs.insert(tc.id.clone(), out);
                } else if is_action(&tc.name) {
                    let before = action_before(&tc.name, &tc.arguments);
                    let uargs = compact_args(&tc.arguments);
                    ui.action_call(&tc.name, &uargs);
                    let out = inv.exec(&tc.name, &tc.arguments);
                    let preview: String = out.chars().take(500).collect();
                    ui.action_result(&tc.name, &preview, out.len());
                    if let Some((path, old)) = before {
                        let new = std::fs::read_to_string(&path).unwrap_or_default();
                        let d = crate::diff::diff(&old, &new, &path);
                        if !d.is_empty() {
                            ui.diff_block(&d);
                        }
                    }
                    hooks::run_post(&tc.name, &tc.arguments, &out);
                    outputs.insert(tc.id.clone(), out);
                } else {
                    let out = inv.exec(&tc.name, &tc.arguments);
                    let preview: String = out.chars().take(500).collect();
                    ui.tool_result(&tc.name, &preview, out.len());
                    hooks::run_post(&tc.name, &tc.arguments, &out);
                    let skill_msg = if tc.name == "skill"
                        && let Some(name) = tc.arguments.get("name").and_then(|v| v.as_str())
                        && !name.is_empty()
                        && !out.starts_with("error:")
                        && !out.contains("available skills")
                    {
                        Some(format!(
                            "You have loaded the skill '{name}'. Apply its instructions:\n\n{out}"
                        ))
                    } else {
                        None
                    };
                    outputs.insert(tc.id.clone(), out);
                    if let Some(msg) = skill_msg {
                        runner.push(crate::types::Message::system(msg));
                    }
                }
            }

            if !open.is_empty() {
                ui.note(format!("⇉ running {} read-only tool(s) in parallel", open.len()).as_str());
                let results: Vec<String> = {
                    let inv = std::sync::Arc::clone(inv);
                    let jobs: Vec<_> = open
                        .iter()
                        .map(|tc| (tc.name.clone(), tc.arguments.clone()))
                        .collect();
                    tokio::task::spawn_blocking(move || {
                        std::thread::scope(|s| {
                            let handles: Vec<_> = jobs
                                .iter()
                                .map(|(name, args)| {
                                    let inv = std::sync::Arc::clone(&inv);
                                    s.spawn(move || inv.exec(name, args))
                                })
                                .collect();
                            handles
                                .into_iter()
                                .map(|handle| {
                                    handle.join().unwrap_or_else(|_| {
                                        "error: parallel tool panicked".to_string()
                                    })
                                })
                                .collect()
                        })
                    })
                    .await
                    .unwrap_or_else(|_| vec!["error: parallel tool failed".to_string()])
                };
                for (tc, res) in open.iter().zip(results) {
                    let preview: String = res.chars().take(500).collect();
                    ui.tool_result(&tc.name, &preview, res.len());
                    hooks::run_post(&tc.name, &tc.arguments, &res);
                    outputs.insert(tc.id.clone(), res);
                }
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
                    let preview: String = res.chars().take(500).collect();
                    ui.tool_result(&kind, &preview, res.len());
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

            if let Some(resume_id) = crate::invoke::take_resume_session() {
                let _ = session.save();
                match Session::load(&resume_id) {
                    Ok(loaded) => {
                        session = loaded;
                        push_resumed(&session, &mut runner, &mut ui);
                    }
                    Err(e) => ui.err(format!("failed to resume: {e}").as_str()),
                }
            }
        }
        session.save()?;
        let secs = turn_start.elapsed().as_secs_f64();
        ui.summary(&format!(
            "{} · {secs:.1}s · {tool_count} tool(s) · {provider_id}/{model}",
            if stopped { "stopped" } else { "done" }
        ));
        ui.ok(if stopped { "turn cancelled" } else { "turn complete" });
    }
    let _ = crate::memory::mind::dispatch(&crate::paths::home());
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
                            if k == "command" {
                                compact_command(s)
                            } else if s.len() > 40 {
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

fn compact_command(command: &str) -> String {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return "".to_string();
    }
    let last = trimmed
        .rsplit("&&")
        .next()
        .unwrap_or(trimmed)
        .trim()
        .trim_start_matches("cd ")
        .trim();
    if last.is_empty() {
        let short: String = trimmed.chars().take(60).collect();
        format!("\"{short}\"")
    } else {
        let short: String = last.chars().take(60).collect();
        let more = if last != short { "…" } else { "" };
        format!("\"{short}{more}\"")
    }
}

fn one_line(text: &str) -> String {
    let mut out = String::new();
    let mut saw_esc = false;
    for c in text.chars() {
        if c == '\x1b' {
            saw_esc = true;
        }
        if saw_esc {
            if c.is_ascii_alphabetic() {
                saw_esc = false;
            }
            continue;
        }
        if c == '\n' || c == '\r' {
            out.push(' ');
        } else if !c.is_control() {
            out.push(c);
        }
    }
    let collapsed: String = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = collapsed.chars();
    let head: String = chars.by_ref().take(80).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

fn is_action(name: &str) -> bool {
    matches!(name, "write" | "edit" | "bash" | "send_file" | "send_album" | "ban_user" | "unban_user" | "create_group" | "create_channel" | "delete_message" | "delete_messages_bulk" | "delete_chat_history" | "set_default_chat_permissions" | "edit_chat_title" | "edit_chat_photo" | "edit_chat_about" | "promote_admin" | "demote_admin" | "invite_to_group" | "remove_user" | "import_contacts" | "add_contact" | "send_contact" | "send_message" | "reply_to_message" | "forward_message" | "forward_messages" | "send_sticker" | "send_gif" | "send_voice" | "send_scheduled_message" | "delete_scheduled_message" | "pin_message" | "unpin_message" | "unpin_all_messages" | "create_poll" | "block_user" | "unblock_user" | "mute_chat" | "unmute_chat" | "archive_chat" | "unarchive_chat" | "leave_chat" | "join_chat_by_link" | "create_folder" | "delete_folder" | "add_chat_to_folder" | "remove_chat_from_folder" | "save_draft" | "clear_draft" | "toggle_slow_mode" | "enable_forum_topics" | "create_forum_topic" | "set_profile_photo" | "delete_profile_photo" | "update_profile" | "set_bot_commands" | "export_chat_invite" | "import_chat_invite" | "watch" | "task" | "kill")
}

fn action_before(name: &str, args: &serde_json::Value) -> Option<(String, String)> {
    if name != "write" && name != "edit" {
        return None;
    }
    let path = args.get("path").and_then(|v| v.as_str())?;
    let old = std::fs::read_to_string(path).ok()?;
    Some((path.to_string(), old))
}

#[cfg(test)]
mod tests {
    use super::{compact_args, compact_command, one_line};
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
        let args = json!({"path": long});
        let s = compact_args(&args);
        assert!(s.contains("..."));
        assert!(!s.contains(&long));
    }

    #[test]
    fn compact_args_command_uses_command_shortening() {
        let long = "z".repeat(200);
        let args = json!({"command": long});
        let s = compact_args(&args);
        assert!(s.starts_with("command="), "got: {s}");
        assert!(!s.contains(&long));
        assert!(s.contains('…'));
    }

    #[test]
    fn one_line_strips_ansi_and_newlines() {
        assert_eq!(one_line("abc\x1b[31mdef\x1b[0m"), "abcdef");
        assert_eq!(one_line("line1\nline2\r\nline3"), "line1 line2 line3");
        assert_eq!(
            one_line("  spaced   out \t text  "),
            "spaced out text"
        );
    }

    #[test]
    fn one_line_truncates_long() {
        let long = "x".repeat(200);
        let s = one_line(&long);
        assert!(s.len() < 100);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn compact_command_keeps_important_tail() {
        assert_eq!(
            compact_command("cd src && cargo build"),
            "\"cargo build\""
        );
        assert_eq!(
            compact_command("cd src && mkdir -p sub && git add . && git commit -m x"),
            "\"git commit -m x\""
        );
        assert_eq!(compact_command("  git push  "), "\"git push\"");
        assert_eq!(compact_command("   "), "");
    }

    #[test]
    fn compact_command_truncates_long_tail() {
        let long = format!("cd x && {}", "a".repeat(200));
        let s = compact_command(&long);
        assert!(s.len() < 80);
        assert!(s.contains('…'));
        let inner: String = s.trim_matches('"').to_string();
        assert!(inner.ends_with('…'));
    }
}

#[derive(Default)]
struct ToolCallPartAccum {
    id: String,
    name: String,
    arguments: String,
}
