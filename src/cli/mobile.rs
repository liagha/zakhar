//! Desktop driver for the mobile turn engine. Runs the exact session protocol
//! the Android app uses (start, poll, approve) so the on-device path can be
//! exercised and verified from a terminal. With `--mock` it talks to the
//! scripted provider instead of a live model, so the whole cycle is testable
//! offline.

use serde_json::json;

pub async fn mobile(
    message: String,
    auto_approve: bool,
    mock: bool,
) -> anyhow::Result<()> {
    let messages = json!([{
        "role": "user",
        "content": message,
    }])
    .to_string();

    let provider: Box<dyn crate::provider::Provider> = if mock {
        Box::new(crate::provider::mock::Script {
            name: "script".to_string(),
            answer: "done: mock turn complete".to_string(),
        })
    } else {
        let cfg = crate::config::Config::load()?;
        let heavy = crate::levels::resolve(&cfg, "heavy");
        let pid = heavy.provider;
        let pcfg = cfg
            .providers
            .get(&pid)
            .ok_or_else(|| anyhow::anyhow!("unknown provider: {pid}"))?;
        let mut pcfg = pcfg.clone();
        pcfg.api_key = crate::registry::resolve_key(&pcfg);
        if !heavy.model.is_empty() {
            pcfg.default_model = heavy.model.clone();
            pcfg.models = vec![heavy.model.clone()];
        }
        Box::new(crate::provider::openai::OpenAI::new(&pid, &pcfg))
    };

    let id = crate::mobile::start(provider, &messages, auto_approve);
    println!("session {id}");

    loop {
        let raw = crate::mobile::poll(&id, 1000);
        let value: serde_json::Value = serde_json::from_str(&raw)?;
        if let Some(err) = value.get("error") {
            anyhow::bail!("{err}");
        }
        let events = value.get("events").and_then(|e| e.as_array()).cloned().unwrap_or_default();
        if events.is_empty() {
            continue;
        }
        for raw_event in events {
            let event: serde_json::Value =
                serde_json::from_str(raw_event.as_str().unwrap_or_default())?;
            let kind = event.get("type").and_then(|t| t.as_str()).unwrap_or_default();
            match kind {
                "text" => {
                    print!("{}", event.get("data").and_then(|d| d.as_str()).unwrap_or_default());
                }
                "tool_approval" => {
                    let name = event.get("name").and_then(|n| n.as_str()).unwrap_or_default();
                    println!();
                    if auto_approve {
                        let _ = crate::mobile::approve(&id, true);
                    } else {
                        print!("[approve {name}? y/N] ");
                        let mut line = String::new();
                        let _ = std::io::stdin().read_line(&mut line);
                        let ok = line.trim().eq_ignore_ascii_case("y") || line.trim() == "yes";
                        let _ = crate::mobile::approve(&id, ok);
                        println!();
                    }
                }
                "tool_result" => {
                    let name = event.get("name").and_then(|n| n.as_str()).unwrap_or_default();
                    let result = event.get("result").and_then(|r| r.as_str()).unwrap_or_default();
                    let approved = event.get("approved").and_then(|a| a.as_bool()).unwrap_or(false);
                    println!();
                    println!("→ {name}{} {result}", if approved { "" } else { " (denied)" });
                }
                "done" => {
                    let text = event.get("text").and_then(|t| t.as_str()).unwrap_or_default();
                    println!();
                    println!("{text}");
                    println!("done");
                    return Ok(());
                }
                "error" => {
                    let msg = event.get("message").and_then(|m| m.as_str()).unwrap_or_default();
                    anyhow::bail!("{msg}");
                }
                "cancelled" => anyhow::bail!("cancelled"),
                _ => {}
            }
        }
    }
}