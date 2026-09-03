use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, serde::Deserialize)]
struct Hook {
    #[allow(dead_code)]
    #[serde(rename = "type")]
    hook_type: String,
    command: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct HookEntry {
    matcher: String,
    hooks: Vec<Hook>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
struct HooksConfig {
    #[serde(default)]
    hooks: HashMap<String, Vec<HookEntry>>,
}

fn load_config() -> Option<HooksConfig> {
    for path in [".zakhar/hooks.json", ".opencode/hooks.json", "hooks.json"] {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(cfg) = serde_json::from_str::<HooksConfig>(&text) {
                return Some(cfg);
            }
            if let Ok(map) = serde_json::from_str::<HashMap<String, Vec<HookEntry>>>(&text) {
                return Some(HooksConfig { hooks: map });
            }
        }
    }
    None
}

fn matches(matcher: &str, tool: &str) -> bool {
    if matcher == "*" || matcher.is_empty() {
        return true;
    }
    if matcher == tool {
        return true;
    }
    // simple regex via contains or |
    if matcher.contains('|') {
        for part in matcher.split('|') {
            if part.trim() == tool {
                return true;
            }
        }
    }
    // fallback substring
    tool.contains(matcher) || matcher.contains(tool)
}

pub fn run_pre(tool: &str, args: &Value) -> Result<(), String> {
    let cfg = match load_config() {
        Some(c) => c,
        None => return Ok(()),
    };
    let key = "PreToolUse";
    if let Some(entries) = cfg.hooks.get(key) {
        for entry in entries {
            if matches(&entry.matcher, tool) {
                for hook in &entry.hooks {
                    let out = std::process::Command::new("sh")
                        .arg("-c")
                        .arg(&hook.command)
                        .env("ZAKHAR_TOOL", tool)
                        .env("ZAKHAR_ARGS", args.to_string())
                        .output();
                    match out {
                        Ok(o) => {
                            let stdout = String::from_utf8_lossy(&o.stdout);
                            let stderr = String::from_utf8_lossy(&o.stderr);
                            if !o.status.success() {
                                let msg = if !stderr.trim().is_empty() { stderr.trim().to_string() } else { stdout.trim().to_string() };
                                return Err(format!("pre-hook blocked {tool}: {} (exit {})", msg, o.status.code().unwrap_or(-1)));
                            }
                        }
                        Err(e) => return Err(format!("pre-hook spawn failed: {e}")),
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn run_post(tool: &str, args: &Value, output: &str) {
    let cfg = match load_config() {
        Some(c) => c,
        None => return,
    };
    let key = "PostToolUse";
    if let Some(entries) = cfg.hooks.get(key) {
        for entry in entries {
            if matches(&entry.matcher, tool) {
                for hook in &entry.hooks {
                    let _ = std::process::Command::new("sh")
                        .arg("-c")
                        .arg(&hook.command)
                        .env("ZAKHAR_TOOL", tool)
                        .env("ZAKHAR_ARGS", args.to_string())
                        .env("ZAKHAR_OUTPUT", output.chars().take(2000).collect::<String>())
                        .output();
                }
            }
        }
    }
}
