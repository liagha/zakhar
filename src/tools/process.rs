//! ps — list and inspect system processes; kill — signal them by pid or pattern.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::handler::Handler;
use crate::types::Tool;

const MAX_ROWS: usize = 500;

#[derive(Debug, Clone, Default)]
struct Proc {
    pid: i32,
    ppid: i32,
    state: String,
    uid: u32,
    rss_kb: usize,
    threads: usize,
    name: String,
    args: String,
}

fn read_procs() -> Vec<Proc> {
    if std::path::Path::new("/proc").is_dir() {
        linux_procs()
    } else {
        ps_procs()
    }
}

fn linux_procs() -> Vec<Proc> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return out;
    };
    for e in entries.flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        let base = e.path();
        if !base.join("stat").exists() {
            continue;
        }
        if let Ok(p) = parse_linux(&base, pid) {
            out.push(p);
        }
    }
    out
}

fn parse_linux(base: &std::path::Path, pid: i32) -> anyhow::Result<Proc> {
    let stat = std::fs::read_to_string(base.join("stat"))?;
    let close = stat
        .rfind(')')
        .ok_or_else(|| anyhow::anyhow!("no comm end"))?;
    let rest: Vec<&str> = stat[close + 2..].split_whitespace().collect();
    let state = rest.first().copied().unwrap_or("?").to_string();
    let ppid: i32 = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);

    let mut p = Proc {
        pid,
        ppid,
        state,
        ..Default::default()
    };

    let status = std::fs::read_to_string(base.join("status")).unwrap_or_default();
    for line in status.lines() {
        if let Some(v) = line.strip_prefix("Name:") {
            p.name = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("VmRSS:") {
            p.rss_kb = v
                .trim()
                .trim_end_matches(" kB")
                .parse()
                .unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("Uid:") {
            p.uid = v
                .split_whitespace()
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("Threads:") {
            p.threads = v.trim().parse().unwrap_or(0);
        }
    }

    let cmdline = std::fs::read(base.join("cmdline")).unwrap_or_default();
    p.args = cmdline
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect::<Vec<_>>()
        .join(" ");
    if p.args.is_empty() {
        p.args = p.name.clone();
    }
    if p.name.is_empty() {
        p.name = p
            .args
            .split_whitespace()
            .next()
            .unwrap_or("?")
            .to_string();
    }
    Ok(p)
}

fn ps_procs() -> Vec<Proc> {
    let mut out = Vec::new();
    let Ok(output) = std::process::Command::new("ps")
        .args(["-axo", "pid=,ppid=,stat=,comm=,args="])
        .output()
    else {
        return out;
    };
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut it = line.splitn(5, ' ');
        let (Some(pid), Some(ppid), Some(state), Some(name), Some(args)) = (
            it.next().and_then(|s| s.parse().ok()),
            it.next().and_then(|s| s.parse().ok()),
            it.next().map(str::trim).filter(|s| !s.is_empty()),
            it.next().map(str::trim).filter(|s| !s.is_empty()),
            it.next().map(str::trim).filter(|s| !s.is_empty()),
        ) else {
            continue;
        };
        out.push(Proc {
            pid,
            ppid,
            state: state.to_string(),
            uid: 0,
            rss_kb: 0,
            threads: 0,
            name: name.to_string(),
            args: args.to_string(),
        });
    }
    out
}

fn users() -> HashMap<u32, String> {
    let mut map = HashMap::new();
    let Ok(passwd) = std::fs::read_to_string("/etc/passwd") else {
        return map;
    };
    for line in passwd.lines() {
        let mut it = line.split(':');
        let (Some(name), Some(_pw), Some(uid)) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        if let Ok(uid) = uid.parse::<u32>() {
            map.entry(uid).or_insert_with(|| name.to_string());
        }
    }
    map
}

pub struct Ps;
impl Handler for Ps {
    fn spec(&self) -> Tool {
        Tool::function("ps", "List or inspect system-wide processes (read-only). action='list' returns processes (optional 'filter' substring over name/args, 'limit' capped at 500, default 50); action='info' shows full detail for one or more 'pid's, comma-separated, or 'all'. Use to see what's running or find a process to kill.", json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["list", "info"], "description": "What to do" },
                "filter": { "type": "string", "description": "Case-insensitive substring over process name or args (for action=list)" },
                "limit": { "type": "integer", "description": "Max rows (for action=list, default 50, max 500)" },
                "pid": { "type": "string", "description": "Process id(s), comma-separated, or 'all' (for action=info)" }
            },
            "required": ["action"]
        }))
    }

    fn run(&self, args: &Value) -> anyhow::Result<String> {
        match args["action"].as_str().unwrap_or("") {
            "list" => {
                let filter = args.get("filter").and_then(|v| v.as_str()).unwrap_or("");
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(50)
                    .clamp(1, MAX_ROWS as u64) as usize;
                let users = users();
                let mut procs = read_procs();
                if !filter.is_empty() {
                    let needle = filter.to_lowercase();
                    procs.retain(|p| {
                        p.name.to_lowercase().contains(&needle)
                            || p.args.to_lowercase().contains(&needle)
                    });
                }
                procs.sort_by_key(|p| p.pid);
                if procs.is_empty() {
                    return Ok("no matching processes".to_string());
                }
                let mut out = String::from("pid     ppid    user     rss_kb    stat  command\n");
                for p in &procs[..procs.len().min(limit)] {
                    let uid = p.uid.to_string();
                    let user = users.get(&p.uid).map(|s| s.as_str()).unwrap_or(&uid);
                    out.push_str(&format!(
                        "{:<7}{:<8}{:<9}{:<10}{:<6}{}\n",
                        p.pid, p.ppid, user, p.rss_kb, p.state, p.args
                    ));
                }
                out.push_str(&format!(
                    "({} of {} processes shown)\n",
                    procs.len().min(limit),
                    procs.len()
                ));
                Ok(out)
            }
            "info" => {
                let raw = args.get("pid").and_then(|v| v.as_str()).unwrap_or("");
                let mut procs = read_procs();
                procs.sort_by_key(|p| p.pid);
                let targets: Vec<i32> = if raw == "all" {
                    procs.iter().map(|p| p.pid).take(50).collect()
                } else {
                    raw.split(',').filter_map(|s| s.trim().parse().ok()).collect()
                };
                if targets.is_empty() {
                    anyhow::bail!("missing pid");
                }
                let users = users();
                let mut out = String::new();
                for pid in &targets {
                    match procs.iter().find(|p| p.pid == *pid) {
                        Some(p) => {
                            let uid = p.uid.to_string();
                            let user = users.get(&p.uid).map(|s| s.as_str()).unwrap_or(&uid);
                            out.push_str(&format!(
                                "pid {}  ppid {}  user {}\n  state {}  threads {}  rss {} kB\n  command {}\n",
                                p.pid, p.ppid, user, p.state, p.threads, p.rss_kb, p.args
                            ));
                        }
                        None => out.push_str(&format!("pid {pid}: not found\n")),
                    }
                }
                Ok(out)
            }
            a => anyhow::bail!("unknown action {a}"),
        }
    }
}

