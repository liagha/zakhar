use std::process::Command;

use crate::config::Config;
use crate::registry;
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

pub async fn run(phrase: Option<String>) -> anyhow::Result<()> {
    match phrase {
        Some(p) if p.trim().is_empty() || p.trim().to_lowercase() == "list" => {
            list().await
        }
        Some(p) if p.trim().to_lowercase().starts_with("drop ") => {
            let id = p.trim().splitn(2, ' ').nth(1).unwrap_or("").trim();
            drop_one(id).await
        }
        Some(p) => set(&p).await,
        None => list().await,
    }
}

async fn list() -> anyhow::Result<()> {
    let pending = reminder::list_pending();
    if pending.is_empty() {
        println!("no pending reminders");
        return Ok(());
    }
    println!("pending reminders:");
    for r in &pending {
        println!(
            "  {} {}\n      until {} ({})",
            r.id,
            r.message,
            r.due_at,
            r.recurring.clone().unwrap_or_else(|| "one-shot".to_string())
        );
    }
    Ok(())
}

async fn drop_one(id: &str) -> anyhow::Result<()> {
    match reminder::drop(id) {
        Some(r) => {
            println!("dropped reminder {} ('{}')", r.id, r.message);
            Ok(())
        }
        None => {
            println!("no reminder matches '{id}'");
            Ok(())
        }
    }
}

async fn set(phrase: &str) -> anyhow::Result<()> {
    let cfg = Config::load()?;
    let registry = registry::build(&cfg);
    let provider_id = registry::default_provider(&cfg);
    let p = registry
        .get(&provider_id)
        .ok_or_else(|| anyhow::anyhow!("unknown provider: {provider_id}"))?;
    let model = p.list_models().first().cloned().unwrap_or_default();

    let now = chrono::Utc::now().to_rfc3339();
    let prompt = format!(
        "You schedule reminders from natural language. Today's time is {now} UTC. \
         Parse the user's phrase and output ONLY JSON with this shape:\n\
         {{\"due_at\":\"<RFC3339 UTC>\",\"message\":\"<what to remind about>\",\"recurring\":null}}\n\
         If the phrase mentions a recurrence (e.g. 'every day', 'daily', 'weekly', 'each morning'), set recurring \
         to the interval string. If it's 'remind me in 20 minutes', compute due_at accordingly. \
         Use the message as given (the 'remind me' preamble can be dropped). Phrase: {phrase}"
    );

    let req = crate::types::ChatRequest {
        model: model.clone(),
        messages: vec![
            crate::types::Message::system(prompt),
            crate::types::Message::user(phrase.to_string()),
        ],
        temperature: Some(0.0),
        max_tokens: Some(200),
        stream: Some(false),
        tools: None,
    };

    let mut stream = p.chat_stream(req).await?;
    let mut body = String::new();
    while let Some(ev) = futures::StreamExt::next(&mut stream).await {
        match ev? {
            crate::provider::ChatStreamEvent::Text(t) => body.push_str(&t),
            _ => {}
        }
    }

    let parsed: serde_json::Value = extract_json(&body)
        .ok_or_else(|| anyhow::anyhow!("could not parse reminder: {body}"))?;

    let due = parsed
        .get("due_at")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let message = parsed
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let recurring = parsed.get("recurring").and_then(|v| v.as_str()).map(String::from);

    if due.is_empty() || message.is_empty() {
        anyhow::bail!("could not parse reminder: {body}");
    }

    let r = reminder::add(message, due, recurring)?;
    println!("reminder set: {} — {}", r.id, r.message);
    println!("  due at {}", r.due_at);
    ensure_daemon();
    Ok(())
}

fn extract_json(body: &str) -> Option<serde_json::Value> {
    let trimmed = body.trim();
    let start = trimmed.find('{')?;
    let end = trimmed.rfind('}')?;
    serde_json::from_str(&trimmed[start..=end]).ok()
}

fn ensure_daemon() {
    use std::process::Command;
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
