use std::path::PathBuf;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::{Message, Role};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub created_at: String,
    pub messages: Vec<Message>,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            created_at: Utc::now().to_rfc3339(),
            messages: Vec::new(),
        }
    }

    pub fn load(id: &str) -> anyhow::Result<Self> {
        let path = dir()?.join(format!("{id}.json"));
        let text = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let dir = dir()?;
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.json", self.id));
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, text)?;
        Ok(())
    }

    pub fn first_user(&self) -> String {
        self.messages
            .iter()
            .find(|m| m.role == Role::User)
            .map(|m| m.content.trim().replace('\n', " "))
            .unwrap_or_default()
    }

    pub fn last_assistant(&self) -> String {
        self.messages
            .iter()
            .rev()
            .find(|m| m.role == Role::Assistant && !m.content.trim().is_empty())
            .map(|m| m.content.trim().to_string())
            .unwrap_or_default()
    }

    pub fn tool_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .messages
            .iter()
            .filter_map(|m| m.tool_calls.as_ref())
            .flatten()
            .map(|t| t.name.clone())
            .filter(|n| !n.is_empty())
            .collect();
        names.sort();
        names.dedup();
        names
    }
}

pub fn dir() -> anyhow::Result<PathBuf> {
    Ok(crate::paths::sessions_dir())
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub created_at: String,
    pub message_count: usize,
}

pub fn list() -> Vec<SessionInfo> {
    let dir = match dir() {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let mut sessions: Vec<SessionInfo> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().map(|ext| ext == "json").unwrap_or(false)
                && let Ok(text) = std::fs::read_to_string(&p)
                    && let Ok(s) = serde_json::from_str::<Session>(&text) {
                        sessions.push(SessionInfo {
                            id: s.id,
                            created_at: s.created_at,
                            message_count: s.messages.len(),
                        });
                    }
        }
    }
    sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    sessions
}

pub fn list_formatted() -> String {
    let sessions = list();
    if sessions.is_empty() {
        return "no saved sessions".to_string();
    }
    let mut out = String::from("saved sessions:\n");
    for s in &sessions {
        let date = chrono::DateTime::parse_from_rfc3339(&s.created_at)
            .ok()
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| s.created_at.clone());
        out.push_str(&format!(
            "  {} — {} ({} messages)\n",
            &s.id[..8],
            date,
            s.message_count
        ));
    }
    out
}

/// Resolve a session id or unique prefix against saved sessions.
pub fn find(prefix: &str) -> Option<String> {
    let sessions = list();
    sessions
        .iter()
        .find(|s| s.id.starts_with(prefix))
        .map(|s| s.id.clone())
}

/// The most recently saved session id, if any.
pub fn last() -> Option<String> {
    list().first().map(|s| s.id.clone())
}

pub fn diff(a: &str, b: &str) -> anyhow::Result<String> {
    let a_id = find(a).ok_or_else(|| anyhow::anyhow!("no session matches '{a}'"))?;
    let b_id = find(b).ok_or_else(|| anyhow::anyhow!("no session matches '{b}'"))?;
    let sa = Session::load(&a_id)?;
    let sb = Session::load(&b_id)?;
    Ok(diff_sessions(&sa, &sb))
}

