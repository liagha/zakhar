use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};

use crate::handler::Handler;
use crate::types::Tool;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Item {
    content: String,
    status: String,
    priority: String,
}

fn todo_path() -> PathBuf {
    crate::paths::home().join("todo.json")
}

fn load_todos() -> Vec<Item> {
    std::fs::read_to_string(todo_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_todos(items: &[Item]) -> anyhow::Result<()> {
    let p = todo_path();
    let dir = p.parent().unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".todo.{}.tmp", std::process::id()));
    std::fs::write(&tmp, serde_json::to_string_pretty(items)?)?;
    std::fs::rename(&tmp, &p)?;
    Ok(())
}

static TODOS: OnceLock<Mutex<Vec<Item>>> = OnceLock::new();

pub fn load_persisted_todos() -> String {
    let items = load_todos();
    if items.is_empty() {
        return String::new();
    }
    let mutex = TODOS.get_or_init(|| Mutex::new(Vec::new()));
    *mutex.lock().unwrap() = items.clone();
    let mut out = String::new();
    for t in &items {
        let icon = match t.status.as_str() {
            "pending" => "○",
            "in_progress" => "●",
            "completed" => "✓",
            "cancelled" => "✗",
            _ => "?",
        };
        out.push_str(&format!("  {} [{}] {} ({})\n", icon, t.status, t.content, t.priority));
    }
    out
}

pub struct Ask;
impl Handler for Ask {
    fn spec(&self) -> Tool {
        Tool::function("ask", "Ask the user clarifying questions with options. Use when requirements are ambiguous or you need a decision. Returns answers as JSON.", json!({
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
        }))
    }
    fn run(&self, args: &Value) -> anyhow::Result<String> {
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
            println!("\n[ask] {}: {}", header, question);
            if let Some(opts) = options {
                for (i, opt) in opts.iter().enumerate() {
                    let label = opt.get("label").and_then(|v| v.as_str()).unwrap_or("");
                    let desc = opt.get("description").and_then(|v| v.as_str()).unwrap_or("");
                    println!("  {}. {} - {}", i + 1, label, desc);
                }
                print!("[ask] Enter choice(s) (number or label, comma for multiple): ");
            } else {
                print!("[ask] Your answer: ");
            }
            std::io::stdout().flush().ok();
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            let input = input.trim().to_string();
            let answer = if let Some(opts) = options {
                let mut chosen = Vec::new();
                for part in input.split(',').map(|s| s.trim()) {
                    if let Ok(n) = part.parse::<usize>()
                        && n > 0 && n <= opts.len()
                        && let Some(label) = opts[n - 1].get("label").and_then(|v| v.as_str())
                    {
                        chosen.push(label.to_string());
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
            println!("[ask] answered: {}", answers.last().unwrap()["answer"].as_str().unwrap_or(""));
        }
        Ok(serde_json::to_string_pretty(&answers).unwrap_or_else(|_| format!("{:?}", answers)))
    }
}

pub struct Todo;
impl Handler for Todo {
    fn spec(&self) -> Tool {
        Tool::function("todo", "Create and maintain a task list. Use for multi-step work: one in_progress at a time, mark completed as you go. Call at start and on progress.", json!({
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
        }))
    }
    fn run(&self, args: &Value) -> anyhow::Result<String> {
        let items = args
            .get("todos")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("missing todos"))?;
        let mut list = Vec::new();
        for t in items {
            let content = t.get("content").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let status = t.get("status").and_then(|v| v.as_str()).unwrap_or("pending").to_string();
            let priority = t.get("priority").and_then(|v| v.as_str()).unwrap_or("medium").to_string();
            if !["pending", "in_progress", "completed", "cancelled"].contains(&status.as_str()) {
                anyhow::bail!("invalid status {status}");
            }
            list.push(Item { content, status, priority });
        }
        let in_progress = list.iter().filter(|t| t.status == "in_progress").count();
        if in_progress > 1 {
            anyhow::bail!("only one task may be in_progress at a time, got {in_progress}");
        }
        save_todos(&list)?;
        let mutex = TODOS.get_or_init(|| Mutex::new(Vec::new()));
        *mutex.lock().unwrap() = list.clone();
        let mut out = String::new();
        out.push_str(&format!("[todo] {} todos:\n", list.len()));
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
        std::io::stdout().flush().ok();
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> (tempfile::TempDir, std::sync::MutexGuard<'static, ()>) {
        let guard = crate::memory::lock();
        let dir = tempfile::tempdir().unwrap();
        crate::paths::set_home(dir.path().join(".zakhar"));
        (dir, guard)
    }

    #[test]
    fn ask_missing_questions_errors() {
        let err = Ask.run(&json!({})).unwrap_err().to_string();
        assert_eq!(err, "missing questions");
    }

    #[test]
    fn ask_empty_questions_errors() {
        let err = Ask.run(&json!({"questions": []})).unwrap_err().to_string();
        assert_eq!(err, "questions is empty");
    }

    #[test]
    fn todo_roundtrip_persists_and_renders() {
        let (_dir, _g) = tmp();
        let out = Todo
            .run(&json!({"todos": [
                {"content": "alpha", "status": "pending", "priority": "high"},
                {"content": "beta", "status": "completed", "priority": "low"}
            ]}))
            .unwrap();
        assert!(out.starts_with("[todo] 2 todos:"), "got: {out}");
        assert!(out.contains("  ○ [pending] alpha (high)"), "got: {out}");
        assert!(out.contains("  ✓ [completed] beta (low)"), "got: {out}");
        let stored: Vec<Item> =
            serde_json::from_str(&std::fs::read_to_string(todo_path()).unwrap()).unwrap();
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].content, "alpha");
    }

    #[test]
    fn todo_invalid_status_errors() {
        let (_dir, _g) = tmp();
        let err = Todo
            .run(&json!({"todos": [{"content": "x", "status": "nope", "priority": "high"}]}))
            .unwrap_err()
            .to_string();
        assert_eq!(err, "invalid status nope");
    }

    #[test]
    fn todo_two_in_progress_errors() {
        let (_dir, _g) = tmp();
        let err = Todo
            .run(&json!({"todos": [
                {"content": "a", "status": "in_progress", "priority": "high"},
                {"content": "b", "status": "in_progress", "priority": "medium"}
            ]}))
            .unwrap_err()
            .to_string();
        assert_eq!(err, "only one task may be in_progress at a time, got 2");
    }

    #[test]
    fn load_persisted_todos_renders_icons() {
        let (_dir, _g) = tmp();
        std::fs::create_dir_all(todo_path().parent().unwrap()).unwrap();
        std::fs::write(
            todo_path(),
            serde_json::to_string(&json!([
                {"content": "a", "status": "pending", "priority": "high"},
                {"content": "b", "status": "in_progress", "priority": "medium"},
                {"content": "c", "status": "cancelled", "priority": "low"}
            ]))
            .unwrap(),
        )
        .unwrap();
        let out = load_persisted_todos();
        assert!(out.contains("  ○ [pending] a (high)"), "got: {out}");
        assert!(out.contains("  ● [in_progress] b (medium)"), "got: {out}");
        assert!(out.contains("  ✗ [cancelled] c (low)"), "got: {out}");
    }

    #[test]
    fn load_persisted_todos_empty_is_blank() {
        let (_dir, _g) = tmp();
        std::fs::create_dir_all(todo_path().parent().unwrap()).unwrap();
        std::fs::write(todo_path(), "[]").unwrap();
        assert_eq!(load_persisted_todos(), "");
    }
}
