//! Model Context Protocol integration.
//!
//! The client half connects to external MCP servers declared in the `[mcp]`
//! config section and mounts their tools into the invoker as `server__tool`
//! functions. The server half runs `zakhar mcp` over stdio, advertising
//! zakhar's read-only and knowledge tools to any MCP client. Both halves talk
//! newline-delimited JSON-RPC (the stdio framing), one message per line.

pub mod client;
pub mod server;

pub const PROTOCOL_VERSION: &str = "2024-11-05";
pub const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

pub fn sanitize(name: &str) -> String {
    let out: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() {
        "_".to_string()
    } else {
        out
    }
}

pub fn write_line(
    writer: &mut impl std::io::Write,
    value: &serde_json::Value,
) -> anyhow::Result<()> {
    serde_json::to_writer(&mut *writer, value)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn sanitize_keeps_safe_chars_and_drops_the_rest() {
        assert_eq!(super::sanitize("my-server/1"), "my-server_1");
        assert_eq!(super::sanitize("سرویس"), "_____");
        assert_eq!(super::sanitize(""), "_");
        assert_eq!(super::sanitize("a.b-c_d"), "a.b-c_d");
    }
}
