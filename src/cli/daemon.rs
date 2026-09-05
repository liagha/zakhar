use std::io::Write;
use std::path::Path;
use std::process::Command;

use crate::memory::{episodic, jobs::Job, mind};
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
            if r.recurring.is_none() {
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
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|x| x == "json").unwrap_or(false)
            && let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(job) = serde_json::from_str::<Job>(&text)
        {
            let _ = std::fs::remove_file(&path);
            tokio::spawn(async move {
                run_job(job).await;
            });
        }
    }
}

async fn run_job(job: Job) {
    let line = match run_one(&job).await {
        Ok(s) => s,
        Err(e) => format!("daemon: {:?} job failed for {}: {e}", job.kind, job.root.display()),
    };
    log(&job.root.join(".zakhar/memory/compaction.log"), &line);
}

async fn run_one(job: &Job) -> anyhow::Result<String> {
    let cfg = crate::config::Config::load()?;
    let registry = crate::registry::build(&cfg);
    let chosen = crate::capabilities::resolve(&cfg, "summary", "light");
    let pid = chosen.provider;
    let provider = registry
        .get(&pid)
        .ok_or_else(|| anyhow::anyhow!("unknown provider: {pid}"))?;
    let model = (!chosen.model.is_empty())
        .then_some(chosen.model.clone())
        .or_else(|| cfg.default_model.clone())
        .or_else(|| provider.list_models().first().cloned())
        .unwrap_or_default();
    match job.kind.as_str() {
        "mind" => mind::run(&job.root, provider, &model).await?,
        _ => {
            let summary = run_summary(job, provider, &model).await?;
            return Ok(format!(
                "daemon: summarized {} events from {} ({} chars)",
                summary.events,
                job.archive.as_deref().map(|p| p.display().to_string()).unwrap_or_default(),
                summary.chars
            ));
        }
    }
    Ok(format!("daemon: mind run finished for {}", job.root.display()))
}

struct Summary {
    events: usize,
    chars: usize,
}

async fn run_summary(job: &Job, provider: &dyn crate::provider::Provider, model: &str) -> anyhow::Result<Summary> {
    let archive = job
        .archive
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("compact job missing archive"))?;
    let events = episodic::read_archive(archive);
    if events.is_empty() {
        anyhow::bail!("archive is empty");
    }
    let summary = episodic::summarize_compaction(&job.root, provider, model, &events).await?;
    Ok(Summary {
        events: events.len(),
        chars: summary.len(),
    })
}

fn log(path: &Path, line: &str) {
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