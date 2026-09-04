pub mod episodic;
pub mod profile;

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
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

pub fn override_path() -> Option<PathBuf> {
    OVERRIDE.get().and_then(|m| m.lock().unwrap().clone())
}

pub fn load_blocks() -> Vec<(String, String)> {
    let mut blocks = Vec::new();

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
            match (recent, memory) {
                (Some(r), Some(m)) => assert!(r < m, "recent must precede memory: {labels:?}"),
                _ => {}
            }
        }));
        let _ = std::env::set_current_dir(&orig);
        result.unwrap();
    }
}
