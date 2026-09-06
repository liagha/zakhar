//! OpenAI-compatible chat provider: SSE stream decoding, bounded retries
//! with Retry-After support, transport timeouts, and strict chunk parsing.

use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use async_trait::async_trait;
use futures::stream::Stream;
use futures::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, RETRY_AFTER, USER_AGENT};
use reqwest::Client;
use serde_json::json;

use super::types::{Chunk, Config};
use super::{ChatStreamEvent, DeltaStream, Provider, ToolCallPart};
use crate::types::ChatRequest;

const CONNECT_TIMEOUT: u64 = 10;
const READ_TIMEOUT: u64 = 120;
const MAX_RETRY_DELAY: u64 = 30;

pub struct OpenAI {
    id: String,
    base_url: String,
    api_key: String,
    default_model: String,
    models: Vec<String>,
    client: Client,
    max_retries: u32,
}

impl OpenAI {
    pub fn new(id: &str, cfg: &Config) -> Self {
        let mut headers = HeaderMap::new();
        if !cfg.user_agent.is_empty()
            && let Ok(ua) = HeaderValue::from_str(&cfg.user_agent)
        {
            headers.insert(USER_AGENT, ua);
        }
        let client = Client::builder()
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT))
            .read_timeout(Duration::from_secs(READ_TIMEOUT))
            .build()
            .unwrap();
        Self {
            id: id.to_string(),
            base_url: cfg.base_url.trim_end_matches('/').to_string(),
            api_key: cfg.api_key.clone(),
            default_model: cfg.default_model.clone(),
            models: cfg.models.clone(),
            client,
            max_retries: 3,
        }
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }
}

#[async_trait]
impl Provider for OpenAI {
    fn id(&self) -> &str {
        &self.id
    }

    fn list_models(&self) -> Vec<String> {
        if !self.models.is_empty() {
            self.models.clone()
        } else if !self.default_model.is_empty() {
            vec![self.default_model.clone()]
        } else {
            vec![]
        }
    }

    async fn chat_stream(&self, request: ChatRequest) -> anyhow::Result<DeltaStream> {
        let mut body = serde_json::to_value(&request)?;
        body["stream"] = json!(true);

        let mut last_err = None;
        for attempt in 0..=self.max_retries {
            let mut builder = self.client.post(self.endpoint()).json(&body);
            if !self.api_key.is_empty() {
                builder = builder.bearer_auth(&self.api_key);
            }

            match builder.send().await {
                Ok(response) if response.status().is_success() => {
                    let byte_stream = response
                        .bytes_stream()
                        .map(|chunk| chunk.map(|b| b.to_vec()).map_err(|e| anyhow::anyhow!(e)));
                    return Ok(Box::pin(SseStream {
                        inner: Box::pin(byte_stream),
                        decoder: SseDecoder::new(),
                        pending: Vec::new(),
                    }) as DeltaStream);
                }
                Ok(response) => {
                    let status = response.status();
                    let retry_in: Option<u64> = if status.as_u16() == 429 {
                        Some(retry_delay(response.headers(), attempt))
                    } else {
                        None
                    };
                    let text = response.text().await.unwrap_or_default();
                    if let Some(delay) = retry_in
                        && attempt < self.max_retries
                    {
                        tokio::time::sleep(Duration::from_secs(delay)).await;
                        last_err = Some(anyhow::anyhow!("provider error {}: {}", status, text));
                        continue;
                    }
                    anyhow::bail!("provider error {}: {}", status, text);
                }
                Err(e) => {
                    if attempt < self.max_retries {
                        let delay = retry_delay(&HeaderMap::new(), attempt);
                        tokio::time::sleep(Duration::from_secs(delay)).await;
                        last_err = Some(e.into());
                        continue;
                    }
                    return Err(e.into());
                }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("max retries exceeded")))
    }
}

fn retry_delay(headers: &HeaderMap, attempt: u32) -> u64 {
    let fallback = 2u64.pow(attempt).min(MAX_RETRY_DELAY);
    headers
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|v| v.min(MAX_RETRY_DELAY))
        .unwrap_or(fallback)
}

struct SseDecoder {
    buffer: Vec<u8>,
}

impl SseDecoder {
    fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    fn feed(&mut self, bytes: &[u8]) -> anyhow::Result<Vec<ChatStreamEvent>> {
        self.buffer.extend_from_slice(bytes);
        let mut events = Vec::new();
        let mut start = 0usize;

        for i in 0..self.buffer.len() {
            if self.buffer[i] == b'\n' {
                let line = &self.buffer[start..i];
                self.handle_line(line, &mut events)?;
                start = i + 1;
            }
        }

        if start > 0 {
            self.buffer.drain(0..start);
        }
        Ok(events)
    }

