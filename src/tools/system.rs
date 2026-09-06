use std::path::Path;

use serde_json::{json, Value};

use crate::handler::Handler;
use crate::types::Tool;

pub struct Skill;
impl Handler for Skill {
    fn spec(&self) -> Tool {
        Tool::function("skill", "Load a skill's instructions. Call when a task matches a skill; with no name, lists available skills.", json!({
            "type": "object",
            "properties": { "name": { "type": "string", "description": "Skill name" } },
            "required": []
        }))
    }
    fn run(&self, args: &Value) -> anyhow::Result<String> {
        let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if name.is_empty() {
            let mut skills = Vec::new();
            for dir in [".opencode/skills", ".claude/skills", "skills", ".zakhar/skills"] {
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for e in entries.flatten() {
                        if let Ok(ft) = e.file_type()
                            && ft.is_dir()
                            && let Some(n) = e.file_name().to_str()
                        {
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
                            && let Some(n) = e.file_name().to_str()
                        {
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
    }
}

pub struct Control;
impl Handler for Control {
    fn spec(&self) -> Tool {
        Tool::function("control", "Control zakhar itself. action='allow' stops asking before mutating tools (use when the user says 'you have my permission'); action='models' lists available models; action='chat' opens the interactive chat with an optional message.", json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["allow", "models", "chat"], "description": "What to control" },
                "message": { "type": "string", "description": "Initial chat message for action=chat" }
            },
            "required": ["action"]
        }))
    }
    fn run(&self, args: &Value) -> anyhow::Result<String> {
        match args["action"].as_str().unwrap_or("") {
            "allow" => {
                crate::invoke::grant();
                Ok("permission granted: will not ask before mutating tools".to_string())
            }
            "models" => crate::invoke::models(),
            "chat" => {
                let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("").to_string();
                crate::invoke::open(message);
                Ok("opening interactive chat".to_string())
            }
            a => anyhow::bail!("unknown action {a}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_cwd(dir: &std::path::Path, f: impl FnOnce()) {
        let orig = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        let _ = std::env::set_current_dir(&orig);
        result.unwrap();
    }

    #[test]
    fn loads_from_zakhar_skills() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".zakhar/skills/custom")).unwrap();
        std::fs::write(dir.path().join(".zakhar/skills/custom/SKILL.md"), "do the thing").unwrap();
        with_cwd(dir.path(), || {
            let tool = Skill;
            let out = tool.run(&json!({"name": "custom"})).unwrap();
            assert_eq!(out, "do the thing");
        });
    }

    #[test]
    fn missing_skill_errors() {
        let dir = tempfile::tempdir().unwrap();
        with_cwd(dir.path(), || {
            let tool = Skill;
            let err = tool.run(&json!({"name": "nope"})).unwrap_err();
            assert!(err.to_string().contains("'nope' not found"));
        });
    }
}
