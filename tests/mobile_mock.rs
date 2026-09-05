use serde_json::json;

#[test]
fn mobile_mock_turn() {
    let messages = json!([{"role":"user","content":"hello"}]).to_string();
    let provider = Box::new(zakhar::provider::mock::Script {
        name: "script".to_string(),
        answer: "done: mock turn complete".to_string(),
    });
    let id = zakhar::mobile::start(provider, &messages, true);
    let mut collected = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut finished = false;
    while std::time::Instant::now() < deadline && !finished {
        let raw = zakhar::mobile::poll(&id, 500);
        eprintln!("raw: {raw}");
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let events = value.get("events").and_then(|e| e.as_array()).cloned().unwrap_or_default();
        for raw_event in events {
            let e: serde_json::Value = serde_json::from_str(raw_event.as_str().unwrap_or_default()).unwrap();
            let kind = e.get("type").and_then(|t| t.as_str()).unwrap_or_default();
            collected.push(kind.to_string());
            if kind == "done" {
                finished = true;
            }
            if kind == "error" {
                panic!("engine error: {e}");
            }
        }
    }
    assert!(finished, "timed out; events so far: {collected:?}");
    assert!(
        collected.windows(3).any(|w| w == ["tool_result", "text", "done"]),
        "unexpected event order: {collected:?}"
    );
    assert!(
        collected.windows(2).any(|w| w == ["text", "done"]),
        "missing final text+done: {collected:?}"
    );
}