/// Textual comparison of two sessions: asks, tools used, final answers,
/// and tool sets unique to each side.
pub fn diff_sessions(a: &Session, b: &Session) -> String {
    let a_tools = a.tool_names();
    let b_tools = b.tool_names();
    let fmt_date = |id: &str, created: &str| {
        let date = chrono::DateTime::parse_from_rfc3339(created)
            .ok()
            .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| created.to_string());
        format!("{} — {}", &id[..8], date)
    };
    let a_title = fmt_date(&a.id, &a.created_at);
    let b_title = fmt_date(&b.id, &b.created_at);

    let a_ask = a.first_user();
    let b_ask = b.first_user();
    let a_done = a.last_assistant();
    let b_done = b.last_assistant();

    let a_short: String = a_done.chars().take(200).collect();
    let b_short: String = b_done.chars().take(200).collect();

    let only_a: Vec<&String> = a_tools.iter().filter(|t| !b_tools.contains(t)).collect();
    let only_b: Vec<&String> = b_tools.iter().filter(|t| !a_tools.contains(t)).collect();

    let mut out = String::from("session diff:\n");
    out.push_str(&format!("  {a_title} — {} messages\n", a.messages.len()));
    if !a_ask.is_empty() {
        out.push_str(&format!("    asked: {a_ask}\n"));
    }
    if !a_short.is_empty() {
        out.push_str(&format!("    done: {a_short}\n"));
    }
    if !a_tools.is_empty() {
        out.push_str(&format!("    tools: {}\n", a_tools.join(", ")));
    }
    out.push_str(&format!("  {b_title} — {} messages\n", b.messages.len()));
    if !b_ask.is_empty() {
        out.push_str(&format!("    asked: {b_ask}\n"));
    }
    if !b_short.is_empty() {
        out.push_str(&format!("    done: {b_short}\n"));
    }
    if !b_tools.is_empty() {
        out.push_str(&format!("    tools: {}\n", b_tools.join(", ")));
    }
    if !only_a.is_empty() {
        out.push_str(&format!(
            "  in {a_title} only:\n    {}\n",
            only_a
                .iter()
                .map(|t| t.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !only_b.is_empty() {
        out.push_str(&format!(
            "  in {b_title} only:\n    {}\n",
            only_b
                .iter()
                .map(|t| t.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    out
}

/// Compact cross-session recap: each recent session's first user request and
/// its final assistant answer, newest first. Used as a context block so a new
/// session knows what was worked on before.
pub fn summarize(limit: usize) -> String {
    let sessions = list();
    if sessions.is_empty() {
        return String::new();
    }
    let sessions: Vec<SessionInfo> = sessions.into_iter().take(limit).collect();
    let mut parts: Vec<String> = Vec::new();
    for info in &sessions {
        let s = match Session::load(&info.id) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let user = s.first_user();
        let assistant = s.last_assistant();
        let date = chrono::DateTime::parse_from_rfc3339(&s.created_at)
            .ok()
            .map(|dt| dt.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| s.created_at.clone());
        let mut entry = format!("[{date}] asked: {user}");
        if !assistant.is_empty() {
            let short: String = assistant.chars().take(400).collect();
            entry.push_str(&format!("\n    did: {short}"));
        }
        parts.push(entry);
    }
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: Role, content: &str) -> Message {
        Message {
            role,
            content: content.to_string(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    fn with_tool(m: Message, name: &str) -> Message {
        Message {
            tool_calls: Some(vec![crate::types::ToolCall {
                id: "c1".to_string(),
                name: name.to_string(),
                arguments: serde_json::Value::Null,
            }]),
            ..m
        }
    }

    #[test]
    fn diff_reports_asks_tools_and_answers() {
        let mut a = Session::new();
        a.messages.push(msg(Role::User, "refactor fetch"));
        a.messages
            .push(with_tool(msg(Role::Assistant, "done"), "read"));
        a.messages
            .push(with_tool(msg(Role::Assistant, ""), "grep"));

        let mut b = Session::new();
        b.messages.push(msg(Role::User, "add timezone"));
        b.messages
            .push(with_tool(msg(Role::Assistant, "done"), "read"));
        b.messages
            .push(with_tool(msg(Role::Assistant, ""), "bash"));
        b.messages.push(msg(Role::Assistant, "fixed"));

        let out = diff_sessions(&a, &b);
        assert!(out.contains("asked: refactor fetch"));
        assert!(out.contains("asked: add timezone"));
        assert!(out.contains("tools: grep, read"));
        assert!(out.contains("tools: bash, read"));
        assert!(out.contains("done: done"));
        assert!(out.contains("done: fixed"));
        assert!(out.contains("only:\n    grep"));
        assert!(out.contains("only:\n    bash"));
    }

    #[test]
    fn looped_tools_are_deduped() {
        let mut s = Session::new();
        s.messages
            .push(with_tool(msg(Role::Assistant, ""), "read"));
        s.messages
            .push(with_tool(msg(Role::Assistant, ""), "read"));
        let names = s.tool_names();
        assert_eq!(names, vec!["read".to_string()]);
    }
}
