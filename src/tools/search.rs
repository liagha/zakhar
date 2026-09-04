use regex::Regex;
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

pub struct Search;
impl Handler for Search {
    fn spec(&self) -> Tool {
        def(
            "search",
            "Search the web (via Brave Search). Returns titles, URLs, and snippets for up to 10 results. Use when you need to find information about a topic, look up current events, or discover resources.",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    }
                },
                "required": ["query"]
            }),
        )
    }

    fn run(&self, args: &Value) -> anyhow::Result<String> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing query"))?;

        if query.trim().is_empty() {
            anyhow::bail!("query cannot be empty");
        }

        let url = format!(
            "https://search.brave.com/search?q={}",
            urlencoding::encode(query)
        );

        let client = reqwest::blocking::Client::builder()
            .user_agent(format!(
                "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
            ))
            .redirect(reqwest::redirect::Policy::limited(5))
            .timeout(std::time::Duration::from_secs(15))
            .build()?;

        let resp = client.get(&url).send()?;
        let status = resp.status();

        if !status.is_success() {
            anyhow::bail!("search failed with status {status}");
        }

        let html = resp.text()?;
        let results = parse_results(&html);

        if results.is_empty() {
            return Ok(format!("no results found for '{query}'"));
        }

        let mut out = String::new();
        for (i, r) in results.iter().enumerate() {
            out.push_str(&format!(
                "{}. {}\n   {}\n   {}\n\n",
                i + 1,
                r.title,
                r.url,
                r.snippet
            ));
        }
        Ok(out.trim().to_string())
    }
}

struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

fn strip_tags(s: &str) -> String {
    if let Ok(re) = Regex::new(r"<[^>]+>") {
        re.replace_all(s, "").to_string()
    } else {
        s.to_string()
    }
}

fn html_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

fn parse_results(html: &str) -> Vec<SearchResult> {
    let mut results = Vec::new();

    let blocks: Vec<&str> = html.split(r#"<div class="snippet svelte"#).collect();

    for block in blocks.into_iter().skip(1) {
        let url = match Regex::new(r#"<a href="(https?://[^"]+)""#) {
            Ok(re) => re
                .captures(block)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
                .unwrap_or_default(),
            Err(_) => continue,
        };

        let title = match Regex::new(r#"<div class="title[^"]*"[^>]*>(.*?)</div>"#) {
            Ok(re) => re
                .captures(block)
                .and_then(|c| c.get(1))
                .map(|m| {
                    let t = strip_tags(m.as_str());
                    html_entities(&t).trim().to_string()
                })
                .unwrap_or_default(),
            Err(_) => continue,
        };

        let snippet = match Regex::new(r#"class="content[^"]*">(.*?)</div>"#) {
            Ok(re) => re
                .captures(block)
                .and_then(|c| c.get(1))
                .map(|m| {
                    let t = strip_tags(m.as_str());
                    html_entities(&t).trim().to_string()
                })
                .unwrap_or_default(),
            Err(_) => continue,
        };

        if !title.is_empty() && !url.is_empty() {
            results.push(SearchResult {
                title,
                url,
                snippet,
            });
        }

        if results.len() >= 10 {
            break;
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_html() -> &'static str {
        r#"<div class="snippet svelte-abc" data-pos="1" data-type="web"><a href="https://rust-lang.org/" target="_self"><div class="title search-snippet-title" title="Rust Programming Language">Rust Programming Language</div></a><div class="generic-snippet"><div class="content">A language empowering everyone to build reliable and efficient software.</div></div></div>"#
    }

    #[test]
    fn parse_brave_results() {
        let results = parse_results(sample_html());
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Rust Programming Language");
        assert_eq!(results[0].url, "https://rust-lang.org/");
        assert!(results[0].snippet.contains("reliable and efficient"));
    }

    #[test]
    fn strip_tags_works() {
        assert_eq!(strip_tags("<b>hello</b>"), "hello");
        assert_eq!(strip_tags("no tags"), "no tags");
    }

    #[test]
    fn html_entities_decode() {
        assert_eq!(html_entities("a &amp; b"), "a & b");
        assert_eq!(html_entities("x &lt; y"), "x < y");
    }
}
