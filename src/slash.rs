use serde_json::json;

use crate::types::Tool;

pub fn tool_def() -> Tool {
    let mut available = vec!["/clear", "/compact", "/init", "/help", "/agents", "/skills", "/memory", "/undo", "/audit", "/sessions", "/resume", "/diff", "/kill"];
    for dir in [".opencode/commands", ".zakhar/commands", "commands"] {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().map(|e| e == "md").unwrap_or(false)
                    && let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                        available.push(Box::leak(format!("/{stem}").into_boxed_str()));
                    }
            }
        }
    }
    Tool::function(
        "slash",
        format!(
            "Invoke a slash command as the AI. Available: {}. Use for /clear, /compact, /init, etc. This is the AI side of slash commands; user can also type /cmd directly.",
            available.join(", ")
        ),
        json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Slash command including leading slash, e.g. /clear" },
                    "args": { "type": "string", "description": "Optional args for the command" }
                },
                "required": ["command"]
            }),
        )
}

pub fn handle_user(input: &str, session: &mut crate::session::Session, runner: &mut crate::agent::Runner<'_>) -> Option<String> {
    let input = input.trim();
    if !input.starts_with('/') {
        return None;
    }
    let mut parts = input.splitn(2, ' ');
    let cmd = parts.next().unwrap_or("").trim();
    let args = parts.next().unwrap_or("").trim();
    Some(dispatch(cmd, args, session, Some(runner), true))
}

pub fn handle_ai(command: &str, args: &str, session: &mut crate::session::Session, runner: &mut crate::agent::Runner<'_>) -> String {
    dispatch(command, args, session, Some(runner), false)
}

