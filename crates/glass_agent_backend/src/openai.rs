//! OpenAI-compatible API backend.
//!
//! Implements [`AgentBackend`](crate::AgentBackend) for any endpoint that speaks
//! the OpenAI `/v1/chat/completions` API with SSE streaming. Covers OpenAI,
//! Google Gemini (OpenAI-compat mode), and local servers (vLLM, llama.cpp, LM Studio).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use crate::{
    AgentBackend, AgentEvent, AgentHandle, BackendError, BackendSpawnConfig, ShutdownToken,
};

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
        let index = tool_call.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
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
    let prompt_tokens = usage.get("prompt_tokens").and_then(|t| t.as_u64())?;
    let completion_tokens = usage.get("completion_tokens").and_then(|t| t.as_u64())?;
    Some(Usage {
        prompt_tokens,
        completion_tokens,
    })
}

// ── OpenAI Backend ───────────────────────────────────────────────────────────

/// Per-spawn state needed to cleanly shut down an OpenAI API session.
struct OpenAiShutdownState {
    stop: Arc<AtomicBool>,
}

/// OpenAI-compatible API backend.
///
/// Supports any endpoint that implements the `/v1/chat/completions` API with
/// SSE streaming: OpenAI, Google Gemini (OpenAI-compat mode), Azure OpenAI,
/// and local servers (vLLM, llama.cpp, LM Studio, Ollama).
pub struct OpenAiBackend {
    api_key: String,
    /// Model identifier (e.g. `"gpt-4o"`, `"gemini-2.0-flash"`).
    pub model: String,
    /// Base URL for the API endpoint (no trailing slash).
    pub endpoint: String,
}

impl OpenAiBackend {
    /// Create a new OpenAI-compatible backend.
    ///
    /// - `model`: defaults to `"gpt-4o"` if empty.
    /// - `endpoint`: defaults to `"https://api.openai.com"` if empty; trailing
    ///   slashes are stripped.
    pub fn new(api_key: &str, model: &str, endpoint: &str) -> Self {
        let model = if model.is_empty() {
            "gpt-4o".to_string()
        } else {
            model.to_string()
        };

        let endpoint = if endpoint.is_empty() {
            "https://api.openai.com".to_string()
        } else {
            endpoint.trim_end_matches('/').to_string()
        };

        Self {
            api_key: api_key.to_string(),
            model,
            endpoint,
        }
    }
}

impl AgentBackend for OpenAiBackend {
    fn name(&self) -> &str {
        "OpenAI API"
    }

    fn spawn(
        &self,
        config: &BackendSpawnConfig,
        generation: u64,
    ) -> Result<AgentHandle, BackendError> {
        let (event_tx, event_rx) = mpsc::channel::<AgentEvent>();
        let (message_tx, message_rx) = mpsc::channel::<String>();

        let stop = Arc::new(AtomicBool::new(false));

        // Clone values for the conversation thread.
        let api_key = self.api_key.clone();
        let model = self.model.clone();
        let endpoint = self.endpoint.clone();
        let system_prompt = config.system_prompt.clone();
        let initial_message = config.initial_message.clone();
        let allowed_tools = config.allowed_tools.clone();

        let stop_clone = Arc::clone(&stop);

        std::thread::Builder::new()
            .name("glass-openai-conversation".into())
            .spawn(move || {
                conversation_loop(
                    api_key,
                    model,
                    endpoint,
                    system_prompt,
                    initial_message,
                    allowed_tools,
                    generation,
                    message_rx,
                    event_tx,
                    stop_clone,
                );
            })
            .map_err(|e| BackendError::SpawnFailed(format!("failed to spawn thread: {e}")))?;

        tracing::info!(
            "OpenAiBackend: conversation thread spawned (model={}, generation={})",
            self.model,
            generation
        );

        Ok(AgentHandle {
            message_tx,
            event_rx,
            generation,
            shutdown_token: ShutdownToken::new(OpenAiShutdownState { stop }),
        })
    }

    fn shutdown(&self, token: ShutdownToken) {
        let Some(state) = token.downcast::<OpenAiShutdownState>() else {
            tracing::warn!("OpenAiBackend::shutdown: token type mismatch");
            return;
        };
        state.stop.store(true, Ordering::Relaxed);
    }
}

// ── Conversation loop ─────────────────────────────────────────────────────────

