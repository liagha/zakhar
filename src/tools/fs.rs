use std::path::Path;

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

pub struct Read;
impl Handler for Read {
    fn spec(&self) -> Tool {
        def("read", "Read the contents of a file.", json!({
            "type": "object",
            "properties": { "path": { "type": "string", "description": "File path to read" } },
            "required": ["path"]
        }))
    }
    fn run(&self, args: &Value) -> anyhow::Result<String> {
        let path = args["path"].as_str().ok_or_else(|| anyhow::anyhow!("missing path"))?;
        Ok(std::fs::read_to_string(path)?)
    }
}

pub struct Write;
impl Handler for Write {
    fn spec(&self) -> Tool {
        def("write", "Write content to a file.", json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path to write" },
                "content": { "type": "string", "description": "Content to write" }
            },
            "required": ["path", "content"]
        }))
    }
    fn run(&self, args: &Value) -> anyhow::Result<String> {
        let path = args["path"].as_str().ok_or_else(|| anyhow::anyhow!("missing path"))?;
        let content = args["content"].as_str().ok_or_else(|| anyhow::anyhow!("missing content"))?;
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
        Ok(format!("wrote {}", content.len()))
    }
}

pub struct Edit;
impl Handler for Edit {
    fn spec(&self) -> Tool {
        def("edit", "Perform exact string replacement in a file. old_string must match file content exactly once; use replace_all=true to replace all occurrences. Prefer write for new files.", json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path to edit" },
                "old_string": { "type": "string", "description": "Exact text to replace, must match file content exactly once" },
                "new_string": { "type": "string", "description": "Replacement text" },
                "replace_all": { "type": "boolean", "description": "Replace all occurrences instead of requiring unique match (default false)" }
            },
            "required": ["path", "old_string", "new_string"]
        }))
    }
    fn run(&self, args: &Value) -> anyhow::Result<String> {
        let path = args["path"].as_str().ok_or_else(|| anyhow::anyhow!("missing path"))?;
        let old = args["old_string"].as_str().ok_or_else(|| anyhow::anyhow!("missing old_string"))?;
        let new = args["new_string"].as_str().ok_or_else(|| anyhow::anyhow!("missing new_string"))?;
        let replace_all = args.get("replace_all").and_then(|v| v.as_bool()).unwrap_or(false);
        let content = std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("read {path}: {e}"))?;
        if replace_all {
            if !content.contains(old) {
                anyhow::bail!("old_string not found in {path}");
            }
            let count = content.matches(old).count();
            std::fs::write(path, content.replace(old, new))?;
            Ok(format!("replaced {count} occurrence(s) in {path}"))
        } else {
            let count = content.matches(old).count();
            if count == 0 {
                anyhow::bail!("old_string not found in {path}");
            }
            if count > 1 {
                anyhow::bail!("Found {count} matches for old_string in {path}. Provide more surrounding lines to make it unique or set replace_all=true");
            }
            std::fs::write(path, content.replacen(old, new, 1))?;
            Ok(format!("replaced 1 occurrence in {path}"))
        }
    }
}

pub struct Glob;
impl Handler for Glob {
    fn spec(&self) -> Tool {
        def("glob", "Find files matching a glob pattern.", json!({
            "type": "object",
            "properties": { "pattern": { "type": "string", "description": "Glob pattern (e.g. src/**/*.rs)" } },
            "required": ["pattern"]
        }))
    }
    fn run(&self, args: &Value) -> anyhow::Result<String> {
        let pattern = args["pattern"].as_str().ok_or_else(|| anyhow::anyhow!("missing pattern"))?;
        let mut results = Vec::new();
        for entry in glob::glob(pattern)? {
            match entry {
                Ok(path) => results.push(path.display().to_string()),
                Err(e) => results.push(format!("error: {e}")),
            }
            if results.len() >= 100 {
                break;
            }
        }
        Ok(results.join("\n"))
    }
}

pub struct Grep;
impl Handler for Grep {
    fn spec(&self) -> Tool {
        def("grep", "Search file contents with regex. Returns file:line:content matches.", json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Regex pattern to search for" },
                "path": { "type": "string", "description": "Directory or file to search in (default: current dir)" }
            },
            "required": ["pattern"]
        }))
    }
    fn run(&self, args: &Value) -> anyhow::Result<String> {
        let pattern = args["pattern"].as_str().ok_or_else(|| anyhow::anyhow!("missing pattern"))?;
        let path = args["path"].as_str().unwrap_or(".");
        let output = std::process::Command::new("grep").arg("-rn").arg(pattern).arg(path).output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.is_empty() {
            Ok("no matches".to_string())
        } else {
            let lines: Vec<&str> = stdout.lines().take(100).collect();
            Ok(lines.join("\n"))
        }
    }
}
