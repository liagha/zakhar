pub mod episodic;
pub mod jobs;
pub mod knowledge;
pub mod mind;
pub mod profile;
pub mod recall;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

#[cfg(test)]
pub(crate) fn set_path(p: PathBuf) {
    let cell = OVERRIDE.get_or_init(|| Mutex::new(None));
    *cell.lock().unwrap() = Some(p);
}

#[cfg(test)]
pub(crate) fn lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn override_path() -> Option<PathBuf> {
    OVERRIDE.get().and_then(|m| m.lock().unwrap().clone())
}

const MEMORY_TOOLS: &str = "You have persistent project memory shared across sessions. Use the \
    context tool to save facts worth keeping and to fetch an exact key. Use the remember tool to \
    recall anything from memory in your own words. Consult memory before guessing, and save what \
    you learn.";

pub fn load_blocks() -> Vec<(String, String)> {
    let mut blocks = Vec::new();

    let _ = knowledge::migrate_once();

    if let Some(text) = profile::load() {
        blocks.push(("profile".to_string(), text));
    }

    let past = crate::session::summarize(5);
    if !past.is_empty() {
        blocks.push(("past work".to_string(), past));
    }

    let recent = episodic::block(10);
    if recent != "no recent events" {
        blocks.push(("recent".to_string(), recent));
    }

    let known = knowledge::block(5);
    if known != "no saved knowledge" {
        blocks.push(("knowledge".to_string(), known));
    }

    let mut parts = Vec::new();
    let candidates = [
        "ZAKHAR.md",
        "CLAUDE.md",
        "AGENTS.md",
        ".zakhar/MEMORY.md",
        ".claude/MEMORY.md",
    ];
    for name in candidates {
        let p = Path::new(name);
        if p.exists()
            && let Ok(text) = std::fs::read_to_string(p)
            && !text.trim().is_empty()
        {
            parts.push(format!("--- {name} ---\n{text}"));
            println!("[memory] loaded {name} ({} bytes)", text.len());
        }
    }
    let p = crate::paths::config_dir().join("memory.md");
    if p.exists()
        && let Ok(text) = std::fs::read_to_string(&p)
        && !text.trim().is_empty()
    {
        parts.push(format!("--- config/memory.md ---\n{text}"));
        println!("[memory] loaded {} ({} bytes)", p.display(), text.len());
    }
    if !parts.is_empty() {
        blocks.push(("memory".to_string(), parts.join("\n\n")));
    }

    blocks.push(("memory tools".to_string(), MEMORY_TOOLS.to_string()));

    blocks
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::MutexGuard;

    fn tmp_store() -> MutexGuard<'static, ()> {
        let guard = lock();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.keep().join("episodic.jsonl");
        set_path(path.clone());
        let _ = std::fs::remove_file(&path);
        guard
    }

    #[test]
    fn recent_precedes_static_memory() {
        let _g = tmp_store();
        let dir = tempfile::tempdir().unwrap();
        let orig = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        std::fs::create_dir_all(".zakhar").unwrap();
        std::fs::write(".zakhar/MEMORY.md", "static bytes").unwrap();
        episodic::append("note", "hello").unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let blocks = load_blocks();
            let labels: Vec<&str> = blocks.iter().map(|(l, _)| l.as_str()).collect();
            let recent = labels.iter().position(|l| *l == "recent");
            let memory = labels.iter().position(|l| *l == "memory");
            if let (Some(r), Some(m)) = (recent, memory) {
                assert!(r < m, "recent must precede memory: {labels:?}")
            }
        }));
        let _ = std::env::set_current_dir(&orig);
        result.unwrap();
    }
}
