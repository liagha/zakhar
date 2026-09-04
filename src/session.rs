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
            if p.extension().map(|ext| ext == "json").unwrap_or(false) {
                if let Ok(text) = std::fs::read_to_string(&p) {
                    if let Ok(s) = serde_json::from_str::<Session>(&text) {
                        sessions.push(SessionInfo {
                            id: s.id,
                            created_at: s.created_at,
                            message_count: s.messages.len(),
                        });
                    }
                }
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
        let user = s
            .messages
            .iter()
            .find(|m| m.role == Role::User)
            .map(|m| m.content.trim().replace('\n', " "))
            .unwrap_or_default();
        let assistant = s
            .messages
            .iter()
            .rev()
            .find(|m| m.role == Role::Assistant)
            .map(|m| m.content.trim())
            .unwrap_or_default();
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