fn parse_signal(sig: &str) -> anyhow::Result<i32> {
    let sig = sig.trim();
    if let Ok(n) = sig.parse::<i32>() {
        return Ok(n);
    }
    Ok(match sig.to_uppercase().as_str() {
        "TERM" => libc::SIGTERM,
        "KILL" => libc::SIGKILL,
        "HUP" => libc::SIGHUP,
        "INT" => libc::SIGINT,
        "QUIT" => libc::SIGQUIT,
        "CONT" => libc::SIGCONT,
        "STOP" => libc::SIGSTOP,
        other => anyhow::bail!("unknown signal '{other}'"),
    })
}

pub struct Kill;
impl Handler for Kill {
    fn spec(&self) -> Tool {
        Tool::function("kill", "Send a signal to system processes (mutating — confirmed by default). Pass 'pid' (or comma-separated pids), or a 'pattern' matching process name or args. 'signal' defaults to TERM (use 9/KILL, 1/HUP, 2/INT, 18/CONT, 19/STOP). Use ps action=list to find pids first.", json!({
            "type": "object",
            "properties": {
                "pid": { "type": "string", "description": "Process id(s), comma-separated" },
                "pattern": { "type": "string", "description": "Substring matching process name or args" },
                "signal": { "type": "string", "description": "Signal: TERM (default), KILL, HUP, INT, QUIT, CONT, STOP, or numeric" }
            },
            "required": []
        }))
    }

    fn run(&self, args: &Value) -> anyhow::Result<String> {
        let pid_raw = args.get("pid").and_then(|v| v.as_str()).unwrap_or("");
        let pattern = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
        let signal = parse_signal(args.get("signal").and_then(|v| v.as_str()).unwrap_or("TERM"))?;

        let procs = read_procs();
        let mut targets: Vec<i32> = pid_raw
            .split(',')
            .filter_map(|s| s.trim().parse::<i32>().ok())
            .collect();
        if !pattern.is_empty() {
            let needle = pattern.to_lowercase();
            for p in &procs {
                if p.name.to_lowercase().contains(&needle)
                    || p.args.to_lowercase().contains(&needle)
                {
                    targets.push(p.pid);
                }
            }
        }
        targets.sort();
        targets.dedup();
        if targets.is_empty() {
            anyhow::bail!("no process matched; use ps to find a pid or pattern first");
        }
        if targets.len() > 100 {
            anyhow::bail!("refusing to signal {} processes at once", targets.len());
        }

        let mut out = String::new();
        for pid in &targets {
            let ret = unsafe { libc::kill(*pid, signal) };
            if ret == 0 {
                out.push_str(&format!("{pid}: signaled\n"));
            } else {
                let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(-1);
                if errno == libc::ESRCH {
                    out.push_str(&format!("{pid}: not found\n"));
                } else if errno == libc::EPERM {
                    out.push_str(&format!("{pid}: permission denied\n"));
                } else {
                    out.push_str(&format!("{pid}: error {errno}\n"));
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ps_lists_this_process() {
        let tool = Ps;
        let out = tool
            .run(&json!({ "action": "list", "filter": "zakhar" }))
            .unwrap();
        assert!(out.contains("zakhar"), "expected self in ps output:\n{out}");
    }

    #[test]
    fn ps_rejects_bad_action() {
        let tool = Ps;
        assert!(tool.run(&json!({ "action": "bogus" })).is_err());
    }

    #[test]
    fn kill_signal_zero_checks_liveness() {
        let tool = Kill;
        let pid = std::process::id();
        let out = tool
            .run(&json!({ "pid": pid.to_string(), "signal": "0" }))
            .unwrap();
        assert!(out.contains(&format!("{pid}: signaled")), "got: {out}");
    }

    #[test]
    fn kill_missing_process() {
        let tool = Kill;
        let out = tool
            .run(&json!({ "pid": "999999999", "signal": "TERM" }))
            .unwrap();
        assert!(out.contains("not found"), "got: {out}");
    }
}