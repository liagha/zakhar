//! regex — match, replace, and validate regular expressions.

use regex::RegexBuilder;
use serde_json::{json, Value};

use crate::handler::Handler;
use crate::types::Tool;

const MAX_HITS: usize = 1000;
const MAX_MATCH_TEXT: usize = 200;

fn build(pattern: &str, ci: bool) -> anyhow::Result<regex::Regex> {
    RegexBuilder::new(pattern)
        .case_insensitive(ci)
        .build()
        .map_err(|e| anyhow::anyhow!("bad pattern: {e}"))
}

pub struct Regex;
impl Handler for Regex {
    fn spec(&self) -> Tool {
        Tool::function("regex", "Work with regular expressions (read-only). action='match' returns all matches of 'pattern' in 'input' with byte offsets and capture groups; action='replace' replaces every match with 'replacement' (supports $1 backrefs); action='validate' just checks the pattern compiles. 'case_insensitive' optional. Use instead of ad-hoc string parsing.", json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["match", "replace", "validate"], "description": "What to do" },
                "pattern": { "type": "string", "description": "Regular expression" },
                "input": { "type": "string", "description": "Text to search or replace in" },
                "replacement": { "type": "string", "description": "Replacement text with $1..$N backreferences (for action=replace)" },
                "case_insensitive": { "type": "boolean", "description": "Ignore case (default false)" }
            },
            "required": ["action", "pattern"]
        }))
    }

    fn run(&self, args: &Value) -> anyhow::Result<String> {
        let action = args["action"].as_str().unwrap_or("");
        let pattern = args.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
        let ci = args
            .get("case_insensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        match action {
            "validate" => {
                build(pattern, ci)?;
                Ok("pattern is valid".to_string())
            }
            "match" => {
                if pattern.is_empty() {
                    anyhow::bail!("missing pattern");
                }
                let input = args.get("input").and_then(|v| v.as_str()).unwrap_or("");
                let limit = args
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(100)
                    .clamp(1, MAX_HITS as u64) as usize;
                let re = build(pattern, ci)?;
                let names: Vec<String> = re
                    .capture_names()
                    .enumerate()
                    .map(|(i, n)| n.unwrap_or(&format!("{i}")).to_string())
                    .collect();
                let mut out = String::new();
                let mut count = 0usize;
                for caps in re.captures_iter(input) {
                    if count >= limit {
                        out.push_str("... (truncated at limit)\n");
                        break;
                    }
                    count += 1;
                    if let Some(m) = caps.get(0) {
                        out.push_str(&format!(
                            "{}:{} {}\n",
                            m.start(),
                            m.end(),
                            m.as_str().chars().take(MAX_MATCH_TEXT).collect::<String>()
                        ));
                        if caps.len() > 1 {
                            let mut g = Vec::new();
                            for (i, label) in names.iter().enumerate().skip(1) {
                                let val = caps
                                    .get(i)
                                    .map(|gm| gm.as_str().to_string())
                                    .unwrap_or_default();
                                g.push(format!("{label}={val}"));
                            }
                            out.push_str(&format!("  groups: {}\n", g.join(" ")));
                        }
                    }
                }
                if count == 0 {
                    out.push_str("no match");
                } else {
                    out.push_str(&format!(
                        "({count} match{})\n",
                        if count == 1 { "" } else { "es" }
                    ));
                }
                Ok(out)
            }
            "replace" => {
                if pattern.is_empty() {
                    anyhow::bail!("missing pattern");
                }
                let input = args.get("input").and_then(|v| v.as_str()).unwrap_or("");
                let replacement = args.get("replacement").and_then(|v| v.as_str()).unwrap_or("");
                let re = build(pattern, ci)?;
                let count = re.find_iter(input).count();
                let replaced = re.replace_all(input, replacement);
                Ok(format!(
                    "replaced {count} match{}{}\n{replaced}\n---",
                    if count == 1 { "" } else { "es" },
                    if count == 0 { " (input unchanged)" } else { "" },
                ))
            }
            a => anyhow::bail!("unknown action {a}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_reports_offsets() {
        let tool = Regex;
        let out = tool
            .run(&json!({"action": "match", "pattern": r"\d+", "input": "abc 12 def 345"}))
            .unwrap();
        assert!(out.contains("4:6 12"), "got: {out}");
        assert!(out.contains("11:14 345"), "got: {out}");
        assert!(out.contains("(2 matches)"), "got: {out}");
    }

    #[test]
    fn match_capture_groups() {
        let tool = Regex;
        let out = tool
            .run(&json!({"action": "match", "pattern": r"(?P<word>\w+)@(\w+)", "input": "ping foo@bar baz"}))
            .unwrap();
        assert!(out.contains("word=foo"), "got: {out}");
        assert!(out.contains("2=bar"), "got: {out}");
        assert!(out.contains("(1 match)"), "got: {out}");
    }

    #[test]
    fn match_case_insensitive() {
        let tool = Regex;
        let out = tool
            .run(&json!({"action": "match", "pattern": "tehran", "input": "TEHRAN", "case_insensitive": true}))
            .unwrap();
        assert!(out.contains("(1 match)"), "got: {out}");
    }

    #[test]
    fn replace_uses_backrefs() {
        let tool = Regex;
        let out = tool
            .run(&json!({"action": "replace", "pattern": r"(\w+)\s+(\w+)", "input": "first last", "replacement": "$2, $1"}))
            .unwrap();
        assert!(out.contains("last, first"), "got: {out}");
    }

    #[test]
    fn validate_and_bad_pattern() {
        let tool = Regex;
        assert!(tool
            .run(&json!({"action": "validate", "pattern": r"a+b"}))
            .unwrap()
            .contains("valid"));
        assert!(tool
            .run(&json!({"action": "match", "pattern": "(", "input": "x"}))
            .is_err());
        assert!(tool.run(&json!({"action": "nope"})).is_err());
    }
}