    fn handle_line(&self, line: &[u8], events: &mut Vec<ChatStreamEvent>) -> anyhow::Result<()> {
        let trimmed = trim(line);
        if trimmed.is_empty() || !trimmed.starts_with(b"data:") {
            return Ok(());
        }
        let data = String::from_utf8_lossy(trim(&trimmed[5..])).to_string();
        if data == "[DONE]" {
            events.push(ChatStreamEvent::Done);
            return Ok(());
        }

        let chunk: Chunk = match serde_json::from_str(&data) {
            Ok(c) => c,
            Err(e) => {
                anyhow::bail!("malformed chunk: {e}");
            }
        };

        for choice in chunk.choices {
            if let Some(delta) = choice.delta {
                if let Some(content) = delta.reasoning_content
                    && !content.is_empty() {
                        events.push(ChatStreamEvent::Reasoning(content));
                    }
                if let Some(content) = delta.content
                    && !content.is_empty() {
                        events.push(ChatStreamEvent::Text(content));
                    }
                if let Some(tool_calls) = delta.tool_calls {
                    for tc in tool_calls {
                        let func = tc.function;
                        events.push(ChatStreamEvent::ToolCall(ToolCallPart {
                            index: tc.index,
                            id: tc.id,
                            name: func.as_ref().and_then(|f| f.name.clone()),
                            arguments: func.and_then(|f| f.arguments),
                        }));
                    }
                }
            }
        }
        Ok(())
    }
}

fn trim(mut b: &[u8]) -> &[u8] {
    while b
        .first()
        .is_some_and(|c| c.is_ascii_whitespace() || c == &b'\r')
    {
        b = &b[1..];
    }
    while b
        .last()
        .is_some_and(|c| c.is_ascii_whitespace() || c == &b'\r')
    {
        b = &b[..b.len() - 1];
    }
    b
}

struct SseStream {
    inner: Pin<Box<dyn Stream<Item = anyhow::Result<Vec<u8>>> + Send>>,
    decoder: SseDecoder,
    pending: Vec<ChatStreamEvent>,
}

impl Stream for SseStream {
    type Item = anyhow::Result<ChatStreamEvent>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if !self.pending.is_empty() {
            return Poll::Ready(Some(Ok(self.pending.remove(0))));
        }
        loop {
            match self.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => match self.decoder.feed(&bytes) {
                    Ok(mut events) => {
                        if events.is_empty() {
                            continue;
                        }
                        let first = events.remove(0);
                        self.pending = events;
                        return Poll::Ready(Some(Ok(first)));
                    }
                    Err(e) => return Poll::Ready(Some(Err(e))),
                },
                Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(e))),
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_headers() -> HeaderMap {
        HeaderMap::new()
    }

    fn retry_headers(seconds: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(RETRY_AFTER, HeaderValue::from_str(seconds).unwrap());
        headers
    }

    fn parse(bytes: &[u8]) -> anyhow::Result<Vec<ChatStreamEvent>> {
        let mut decoder = SseDecoder::new();
        let mut events = decoder.feed(bytes)?;
        let tail = decoder.feed(b"\n")?;
        events.extend(tail);
        Ok(events)
    }

    #[test]
    fn parses_text_chunks() {
        let bytes = b"data: {\"choices\":[{\"delta\":{\"content\":\"hel\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\ndata: [DONE]\n";
        let events = parse(bytes).unwrap();
        assert!(matches!(events[0], ChatStreamEvent::Text(ref t) if t == "hel"));
        assert!(matches!(events[1], ChatStreamEvent::Text(ref t) if t == "lo"));
        assert!(matches!(events[2], ChatStreamEvent::Done));
    }

    #[test]
    fn skips_non_data_lines() {
        let bytes = b": keep-alive\ndata: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n";
        let events = parse(bytes).unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], ChatStreamEvent::Text(ref t) if t == "x"));
    }

    #[test]
    fn ignores_empty_content() {
        let bytes = b"data: {\"choices\":[{\"delta\":{\"content\":\"\"}}]}\n";
        let events = parse(bytes).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn buffered_lines_across_chunks() {
        let mut decoder = SseDecoder::new();
        let part1 = b"data: {\"choices\":[{\"delt";
        let part2 = b"a\":{\"content\":\"hi\"}}]}\n";
        let e1 = decoder.feed(part1).unwrap();
        assert!(e1.is_empty());
        let e2 = decoder.feed(part2).unwrap();
        assert!(matches!(e2[0], ChatStreamEvent::Text(ref t) if t == "hi"));
    }

    #[test]
    fn utf8_split_across_chunks() {
        let mut decoder = SseDecoder::new();
        let msg = "data: {\"choices\":[{\"delta\":{\"content\":\"\u{6c49}\"}}]}\n";
        let bytes = msg.as_bytes();
        let split = bytes.len() / 2;
        let e1 = decoder.feed(&bytes[..split]).unwrap();
        assert!(e1.is_empty());
        let e2 = decoder.feed(&bytes[split..]).unwrap();
        assert!(matches!(e2[0], ChatStreamEvent::Text(ref t) if t == "\u{6c49}"));
    }

    #[test]
    fn rejects_malformed_chunks() {
        let mut decoder = SseDecoder::new();
        let err = decoder.feed(b"data: {not json}\n").unwrap_err();
        assert!(err.to_string().contains("malformed chunk"));
    }

    #[test]
    fn retry_delay_falls_back_exponential() {
        assert_eq!(retry_delay(&empty_headers(), 0), 1);
        assert_eq!(retry_delay(&empty_headers(), 2), 4);
        assert_eq!(retry_delay(&empty_headers(), 10), MAX_RETRY_DELAY);
    }

    #[test]
    fn retry_delay_prefers_header() {
        assert_eq!(retry_delay(&retry_headers("3"), 5), 3);
        assert_eq!(retry_delay(&retry_headers("200"), 0), MAX_RETRY_DELAY);
        assert_eq!(retry_delay(&retry_headers("abc"), 3), 8);
    }
}