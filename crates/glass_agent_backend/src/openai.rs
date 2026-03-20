//! OpenAI-compatible API backend.
//!
//! Implements [`AgentBackend`](crate::AgentBackend) for any endpoint that speaks
//! the OpenAI `/v1/chat/completions` API with SSE streaming. Covers OpenAI,
//! Google Gemini (OpenAI-compat mode), and local servers (vLLM, llama.cpp, LM Studio).

// Types and helpers in this module are consumed by the full backend
// implementation which is being built incrementally.
#![allow(dead_code)]

// ── Types ─────────────────────────────────────────────────────────────────────

/// Token usage reported in the final SSE chunk.
#[derive(Debug, Clone)]
pub(crate) struct Usage {
    pub(crate) prompt_tokens: u64,
    pub(crate) completion_tokens: u64,
}

/// A parsed SSE chunk from an OpenAI-compatible streaming response.
#[derive(Debug, Clone)]
pub(crate) enum SseChunk {
    /// A fragment of assistant text content.
    TextDelta(String),
    /// A fragment of a tool call (may be spread across multiple deltas).
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        function_name: Option<String>,
        arguments_delta: String,
    },
    /// A reasoning/thinking content fragment (o-series models).
    ReasoningDelta(String),
    /// The stream is complete. Carries optional token usage from the final chunk.
    Done { usage: Option<Usage> },
}

// ── SSE line parser ───────────────────────────────────────────────────────────

/// Parse a single SSE line into an [`SseChunk`].
///
/// Returns `None` for lines that carry no actionable data (empty lines,
/// `event:` lines, SSE comments starting with `:`).
pub(crate) fn parse_sse_line(line: &str) -> Option<SseChunk> {
    // Only process `data:` lines.
    let data = line.strip_prefix("data:")?;
    let data = data.trim();

    // Terminal sentinel.
    if data == "[DONE]" {
        return Some(SseChunk::Done { usage: None });
    }

    // Parse JSON payload.
    let v: serde_json::Value = serde_json::from_str(data).ok()?;

    // ── finish_reason == "stop" ───────────────────────────────────────────────
    if let Some(finish_reason) = v
        .pointer("/choices/0/finish_reason")
        .and_then(|r| r.as_str())
    {
        if finish_reason == "stop" {
            let usage = extract_usage(&v);
            return Some(SseChunk::Done { usage });
        }
    }

    // ── Top-level usage (final chunk without finish_reason) ───────────────────
    // Some providers send a trailing chunk that has usage but no choices delta.
    if v.get("usage").is_some() && v.pointer("/choices/0/delta").is_none() {
        let usage = extract_usage(&v);
        return Some(SseChunk::Done { usage });
    }

    // ── delta content ─────────────────────────────────────────────────────────
    let delta = v.pointer("/choices/0/delta")?;

    // reasoning_content (o-series models)
    if let Some(reasoning) = delta.get("reasoning_content").and_then(|c| c.as_str()) {
        if !reasoning.is_empty() {
            return Some(SseChunk::ReasoningDelta(reasoning.to_owned()));
        }
    }

    // tool_calls[0]
    if let Some(tool_call) = delta
        .get("tool_calls")
        .and_then(|tc| tc.as_array())
        .and_then(|arr| arr.first())
    {
        let index = tool_call
            .get("index")
            .and_then(|i| i.as_u64())
            .unwrap_or(0) as usize;
        let id = tool_call
            .get("id")
            .and_then(|i| i.as_str())
            .map(str::to_owned);
        let function_name = tool_call
            .pointer("/function/name")
            .and_then(|n| n.as_str())
            .map(str::to_owned);
        let arguments_delta = tool_call
            .pointer("/function/arguments")
            .and_then(|a| a.as_str())
            .unwrap_or("")
            .to_owned();

        return Some(SseChunk::ToolCallDelta {
            index,
            id,
            function_name,
            arguments_delta,
        });
    }

    // text content
    if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
        if !content.is_empty() {
            return Some(SseChunk::TextDelta(content.to_owned()));
        }
    }

    None
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn extract_usage(v: &serde_json::Value) -> Option<Usage> {
    let usage = v.get("usage")?;
    let prompt_tokens = usage
        .get("prompt_tokens")
        .and_then(|t| t.as_u64())?;
    let completion_tokens = usage
        .get("completion_tokens")
        .and_then(|t| t.as_u64())?;
    Some(Usage {
        prompt_tokens,
        completion_tokens,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_text_delta() {
        let line = r#"data: {"choices":[{"delta":{"content":"Hello"},"index":0}]}"#;
        match parse_sse_line(line) {
            Some(SseChunk::TextDelta(text)) => assert_eq!(text, "Hello"),
            other => panic!("expected TextDelta, got {other:?}"),
        }
    }

    #[test]
    fn parse_done_marker() {
        let line = "data: [DONE]";
        match parse_sse_line(line) {
            Some(SseChunk::Done { .. }) => {}
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn parse_tool_call_delta() {
        let line = r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_abc","function":{"name":"Bash","arguments":"{\"cmd\":"}}]},"index":0}]}"#;
        match parse_sse_line(line) {
            Some(SseChunk::ToolCallDelta {
                index,
                id,
                function_name,
                arguments_delta,
            }) => {
                assert_eq!(index, 0);
                assert_eq!(id.as_deref(), Some("call_abc"));
                assert_eq!(function_name.as_deref(), Some("Bash"));
                assert_eq!(arguments_delta, r#"{"cmd":"#);
            }
            other => panic!("expected ToolCallDelta, got {other:?}"),
        }
    }

    #[test]
    fn parse_finish_stop() {
        let line = r#"data: {"choices":[{"delta":{},"finish_reason":"stop","index":0}]}"#;
        match parse_sse_line(line) {
            Some(SseChunk::Done { .. }) => {}
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn parse_non_data_line_returns_none() {
        assert!(parse_sse_line("event: ping").is_none());
        assert!(parse_sse_line("").is_none());
        assert!(parse_sse_line(": comment").is_none());
    }

    #[test]
    fn parse_usage_in_final_chunk() {
        let line = r#"data: {"usage":{"prompt_tokens":10,"completion_tokens":20}}"#;
        match parse_sse_line(line) {
            Some(SseChunk::Done {
                usage: Some(usage),
            }) => {
                assert_eq!(usage.prompt_tokens, 10);
                assert_eq!(usage.completion_tokens, 20);
            }
            other => panic!("expected Done with usage, got {other:?}"),
        }
    }

    #[test]
    fn parse_empty_content_returns_none() {
        let line = r#"data: {"choices":[{"delta":{"content":""},"index":0}]}"#;
        assert!(parse_sse_line(line).is_none());
    }
}