fn dispatch(cmd: &str, args: &str, session: &mut crate::session::Session, runner: Option<&mut crate::agent::Runner<'_>>, is_user: bool) -> String {
    match cmd {
        "/clear" => {
            let before = session.messages.len();
            session.messages.retain(|m| m.role == crate::types::Role::System);
            let _ = session.save();
            if let Some(r) = runner {
                r.messages_mut().retain(|m| m.role == crate::types::Role::System);
            }
            format!("cleared session, removed {} messages", before - session.messages.len())
        }
        "/compact" => {
            if session.messages.len() <= 4 {
                return "nothing to compact".to_string();
            }
            let keep = 10;
            let compacted = session.messages.len().saturating_sub(keep);
            let kept: Vec<_> = session.messages.iter().rev().take(keep).cloned().collect();
            let mut new_msgs = Vec::new();
            new_msgs.push(crate::types::Message::system(format!(
                "[compact] summarized {compacted} earlier messages (is_user={is_user})"
            )));
            new_msgs.extend(kept.into_iter().rev());
            session.messages = new_msgs.clone();
            if let Some(r) = runner {
                let r_keep = r.messages().len().min(keep + 2);
                let r_kept: Vec<_> = r.messages().iter().rev().take(r_keep).cloned().collect();
                let mut r_new = Vec::new();
                r_new.extend(r.messages().iter().filter(|m| m.role == crate::types::Role::System).cloned());
                r_new.extend(r_kept.into_iter().rev());
                *r.messages_mut() = r_new;
            }
            let _ = session.save();
            format!("compacted {compacted} messages, kept {keep}")
        }
        "/init" => {
            let path = "ZAKHAR.md";
            if std::path::Path::new(path).exists() {
                return format!("{path} already exists");
            }
            let content = format!(
                "# Zakhar Memory\n\nProject: {}\nCreated: {}\n\n## Agents\n{}\n",
                std::env::current_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default(),
                chrono::Utc::now().format("%Y-%m-%d"),
                args
            );
            match std::fs::write(path, content) {
                Ok(_) => format!("created {path}"),
                Err(e) => format!("error creating {path}: {e}"),
            }
        }
        "/kill" => kill_tasks(args),
        "/help" => {
            let mut out = String::new();
            out.push_str("slash commands (user: type /cmd, AI: call slash tool):\n");
            out.push_str("  /clear - clear session history\n");
            out.push_str("  /compact - compact old messages\n");
            out.push_str("  /init - create ZAKHAR.md\n");
            out.push_str("  /help - this help\n");
            out.push_str("  /agents - list agents\n");
            out.push_str("  /skills - list skills (same as skill tool)\n");
            out.push_str("  /memory - browse/search/drop/stale/compact memory\n");
            out.push_str("  /memory mind - trigger a background mind consolidation run\n");
            out.push_str("  /undo [n] - revert the last n mutable tool operations\n");
            out.push_str("  /audit [n] - show the agent ledger of tool operations\n");
            out.push_str("  /sessions - list saved sessions\n");
            out.push_str("  /resume [<id>] - resume a previous session (default: newest)\n");
            out.push_str("  /diff <id1> <id2> - compare two sessions (asks, tools, answers)\n");
            out.push_str("  /kill - kill all background tasks\n");
            out.push_str("  /kill <id> [...] - kill specific task(s)\n");
            for dir in [".opencode/commands", ".zakhar/commands"] {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for e in entries.flatten() {
                        let p = e.path();
                        if p.extension().map(|e| e == "md").unwrap_or(false) {
                            out.push_str(&format!("  /{} (custom from {})\n", p.file_stem().unwrap().to_string_lossy(), dir));
                        }
                    }
                }
            }
            out
        }
        "/agents" => {
            let cfg = crate::config::Config::load().unwrap_or_default();
            if cfg.agents.is_empty() {
                return "no agents configured".to_string();
            }
            let mut out = String::new();
            for (name, ag) in cfg.agents {
                out.push_str(&format!("- {name}: model={} tools={:?}\n  prompt: {}\n", ag.model, ag.tools, ag.prompt.chars().take(80).collect::<String>()));
            }
            out
        }
        "/skills" => {
            let mut out = String::new();
            for dir in [".opencode/skills", ".zakhar/skills", "skills"] {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for e in entries.flatten() {
                        if e.file_type().map(|f| f.is_dir()).unwrap_or(false) {
                            out.push_str(&format!("- {} ({})\n", e.file_name().to_string_lossy(), dir));
                        }
                    }
                }
            }
            if out.is_empty() {
                "no skills found".to_string()
            } else {
                out
            }
        }
        "/memory" => memory(args),
        "/undo" => {
            let n = args.parse().unwrap_or(1);
            crate::ledger::undo(n).unwrap_or_else(|e| format!("error: {e}"))
        }
        "/audit" => {
            let n = args.parse().unwrap_or(20);
            crate::ledger::audit(n)
        }
        "/sessions" => crate::session::list_formatted(),
        "/resume" => {
            if args.is_empty() {
                return match crate::session::last() {
                    Some(id) => {
                        crate::invoke::resume_session(id.clone());
                        format!("resuming newest session {}", &id[..8])
                    }
                    None => "no saved sessions to resume".to_string(),
                };
            }
            match crate::session::find(args) {
                Some(id) => {
                    crate::invoke::resume_session(id.clone());
                    format!("resuming session {}", &id[..8])
                }
                None => format!("no session matches '{args}'"),
            }
        }
        "/diff" => {
            let mut parts = args.split_whitespace();
            let a = parts.next().unwrap_or("").trim();
            let b = parts.next().unwrap_or("").trim();
            if a.is_empty() || b.is_empty() {
                return "usage: /diff <id1> <id2>".to_string();
            }
            crate::session::diff(a, b).unwrap_or_else(|e| format!("error: {e}"))
        }
        _ => {
            let name = cmd.trim_start_matches('/');
            let candidates = [
                format!(".opencode/commands/{}.md", name),
                format!(".zakhar/commands/{}.md", name),
                format!("commands/{}.md", name),
            ];
            for p in candidates {
                if std::path::Path::new(&p).exists()
                    && let Ok(content) = std::fs::read_to_string(&p) {
                        return format!("[slash:{name}]\n{content}\n--- args: {args}");
                    }
            }
            format!("unknown slash command {cmd}. Try /help")
        }
    }
}