/// Main conversation loop running on a dedicated thread.
///
/// Manages the full message history, sends HTTP requests to the API,
/// streams SSE responses, executes tool calls via IPC, and emits events.
#[allow(clippy::too_many_arguments)]
fn conversation_loop(
    api_key: String,
    model: String,
    endpoint: String,
    system_prompt: String,
    initial_message: Option<String>,
    allowed_tools: Vec<String>,
    generation: u64,
    message_rx: mpsc::Receiver<String>,
    event_tx: mpsc::Sender<AgentEvent>,
    stop: Arc<AtomicBool>,
) {
    let ipc = crate::ipc_tools::SyncIpcClient::new();

    // Emit session init event.
    let session_id = format!("openai-{}", generation);
    let _ = event_tx.send(AgentEvent::Init {
        session_id: session_id.clone(),
    });

    // Build initial conversation history.
    let mut messages: Vec<serde_json::Value> =
        vec![serde_json::json!({"role": "system", "content": system_prompt})];

    // Send initial message if provided.
    if let Some(msg) = initial_message {
        messages.push(serde_json::json!({"role": "user", "content": msg}));
        if !do_turn(
            &api_key,
            &model,
            &endpoint,
            &mut messages,
            &allowed_tools,
            &ipc,
            &event_tx,
            &stop,
        ) {
            let _ = event_tx.send(AgentEvent::Crashed);
            return;
        }
    }

    // Main loop: wait for messages from orchestrator.
    for content in message_rx.iter() {
        if stop.load(Ordering::Relaxed) {
            break;
        }

        let user_content = extract_user_content(&content);
        messages.push(serde_json::json!({"role": "user", "content": user_content}));

        if !do_turn(
            &api_key,
            &model,
            &endpoint,
            &mut messages,
            &allowed_tools,
            &ipc,
            &event_tx,
            &stop,
        ) {
            break;
        }
    }

    let _ = event_tx.send(AgentEvent::Crashed);
}

// ── Turn execution ────────────────────────────────────────────────────────────

/// Accumulated state for a single tool call being streamed in.
#[derive(Default)]
struct AccumulatedToolCall {
    id: String,
    name: String,
    arguments: String,
}

/// Execute one complete conversation turn.
///
/// Sends the request, streams the SSE response, and handles tool call loops.
/// Returns `false` if the thread should exit (e.g. HTTP error, shutdown).
#[allow(clippy::too_many_arguments)]
fn do_turn(
    api_key: &str,
    model: &str,
    endpoint: &str,
    messages: &mut Vec<serde_json::Value>,
    allowed_tools: &[String],
    ipc: &crate::ipc_tools::SyncIpcClient,
    event_tx: &mpsc::Sender<AgentEvent>,
    stop: &Arc<AtomicBool>,
) -> bool {
    let max_tool_rounds = 10;

    for _round in 0..max_tool_rounds {
        if stop.load(Ordering::Relaxed) {
            return false;
        }

        // Build request body.
        let mut body = serde_json::json!({
            "model": model,
            "messages": messages,
            "stream": true,
            "stream_options": { "include_usage": true },
        });

        // Add tool definitions if allowed_tools is non-empty.
        if !allowed_tools.is_empty() {
            let tools: Vec<serde_json::Value> = allowed_tools
                .iter()
                .map(|name| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": name,
                            "description": format!("Glass MCP tool: {}", name),
                            "parameters": { "type": "object", "properties": {} }
                        }
                    })
                })
                .collect();
            body["tools"] = serde_json::json!(tools);
        }

        // Make HTTP request.
        let url = format!("{}/v1/chat/completions", endpoint);
        let body_str = serde_json::to_string(&body).unwrap_or_default();
        let response = match ureq::post(&url)
            .header("Authorization", &format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .send(body_str.as_bytes())
        {
            Ok(resp) => resp,
            Err(e) => {
                tracing::error!("OpenAiBackend: HTTP request failed: {}", e);
                return false;
            }
        };

        // Read SSE stream line by line.
        let body_reader = response.into_body().into_reader();
        let reader = std::io::BufReader::new(body_reader);
        let mut accumulated_text = String::new();
        let mut tool_calls: Vec<AccumulatedToolCall> = Vec::new();
        let mut total_usage: Option<Usage> = None;

        for line in std::io::BufRead::lines(reader) {
            if stop.load(Ordering::Relaxed) {
                return false;
            }

            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!("OpenAiBackend: SSE read error: {}", e);
                    break;
                }
            };

            if let Some(chunk) = parse_sse_line(&line) {
                match chunk {
                    SseChunk::TextDelta(text) => {
                        accumulated_text.push_str(&text);
                    }
                    SseChunk::ToolCallDelta {
                        index,
                        id,
                        function_name,
                        arguments_delta,
                    } => {
                        // Grow tool_calls vec if needed.
                        while tool_calls.len() <= index {
                            tool_calls.push(AccumulatedToolCall::default());
                        }
                        let tc = &mut tool_calls[index];
                        if let Some(id) = id {
                            tc.id = id;
                        }
                        if let Some(name) = function_name {
                            tc.name = name;
                        }
                        tc.arguments.push_str(&arguments_delta);
                    }
                    SseChunk::ReasoningDelta(text) => {
                        let _ = event_tx.send(AgentEvent::Thinking { text });
                    }
                    SseChunk::Done { usage } => {
                        total_usage = usage;
                        break;
                    }
                }
            }
        }

        // If we got tool calls, execute them and continue the loop.
        if !tool_calls.is_empty() {
            // Emit tool call events for the transcript.
            for tc in &tool_calls {
                let _ = event_tx.send(AgentEvent::ToolCall {
                    name: tc.name.clone(),
                    id: tc.id.clone(),
                    input: tc.arguments.clone(),
                });
            }

            // Add assistant message with tool_calls to history.
            let tool_calls_json: Vec<serde_json::Value> = tool_calls
                .iter()
                .map(|tc| {
                    serde_json::json!({
                        "id": tc.id,
                        "type": "function",
                        "function": {
                            "name": tc.name,
                            "arguments": tc.arguments,
                        }
                    })
                })
                .collect();

            let mut assistant_msg = serde_json::json!({"role": "assistant"});
            if !accumulated_text.is_empty() {
                assistant_msg["content"] = serde_json::json!(accumulated_text);
            }
            assistant_msg["tool_calls"] = serde_json::json!(tool_calls_json);
            messages.push(assistant_msg);

            // Execute each tool call via IPC.
            for tc in &tool_calls {
                let params: serde_json::Value =
                    serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({}));

                let result = match ipc.call_tool(&tc.name, params) {
                    Ok(v) => serde_json::to_string(&v).unwrap_or_else(|_| "null".to_string()),
                    Err(e) => format!("Tool error: {}", e),
                };

                let _ = event_tx.send(AgentEvent::ToolResult {
                    tool_use_id: tc.id.clone(),
                    content: result.clone(),
                });

                messages.push(serde_json::json!({
                    "role": "tool",
                    "tool_call_id": tc.id,
                    "content": result,
                }));
            }

            // Continue to next round (send another request with tool results).
            continue;
        }

        // No tool calls — this is the final response.
        if !accumulated_text.is_empty() {
            let _ = event_tx.send(AgentEvent::AssistantText {
                text: accumulated_text.clone(),
            });
        }

        // Add to history.
        messages.push(serde_json::json!({"role": "assistant", "content": accumulated_text}));

        // Emit turn complete with cost estimate.
        let cost_usd = total_usage
            .map(|u| {
                // Rough cost estimate: $2.50/1M input, $10/1M output for GPT-4o.
                (u.prompt_tokens as f64 * 2.5 / 1_000_000.0)
                    + (u.completion_tokens as f64 * 10.0 / 1_000_000.0)
            })
            .unwrap_or(0.0);
        let _ = event_tx.send(AgentEvent::TurnComplete { cost_usd });

        return true; // Turn complete, ready for next message.
    }

    tracing::warn!(
        "OpenAiBackend: max tool rounds ({}) exceeded",
        max_tool_rounds
    );
    true
}

