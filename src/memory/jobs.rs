use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    #[serde(default = "default_kind")]
    pub kind: String,
    pub root: PathBuf,
    #[serde(default)]
    pub archive: Option<PathBuf>,
    pub created: String,
}

fn default_kind() -> String {
    "compact".to_string()
}

pub fn enqueue(kind: &str, root: &Path, archive: Option<&Path>) -> anyhow::Result<()> {
    if cfg!(test) {
        return Ok(());
    }
    let archive = match archive {
        Some(p) if !p.is_absolute() => Some(root.join(p)),
        other => other.map(|p| p.to_path_buf()),
    };
    let job = Job {
        kind: kind.to_string(),
        root: root.to_path_buf(),
        archive,
        created: Utc::now().to_rfc3339(),
    };
    let dir = crate::paths::jobs();
    std::fs::create_dir_all(&dir)?;
    let name = format!("{kind}-{}.json", Utc::now().format("%Y%m%d-%H%M%S-%6f"));
    std::fs::write(dir.join(name), serde_json::to_string(&job)?)?;
    crate::cli::daemon::ensure_daemon();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_serde_roundtrip() {
        let job = Job {
            kind: "mind".to_string(),
            root: PathBuf::from("/tmp/p"),
            archive: None,
            created: Utc::now().to_rfc3339(),
        };
        let text = serde_json::to_string(&job).unwrap();
        let back: Job = serde_json::from_str(&text).unwrap();
        assert_eq!(back.kind, "mind");
        assert_eq!(back.root, job.root);
        assert!(back.archive.is_none());
    }

    #[test]
    fn old_compact_job_deserializes() {
        let text = r#"{"root":"/tmp/p","archive":"/tmp/a.jsonl","created":"2026-01-01T00:00:00Z"}"#;
        let job: Job = serde_json::from_str(text).unwrap();
        assert_eq!(job.kind, "compact");
        assert_eq!(job.archive.as_deref(), Some(Path::new("/tmp/a.jsonl")));
    }

    #[test]
    fn enqueue_skips_in_tests() {
        let root = tempfile::tempdir().unwrap();
        assert!(enqueue("compact", root.path(), None).is_ok());
    }
}