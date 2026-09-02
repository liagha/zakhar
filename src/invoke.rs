use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};

use crate::types::{Function, Tool};

type Executor = Box<dyn Fn(&Value) -> anyhow::Result<String> + Send + Sync>;

static TODOS: OnceLock<Mutex<Vec<Todo>>> = OnceLock::new();
static BG_TASKS: OnceLock<Mutex<HashMap<String, BgTask>>> = OnceLock::new();

struct BgTask {
    id: String,
    command: String,
    file: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Todo {
    content: String,
    status: String,
    priority: String,
}

pub const READONLY_TOOLS: &[&str] = &[
    "read", "glob", "grep", "ask_user", "todowrite", "skill", "task_output", "task_list", "slash", "delegate", "handoff",
];

pub struct Invoke {
    tools: HashMap<String, ToolDef>,
}

struct ToolDef {
    tool: Tool,
    executor: Executor,
}

impl Invoke {
    pub fn new() -> Self {
        let mut tools = HashMap::new();
        register_bash(&mut tools);
        register_read(&mut tools);
        register_write(&mut tools);
        register_edit(&mut tools);
        register_glob(&mut tools);
        register_grep(&mut tools);
        register_ask_user(&mut tools);
        register_todowrite(&mut tools);
        register_skill(&mut tools);
        register_task_output(&mut tools);
        register_task_list(&mut tools);
        Self { tools }
    }

    pub fn definitions(&self) -> Vec<Tool> {
        self.tools.values().map(|t| t.tool.clone()).collect()
    }

    pub fn filtered_definitions(&self, allowed: &[String]) -> Vec<Tool> {
        if allowed.is_empty() {
            return self.definitions();
        }
        self.tools
            .values()
            .filter(|t| allowed.contains(&t.tool.function.name))
            .map(|t| t.tool.clone())
            .collect()
    }

    pub fn exec(&self, name: &str, args: &Value) -> String {
        let def = match self.tools.get(name) {
            Some(d) => d,
            None => return format!("error: unknown tool: {name}"),
        };
        match (def.executor)(args) {
            Ok(v) => v,
            Err(e) => format!("error: {e}"),
        }
    }
}

fn register_bash(tools: &mut HashMap<String, ToolDef>) {
    let executor: Executor = Box::new(|args| {
        let cmd = args["command"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing command"))?
            .to_string();
        let bg = args
            .get("run_in_background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if bg {
            let id = &uuid::Uuid::new_v4().to_string()[..8];
            let file = format!("/tmp/zakhar_bg_{}.log", id);
            std::fs::write(&file, "")?;
            let file_clone = file.clone();
            let cmd_clone = cmd.clone();
            let id_owned = id.to_string();
            std::thread::spawn(move || {
                let out = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(&cmd_clone)
                    .output();
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
                let _ = std::fs::write(&file_clone, content);
            });
            let map = BG_TASKS.get_or_init(|| Mutex::new(HashMap::new()));
            map.lock().unwrap().insert(
                id.to_string(),
                BgTask {
                    id: id_owned.clone(),
                    command: cmd.clone(),
                    file: file.clone(),
                },
            );
            Ok(format!("background task {id_owned} started: {cmd} (log: {file}) use task_output to check"))
        } else {
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(&cmd)
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
    });
    tools.insert(
        "bash".to_string(),
        ToolDef {
            tool: Tool {
                tool_type: "function".to_string(),
                function: Function {
                    name: "bash".to_string(),
                    description: "Run a shell command. Returns stdout+stderr. Set run_in_background=true to run without waiting, then use task_output/task_list.".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "command": { "type": "string", "description": "Shell command to execute" },
                            "run_in_background": { "type": "boolean", "description": "Run in background, return task id immediately (default false)" }
                        },
                        "required": ["command"]
                    }),
                },
            },
            executor,
        },
    );
}

fn register_task_output(tools: &mut HashMap<String, ToolDef>) {
    let executor: Executor = Box::new(|args| {
        let id = args.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
        if id.is_empty() {
            anyhow::bail!("missing task_id");
        }
        let map = BG_TASKS.get().ok_or_else(|| anyhow::anyhow!("no background tasks"))?;
        let tasks = map.lock().unwrap();
        let task = tasks.get(id).ok_or_else(|| anyhow::anyhow!("unknown task {id}. Use task_list"))?;
        let content = std::fs::read_to_string(&task.file).unwrap_or_else(|_| "(no output yet)".to_string());
        if content.is_empty() {
            Ok(format!("task {id} ({}) still running, log: {} (empty so far)", task.command, task.file))
        } else {
            Ok(format!("task {id} ({}):\n{}", task.command, content))
        }
    });
    tools.insert(
        "task_output".to_string(),
        ToolDef {
            tool: Tool {
                tool_type: "function".to_string(),
                function: Function {
                    name: "task_output".to_string(),
                    description: "Get output of a background bash task. Use task_list to see ids.".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "task_id": { "type": "string", "description": "Background task id from bash run_in_background" }
                        },
                        "required": ["task_id"]
                    }),
                },
            },
            executor,
        },
    );
}

