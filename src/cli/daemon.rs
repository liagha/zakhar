use std::process::Command;

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