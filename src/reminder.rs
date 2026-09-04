use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reminder {
    pub id: String,
    pub message: String,
    pub due_at: String,
    pub recurring: Option<String>,
    pub created_at: String,
    pub done: bool,
}

fn store_path() -> PathBuf {
    crate::paths::reminders_path()
}

pub fn load() -> Vec<Reminder> {
    std::fs::read_to_string(store_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save(reminders: &[Reminder]) -> anyhow::Result<()> {
    let p = store_path();
    let dir = p.parent().unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".reminders.{}.tmp", std::process::id()));
    std::fs::write(&tmp, serde_json::to_string_pretty(reminders)?)?;
    std::fs::rename(&tmp, &p)?;
    Ok(())
}

pub fn add(message: String, due_at: String, recurring: Option<String>) -> anyhow::Result<Reminder> {
    let mut list = load();
    let r = Reminder {
        id: format!("{:x}", uuid::Uuid::new_v4().simple()),
        message,
        due_at,
        recurring,
        created_at: Utc::now().to_rfc3339(),
        done: false,
    };
    list.push(r.clone());
    save(&list)?;
    Ok(r)
}

pub fn list_pending() -> Vec<Reminder> {
    load().into_iter().filter(|r| !r.done).collect()
}

pub fn drop(id: &str) -> Option<Reminder> {
    let mut list = load();
    let mut removed = None;
    list.retain(|r| {
        if r.id.starts_with(id) {
            removed = Some(r.clone());
            false
        } else {
            true
        }
    });
    if removed.is_some() {
        let _ = save(&list);
    }
    removed
}

pub fn mark_done(id: &str) {
    let mut list = load();
    for r in &mut list {
        if r.id.starts_with(id) {
            r.done = true;
        }
    }
    let _ = save(&list);
}

pub fn parse_due(ts: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(ts).ok().map(|d| d.with_timezone(&Utc))
}

pub fn due_and_due() -> Vec<Reminder> {
    let now = Utc::now();
    list_pending()
        .into_iter()
        .filter(|r| parse_due(&r.due_at).map(|d| d <= now).unwrap_or(false))
        .collect()
}
