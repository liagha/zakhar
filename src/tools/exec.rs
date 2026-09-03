use std::collections::HashMap;
use std::io::Write as _;
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

fn stream(stream: std::process::ChildStdout, buffer: Arc<Mutex<Buffer>>) {
    use std::io::BufRead;
    for line in std::io::BufReader::new(stream).lines() {
        if let Ok(line) = line {
            buffer.lock().unwrap().push(&format!("{line}\n"));
        } else {
            break;
        }
    }
}

fn stream_err(stream: std::process::ChildStderr, buffer: Arc<Mutex<Buffer>>) {
    use std::io::BufRead;
    for line in std::io::BufReader::new(stream).lines() {
        if let Ok(line) = line {
            let mut b = buffer.lock().unwrap();
            b.push(&format!("[stderr] {line}\n"));
        } else {
            break;
        }
    }
}

struct Job {
    command: String,
    file: String,
}

static TASKS: OnceLock<Mutex<HashMap<String, Job>>> = OnceLock::new();

pub struct Bash;
impl Handler for Bash {
    fn spec(&self) -> Tool {
        def("bash", "Run a shell command. Returns stdout+stderr. Set detach=true to run without waiting, then use task to check.", json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Shell command to execute" },
                "detach": { "type": "boolean", "description": "Run in background, return task id immediately (default false)" }
            },
            "required": ["command"]
        }))
    }
    fn run(&self, args: &Value) -> anyhow::Result<String> {
        let command = args["command"].as_str().ok_or_else(|| anyhow::anyhow!("missing command"))?.to_string();
        let detach = args.get("detach").and_then(|v| v.as_bool()).unwrap_or(false);
        if detach {
            let id = &uuid::Uuid::new_v4().to_string()[..8];
            let file = format!("/tmp/zakhar_bg_{id}.log");
            std::fs::write(&file, "")?;
            let out_file = file.clone();
            let out_command = command.clone();
            let out_id = id.to_string();
            std::thread::spawn(move || {
                let out = std::process::Command::new("sh").arg("-c").arg(&out_command).output();
                let mut content = String::new();
                match out {
                    Ok(o) => {
                        if !o.stdout.is_empty() {
                            content.push_str(&String::from_utf8_lossy(&o.stdout));
                        }
                        if !o.stderr.is_empty() {
                            if !content.is_empty() {
                                content.push('\n');
                            }
                            content.push_str(&String::from_utf8_lossy(&o.stderr));
                        }
                        if content.is_empty() {
                            content = format!("exit code: {}", o.status.code().unwrap_or(-1));
                        } else {
                            content.push_str(&format!("\n[exit code: {}]", o.status.code().unwrap_or(-1)));
                        }
                    }
                    Err(e) => content = format!("spawn error: {e}"),
                }
                let _ = std::fs::write(&out_file, content);
            });
            let store = TASKS.get_or_init(|| Mutex::new(HashMap::new()));
            store.lock().unwrap().insert(
                id.to_string(),
                Job { command: command.clone(), file: file.clone() },
            );
            Ok(format!("background task {out_id} started: {command} (log: {file}) use task to check"))
        } else {
            let output = std::process::Command::new("sh").arg("-c").arg(&command).output()?;
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
        def("task", "Inspect detached bash tasks. action='output' reads a task's log (needs task_id); action='list' lists all tasks.", json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["output", "list"], "description": "What to do with tasks" },
                "task_id": { "type": "string", "description": "Task id from bash detach=true (for action=output)" }
            },
            "required": ["action"]
        }))
    }
    fn run(&self, args: &Value) -> anyhow::Result<String> {
        match args["action"].as_str().unwrap_or("") {
            "output" => {
                let id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
                if id.is_empty() {
                    anyhow::bail!("missing task_id");
                }
                let store = TASKS.get().ok_or_else(|| anyhow::anyhow!("no background tasks"))?;
                let tasks = store.lock().unwrap();
                let task = tasks.get(id).ok_or_else(|| anyhow::anyhow!("unknown task {id}. Use task action=list"))?;
                let content = std::fs::read_to_string(&task.file).unwrap_or_else(|_| "(no output yet)".to_string());
                if content.is_empty() {
                    Ok(format!("task {id} ({}) still running, log: {} (empty so far)", task.command, task.file))
                } else {
                    Ok(format!("task {id} ({}):\n{}", task.command, content))
                }
            }
            "list" => {
                let store = TASKS.get().ok_or_else(|| anyhow::anyhow!("no background tasks yet"))?;
                let tasks = store.lock().unwrap();
                if tasks.is_empty() {
                    return Ok("no background tasks".to_string());
                }
                let mut out = String::new();
                for (id, t) in tasks.iter() {
                    let size = std::fs::metadata(&t.file).map(|m| m.len()).unwrap_or(0);
                    out.push_str(&format!("{}: {} (log: {}, {} bytes)\n", id, t.command, t.file, size));
                }
                Ok(out)
            }
            a => anyhow::bail!("unknown action {a}"),
        }
    }
}

