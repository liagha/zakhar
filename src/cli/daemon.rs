use std::io::Write;
use std::path::Path;
use std::process::Command;

use chrono::Utc;
use crate::memory::{episodic, jobs::{self, Job}, mind};
use crate::reminder;

pub fn notify(message: &str) {
    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("notify-send")
            .arg("zakhar")
            .arg(message)
            .status();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("osascript")
            .arg("-e")
            .arg(format!("display notification \"{message}\" with title \"zakhar\""))
            .status();
    }
}

pub fn ensure_daemon() {
    let exe = std::env::current_exe().ok();
    let mut cmd = match exe {
        Some(e) => Command::new(e),
        None => return,
    };
    cmd.arg("daemon");
    if std::env::var("ZAKHAR_NO_DAEMON").is_ok() {
        return;
    }
    let already = Command::new("pgrep")
        .arg("-f")
        .arg("zakhar daemon")
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);
    if already {
        return;
    }
    let _ = cmd
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn();
}

pub async fn run() -> anyhow::Result<()> {
    println!("zakhar daemon started (pid {})", std::process::id());
    loop {
        drain_jobs();
        for r in reminder::due_and_due() {
            let msg = format!("⏰ {}", r.message);
            notify(&msg);
            println!("⏰ fired: {} — {}", r.id, r.message);
            if !r.is_recurring() {
                reminder::mark_done(&r.id);
            } else if let Some(rr) = r.recurring {
                let next = advance(&r.due_at, &rr);
                if let Some(next) = next {
                    let _ = reminder::drop(&r.id);
                    let _ = reminder::add(r.message.clone(), next, Some(rr));
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    }
}

fn drain_jobs() {
    let dir = crate::paths::jobs();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let now = Utc::now();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|x| x == "json").unwrap_or(false)
            && let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(mut job) = serde_json::from_str::<Job>(&text)
        {
            if !job.due(now) {
                continue;
            }
            let _ = std::fs::remove_file(&path);
            let log = job.root.join("memory").join("compaction.log");
            tokio::spawn(async move {
                match run_job(&job).await {
                    Ok(line) => log_line(&log, &line),
                    Err(e) => {
                        if job.retry() {
                            let _ = jobs::persist(&job);
                        } else {
                            log_line(
                                &log,
                                &format!(
                                    "daemon: {:?} job gave up after {} attempts: {e}",
                                    job.kind, job.attempts
                                ),
                            );
                        }
                    }
                }
            });
        }
    }
}

async fn run_job(job: &Job) -> anyhow::Result<String> {
    let cfg = crate::config::Config::load()?;
    let registry = crate::registry::build(&cfg);
    let routes = crate::capabilities::chain(&cfg, "summary", "light");
    let provider_box = crate::fallback::build(&registry, &routes, crate::fallback::Decide::Auto)?;
    let provider: &dyn crate::provider::Provider = provider_box.as_ref();
    let model = routes.first().map(|r| r.model.clone()).unwrap_or_default();
    match job.kind.as_str() {
        "mind" => {
            mind::run(&job.root, provider, &model).await?;
            Ok(format!("daemon: mind run finished for {}", job.root.display()))
        }
        _ => {
            let archive = job
                .archive
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("compact job missing archive"))?;
            let events = episodic::read_archive(archive);
            if events.is_empty() {
                anyhow::bail!("archive is empty");
            }
            let summary = episodic::summarize_compaction(&job.root, provider, &model, &events).await?;
            Ok(format!(
                "daemon: summarized {} events from {} ({} chars)",
                events.len(),
                job.archive.as_deref().map(|p| p.display().to_string()).unwrap_or_default(),
                summary.len()
            ))
        }
    }
}

fn log_line(path: &Path, line: &str) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{line}");
    }
}