// ── Message extraction ────────────────────────────────────────────────────────

/// Extract user content from a stream-json formatted message.
///
/// The orchestrator sends messages as:
/// `{"type":"user","message":{"role":"user","content":"..."}}`
///
/// We extract just the content string. Falls back to using the raw string
/// if the JSON format doesn't match.
fn extract_user_content(raw: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
        if let Some(content) = v.pointer("/message/content").and_then(|c| c.as_str()) {
            return content.to_string();
        }
    }
    // Fallback: use the raw string as content.
    raw.to_string()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── SSE parser tests ─────────────────────────────────────────────────────

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
            Some(SseChunk::Done { usage: Some(usage) }) => {
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

    // ── Backend struct tests ─────────────────────────────────────────────────

    #[test]
    fn backend_name() {
        let b = OpenAiBackend::new("key", "gpt-4o", "");
        assert_eq!(b.name(), "OpenAI API");
    }

    #[test]
    fn default_model_is_gpt4o() {
        let b = OpenAiBackend::new("key", "", "");
        assert_eq!(b.model, "gpt-4o");
    }

    #[test]
    fn default_endpoint_is_openai() {
        let b = OpenAiBackend::new("key", "", "");
        assert_eq!(b.endpoint, "https://api.openai.com");
    }

    #[test]
    fn custom_endpoint_strips_trailing_slash() {
        let b = OpenAiBackend::new("key", "", "http://localhost:8080/");
        assert_eq!(b.endpoint, "http://localhost:8080");
    }

    #[test]
    fn extract_content_from_stream_json() {
        let raw = r#"{"type":"user","message":{"role":"user","content":"hello world"}}"#;
        assert_eq!(extract_user_content(raw), "hello world");
    }

    #[test]
    fn extract_content_fallback_to_raw() {
        assert_eq!(extract_user_content("plain text"), "plain text");
    }
}
