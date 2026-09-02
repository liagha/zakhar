use std::collections::HashMap;
use std::path::Path;

use serde_json::{json, Value};

use crate::types::{Function, Tool};

type Executor = Box<dyn Fn(&Value) -> anyhow::Result<String> + Send + Sync>;

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
    let executor: Executor =
        Box::new(|args| {
            let cmd = args["command"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("missing command"))?;
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(cmd)
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
        });
    tools.insert(
        "bash".to_string(),
        ToolDef {
            tool: Tool {
                tool_type: "function".to_string(),
                function: Function {
                    name: "bash".to_string(),
                    description: "Run a shell command. Returns stdout+stderr.".to_string(),
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "command": {
                                "type": "string",
                                "description": "Shell command to execute"
                            }
                        },
                        "required": ["command"]
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