fn advance(due: &str, recurring: &str) -> Option<String> {
    let base = reminder::parse_due(due)?;
    let low = recurring.to_lowercase();
    let next = if low.contains("hour") {
        base + chrono::Duration::hours(1)
    } else if low.contains("minute") {
        base + chrono::Duration::minutes(5)
    } else if low.contains("day") || low.contains("daily") || low.contains("morning") || low.contains("noon") {
        base + chrono::Duration::days(1)
    } else if low.contains("week") {
        base + chrono::Duration::weeks(1)
    } else if low.contains("month") {
        base + chrono::Duration::days(30)
    } else {
        base + chrono::Duration::days(1)
    };
    Some(next.to_rfc3339())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::lock;
    use crate::paths::{self, set_home};
    use tempfile::TempDir;

    fn tmp() -> (TempDir, std::sync::MutexGuard<'static, ()>) {
        let dir = tempfile::tempdir().unwrap();
        let g = lock();
        set_home(dir.path().join(".zakhar"));
        (dir, g)
    }

    async fn wait_for(pred: impl Fn() -> bool) {
        for _ in 0..250 {
            if pred() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("timeout waiting for condition");
    }

    #[test]
    fn advance_hourly() {
        let due = "2026-01-01T12:00:00Z";
        let next = advance(due, "every hour").unwrap();
        assert!(next.contains("13:00"));
    }

    #[test]
    fn advance_daily() {
        let due = "2026-01-01T12:00:00Z";
        let next = advance(due, "daily").unwrap();
        assert!(next.contains("2026-01-02"));
    }

    #[test]
    fn advance_weekly() {
        let due = "2026-01-01T12:00:00Z";
        let next = advance(due, "weekly").unwrap();
        assert!(next.contains("2026-01-08"));
    }

    #[test]
    fn advance_monthly() {
        let due = "2026-01-01T12:00:00Z";
        let next = advance(due, "monthly").unwrap();
        assert!(next.contains("2026-01-31"));
    }

    #[test]
    fn advance_minute() {
        let due = "2026-01-01T12:00:00Z";
        let next = advance(due, "every minute").unwrap();
        assert!(next.contains("12:05"));
    }

    #[test]
    fn advance_unknown_defaults_day() {
        let due = "2026-01-01T12:00:00Z";
        let next = advance(due, "yearly sound").unwrap();
        assert!(next.contains("2026-01-02"));
    }

    #[test]
    fn advance_parse_fail_is_none() {
        assert!(advance("garbage", "daily").is_none());
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn drain_jobs_retry_flow() {
        let (dir, _g) = tmp();
        let jobs_dir = paths::jobs();
        std::fs::create_dir_all(&jobs_dir).unwrap();
        let root = dir.path().join("proj");
        std::fs::create_dir_all(root.join("memory")).unwrap();

        let job = Job {
            kind: "bogus".into(),
            root: root.clone(),
            archive: None,
            created: Utc::now().to_rfc3339(),
            attempts: 0,
            next_due: None,
        };
        jobs::persist(&job).unwrap();

        let json_count = || {
            std::fs::read_dir(&jobs_dir)
                .unwrap()
                .filter(|e| {
                    e.is_ok()
                        && e.as_ref()
                            .unwrap()
                            .path()
                            .extension()
                            .is_some_and(|x| x == "json")
                })
                .count()
        };
        let one_json = || {
            std::fs::read_dir(&jobs_dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .find(|e| e.path().extension().is_some_and(|x| x == "json"))
        };
        let current = || {
            let path = one_json().expect("job file").path();
            serde_json::from_str::<Job>(&std::fs::read_to_string(&path).unwrap()).unwrap()
        };
        let force_past = |job: &Job| {
            let path = one_json().expect("job file").path();
            let mut past = job.clone();
            past.next_due = Some((Utc::now() - chrono::Duration::seconds(60)).to_rfc3339());
            std::fs::write(path, serde_json::to_string(&past).unwrap()).unwrap();
        };

        assert_eq!(json_count(), 1);

        drain_jobs();
        wait_for(|| json_count() == 1).await;
        assert_eq!(current().attempts, 1);
        assert!(current().next_due.is_some());

        drain_jobs();
        wait_for(|| json_count() == 1).await;
        assert_eq!(current().attempts, 1);

        let log_path = root.join("memory").join("compaction.log");

        for expected in 2..=5 {
            force_past(&current());
            drain_jobs();
            wait_for(|| json_count() == 1).await;
            assert_eq!(current().attempts, expected, "attempt {expected}");
        }

        force_past(&current());
        drain_jobs();
        wait_for(|| {
            std::fs::read_to_string(&log_path)
                .map(|l| l.contains("gave up after 5 attempts"))
                .unwrap_or(false)
        })
        .await;
        let log = std::fs::read_to_string(&log_path).unwrap();
        assert!(log.contains("gave up after 5 attempts"), "{log}");
    }
}