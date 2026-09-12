use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

const MAX_ATTEMPTS: u8 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    #[serde(default = "default_kind")]
    pub kind: String,
    pub root: PathBuf,
    #[serde(default)]
    pub archive: Option<PathBuf>,
    pub created: String,
    #[serde(default)]
    pub attempts: u8,
    #[serde(default)]
    pub next_due: Option<String>,
}

fn default_kind() -> String {
    "compact".to_string()
}

impl Job {
    pub fn retry(&mut self) -> bool {
        if self.attempts >= MAX_ATTEMPTS {
            return false;
        }
        self.attempts += 1;
        self.next_due = Some((Utc::now() + backoff(self.attempts)).to_rfc3339());
        true
    }

    pub fn due(&self, now: DateTime<Utc>) -> bool {
        match &self.next_due {
            None => true,
            Some(ts) => DateTime::parse_from_rfc3339(ts)
                .map(|t| t.with_timezone(&Utc) <= now)
                .unwrap_or(true),
        }
    }
}

fn backoff(attempt: u8) -> Duration {
    match attempt {
        1 => Duration::minutes(1),
        2 => Duration::minutes(5),
        3 => Duration::minutes(30),
        4 => Duration::hours(2),
        _ => Duration::hours(12),
    }
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
        attempts: 0,
        next_due: None,
    };
    persist(&job)?;
    crate::cli::daemon::ensure_daemon();
    Ok(())
}

pub fn persist(job: &Job) -> anyhow::Result<()> {
    let dir = crate::paths::jobs();
    std::fs::create_dir_all(&dir)?;
    let name = format!("{}-{}.json", job.kind, Utc::now().format("%Y%m%d-%H%M%S-%6f"));
    std::fs::write(dir.join(name), serde_json::to_string(job)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job() -> Job {
        Job {
            kind: "compact".to_string(),
            root: PathBuf::from("/tmp/proj"),
            archive: None,
            created: Utc::now().to_rfc3339(),
            attempts: 0,
            next_due: None,
        }
    }

    #[test]
    fn job_serde_roundtrip() {
        let job = Job {
            kind: "mind".to_string(),
            root: PathBuf::from("/tmp/p"),
            archive: None,
            created: Utc::now().to_rfc3339(),
            attempts: 2,
            next_due: Some(Utc::now().to_rfc3339()),
        };
        let text = serde_json::to_string(&job).unwrap();
        let back: Job = serde_json::from_str(&text).unwrap();
        assert_eq!(back.kind, "mind");
        assert_eq!(back.root, job.root);
        assert!(back.archive.is_none());
        assert_eq!(back.attempts, 2);
        assert!(back.next_due.is_some());
    }

    #[test]
    fn old_compact_job_deserializes() {
        let text = r#"{"root":"/tmp/p","archive":"/tmp/a.jsonl","created":"2026-01-01T00:00:00Z"}"#;
        let job: Job = serde_json::from_str(text).unwrap();
        assert_eq!(job.kind, "compact");
        assert_eq!(job.archive.as_deref(), Some(Path::new("/tmp/a.jsonl")));
        assert_eq!(job.attempts, 0);
        assert!(job.next_due.is_none());
    }

    #[test]
    fn enqueue_skips_in_tests() {
        let root = tempfile::tempdir().unwrap();
        assert!(enqueue("compact", root.path(), None).is_ok());
    }

    #[test]
    fn retry_bumps_attempts_and_schedules() {
        let mut job = job();
        assert!(job.retry());
        assert_eq!(job.attempts, 1);
        assert!(job.next_due.is_some());
    }

    #[test]
    fn retry_gives_up_after_max() {
        let mut job = job();
        for _ in 0..MAX_ATTEMPTS {
            assert!(job.retry());
        }
        assert_eq!(job.attempts, MAX_ATTEMPTS);
        assert!(!job.retry());
        assert_eq!(job.attempts, MAX_ATTEMPTS);
    }

    #[test]
    fn backoff_widens() {
        let unit = Duration::minutes(1);
        assert!(backoff(4) > backoff(3));
        assert!(backoff(3) > backoff(2));
        assert!(backoff(2) > backoff(1));
        let duty = backoff(5);
        assert!(duty > backoff(4));
        assert!(duty <= unit * 12 * 60);
    }

    #[test]
    fn due_respects_schedule() {
        let now = Utc::now();
        let mut job = job();
        assert!(job.due(now));
        job.next_due = Some((now - Duration::seconds(5)).to_rfc3339());
        assert!(job.due(now));
        job.next_due = Some((now + Duration::seconds(300)).to_rfc3339());
        assert!(!job.due(now));
        job.next_due = Some("nonsense".to_string());
        assert!(job.due(now));
    }
}