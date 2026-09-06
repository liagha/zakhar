use std::path::PathBuf;

use serde_json::{json, Value};

use crate::handler::Handler;
use crate::types::Tool;

const CAPS: &[&str] = &[
    "git", "cargo", "python3", "node", "npm", "docker", "jq", "curl", "wget", "gh", "tmux",
    "ffmpeg", "magick", "grim", "slurp", "wl-copy", "xclip", "xsel", "grep", "rg", "fd",
    "clang", "gcc", "make", "pacman", "apt", "dnf", "nix", "flatpak", "snap",
];

pub struct Env;
impl Handler for Env {
    fn spec(&self) -> Tool {
        Tool::function("env", "Report the machine: os, arch, user, home, cwd, shell, terminal, display server (wayland/x11), timezone, and which capability commands (git, cargo, python3, ...) are on PATH. Use this to pick tools that exist on this machine before inventing commands.", json!({
            "type": "object",
            "properties": {},
            "required": []
        }))
    }

    fn run(&self, _args: &Value) -> anyhow::Result<String> {
        let mut out = String::new();
        out.push_str(&format!(
            "system: {} {} ({})\n",
            std::env::consts::OS,
            std::env::consts::ARCH,
            hostname()
        ));
        out.push_str(&format!(
            "user: {}  home: {}\n",
            env_or("USER", "?"),
            env_or("HOME", "?")
        ));
        out.push_str(&format!(
            "cwd: {}\n",
            std::env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "?".into())
        ));
        out.push_str(&format!(
            "shell: {}  terminal: {}\n",
            env_or("SHELL", "?"),
            env_or("TERM", "?")
        ));
        let display = if env_or("WAYLAND_DISPLAY", "").is_empty() {
            if env_or("DISPLAY", "").is_empty() {
                "none".to_string()
            } else {
                format!("x11 ({})", env_or("DISPLAY", ""))
            }
        } else {
            format!("wayland ({})", env_or("WAYLAND_DISPLAY", ""))
        };
        out.push_str(&format!(
            "display: {display}  session: {}\n",
            env_or("XDG_SESSION_TYPE", "?")
        ));
        out.push_str(&format!(
            "timezone: {}\n",
            chrono::Local::now().format("%Z (%z)")
        ));
        out.push_str("on path:");
        let caps = available();
        if caps.is_empty() {
            out.push_str(" none\n");
        } else {
            out.push('\n');
            for c in caps {
                out.push_str(&format!("  {c}\n"));
            }
        }
        Ok(out)
    }
}

fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| fallback.to_string())
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

fn available() -> Vec<&'static str> {
    let path = env_or("PATH", "");
    let mut caps: Vec<&'static str> = Vec::new();
    for cap in CAPS {
        if path
            .split(':')
            .map(PathBuf::from)
            .any(|d| d.join(cap).is_file())
        {
            caps.push(cap);
        }
    }
    caps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_system() {
        let tool = Env;
        let out = tool.run(&json!({})).unwrap();
        assert!(out.contains("system:"));
        assert!(out.contains("cwd:"));
        assert!(out.contains("on path:"));
    }
}