fn memory(args: &str) -> String {
    let mut parts = args.splitn(2, ' ');
    let sub = parts.next().unwrap_or("").trim();
    let rest = parts.next().unwrap_or("").trim();

    let events = crate::memory::episodic::block(20);
    let mut out = String::new();

    match sub {
        "" => {
            out.push_str("## knowledge\n");
            let block = crate::memory::knowledge::block(8);
            if block == "no saved knowledge" {
                out.push_str("  (none)\n");
            } else {
                for line in block.lines() {
                    out.push_str(&format!("  {line}\n"));
                }
            }
            out.push_str("## recent events\n");
            out.push_str(&format!("  {}", events.replace('\n', "\n  ")));
        }
        "drop" => {
            if rest.is_empty() {
                return "usage: /memory drop <key-or-id>".to_string();
            }
            match crate::memory::knowledge::remove(rest) {
                Ok(Some(item)) => out.push_str(&format!(
                    "dropped knowledge '{}' ({} bytes)",
                    item.summary,
                    item.detail.as_ref().map(|d| d.len()).unwrap_or(0)
                )),
                Ok(None) => out.push_str(&format!("no knowledge '{rest}'")),
                Err(e) => out.push_str(&format!("error: {e}")),
            }
        }
        "compact" => {
            match crate::memory::episodic::compact() {
                Ok(events) if events.is_empty() => out.push_str("nothing to compact (below threshold)"),
                Ok(events) => out.push_str(&format!("archived {} events; summary agent dispatched in the background", events.len())),
                Err(e) => out.push_str(&format!("error: {e}")),
            }
        }
        "mind" => {
            match crate::memory::mind::dispatch(&std::env::current_dir().unwrap_or_default()) {
                Ok(_) => out.push_str("mind consolidation dispatched in the background"),
                Err(e) => out.push_str(&format!("error: {e}")),
            }
        }
        "stale" => {
            let days = rest.parse().unwrap_or(30);
            let stale = crate::memory::knowledge::stale(days);
            if stale.is_empty() {
                out.push_str(&format!("no stale knowledge (>{days} days since last access)"));
            } else {
                out.push_str(&format!("stale knowledge (>{days} days since last access):\n"));
                for key in stale {
                    out.push_str(&format!("  - {key}\n"));
                }
            }
        }
        "search" => {
            if rest.is_empty() {
                return "usage: /memory search <text>".to_string();
            }
            out.push_str("## knowledge matches\n");
            let store = crate::memory::knowledge::load();
            let hits = crate::memory::recall::remember(rest, &store, 5);
            if hits.is_empty() {
                out.push_str("  (none)\n");
            } else {
                for hit in hits {
                    let open = if hit.loop_open { " (open loop)" } else { "" };
                    let when: String = hit.item.updated.chars().take(16).collect();
                    out.push_str(&format!(
                        "  - {} ({} · {:.2} · {when}){open}\n",
                        hit.item.summary,
                        hit.item.kind,
                        hit.item.salience
                    ));
                }
            }
            out.push_str("## event matches\n");
            let needle = rest.to_lowercase();
            let mut found = false;
            for e in crate::memory::episodic::recent(200) {
                if e.text.to_lowercase().contains(&needle) || e.kind.to_lowercase().contains(&needle) {
                    out.push_str(&format!("  [{}] {}: {}\n", e.ts, e.kind, e.text));
                    found = true;
                }
            }
            if !found {
                out.push_str("  (none)\n");
            }
        }
        other => out.push_str(&format!("unknown /memory subcommand '{other}'. Try /memory, /memory drop <key>, /memory compact, /memory stale [<days>], /memory search <text>, /memory mind")),
    }

    out
}

fn kill_tasks(args: &str) -> String {
    use crate::handler::Handler;
    use crate::tools::Task;
    let task = Task;
    let args = args.trim();
    if args.is_empty() {
        match task.run(&json!({"action": "kill", "kill": "all"})) {
            Ok(out) => out,
            Err(e) => format!("error: {e}"),
        }
    } else {
        let ids: Vec<&str> = args.split_whitespace().collect();
        if ids.len() == 1 {
            match task.run(&json!({"action": "kill", "task_id": ids[0]})) {
                Ok(out) => out,
                Err(e) => format!("error: {e}"),
            }
        } else {
            let id_vals: Vec<serde_json::Value> = ids.iter().map(|s| json!(s)).collect();
            match task.run(&json!({"action": "kill", "task_ids": id_vals})) {
                Ok(out) => out,
                Err(e) => format!("error: {e}"),
            }
        }
    }
}