pub struct Watch;
impl Handler for Watch {
    fn spec(&self) -> Tool {
        def("watch", "Run a long-lived command and interact with it like a parent process. action='start' spawns the command and returns a task id; action='read' returns output that arrived since your last read (plus exit status if it finished); action='send' writes input to the process's stdin; action='stop' terminates it. Use for servers, tail -f, longs-running tools you need to monitor across turns. Output is capped.", json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["start", "read", "send", "stop"], "description": "What to do with the process" },
                "command": { "type": "string", "description": "Command to run (for action=start)" },
                "task_id": { "type": "string", "description": "Process id (for read/send/stop)" },
                "input": { "type": "string", "description": "Line to write to the process stdin (for action=send)" },
                "cwd": { "type": "string", "description": "Working directory (for action=start, default current)" }
            },
            "required": ["action"]
        }))
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
                cmd.arg("-c").arg(command).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
                if let Some(dir) = cwd {
                    cmd.current_dir(dir);
                }
                let mut child = cmd.spawn()?;
                let stdin = child.stdin.take().ok_or_else(|| anyhow::anyhow!("no stdin"))?;
                let stdout = child.stdout.take().ok_or_else(|| anyhow::anyhow!("no stdout"))?;
                let stderr = child.stderr.take().ok_or_else(|| anyhow::anyhow!("no stderr"))?;
                let buffer = Arc::new(Mutex::new(Buffer { text: String::new(), cap: 100_000 }));
                let b_out = Arc::clone(&buffer);
                let b_err = Arc::clone(&buffer);
                std::thread::spawn(move || stream(stdout, b_out));
                std::thread::spawn(move || stream_err(stderr, b_err));
                let mut store = watch_store().lock().unwrap();
                store.insert(
                    id.to_string(),
                    Watcher { child, stdin, buffer, cursor: 0 },
                );
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
                let mut w = store.remove(id).ok_or_else(|| anyhow::anyhow!("unknown task {id}"))?;
                let _ = w.child.kill();
                let _ = w.child.wait();
                w.cursor = w.buffer.lock().unwrap().text.chars().count();
                Ok(format!("stopped task {id}; final output:\n{}", w.buffer.lock().unwrap().text))
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
    fn watch_start_send_read_stop() {
        let tool = Watch;
        assert!(tool.run(&json!({"action": "start"})).is_err(), "start without command");
        assert!(tool.run(&json!({"action": "read", "task_id": "nope"})).is_err(), "unknown id");
        assert!(tool.run(&json!({"action": "send", "task_id": "nope", "input": "x"})).is_err(), "unknown id send");

        let started = tool
            .run(&json!({"action": "start", "command": "cat"}))
            .unwrap();
        let id = started.split("task ").nth(1).and_then(|s| s.split(':').next()).unwrap().to_string();

        tool.run(&json!({"action": "send", "task_id": &id, "input": "hello"})).unwrap();
        let out = wait_for(&tool, &id, "hello");
        assert!(out.contains("hello"), "expected echoed hello, got: {out}");

        tool.run(&json!({"action": "send", "task_id": &id, "input": "world"})).unwrap();
        let out = wait_for(&tool, &id, "world");
        assert!(out.contains("world"), "expected echoed world, got: {out}");

        let stopped = tool.run(&json!({"action": "stop", "task_id": &id})).unwrap();
        assert!(stopped.contains("stopped task"));
    }
}
