use std::collections::HashMap;
use std::io::Write as _;
use std::os::unix::process::CommandExt as _;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{json, Value};

use crate::handler::Handler;
use crate::types::{Function, Tool};

fn def(name: &str, description: &str, parameters: Value) -> Tool {
    Tool {
        tool_type: "function".to_string(),
        function: Function {
            name: name.to_string(),
            description: description.to_string(),
            parameters,
        },
    }
}

struct Buffer {
    text: String,
    cap: usize,
}

impl Buffer {
    fn new(cap: usize) -> Self {
        Self { text: String::new(), cap }
    }

    fn push(&mut self, s: &str) {
        self.text.push_str(s);
        if self.text.len() > self.cap {
            let drop = self.text.len() - self.cap;
            if let Some(idx) = self.text.char_indices().map(|(i, _)| i).nth(drop) {
                self.text.drain(..idx);
            } else {
                self.text.clear();
            }
        }
    }
}

fn stream(stdout: std::process::ChildStdout, buffer: Arc<Mutex<Buffer>>) {
    use std::io::BufRead;
    for line in std::io::BufReader::new(stdout).lines() {
        if let Ok(line) = line {
            buffer.lock().unwrap().push(&format!("{line}\n"));
        } else {
            break;
        }
    }
}

fn stream_err(stderr: std::process::ChildStderr, buffer: Arc<Mutex<Buffer>>) {
    use std::io::BufRead;
    for line in std::io::BufReader::new(stderr).lines() {
        if let Ok(line) = line {
            buffer.lock().unwrap().push(&format!("[stderr] {line}\n"));
        } else {
            break;
        }
    }
}

struct BgJob {
    command: String,
    child: Child,
    buffer: Arc<Mutex<Buffer>>,
}

impl BgJob {
    fn output(&self) -> String {
        let b = self.buffer.lock().unwrap();
        if b.text.is_empty() {
            "(no output yet)".to_string()
        } else {
            b.text.clone()
        }
    }

    fn is_running(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_none()
    }

    fn final_output(mut self) -> String {
        self.kill_group();
        let _ = self.child.wait();
        let code = self.child.wait().ok().and_then(|s| s.code()).unwrap_or(-1);
        let mut out = self.buffer.lock().unwrap().text.clone();
        if out.is_empty() {
            out = format!("[exit code: {code}]");
        } else {
            out.push_str(&format!("\n[exit code: {code}]"));
        }
        out
    }

    fn kill_group(&self) {
        #[cfg(unix)]
        {
            let pid = self.child.id() as i32;
            unsafe { libc::kill(-pid, libc::SIGKILL); }
        }
        #[cfg(not(unix))]
        {
            let _ = self.child.kill();
        }
    }
}

static TASKS: OnceLock<Mutex<HashMap<String, BgJob>>> = OnceLock::new();

fn tasks() -> &'static Mutex<HashMap<String, BgJob>> {
    TASKS.get_or_init(|| Mutex::new(HashMap::new()))
}

struct Watcher {
    child: Child,
    stdin: ChildStdin,
    buffer: Arc<Mutex<Buffer>>,
    cursor: usize,
}

static WATCH: OnceLock<Mutex<HashMap<String, Watcher>>> = OnceLock::new();

fn watch_store() -> &'static Mutex<HashMap<String, Watcher>> {
    WATCH.get_or_init(|| Mutex::new(HashMap::new()))
}

