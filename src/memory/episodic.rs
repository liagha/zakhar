use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

const CAP: usize = 500;
const ROLL: usize = 100;
const TEXT_MAX: usize = 300;

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

/// Append an event. Returns the archived chunk if compaction was triggered.
pub fn append(kind: &str, text: &str) -> anyhow::Result<Vec<Event>> {
    let path = path();
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(dir)?;
    let trimmed = text.trim();
    let stored = if trimmed.len() > TEXT_MAX {
        let truncated: String = trimmed.chars().take(TEXT_MAX).collect();
        format!("{truncated}…")
    } else {
        trimmed.to_string()
    };
    let event = Event {
        ts: Utc::now().to_rfc3339(),
        kind: kind.to_string(),
        text: stored,
    };
    let line = serde_json::to_string(&event)?;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(file, "{line}")?;
    if read().len() > CAP {
        compact()
    } else {
        Ok(Vec::new())
    }
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

pub fn recent_json(n: usize) -> String {
    serde_json::to_string(&recent(n)).unwrap_or_else(|_| "[]".to_string())
}

/// Raw archival: move oldest events to archive file, keep ROLL newest.
/// Returns the archived chunk (empty if nothing was archived).
pub fn compact() -> anyhow::Result<Vec<Event>> {
    let path = path();
    let events = read();
    if events.len() <= ROLL {
        return Ok(Vec::new());
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

    let _ = dispatch_compact(&archive);

    Ok(chunk)
}

/// Background memory-agent job: boil an archived chunk into prose on NOTES.md.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    /// Absolute project dir; `.zakhar` lives inside it.
    pub root: PathBuf,
    /// Absolute path to the archived chunk.
    pub archive: PathBuf,
    pub created: String,
}

/// Hand the summarisation to the background daemon. Writes a job to the shared
/// ~/.zakhar/jobs mailbox and makes sure a daemon is running; the daemon
/// distills the chunk into NOTES.md in its own process, so the caller never
/// waits and the main agent never sees it.
pub fn dispatch_compact(archive: &Path) -> anyhow::Result<()> {
    if cfg!(test) || std::env::var("ZAKHAR_NO_COMPACT").is_ok() {
        return Ok(());
    }
    let root = std::env::current_dir()?;
    let archive = if archive.is_absolute() {
        archive.to_path_buf()
    } else {
        root.join(archive)
    };
    let job = Job {
        root,
        archive,
        created: Utc::now().to_rfc3339(),
    };
    let dir = crate::paths::jobs();
    std::fs::create_dir_all(&dir)?;
    let name = format!("compact-{}.json", Utc::now().format("%Y%m%d-%H%M%S-%6f"));
    std::fs::write(dir.join(name), serde_json::to_string(&job)?)?;
    crate::cli::daemon::ensure_daemon();
    Ok(())
}

/// Read archived events back from an archive file (used by the background
/// daemon, which does not share this process's working directory).
pub fn read_archive(archive: &Path) -> Vec<Event> {
    std::fs::read_to_string(archive)
        .ok()
        .map(|t| {
            t.lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Async LLM summarisation: call the model to distill a chunk of archived
/// events into a concise prose summary, then append it to NOTES.md.
/// `root` is the project dir the `.zakhar` home lives in.
pub async fn summarize_compaction(
    root: &Path,
    provider: &dyn crate::provider::Provider,
    model: &str,
    events: &[Event],
) -> anyhow::Result<String> {
    if events.is_empty() {
        return Ok("no events to summarise".to_string());
    }

    let event_lines: Vec<String> = events
        .iter()
        .map(|e| format!("[{}] {}: {}", e.ts, e.kind, e.text))
        .collect();
    let event_text = event_lines.join("\n");

    let request = crate::types::ChatRequest {
        model: model.to_string(),
        messages: vec![
            crate::types::Message::system(
                "You are a memory summarisation assistant. You will receive a batch of \
                 chronological events from a user's work session. Distill them into a \
                 concise, human-readable prose summary (3-6 sentences). Focus on what \
                 was done, what was decided, and what the outcome was. Use clear, \
                 natural language. Do not include raw timestamps or JSON.".to_string(),
            ),
            crate::types::Message::user(format!(
                "Summarise these {} events:\n\n{}",
                events.len(),
                event_text
            )),
        ],
        temperature: Some(0.3),
        max_tokens: Some(512),
        stream: Some(false),
        tools: None,
    };

    let mut stream = provider.chat_stream(request).await?;

    let mut summary = String::new();
    while let Some(event) = futures::StreamExt::next(&mut stream).await {
        match event? {
            crate::provider::ChatStreamEvent::Text(t) => summary.push_str(&t),
            crate::provider::ChatStreamEvent::Done => break,
            _ => {}
        }
    }

    if summary.trim().is_empty() {
        anyhow::bail!("LLM returned empty summary");
    }

    let summary = summary.trim().to_string();

    let notes = root.join(".zakhar").join("NOTES.md");
    let stamp = Utc::now().format("%Y-%m-%d %H:%M");
    let entry = format!("\n## Summary @ {stamp} ({} events)\n{}\n", events.len(), summary);
    let mut notes_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&notes)?;
    writeln!(notes_file, "{entry}")?;

    Ok(summary)
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
        let archived = append("note", "trigger").unwrap();
        assert!(!archived.is_empty(), "compaction should have triggered");
        let events = read();
        assert_eq!(events.len(), ROLL);
        assert!(events.first().map(|e| e.text == "n401").unwrap_or(false));
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
        let events = compact().unwrap();
        assert_eq!(events.len(), 20);
        let kept = read();
        assert_eq!(kept.len(), ROLL);
        assert!(kept.first().map(|e| e.text == "n20").unwrap_or(false));
    }

    #[test]
    fn compact_noop_below_threshold() {
        let _g = tmp_store();
        append("note", "solo").unwrap();
        let events = compact().unwrap();
        assert!(events.is_empty());
    }
}
