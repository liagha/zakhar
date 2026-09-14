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
            out.push_str("  /resume [<id>] - resume a previous session (shows picker if no id)\n");
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
                let sessions = crate::session::list();
                if sessions.is_empty() {
                    return "no saved sessions to resume".to_string();
                }
                let mut out = String::from("saved sessions:\n");
                for (i, s) in sessions.iter().enumerate() {
                    let date = chrono::DateTime::parse_from_rfc3339(&s.created_at)
                        .ok()
                        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| s.created_at.clone());
                    let label = if s.title.is_empty() {
                        format!("{} messages)", s.message_count)
                    } else {
                        format!("{} ({} messages)", s.title, s.message_count)
                    };
                    out.push_str(&format!("  {}. {} — {} — {}\n", i + 1, &s.id[..8], date, label));
                }
                if is_user {
                    print!("{out}");
                    print!("\npick session (1-{}) or Enter for newest: ", sessions.len());
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                    let mut input = String::new();
                    if std::io::stdin().read_line(&mut input).is_ok() {
                        let input = input.trim();
                        if input.is_empty() {
                            let id = sessions.first().unwrap().id.clone();
                            crate::invoke::resume_session(id.clone());
                            return format!("resuming newest session {}", &id[..8]);
                        }
                        if let Ok(n) = input.parse::<usize>()
                            && n >= 1
                            && n <= sessions.len()
                        {
                            let id = sessions[n - 1].id.clone();
                            crate::invoke::resume_session(id.clone());
                            return format!("resuming session {}", &id[..8]);
                        }
                        return format!("invalid choice '{input}', enter 1-{}", sessions.len());
                    }
                }
                let id = sessions.first().unwrap().id.clone();
                crate::invoke::resume_session(id.clone());
                return format!("resuming newest session {}", &id[..8]);
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
            match crate::memory::mind::dispatch(&crate::paths::home()) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::Runner;
    use crate::provider::mock::Script;
    use crate::types::{Message, Role};
    use serde_json::json;

    fn tmp() -> (tempfile::TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = crate::memory::lock();
        let dir = tempfile::tempdir().unwrap();
        crate::paths::set_home(dir.path().join(".zakhar"));
        (dir, guard)
    }

    fn runner(provider: &Script) -> Runner<'_> {
        Runner::new(provider, provider.name.clone(), None)
    }

    fn session_with(count: usize) -> crate::session::Session {
        let mut s = crate::session::Session::new();
        s.messages.push(Message::system("sys"));
        for i in 0..count {
            s.messages.push(Message::user(format!("u{i}")));
            s.messages.push(Message::assistant(format!("a{i}"), None));
        }
        s
    }

    #[test]
    fn plain_input_returns_none() {
        let (_dir, _g) = tmp();
        let provider = Script { name: "m".into(), answer: "ok".into() };
        let mut r = runner(&provider);
        let mut s = crate::session::Session::new();
        assert!(handle_user("hello world", &mut s, &mut r).is_none());
    }

    #[test]
    fn clear_keeps_system_only() {
        let (_dir, _g) = tmp();
        let provider = Script { name: "m".into(), answer: "ok".into() };
        let mut r = runner(&provider);
        r.push(Message::system("bot sys"));
        r.push(Message::user("user ask"));
        let mut s = crate::session::Session::new();
        s.messages.push(Message::system("sys"));
        s.messages.push(Message::user("hello"));
        s.messages.push(Message::assistant("hi", None));
        let out = handle_user("/clear", &mut s, &mut r).unwrap();
        assert_eq!(out, "cleared session, removed 2 messages");
        assert_eq!(s.messages.len(), 1);
        assert_eq!(r.messages().len(), 1);
        assert!(crate::session::list().iter().any(|i| i.id == s.id));
    }

    #[test]
    fn compact_small_session_is_noop() {
        let (_dir, _g) = tmp();
        let provider = Script { name: "m".into(), answer: "ok".into() };
        let mut r = runner(&provider);
        let mut s = session_with(1);
        let out = handle_user("/compact", &mut s, &mut r).unwrap();
        assert_eq!(out, "nothing to compact");
        assert_eq!(s.messages.len(), 3);
    }

    #[test]
    fn compact_keeps_tail_and_marks_summary() {
        let (_dir, _g) = tmp();
        let provider = Script { name: "m".into(), answer: "ok".into() };
        let mut r = runner(&provider);
        let mut s = session_with(12);
        let out = handle_user("/compact", &mut s, &mut r).unwrap();
        assert_eq!(out, "compacted 15 messages, kept 10");
        assert_eq!(s.messages.len(), 11);
        assert_eq!(s.messages[0].role, Role::System);
        assert!(s.messages[0].content.contains("summarized 15"), "got: {}", s.messages[0].content);
    }

    #[test]
    fn unknown_command_reports_help() {
        let (_dir, _g) = tmp();
        let provider = Script { name: "m".into(), answer: "ok".into() };
        let mut r = runner(&provider);
        let mut s = crate::session::Session::new();
        let out = handle_user("/bogus", &mut s, &mut r).unwrap();
        assert_eq!(out, "unknown slash command /bogus. Try /help");
    }

    #[test]
    fn help_lists_commands() {
        let (_dir, _g) = tmp();
        let provider = Script { name: "m".into(), answer: "ok".into() };
        let mut r = runner(&provider);
        let mut s = crate::session::Session::new();
        let out = handle_user("/help", &mut s, &mut r).unwrap();
        assert!(out.contains("/clear"), "got: {out}");
        assert!(out.contains("/undo"), "got: {out}");
        assert!(out.contains("/diff"), "got: {out}");
    }

    #[test]
    fn agents_without_config_says_none() {
        let (_dir, _g) = tmp();
        let provider = Script { name: "m".into(), answer: "ok".into() };
        let mut r = runner(&provider);
        let mut s = crate::session::Session::new();
        let out = handle_user("/agents", &mut s, &mut r).unwrap();
        assert_eq!(out, "no agents configured");
    }

    #[test]
    fn resume_without_sessions_says_none() {
        let (_dir, _g) = tmp();
        let provider = Script { name: "m".into(), answer: "ok".into() };
        let mut r = runner(&provider);
        let mut s = crate::session::Session::new();
        let out = handle_user("/resume", &mut s, &mut r).unwrap();
        assert_eq!(out, "no saved sessions to resume");
    }

    #[test]
    fn resume_finds_by_prefix() {
        let (_dir, _g) = tmp();
        let mut saved = crate::session::Session::new();
        saved.id = "cafe0000-0000-0000-0000-000000000000".to_string();
        saved.messages.push(Message::user("hello"));
        saved.save().unwrap();
        let provider = Script { name: "m".into(), answer: "ok".into() };
        let mut r = runner(&provider);
        let mut s = crate::session::Session::new();
        let out = handle_user("/resume cafe", &mut s, &mut r).unwrap();
        assert_eq!(out, "resuming session cafe0000");
        assert_eq!(
            crate::invoke::take_resume_session(),
            Some(saved.id.clone())
        );
    }

    #[test]
    fn resume_unknown_prefix_reports() {
        let (_dir, _g) = tmp();
        let provider = Script { name: "m".into(), answer: "ok".into() };
        let mut r = runner(&provider);
        let mut s = crate::session::Session::new();
        let out = handle_user("/resume zzz", &mut s, &mut r).unwrap();
        assert_eq!(out, "no session matches 'zzz'");
    }

    #[test]
    fn sessions_lists_saved() {
        let (_dir, _g) = tmp();
        let provider = Script { name: "m".into(), answer: "ok".into() };
        let mut r = runner(&provider);
        let mut s = crate::session::Session::new();
        assert_eq!(handle_user("/sessions", &mut s, &mut r).unwrap(), "no saved sessions");
        let mut saved = crate::session::Session::new();
        saved.id = "12345678-0000-0000-0000-000000000000".to_string();
        saved.save().unwrap();
        let out = handle_user("/sessions", &mut s, &mut r).unwrap();
        assert!(out.contains("saved sessions:"), "got: {out}");
        assert!(out.contains("12345678"), "got: {out}");
    }

    #[test]
    fn undo_reverts_latest_operation() {
        let (dir, _g) = tmp();
        let path = dir.path().join("f.txt");
        std::fs::write(&path, "v1").unwrap();
        let revert = crate::ledger::snapshot(path.to_str().unwrap()).unwrap();
        crate::ledger::record("write", &json!({"path": path}), "wrote v2", Some(revert)).unwrap();
        std::fs::write(&path, "v2").unwrap();
        let provider = Script { name: "m".into(), answer: "ok".into() };
        let mut r = runner(&provider);
        let mut s = crate::session::Session::new();
        let out = handle_user("/undo", &mut s, &mut r).unwrap();
        assert_eq!(out, "reverted 1 operation(s)");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "v1");
    }

    #[test]
    fn audit_shows_ledger() {
        let (_dir, _g) = tmp();
        crate::ledger::record("bash", &json!({"command": "echo"}), "ok", None).unwrap();
        let provider = Script { name: "m".into(), answer: "ok".into() };
        let mut r = runner(&provider);
        let mut s = crate::session::Session::new();
        let out = handle_user("/audit", &mut s, &mut r).unwrap();
        assert!(out.contains("bash"), "got: {out}");
    }

    #[test]
    fn diff_requires_and_compares() {
        let (_dir, _g) = tmp();
        let provider = Script { name: "m".into(), answer: "ok".into() };
        let mut r = runner(&provider);
        let mut s = crate::session::Session::new();
        assert_eq!(handle_user("/diff", &mut s, &mut r).unwrap(), "usage: /diff <id1> <id2>");
        let mut a = crate::session::Session::new();
        a.id = "aaaa0000-0000-0000-0000-000000000000".to_string();
        a.created_at = "2026-01-01T00:00:00Z".to_string();
        a.messages.push(Message::user("first ask"));
        a.save().unwrap();
        let mut b = crate::session::Session::new();
        b.id = "bbbb0000-0000-0000-0000-000000000000".to_string();
        b.created_at = "2026-06-01T00:00:00Z".to_string();
        b.messages.push(Message::user("second ask"));
        b.save().unwrap();
        let out = handle_user("/diff aaaa bbbb", &mut s, &mut r).unwrap();
        assert!(out.contains("session diff:"), "got: {out}");
        assert!(out.contains("asked: first ask"), "got: {out}");
        assert!(out.contains("asked: second ask"), "got: {out}");
        let bad = handle_user("/diff aaaa zzzz", &mut s, &mut r).unwrap();
        assert!(bad.starts_with("error:"), "got: {bad}");
    }

    #[test]
    fn memory_empty_shows_none() {
        let (dir, _g) = tmp();
        crate::memory::knowledge::set_path(dir.path().join("memory/knowledge.jsonl"));
        crate::memory::set_path(dir.path().join("memory/episodic.jsonl"));
        let provider = Script { name: "m".into(), answer: "ok".into() };
        let mut r = runner(&provider);
        let mut s = crate::session::Session::new();
        let out = handle_user("/memory", &mut s, &mut r).unwrap();
        assert!(out.contains("## knowledge"), "got: {out}");
        assert!(out.contains("(none)"), "got: {out}");
        assert!(out.contains("## recent events"), "got: {out}");
    }

    #[test]
    fn memory_drop_removes_saved() {
        let (dir, _g) = tmp();
        crate::memory::knowledge::set_path(dir.path().join("memory/knowledge.jsonl"));
        crate::memory::set_path(dir.path().join("memory/episodic.jsonl"));
        crate::memory::knowledge::save_pair("deploy key", "in vault", "test").unwrap();
        let provider = Script { name: "m".into(), answer: "ok".into() };
        let mut r = runner(&provider);
        let mut s = crate::session::Session::new();
        let out = handle_user("/memory drop deploy key", &mut s, &mut r).unwrap();
        assert!(out.contains("dropped knowledge 'deploy key'"), "got: {out}");
        assert!(crate::memory::knowledge::load().is_empty());
        let missing = handle_user("/memory drop nothing", &mut s, &mut r).unwrap();
        assert_eq!(missing, "no knowledge 'nothing'");
    }

    #[test]
    fn memory_search_finds_item() {
        let (dir, _g) = tmp();
        crate::memory::knowledge::set_path(dir.path().join("memory/knowledge.jsonl"));
        crate::memory::set_path(dir.path().join("memory/episodic.jsonl"));
        crate::memory::knowledge::save_pair("router ip", "192.168.1.1", "test").unwrap();
        let provider = Script { name: "m".into(), answer: "ok".into() };
        let mut r = runner(&provider);
        let mut s = crate::session::Session::new();
        let out = handle_user("/memory search router", &mut s, &mut r).unwrap();
        assert!(out.contains("## knowledge matches"), "got: {out}");
        assert!(out.contains("router ip"), "got: {out}");
        assert!(out.contains("## event matches"), "got: {out}");
    }

    #[test]
    fn memory_stale_reports_none_fresh() {
        let (dir, _g) = tmp();
        crate::memory::knowledge::set_path(dir.path().join("memory/knowledge.jsonl"));
        crate::memory::set_path(dir.path().join("memory/episodic.jsonl"));
        crate::memory::knowledge::save_pair("fresh fact", "x", "test").unwrap();
        let provider = Script { name: "m".into(), answer: "ok".into() };
        let mut r = runner(&provider);
        let mut s = crate::session::Session::new();
        let out = handle_user("/memory stale 0", &mut s, &mut r).unwrap();
        assert!(out.contains("no stale knowledge"), "got: {out}");
    }

    #[test]
    fn memory_mind_dispatches() {
        let (_dir, _g) = tmp();
        let provider = Script { name: "m".into(), answer: "ok".into() };
        let mut r = runner(&provider);
        let mut s = crate::session::Session::new();
        let out = handle_user("/memory mind", &mut s, &mut r).unwrap();
        assert_eq!(out, "mind consolidation dispatched in the background");
    }

    #[test]
    fn init_creates_memory_file() {
        let (dir, _g) = tmp();
        let orig = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let provider = Script { name: "m".into(), answer: "ok".into() };
            let mut r = runner(&provider);
            let mut s = crate::session::Session::new();
            let out = handle_user("/init", &mut s, &mut r).unwrap();
            assert_eq!(out, "created ZAKHAR.md");
            assert!(dir.path().join("ZAKHAR.md").exists());
            let again = handle_user("/init", &mut s, &mut r).unwrap();
            assert_eq!(again, "ZAKHAR.md already exists");
        }));
        let _ = std::env::set_current_dir(&orig);
        result.unwrap();
    }

    #[test]
    fn custom_command_files_win() {
        let (dir, _g) = tmp();
        let orig = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            std::fs::create_dir_all(".opencode/commands").unwrap();
            std::fs::write(".opencode/commands/greet.md", "say hi to the user").unwrap();
            let provider = Script { name: "m".into(), answer: "ok".into() };
            let mut r = runner(&provider);
            let mut s = crate::session::Session::new();
            let out = handle_user("/greet bob", &mut s, &mut r).unwrap();
            assert!(out.contains("[slash:greet]"), "got: {out}");
            assert!(out.contains("say hi to the user"), "got: {out}");
            assert!(out.contains("--- args: bob"), "got: {out}");
        }));
        let _ = std::env::set_current_dir(&orig);
        result.unwrap();
    }
}