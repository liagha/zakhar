use serde_json::{json, Value};

use crate::handler::Handler;
use crate::types::Tool;

pub struct Json;
impl Handler for Json {
    fn spec(&self) -> Tool {
        Tool::function("json", "Work with JSON text. action='validate' checks 'input' is valid JSON; action='format' pretty-prints it; action='query' returns the value at 'path' (dot-separated keys with array indexes, e.g. 'data.items[0].name' or '.data.items[0].name' or root pointer '/data'). Handles both objects and arrays.", json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["validate", "format", "query"], "description": "What to do" },
                "input": { "type": "string", "description": "JSON text to process" },
                "path": { "type": "string", "description": "Value path to extract (for action=query)" }
            },
            "required": ["action", "input"]
        }))
    }

    fn run(&self, args: &Value) -> anyhow::Result<String> {
        let input = args["input"].as_str().unwrap_or("");
        let parsed: Value = serde_json::from_str(input)
            .map_err(|e| anyhow::anyhow!("invalid json: {e}"))?;
        match args["action"].as_str().unwrap_or("") {
            "validate" => Ok(format!("valid json: {}", kind(&parsed))),
            "format" | "pretty" => {
                Ok(serde_json::to_string_pretty(&parsed)?)
            }
            "query" => {
                let path = args["path"].as_str().unwrap_or("");
                let val = follow(&parsed, path)?;
                Ok(format!("{}: {}", kind(val), serde_json::to_string(val)?))
            }
            a => anyhow::bail!("unknown action {a}"),
        }
    }
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn follow<'v>(v: &'v Value, path: &str) -> anyhow::Result<&'v Value> {
    let path = path.trim().trim_start_matches('.');
    if path.is_empty() {
        return Ok(v);
    }
    if path.starts_with('/') {
        return v
            .pointer(path)
            .ok_or_else(|| anyhow::anyhow!("no value at '{path}'"));
    }
    let mut cur = v;
    for seg in path.split('.') {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        let key = seg.split('[').next().unwrap_or("");
        if !key.is_empty() {
            cur = obj_key(cur, key)?;
        }
        for bracket in seg.split('[').skip(1) {
            let idx: usize = bracket
                .trim_end_matches(']')
                .parse()
                .map_err(|_| anyhow::anyhow!("bad index '{bracket}'"))?;
            cur = cur
                .get(idx)
                .ok_or_else(|| anyhow::anyhow!("no index '{idx}' in array"))?;
        }
    }
    Ok(cur)
}

fn obj_key<'v>(v: &'v Value, key: &str) -> anyhow::Result<&'v Value> {
    v.get(key)
        .ok_or_else(|| anyhow::anyhow!("no key '{key}' in object"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(input: &str, action: &str, path: &str) -> String {
        Json
            .run(&json!({ "input": input, "action": action, "path": path }))
            .unwrap()
    }

    const DOC: &str = r#"{"name":"zakhar","tags":["cli","careful"],"v":1.5,"nested":{"deep":[10,20]}}"#;

    #[test]
    fn validate_kind() {
        assert_eq!(tool(r#"{"a":[]}"#, "validate", ""), "valid json: object");
        assert_eq!(tool(r#"[1,2]"#, "validate", ""), "valid json: array");
    }

    #[test]
    fn query_paths() {
        assert_eq!(tool(DOC, "query", "name"), "string: \"zakhar\"");
        assert_eq!(tool(DOC, "query", ".tags[0]"), "string: \"cli\"");
        assert_eq!(tool(DOC, "query", "nested.deep[1]"), "number: 20");
        assert_eq!(tool(DOC, "query", "/nested/deep/0"), "number: 10");
        let err = Json
            .run(&json!({ "input": DOC, "action": "query", "path": "nope" }))
            .unwrap_err();
        assert!(err.to_string().contains("no key 'nope'"));
    }
}