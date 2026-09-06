use regex::Regex;
use serde_json::{json, Value};

use crate::handler::Handler;
use crate::types::Tool;

/// Strip HTML tags and extract readable text.
fn html_to_text(html: &str) -> String {
    let mut s = html.to_string();
    s = drop_nodes(&s, &["script", "style", "noscript", "svg"]);
    s = drop_comments(&s);
    s = mark_blocks(&s);
    s = strip_tags(&s);
    s = decode_entities(&s);
    s = collapse_ws(&s);
    s.trim().to_string()
}

fn replace(s: &str, pattern: &str, with: &str) -> String {
    match Regex::new(pattern) {
        Ok(re) => re.replace_all(s, with).to_string(),
        Err(_) => s.to_string(),
    }
}

fn drop_nodes(s: &str, tags: &[&str]) -> String {
    let mut out = s.to_string();
    for tag in tags {
        let pattern = format!("(?is)<{tag}[^>]*>.*?</{tag}>");
        out = replace(&out, &pattern, " ");
    }
    out
}

fn drop_comments(s: &str) -> String {
    replace(s, r"(?s)<!--.*?-->", " ")
}

fn mark_blocks(s: &str) -> String {
    let pattern = r"(?i)</?(p|div|h[1-6]|br|li|tr|blockquote|pre|section|article|header|footer|nav|main|aside)[^>]*/?>";
    replace(s, pattern, "\n")
}

fn strip_tags(s: &str) -> String {
    replace(s, r"<[^>]+>", " ")
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

fn collapse_ws(s: &str) -> String {
    let one = replace(s, r"[ \t]+", " ");
    replace(&one, r"\n{3,}", "\n\n")
}

const MAX_BYTES: usize = 500_000;

pub struct Fetch;
impl Handler for Fetch {
    fn spec(&self) -> Tool {
        Tool::function(
            "fetch",
            "Fetch a URL via HTTP GET. Returns the response body as text (HTML is stripped to readable text). Use for research, reading documentation, checking APIs, etc.",
            json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "URL to fetch (must start with http:// or https://)"
                    }
                },
                "required": ["url"]
            }),
        )
    }

    fn run(&self, args: &Value) -> anyhow::Result<String> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing url"))?;

        if !url.starts_with("http://") && !url.starts_with("https://") {
            anyhow::bail!("url must start with http:// or https://");
        }

        let client = reqwest::blocking::Client::builder()
            .user_agent(format!("zakhar/{}", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::limited(5))
            .timeout(std::time::Duration::from_secs(15))
            .build()?;

        let resp = client.get(url).send()?;

        let status = resp.status();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let bytes = resp.bytes()?;
        if bytes.len() > MAX_BYTES {
            let text = String::from_utf8_lossy(&bytes[..MAX_BYTES]);
            let truncated = html_to_text(&text);
            return Ok(format!(
                "[truncated at {MAX_BYTES} bytes, status={status}, type={content_type}]\n{truncated}\n…"
            ));
        }

        let raw = String::from_utf8_lossy(&bytes).to_string();

        let body = if content_type.contains("text/html") || raw.trim_start().starts_with("<!DOCTYPE")
            || raw.trim_start().starts_with("<html")
        {
            html_to_text(&raw)
        } else {
            raw
        };

        Ok(format!(
            "[status={status}, type={content_type}, {} bytes]\n{}",
            bytes.len(),
            body
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_to_text_strips_tags() {
        let html = r#"<html><head><title>Test</title></head><body>
            <h1>Hello</h1>
            <p>This is a <strong>test</strong> page.</p>
            <script>alert('x')</script>
            <!-- comment -->
            <style>.x{color:red}</style>
        </body></html>"#;
        let text = html_to_text(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("test page"));
        assert!(!text.contains("<script>"));
        assert!(!text.contains("alert"));
        assert!(!text.contains("color:red"));
        assert!(!text.contains("<!--"));
    }

    #[test]
    fn html_to_text_decodes_entities() {
        let html = "a &amp; b &lt; c &gt; d &quot;e&quot;";
        let text = html_to_text(html);
        assert_eq!(text, "a & b < c > d \"e\"");
    }

    #[test]
    fn url_must_be_http() {
        let tool = Fetch;
        let err = tool.run(&json!({"url": "ftp://example.com"})).unwrap_err();
        assert!(err.to_string().contains("must start with http"));
    }
}
