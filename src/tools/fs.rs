use std::path::Path;

use serde_json::{json, Value};

use crate::handler::Handler;
use crate::types::Tool;

pub struct Read;
impl Handler for Read {
    fn spec(&self) -> Tool {
        Tool::function("read", "Read the contents of a file.", json!({
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
        Tool::function("write", "Write content to a file.", json!({
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
        Tool::function("edit", "Perform exact string replacement in a file. old_string must match file content exactly once; use replace_all=true to replace all occurrences. Prefer write for new files.", json!({
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
        Tool::function("glob", "Find files matching a glob pattern.", json!({
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
        Tool::function("grep", "Search file contents with regex. Returns file:line:content matches.", json!({
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_returns_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.txt");
        std::fs::write(&path, "hello").unwrap();
        assert_eq!(Read.run(&json!({"path": path})).unwrap(), "hello");
    }

    #[test]
    fn read_missing_path_errors() {
        let err = Read.run(&json!({})).unwrap_err().to_string();
        assert_eq!(err, "missing path");
    }

    #[test]
    fn write_creates_parents_and_reports_len() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/deep/note.txt");
        let out = Write.run(&json!({"path": path, "content": "abcd"})).unwrap();
        assert_eq!(out, "wrote 4");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "abcd");
    }

    #[test]
    fn write_missing_args_error() {
        let err = Write.run(&json!({})).unwrap_err().to_string();
        assert_eq!(err, "missing path");
        let err = Write.run(&json!({"path": "x"})).unwrap_err().to_string();
        assert_eq!(err, "missing content");
    }

    #[test]
    fn edit_unique_match_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.txt");
        std::fs::write(&path, "one two one").unwrap();
        let out = Edit
            .run(&json!({"path": path, "old_string": "two", "new_string": "TWO"}))
            .unwrap();
        assert_eq!(out, format!("replaced 1 occurrence in {}", path.display()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one TWO one");
    }

    #[test]
    fn edit_no_match_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.txt");
        std::fs::write(&path, "hello").unwrap();
        let err = Edit
            .run(&json!({"path": path, "old_string": "zzz", "new_string": "x"}))
            .unwrap_err()
            .to_string();
        assert_eq!(err, format!("old_string not found in {}", path.display()));
    }

    #[test]
    fn edit_ambiguous_match_requires_replace_all() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.txt");
        std::fs::write(&path, "a b a").unwrap();
        let err = Edit
            .run(&json!({"path": path, "old_string": "a", "new_string": "c"}))
            .unwrap_err()
            .to_string();
        assert!(err.starts_with("Found 2 matches"), "got: {err}");
        assert!(err.contains("replace_all=true"));
    }

    #[test]
    fn edit_replace_all_replaces_everywhere() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.txt");
        std::fs::write(&path, "a b a").unwrap();
        let out = Edit
            .run(&json!({"path": path, "old_string": "a", "new_string": "c", "replace_all": true}))
            .unwrap();
        assert_eq!(out, format!("replaced 2 occurrence(s) in {}", path.display()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "c b c");
    }

    #[test]
    fn glob_finds_matching_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        std::fs::write(dir.path().join("b.md"), "").unwrap();
        let pattern = dir.path().join("*.txt");
        let out = Glob.run(&json!({"pattern": pattern})).unwrap();
        assert!(out.contains("a.txt"), "got: {out}");
        assert!(!out.contains("b.md"));
    }

    #[test]
    fn glob_missing_pattern_errors() {
        let err = Glob.run(&json!({})).unwrap_err().to_string();
        assert_eq!(err, "missing pattern");
    }

    #[test]
    fn grep_reports_hits_and_misses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.txt");
        std::fs::write(&path, "hello world\nfoo bar").unwrap();
        let out = Grep.run(&json!({"pattern": "hello", "path": path})).unwrap();
        assert!(out.contains("hello world"), "got: {out}");
        let miss = Grep.run(&json!({"pattern": "zzz", "path": path})).unwrap();
        assert_eq!(miss, "no matches");
    }
}