fn register_task_list(tools: &mut HashMap<String, ToolDef>) {
    let executor: Executor = Box::new(|_args| {
        let map = BG_TASKS.get().ok_or_else(|| anyhow::anyhow!("no background tasks yet"))?;
        let tasks = map.lock().unwrap();
        if tasks.is_empty() {
            return Ok("no background tasks".to_string());
        }
        let mut out = String::new();
        for (id, t) in tasks.iter() {
            let size = std::fs::metadata(&t.file).map(|m| m.len()).unwrap_or(0);
            out.push_str(&format!("{}: {} (log: {}, {} bytes)\n", id, t.command, t.file, size));
        }
        Ok(out)
    });
    tools.insert(
        "task_list".to_string(),
        ToolDef {
            tool: Tool {
                tool_type: "function".to_string(),
                function: Function {
                    name: "task_list".to_string(),
                    description: "List background bash tasks with ids and commands.".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {},
                        "required": []
                    }),
                },
            },
            executor,
        },
    );
}

fn register_read(tools: &mut HashMap<String, ToolDef>) {
    let executor: Executor =
        Box::new(|args| {
            let path = args["path"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing path"))?;
            let content = std::fs::read_to_string(path)?;
            Ok(content)
        });
    tools.insert(
        "read".to_string(),
        ToolDef {
            tool: Tool {
                tool_type: "function".to_string(),
                function: Function {
                    name: "read".to_string(),
                    description: "Read the contents of a file.".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "File path to read"
                            }
                        },
                        "required": ["path"]
                    }),
                },
            },
            executor,
        },
    );
}

fn register_write(tools: &mut HashMap<String, ToolDef>) {
    let executor: Executor =
        Box::new(|args| {
            let path = args["path"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing path"))?;
            let content = args["content"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing content"))?;
            if let Some(parent) = Path::new(path).parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, content)?;
            Ok(format!("wrote {}", content.len()))
        });
    tools.insert(
        "write".to_string(),
        ToolDef {
            tool: Tool {
                tool_type: "function".to_string(),
                function: Function {
                    name: "write".to_string(),
                    description: "Write content to a file.".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "File path to write"
                            },
                            "content": {
                                "type": "string",
                                "description": "Content to write"
                            }
                        },
                        "required": ["path", "content"]
                    }),
                },
            },
            executor,
        },
    );
}

fn register_edit(tools: &mut HashMap<String, ToolDef>) {
    let executor: Executor = Box::new(|args| {
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing path"))?;
        let old = args["old_string"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing old_string"))?;
        let new = args["new_string"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing new_string"))?;
        let replace_all = args
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("read {path}: {e}"))?;
        if replace_all {
            if !content.contains(old) {
                anyhow::bail!("old_string not found in {path}");
            }
            let count = content.matches(old).count();
            let updated = content.replace(old, new);
            std::fs::write(path, updated)?;
            Ok(format!("replaced {count} occurrence(s) in {path}"))
        } else {
            let count = content.matches(old).count();
            if count == 0 {
                anyhow::bail!("old_string not found in {path}");
            }
            if count > 1 {
                anyhow::bail!(
                    "Found {count} matches for old_string in {path}. Provide more surrounding lines to make it unique or set replace_all=true"
                );
            }
            let updated = content.replacen(old, new, 1);
            std::fs::write(path, updated)?;
            Ok(format!("replaced 1 occurrence in {path}"))
        }
    });
    tools.insert(
        "edit".to_string(),
        ToolDef {
            tool: Tool {
                tool_type: "function".to_string(),
                function: Function {
                    name: "edit".to_string(),
                    description: "Perform exact string replacement in a file. old_string must match file content exactly once; use replace_all=true to replace all occurrences. Prefer write for new files.".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "File path to edit" },
                            "old_string": { "type": "string", "description": "Exact text to replace, must match file content exactly once" },
                            "new_string": { "type": "string", "description": "Replacement text" },
                            "replace_all": { "type": "boolean", "description": "Replace all occurrences instead of requiring unique match (default false)" }
                        },
                        "required": ["path", "old_string", "new_string"]
                    }),
                },
            },
            executor,
        },
    );
}

