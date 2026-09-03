use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

const CAP: usize = 500;
const ROLL: usize = 100;

fn path() -> PathBuf {
    if let Some(p) = crate::memory::override_path() {
        return p;
    }
    PathBuf::from(".zakhar/memory/episodic.jsonl")
}

fn parent_dir(path: &Path) -> PathBuf {
    path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub ts: String,
    pub kind: String,
    pub text: String,
}

pub fn append(kind: &str, text: &str) -> anyhow::Result<()> {
    let path = path();
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(dir)?;
    let event = Event {
        ts: Utc::now().to_rfc3339(),
        kind: kind.to_string(),
        text: text.to_string(),
    };
    let line = serde_json::to_string(&event)?;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(file, "{line}")?;
    if read().len() > CAP {
        compact()?;
    }
    Ok(())
}

fn read() -> Vec<Event> {
    std::fs::read_to_string(path())
        .ok()
        .map(|t| {
            t.lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()
        })
        .unwrap_or_default()
}

pub fn recent(n: usize) -> Vec<Event> {
    let mut events = read();
    let first = events.len().saturating_sub(n);
    events.drain(..first);
    events
}

pub fn block(n: usize) -> String {
    let events = recent(n);
    if events.is_empty() {
        return "no recent events".to_string();
    }
    let mut out = String::new();
    for e in events {
        out.push_str(&format!("[{}] {}: {}\n", e.ts, e.kind, e.text));
    }
    out
}

pub fn compact() -> anyhow::Result<String> {
    let path = path();
    let events = read();
    if events.len() <= ROLL {
        return Ok(format!("only {} events, nothing to compact", events.len()));
    }
    let chunk: Vec<Event> = events[..events.len() - ROLL].to_vec();
    let kept: Vec<Event> = events[events.len() - ROLL..].to_vec();

    let dir = parent_dir(&path);
    let archive_dir = dir.join("archive");
    std::fs::create_dir_all(&archive_dir)?;
    let stamp = Utc::now().format("%Y%m%d-%H%M%S");
    let archive = archive_dir.join(format!("episodic-{stamp}.jsonl"));
    let mut file = std::fs::File::create(&archive)?;
    for e in &chunk {
        writeln!(file, "{}", serde_json::to_string(e)?)?;
    }

    let mut log = std::fs::File::create(&path)?;
    for e in &kept {
        writeln!(log, "{}", serde_json::to_string(e)?)?;
    }

    let preview: Vec<String> = chunk
        .iter()
        .rev()
        .take(3)
        .map(|e| {
            let t = e.text.trim();
            let t: String = t.chars().take(80).collect();
            t
        })
        .collect();

    let notes = dir
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("NOTES.md");
    let mut out = String::new();
    out.push_str(&format!(
        "## Archived {} events @ {stamp}\n",
        chunk.len()
    ));
    out.push_str(&format!("raw: {}\n", archive.display()));
    out.push_str(&format!("recent:\n{}\n", preview.join("\n")));
    let mut notes_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&notes)?;
    writeln!(notes_file, "{out}")?;

    Ok(format!(
        "archived {} events to {}, kept {}",
        chunk.len(),
        archive.display(),
        kept.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::MutexGuard;

    fn tmp_store() -> MutexGuard<'static, ()> {
        let guard = crate::memory::lock();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.keep().join("episodic.jsonl");
        crate::memory::set_path(path.clone());
        let _ = std::fs::remove_file(&path);
        guard
    }

    #[test]
    fn recent_returns_last_n_oldest_first() {
        let _g = tmp_store();
        for i in 0..5 {
            append("note", &format!("event {i}")).unwrap();
        }
        let events = recent(3);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].text, "event 2");
        assert_eq!(events[2].text, "event 4");
    }

    #[test]
    fn recent_returns_all_when_fewer() {
        let _g = tmp_store();
        append("note", "only").unwrap();
        let events = recent(10);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].text, "only");
    }

    #[test]
    fn append_auto_compacts_over_cap() {
        let _g = tmp_store();
        for i in 0..CAP {
            append("note", &format!("n{i}")).unwrap();
        }
        let events = read();
        assert_eq!(events.len(), CAP);
        append("note", "trigger").unwrap();
        let events = read();
        assert_eq!(events.len(), ROLL);
        assert_eq!(events[0].text, "n401");
    }

    #[test]
    fn empty_log_block() {
        let _g = tmp_store();
        assert!(block(10).contains("no recent events"));
    }

    #[test]
    fn compact_archives_oldest_keeps_newest() {
        let _g = tmp_store();
        for i in 0..ROLL + 20 {
            append("note", &format!("n{i}")).unwrap();
        }
        let out = compact().unwrap();
        assert!(out.contains("archived"));
        let events = read();
        assert_eq!(events.len(), ROLL);
        assert!(events.first().map(|e| e.text == "n20").unwrap_or(false));
    }

    #[test]
    fn compact_noop_below_threshold() {
        let _g = tmp_store();
        append("note", "solo").unwrap();
        let out = compact().unwrap();
        assert!(out.contains("nothing to compact"));
    }
}
