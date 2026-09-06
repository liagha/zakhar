use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::{Mutex, OnceLock};

const FILE: &str = ".zakhar/ledger.jsonl";
const BACK_DIR: &str = ".zakhar/ledger/back";
const CAP: usize = 2000;
const TRIM: usize = 500;

static LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Revert {
    pub path: String,
    pub old_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub ts: String,
    pub tool: String,
    pub args: serde_json::Value,
    pub digest: String,
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revert: Option<Revert>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverted_at: Option<String>,
}

pub fn digest_of(args: &serde_json::Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(serde_json::to_string(args).unwrap_or_default());
    format!("{:x}", hasher.finalize())[..12].to_string()
}

pub fn snapshot(path: &str) -> Option<Revert> {
    if path.is_empty() {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(Revert {
        path: path.to_string(),
        old_b64: B64.encode(bytes),
    })
}

pub fn record(tool: &str, args: &serde_json::Value, outcome: &str, revert: Option<Revert>) -> anyhow::Result<()> {
    let _g = lock();
    std::fs::create_dir_all(".zakhar")?;
    let id = crate::memory::knowledge::uid();
    if let Some(r) = &revert {
        let dir = std::path::Path::new(BACK_DIR);
        std::fs::create_dir_all(dir)?;
        let bytes = B64.decode(&r.old_b64)?;
        std::fs::write(dir.join(format!("{id}.bak")), bytes)?;
    }
    let entry = Entry {
        id,
        ts: Utc::now().to_rfc3339(),
        tool: tool.to_string(),
        args: args.clone(),
        digest: digest_of(args),
        outcome: outcome.chars().take(200).collect(),
        revert,
        reverted_at: None,
    };
    let mut all = read();
    all.push(entry);
    if all.len() > CAP {
        all.drain(..(all.len() - CAP).min(TRIM));
    }
    write_all(&all)
}

pub fn read() -> Vec<Entry> {
    std::fs::read_to_string(FILE)
        .ok()
        .map(|t| {
            t.lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()
        })
        .unwrap_or_default()
}

pub fn undo(n: usize) -> anyhow::Result<String> {
    let _g = lock();
    let mut all = read();
    let mut reverted = 0;
    for entry in all.iter_mut().rev().take(n.max(1)) {
        if entry.reverted_at.is_some() {
            continue;
        }
        let Some(r) = &entry.revert else {
            continue;
        };
        let bytes = B64.decode(&r.old_b64)?;
        std::fs::write(&r.path, bytes)?;
        entry.reverted_at = Some(Utc::now().to_rfc3339());
        reverted += 1;
    }
    if reverted == 0 {
        return Ok("nothing reversible to undo".to_string());
    }
    write_all(&all)?;
    Ok(format!("reverted {reverted} operation(s)"))
}

pub fn audit(n: usize) -> String {
    let entries = read();
    if entries.is_empty() {
        return "ledger empty".to_string();
    }
    let mut out = String::new();
    for e in entries.iter().rev().take(n.max(1)) {
        let mark = if e.reverted_at.is_some() { " [reverted]" } else { "" };
        let args8: String = serde_json::to_string(&e.args).unwrap_or_default();
        let args8: String = args8.chars().take(80).collect();
        let when: String = e.ts.chars().take(16).collect();
        out.push_str(&format!(
            "{} {} {} ({}){} :: {}\n",
            when,
            e.tool,
            e.digest,
            args8,
            mark,
            e.outcome.chars().take(60).collect::<String>()
        ));
    }
    out
}

fn write_all(entries: &[Entry]) -> anyhow::Result<()> {
    let mut out = String::new();
    for e in entries {
        out.push_str(&serde_json::to_string(e)?);
        out.push('\n');
    }
    let path = std::path::Path::new(FILE);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_file_name(format!(".{}.tmp", path.file_name().map(|f| f.to_string_lossy()).unwrap_or_default()));
    std::fs::write(&tmp, out)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root() -> (tempfile::TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = crate::memory::lock();
        let dir = tempfile::tempdir().unwrap();
        (dir, guard)
    }

    #[test]
    fn record_undo_and_audit_roundtrip() {
        let (dir, _g) = tmp_root();
        let orig = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub/notes.txt"), "v1").unwrap();
        let r = snapshot("sub/notes.txt").unwrap();
        std::fs::write(dir.path().join("sub/notes.txt"), "v2").unwrap();
        record("write", &serde_json::json!({"path": "sub/notes.txt"}), "wrote v2", Some(r)).unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("sub/notes.txt")).unwrap(), "v2");
        let out = undo(1).unwrap();
        assert!(out.contains("reverted 1"), "got: {out}");
        assert_eq!(std::fs::read_to_string(dir.path().join("sub/notes.txt")).unwrap(), "v1");
        let audit_out = audit(5);
        assert!(audit_out.contains("write"), "got: {audit_out}");
        std::env::set_current_dir(&orig).unwrap();
    }

    #[test]
    fn non_reversible_undo_reports_nothing() {
        let (dir, _g) = tmp_root();
        let orig = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();
        record("bash", &serde_json::json!({"command": "echo hi"}), "ran", None).unwrap();
        let out = undo(1).unwrap();
        assert!(out.contains("nothing reversible"), "got: {out}");
        std::env::set_current_dir(&orig).unwrap();
    }
}