pub struct Bash;
impl Handler for Bash {
    fn spec(&self) -> Tool {
        def(
            "bash",
            "Run a shell command. Returns stdout+stderr. Set detach=true to run in background and return a task id immediately. Use task to check output or kill.",
            json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Shell command to execute" },
                    "detach": { "type": "boolean", "description": "Run in background, return task id immediately (default false)" }
                },
                "required": ["command"]
            }),
        )
    }

    fn run(&self, args: &Value) -> anyhow::Result<String> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing command"))?
            .to_string();
        let detach = args.get("detach").and_then(|v| v.as_bool()).unwrap_or(false);

        if detach {
            let id = &uuid::Uuid::new_v4().to_string()[..8];
            let mut child = Command::new("sh")
                .arg("-c")
                .arg(&command)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .process_group(0)
                .spawn()?;
            let stdout = child.stdout.take().ok_or_else(|| anyhow::anyhow!("no stdout"))?;
            let stderr = child.stderr.take().ok_or_else(|| anyhow::anyhow!("no stderr"))?;
            let buffer = Arc::new(Mutex::new(Buffer::new(100_000)));
            let b_out = Arc::clone(&buffer);
            let b_err = Arc::clone(&buffer);
            std::thread::spawn(move || stream(stdout, b_out));
            std::thread::spawn(move || stream_err(stderr, b_err));
            tasks().lock().unwrap().insert(
                id.to_string(),
                BgJob { command: command.clone(), child, buffer },
            );
            Ok(format!("background task {id}: {command} (use task to check or kill)"))
        } else {
            let output = Command::new("sh")
                .arg("-c")
                .arg(&command)
                .output()?;
            let mut result = String::new();
            if !output.stdout.is_empty() {
                result.push_str(&String::from_utf8_lossy(&output.stdout));
            }
            if !output.stderr.is_empty() {
                if !result.is_empty() {
                    result.push('\n');
                }
                result.push_str(&String::from_utf8_lossy(&output.stderr));
            }
            if result.is_empty() {
                result = format!("exit code: {}", output.status.code().unwrap_or(-1));
            }
            Ok(result)
        }
    }
}

pub struct Task;
impl Handler for Task {
    fn spec(&self) -> Tool {
        def(
            "task",
            "Manage background bash tasks. action='list' shows all tasks; action='output' reads a task's output; action='kill' terminates one or more tasks (pass task_id or task_ids array, or kill='all').",
            json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["list", "output", "kill"], "description": "What to do" },
                    "task_id": { "type": "string", "description": "Single task id (for output or kill)" },
                    "task_ids": { "type": "array", "items": { "type": "string" }, "description": "Multiple task ids to kill at once" },
                    "kill": { "type": "string", "description": "Set to 'all' to kill all background tasks" }
                },
                "required": ["action"]
            }),
        )
    }

    fn run(&self, args: &Value) -> anyhow::Result<String> {
        match args["action"].as_str().unwrap_or("") {
            "list" => {
                let store = tasks();
                let mut tasks = store.lock().unwrap();
                if tasks.is_empty() {
                    return Ok("no background tasks".to_string());
                }
                let mut out = String::new();
                for (id, t) in tasks.iter_mut() {
                    let running = t.child.try_wait().ok().flatten().is_none();
                    let status = if running { "running" } else { "finished" };
                    let size = t.buffer.lock().unwrap().text.len();
                    out.push_str(&format!("{id}: {} [{}] ({} bytes)\n", t.command, status, size));
                }
                Ok(out)
            }
            "output" => {
                let id = args
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if id.is_empty() {
                    anyhow::bail!("missing task_id");
                }
                let store = tasks();
                let mut map = store.lock().unwrap();
                let job = map.get_mut(id).ok_or_else(|| anyhow::anyhow!("unknown task {id}. Use task action=list"))?;
                let running = job.is_running();
                let output = job.output();
                if running {
                    Ok(format!("task {id} ({}) [running]:\n{}", job.command, output))
                } else {
                    Ok(format!("task {id} ({}) [finished]:\n{}", job.command, output))
                }
            }
            "kill" => {
                let all = args.get("kill").and_then(|v| v.as_str()) == Some("all");
                let single = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
                let multi: Vec<String> = args
                    .get("task_ids")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();

                let mut ids: Vec<String> = if all {
                    let store = tasks();
                    store.lock().unwrap().keys().cloned().collect()
                } else if !multi.is_empty() {
                    multi
                } else if !single.is_empty() {
                    vec![single.to_string()]
                } else {
                    anyhow::bail!("pass task_id, task_ids, or kill='all'");
                };

                ids.sort();
                ids.dedup();

                let store = tasks();
                let mut map = store.lock().unwrap();
                let mut results = Vec::new();

                for id in &ids {
                    match map.remove(id) {
                        Some(job) => {
                            let output = job.final_output();
                            results.push(format!("{id}: killed\n{output}"));
                        }
                        None => {
                            results.push(format!("{id}: not found"));
                        }
                    }
                }

                Ok(results.join("\n\n"))
            }
            a => anyhow::bail!("unknown action {a}"),
        }
    }
}

