//! End-to-end tests that spawn the real `zakhar mcp` binary as a child MCP
//! server, mount it through the invoker, and check its read-only tools.

use std::collections::HashMap;

use serde_json::json;

use zakhar::config::{Config, Mcp, Server};
use zakhar::invoke::Invoke;

fn test_cfg() -> Config {
    let mut servers = HashMap::new();
    servers.insert(
        "fs".to_string(),
        Server {
            command: env!("CARGO_BIN_EXE_zakhar").to_string(),
            args: vec!["mcp".to_string()],
        },
    );
    Config {
        mcp: Mcp { servers },
        ..Config::default()
    }
}

#[test]
fn mounts_and_calls_remote_tool() {
    let mut inv = Invoke::new();
    let labels = inv.mount_servers(&test_cfg());
    assert!(
        labels.iter().any(|l| l.starts_with("fs (") && l.contains("tool")),
        "labels: {labels:?}"
    );
    let out = inv.exec("fs__time", &json!({}));
    assert!(out.contains("utc:"), "fs__time output: {out}");
    assert!(!out.starts_with("error:"), "fs__time output: {out}");
}

#[test]
fn remote_tools_have_namespaced_definitions() {
    let mut inv = Invoke::new();
    inv.mount_servers(&test_cfg());
    let names: Vec<String> = inv
        .definitions()
        .into_iter()
        .map(|t| t.function.name)
        .collect();
    for name in ["fs__time", "fs__read", "fs__fetch", "fs__remember"] {
        assert!(names.contains(&name.to_string()), "missing {name}: {names:?}");
    }
    assert!(!names.iter().any(|n| n.contains("__bash")), "bash leaked: {names:?}");
}