fn register_glob(tools: &mut HashMap<String, ToolDef>) {
    let executor: Executor =
        Box::new(|args| {
            let pattern = args["pattern"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing pattern"))?;
            let entries = glob::glob(pattern)?;
            let mut results = Vec::new();
            for entry in entries {
                match entry {
                    Ok(path) => results.push(path.display().to_string()),
                    Err(e) => results.push(format!("error: {e}")),
                }
                if results.len() >= 100 {
                    break;
                }
            }
            Ok(results.join("\n"))
        });
    tools.insert(
        "glob".to_string(),
        ToolDef {
            tool: Tool {
                tool_type: "function".to_string(),
                function: Function {
                    name: "glob".to_string(),
                    description: "Find files matching a glob pattern.".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "pattern": {
                                "type": "string",
                                "description": "Glob pattern (e.g. src/**/*.rs)"
                            }
                        },
                        "required": ["pattern"]
                    }),
                },
            },
            executor,
        },
    );
}

fn register_grep(tools: &mut HashMap<String, ToolDef>) {
    let executor: Executor =
        Box::new(|args| {
            let pattern = args["pattern"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing pattern"))?;
            let path = args["path"].as_str().unwrap_or(".");
            let output = std::process::Command::new("grep")
                .arg("-rn")
                .arg(pattern)
                .arg(path)
                .output()?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.is_empty() {
                Ok("no matches".to_string())
            } else {
                let lines: Vec<&str> = stdout.lines().take(100).collect();
                Ok(lines.join("\n"))
            }
        });
    tools.insert(
        "grep".to_string(),
        ToolDef {
            tool: Tool {
                tool_type: "function".to_string(),
                function: Function {
                    name: "grep".to_string(),
                    description: "Search file contents with regex. Returns file:line:content matches.".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "pattern": {
                                "type": "string",
                                "description": "Regex pattern to search for"
                            },
                            "path": {
                                "type": "string",
                                "description": "Directory or file to search in (default: current dir)"
                            }
                        },
                        "required": ["pattern"]
                    }),
                },
            },
            executor,
        },
    );
}

fn register_ask_user(tools: &mut HashMap<String, ToolDef>) {
    let executor: Executor = Box::new(|args| {
        let questions = args
            .get("questions")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("missing questions"))?;
        if questions.is_empty() {
            anyhow::bail!("questions is empty");
        }
        let mut answers = Vec::new();
        for q in questions {
            let header = q.get("header").and_then(|v| v.as_str()).unwrap_or("Question");
            let question = q.get("question").and_then(|v| v.as_str()).unwrap_or("");
            let options = q.get("options").and_then(|v| v.as_array());
            println!("\n[ask_user] {}: {}", header, question);
            if let Some(opts) = options {
                for (i, opt) in opts.iter().enumerate() {
                    let label = opt.get("label").and_then(|v| v.as_str()).unwrap_or("");
                    let desc = opt.get("description").and_then(|v| v.as_str()).unwrap_or("");
                    println!("  {}. {} - {}", i + 1, label, desc);
                }
                print!("[ask_user] Enter choice(s) (number or label, comma for multiple): ");
            } else {
                print!("[ask_user] Your answer: ");
            }
            std::io::Write::flush(&mut std::io::stdout()).ok();
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            let input = input.trim().to_string();
            let answer = if let Some(opts) = options {
                // map numbers to labels
                let mut chosen = Vec::new();
                for part in input.split(',').map(|s| s.trim()) {
                    if let Ok(n) = part.parse::<usize>()
                        && n > 0 && n <= opts.len()
                            && let Some(l) = opts[n - 1].get("label").and_then(|v| v.as_str()) {
                                chosen.push(l.to_string());
                                continue;
                            }
                    if !part.is_empty() {
                        chosen.push(part.to_string());
                    }
                }
                if chosen.is_empty() { input.clone() } else { chosen.join(", ") }
            } else {
                input.clone()
            };
            answers.push(json!({"header": header, "question": question, "answer": answer}));
            println!("[ask_user] → answered: {}", answers.last().unwrap()["answer"].as_str().unwrap_or(""));
        }
        Ok(serde_json::to_string_pretty(&answers).unwrap_or_else(|_| format!("{:?}", answers)))
    });
    tools.insert(
        "ask_user".to_string(),
        ToolDef {
            tool: Tool {
                tool_type: "function".to_string(),
                function: Function {
                    name: "ask_user".to_string(),
                    description: "Ask the user clarifying questions with options. Use when requirements are ambiguous or you need a decision. Returns user's answers as JSON.".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "questions": {
                                "type": "array",
                                "description": "Questions to ask, each with header, question, options",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "header": { "type": "string", "description": "Short label (max 30 chars)" },
                                        "question": { "type": "string", "description": "Complete question" },
                                        "options": {
                                            "type": "array",
                                            "description": "Available choices",
                                            "items": {
                                                "type": "object",
                                                "properties": {
                                                    "label": { "type": "string" },
                                                    "description": { "type": "string" }
                                                },
                                                "required": ["label", "description"]
                                            }
                                        }
                                    },
                                    "required": ["header", "question", "options"]
                                }
                            }
                        },
                        "required": ["questions"]
                    }),
                },
            },
            executor,
        },
    );
}

