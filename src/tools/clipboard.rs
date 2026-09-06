use std::process::{Command, Stdio};

use serde_json::{json, Value};

use crate::handler::Handler;
use crate::types::Tool;

const MAX_GET: usize = 20_000;

pub struct Clipboard;
impl Handler for Clipboard {
    fn spec(&self) -> Tool {
        Tool::function("clipboard", "Read or write the system clipboard. action='get' returns the clipboard text; action='set' replaces it with 'text'. Uses wl-paste/wl-copy, xclip/xsel, or pbpaste/pbcopy as available.", json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["get", "set"] },
                "text": { "type": "string", "description": "Text to set on the clipboard (for action=set)" }
            },
            "required": ["action"]
        }))
    }
    fn run(&self, args: &Value) -> anyhow::Result<String> {
        match args["action"].as_str().unwrap_or("") {
            "get" => get_clipboard(),
            "set" => {
                let text = args["text"].as_str().ok_or_else(|| anyhow::anyhow!("missing text"))?;
                set_clipboard(text)?;
                Ok(format!("copied {} chars to clipboard", text.len()))
            }
            a => anyhow::bail!("unknown action {a}"),
        }
    }
}

fn get_clipboard() -> anyhow::Result<String> {
    let attempts: &[&[&str]] = &[
        &["wl-paste", "--no-newline"],
        &["xclip", "-selection", "clipboard", "-o"],
        &["xsel", "--clipboard", "--output"],
        &["pbpaste"],
    ];
    for args in attempts {
        let out = match Command::new(args[0]).args(&args[1..]).output() {
            Ok(out) if out.status.success() => out,
            _ => continue,
        };
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        if text.chars().count() > MAX_GET {
            let shown: String = text.chars().take(MAX_GET).collect();
            return Ok(format!("[truncated to {MAX_GET} chars]\n{shown}"));
        }
        return Ok(text);
    }
    Ok("(clipboard empty or no tool found)".to_string())
}

fn set_clipboard(text: &str) -> anyhow::Result<()> {
    use std::io::Write;

    let attempts: &[&[&str]] = &[
        &["wl-copy"],
        &["xclip", "-selection", "clipboard"],
        &["xsel", "--clipboard", "--input"],
        &["pbcopy"],
    ];
    let mut last_err = None;
    for args in attempts {
        match Command::new(args[0])
            .args(&args[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(text.as_bytes());
                }
                let status = child.wait();
                if status.map(|s| s.success()).unwrap_or(false) {
                    return Ok(());
                }
            }
            Err(e) => last_err = Some(e.to_string()),
        }
    }
    anyhow::bail!("no clipboard tool available: {last_err:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_action_errors() {
        let tool = Clipboard;
        assert!(tool.run(&json!({"action": "bogus"})).is_err());
    }

    #[test]
    fn set_missing_text_errors() {
        let tool = Clipboard;
        assert!(tool.run(&json!({"action": "set"})).is_err());
    }
}