pub struct Watch;
impl Handler for Watch {
    fn spec(&self) -> Tool {
        def(
            "watch",
            "Run a long-lived command and interact with it like a parent process. action='start' spawns the command; action='read' returns new output; action='send' writes to stdin; action='stop' terminates it. Use for servers, tail -f, or interactive tools you need to monitor across turns. Output is capped.",
            json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["start", "read", "send", "stop"], "description": "What to do with the process" },
                    "command": { "type": "string", "description": "Command to run (for action=start)" },
                    "task_id": { "type": "string", "description": "Process id (for read/send/stop)" },
                    "input": { "type": "string", "description": "Line to write to the process stdin (for action=send)" },
                    "cwd": { "type": "string", "description": "Working directory (for action=start, default current)" }
                },
                "required": ["action"]
            }),
        )
    }

    fn run(&self, args: &Value) -> anyhow::Result<String> {
        match args["action"].as_str().unwrap_or("") {
            "start" => {
                let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
                if command.is_empty() {
                    anyhow::bail!("missing command");
                }
                let cwd = args.get("cwd").and_then(|v| v.as_str());
                let id = &uuid::Uuid::new_v4().to_string()[..8];
                let mut cmd = Command::new("sh");
                cmd.arg("-c")
                    .arg(command)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                if let Some(dir) = cwd {
                    cmd.current_dir(dir);
                }
                let mut child = cmd.process_group(0).spawn()?;
                let stdin = child.stdin.take().ok_or_else(|| anyhow::anyhow!("no stdin"))?;
                let stdout = child.stdout.take().ok_or_else(|| anyhow::anyhow!("no stdout"))?;
                let stderr = child.stderr.take().ok_or_else(|| anyhow::anyhow!("no stderr"))?;
                let buffer = Arc::new(Mutex::new(Buffer::new(100_000)));
                let b_out = Arc::clone(&buffer);
                let b_err = Arc::clone(&buffer);
                std::thread::spawn(move || stream(stdout, b_out));
                std::thread::spawn(move || stream_err(stderr, b_err));
                watch_store()
                    .lock()
                    .unwrap()
                    .insert(id.to_string(), Watcher { child, stdin, buffer, cursor: 0 });
                Ok(format!("started task {id}: {command} (use watch read task_id={id})"))
            }
            "read" => {
                let id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
                if id.is_empty() {
                    anyhow::bail!("missing task_id");
                }
                let mut store = watch_store().lock().unwrap();
                let w = store.get_mut(id).ok_or_else(|| anyhow::anyhow!("unknown task {id}"))?;
                let text = w.buffer.lock().unwrap().text.clone();
                let new: String = text.chars().skip(w.cursor).collect();
                w.cursor = text.chars().count();
                let finished = w.child.try_wait()?;
                let mut out = String::new();
                if let Some(status) = finished {
                    out.push_str(&format!("[process exited: {status}]\n"));
                } else {
                    out.push_str("[still running]\n");
                }
                if new.is_empty() {
                    out.push_str("(no new output since last read)");
                } else {
                    out.push_str(&new);
                }
                Ok(out)
            }
            "send" => {
                let id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
                let input = args.get("input").and_then(|v| v.as_str()).unwrap_or("");
                if id.is_empty() {
                    anyhow::bail!("missing task_id");
                }
                let mut store = watch_store().lock().unwrap();
                let w = store.get_mut(id).ok_or_else(|| anyhow::anyhow!("unknown task {id}"))?;
                if input.is_empty() {
                    anyhow::bail!("missing input");
                }
                w.stdin.write_all(input.as_bytes())?;
                w.stdin.write_all(b"\n")?;
                w.stdin.flush()?;
                Ok(format!("sent input to task {id}"))
            }
            "stop" => {
                let id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
                if id.is_empty() {
                    anyhow::bail!("missing task_id");
                }
                let mut store = watch_store().lock().unwrap();
                let mut w = store
                    .remove(id)
                    .ok_or_else(|| anyhow::anyhow!("unknown task {id}"))?;
                #[cfg(unix)]
                {
                    let pid = w.child.id() as i32;
                    unsafe { libc::kill(-pid, libc::SIGKILL); }
                }
                #[cfg(not(unix))]
                {
                    let _ = w.child.kill();
                }
                let _ = w.child.wait();
                w.cursor = w.buffer.lock().unwrap().text.chars().count();
                Ok(format!(
                    "stopped task {id}; final output:\n{}",
                    w.buffer.lock().unwrap().text
                ))
            }
            a => anyhow::bail!("unknown action {a}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for(tool: &Watch, id: &str, needle: &str) -> String {
        for _ in 0..50 {
            let out = tool.run(&json!({"action": "read", "task_id": id})).unwrap();
            if out.contains(needle) {
                return out;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        tool.run(&json!({"action": "read", "task_id": id})).unwrap()
    }

    #[test]
    fn bash_detach_and_task() {
        let bash = Bash;
        let task = Task;

        let started = bash
            .run(&json!({"command": "echo hello-bg", "detach": true}))
            .unwrap();
        let id = started
            .split("task ")
            .nth(1)
            .and_then(|s| s.split(':').next())
            .unwrap()
            .to_string();

        let list = task.run(&json!({"action": "list"})).unwrap();
        assert!(list.contains(&id));

        for _ in 0..30 {
            let out = task.run(&json!({"action": "output", "task_id": &id})).unwrap();
            if out.contains("hello-bg") {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("never saw output");
    }

    #[test]
    fn task_kill_single() {
        let _g = crate::memory::lock();
        let bash = Bash;
        let task = Task;

        let started = bash
            .run(&json!({"command": "sleep 999", "detach": true}))
            .unwrap();
        let id = started
            .split("task ")
            .nth(1)
            .and_then(|s| s.split(':').next())
            .unwrap()
            .to_string();

        std::thread::sleep(std::time::Duration::from_millis(50));
        let killed = task.run(&json!({"action": "kill", "task_id": &id})).unwrap();
        assert!(killed.contains("killed"));
        assert!(task.run(&json!({"action": "kill", "task_id": &id})).unwrap().contains("not found"));
    }

    #[test]
    fn task_kill_all() {
        let _g = crate::memory::lock();
        let bash = Bash;
        let task = Task;

        let r1 = bash.run(&json!({"command": "sleep 888", "detach": true})).unwrap();
        let r2 = bash.run(&json!({"command": "sleep 777", "detach": true})).unwrap();
        let _ = r1;
        let _ = r2;

        std::thread::sleep(std::time::Duration::from_millis(50));
        let killed = task.run(&json!({"action": "kill", "kill": "all"})).unwrap();
        assert!(killed.contains("killed"));
        let list = task.run(&json!({"action": "list"})).unwrap();
        assert!(list.contains("no background tasks"));
    }

    #[test]
    fn watch_start_send_read_stop() {
        let tool = Watch;
        assert!(tool.run(&json!({"action": "start"})).is_err());
        assert!(tool.run(&json!({"action": "read", "task_id": "nope"})).is_err());
        assert!(
            tool.run(&json!({"action": "send", "task_id": "nope", "input": "x"})).is_err()
        );

        let started = tool
            .run(&json!({"action": "start", "command": "cat"}))
            .unwrap();
        let id = started
            .split("task ")
            .nth(1)
            .and_then(|s| s.split(':').next())
            .unwrap()
            .to_string();

        tool.run(&json!({"action": "send", "task_id": &id, "input": "hello"}))
            .unwrap();
        let out = wait_for(&tool, &id, "hello");
        assert!(out.contains("hello"), "expected echoed hello, got: {out}");

        tool.run(&json!({"action": "send", "task_id": &id, "input": "world"}))
            .unwrap();
        let out = wait_for(&tool, &id, "world");
        assert!(out.contains("world"), "expected echoed world, got: {out}");

        let stopped = tool
            .run(&json!({"action": "stop", "task_id": &id}))
            .unwrap();
        assert!(stopped.contains("stopped task"));
    }
}