fn register_todowrite(tools: &mut HashMap<String, ToolDef>) {
    let executor: Executor = Box::new(|args| {
        let todos = args
            .get("todos")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("missing todos"))?;
        let mut list = Vec::new();
        for t in todos {
            let content = t.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let status = t.get("status").and_then(|v| v.as_str()).unwrap_or("pending").to_string();
            let priority = t.get("priority").and_then(|v| v.as_str()).unwrap_or("medium").to_string();
            if !["pending", "in_progress", "completed", "cancelled"].contains(&status.as_str()) {
                anyhow::bail!("invalid status {status}");
            }
            list.push(Todo { content, status, priority });
        }
        let in_progress = list.iter().filter(|t| t.status == "in_progress").count();
        if in_progress > 1 {
            anyhow::bail!("only one task may be in_progress at a time, got {in_progress}");
        }
        let mutex = TODOS.get_or_init(|| Mutex::new(Vec::new()));
        *mutex.lock().unwrap() = list.clone();
        let mut out = String::new();
        out.push_str(&format!("[todowrite] {} todos:\n", list.len()));
        for t in &list {
            let icon = match t.status.as_str() {
                "pending" => "○",
                "in_progress" => "●",
                "completed" => "✓",
                "cancelled" => "✗",
                _ => "?",
            };
            out.push_str(&format!("  {} [{}] {} ({})\n", icon, t.status, t.content, t.priority));
        }
        println!("{out}");
        std::io::Write::flush(&mut std::io::stdout()).ok();
        Ok(out)
    });
    tools.insert(
        "todowrite".to_string(),
        ToolDef {
            tool: Tool {
                tool_type: "function".to_string(),
                function: Function {
                    name: "todowrite".to_string(),
                    description: "Create and maintain a task list. Use for multi-step work: one in_progress at a time, mark completed as you go. Call at start and on progress.".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "todos": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "content": { "type": "string" },
                                        "priority": { "type": "string", "enum": ["high", "medium", "low"] },
                                        "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "cancelled"] }
                                    },
                                    "required": ["content", "status", "priority"]
                                }
                            }
                        },
                        "required": ["todos"]
                    }),
                },
            },
            executor,
        },
    );
}

fn register_skill(tools: &mut HashMap<String, ToolDef>) {
    let executor: Executor = Box::new(|args| {
        let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() {
            // list available
            let mut skills = Vec::new();
            for dir in [".opencode/skills", ".claude/skills", "skills", ".zakhar/skills"] {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for e in entries.flatten() {
                        if let Ok(ft) = e.file_type()
                            && ft.is_dir()
                                && let Some(n) = e.file_name().to_str() {
                                    skills.push(format!("{n} ({dir}/{n})"));
                                }
                    }
                }
            }
            if let Some(home) = dirs::config_dir() {
                let p = home.join("zakhar/skills");
                if let Ok(entries) = std::fs::read_dir(p) {
                    for e in entries.flatten() {
                        if let Ok(ft) = e.file_type()
                            && ft.is_dir()
                                && let Some(n) = e.file_name().to_str() {
                                    skills.push(format!("{n} (config)"));
                                }
                    }
                }
            }
            if skills.is_empty() {
                return Ok("no skills found. Create .opencode/skills/<name>/SKILL.md".to_string());
            }
            return Ok(format!("available skills:\n{}", skills.join("\n")));
        }
        let candidates = [
            format!(".opencode/skills/{}/SKILL.md", name),
            format!(".claude/skills/{}/SKILL.md", name),
            format!("skills/{}/SKILL.md", name),
            format!(".zakhar/skills/{}/SKILL.md", name),
        ];
        let mut home_candidates = Vec::new();
        if let Some(home) = dirs::config_dir() {
            home_candidates.push(home.join(format!("zakhar/skills/{}/SKILL.md", name)));
            home_candidates.push(home.join(format!("opencode/skills/{}/SKILL.md", name)));
        }
        for p in candidates.iter().map(Path::new).chain(home_candidates.iter().map(|p| p.as_path())) {
            if p.exists() {
                let content = std::fs::read_to_string(p)?;
                println!("[skill] loaded {name} from {}", p.display());
                return Ok(content);
            }
        }
        anyhow::bail!("skill '{name}' not found. Tried: {:?} {:?}", candidates, home_candidates)
    });
    tools.insert(
        "skill".to_string(),
        ToolDef {
            tool: Tool {
                tool_type: "function".to_string(),
                function: Function {
                    name: "skill".to_string(),
                    description: "Load a skill's instructions (e.g., plan, agent-creator). Call when task matches a skill. With no name, lists available skills.".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "description": "Skill name from available_skills" }
                        },
                        "required": []
                    }),
                },
            },
            executor,
        },
    );
}
