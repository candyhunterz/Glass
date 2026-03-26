//! Anthropic Messages API backend.
//!
//! Implements [`AgentBackend`](crate::AgentBackend) for the Anthropic Messages API
//! with SSE streaming, native thinking blocks, and tool_use/tool_result content blocks.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use crate::{
    AgentBackend, AgentEvent, AgentHandle, BackendError, BackendSpawnConfig, ConversationConfig,
    ShutdownToken,
};

// ── SSE event types ──────────────────────────────────────────────────────────

/// Parsed SSE event from the Anthropic Messages streaming API.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum AnthropicSseEvent {
    /// A fragment of assistant text content.
    TextDelta(String),
    /// A fragment of extended thinking content.
    ThinkingDelta(String),
    /// A new tool_use content block has started.
    ToolUseStart {
        index: usize,
        id: String,
        name: String,
    },
    /// A fragment of tool input JSON.
    InputJsonDelta { index: usize, partial_json: String },
    /// Message-level delta with stop reason and output token count.
    MessageDelta {
        stop_reason: Option<String>,
        output_tokens: Option<u64>,
    },
    /// The message stream is complete.
    MessageStop,
    /// The message has started; carries input token usage.
    MessageStart { input_tokens: Option<u64> },
}

// ── SSE line parser ──────────────────────────────────────────────────────────

/// Parse a single SSE `data:` line into an [`AnthropicSseEvent`].
///
/// Returns `None` for lines that carry no actionable data (`event:` lines,
/// empty lines, SSE comments starting with `:`).
pub(crate) fn parse_anthropic_sse_line(line: &str) -> Option<AnthropicSseEvent> {
    // Only process `data:` lines.
    let data = line.strip_prefix("data:")?;
    let data = data.trim();

    if data.is_empty() {
        return None;
    }

    let v: serde_json::Value = serde_json::from_str(data).ok()?;
    let event_type = v.get("type")?.as_str()?;

    match event_type {
        "message_start" => {
            let input_tokens = v
                .pointer("/message/usage/input_tokens")
                .and_then(|t| t.as_u64());
            Some(AnthropicSseEvent::MessageStart { input_tokens })
        }

        "content_block_start" => {
            let index = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
            let block = v.get("content_block")?;
            let block_type = block.get("type")?.as_str()?;

            match block_type {
                "tool_use" => {
                    let id = block
                        .get("id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = block
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string();
                    Some(AnthropicSseEvent::ToolUseStart { index, id, name })
                }
                // text and thinking blocks start with empty content; no event needed.
                _ => None,
            }
        }

        "content_block_delta" => {
            let index = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
            let delta = v.get("delta")?;
            let delta_type = delta.get("type")?.as_str()?;

            match delta_type {
                "text_delta" => {
                    let text = delta.get("text")?.as_str()?.to_string();
                    if text.is_empty() {
                        None
                    } else {
                        Some(AnthropicSseEvent::TextDelta(text))
                    }
                }
                "thinking_delta" => {
                    let thinking = delta.get("thinking")?.as_str()?.to_string();
                    if thinking.is_empty() {
                        None
                    } else {
                        Some(AnthropicSseEvent::ThinkingDelta(thinking))
                    }
                }
                "input_json_delta" => {
                    let partial_json = delta
                        .get("partial_json")
                        .and_then(|p| p.as_str())
                        .unwrap_or("")
                        .to_string();
                    Some(AnthropicSseEvent::InputJsonDelta {
                        index,
                        partial_json,
                    })
                }
                _ => None,
            }
        }

        "message_delta" => {
            let stop_reason = v
                .pointer("/delta/stop_reason")
                .and_then(|s| s.as_str())
                .map(str::to_owned);
            let output_tokens = v.pointer("/usage/output_tokens").and_then(|t| t.as_u64());
            Some(AnthropicSseEvent::MessageDelta {
                stop_reason,
                output_tokens,
            })
        }

        "message_stop" => Some(AnthropicSseEvent::MessageStop),

        // content_block_stop, ping, etc. — ignored.
        _ => None,
    }
}

// ── Anthropic Backend ────────────────────────────────────────────────────────

/// Per-spawn state needed to cleanly shut down an Anthropic API session.
struct AnthropicShutdownState {
    stop: Arc<AtomicBool>,
}

/// Anthropic Messages API backend.
///
/// Supports the Anthropic `/v1/messages` endpoint with SSE streaming,
/// native thinking blocks, and tool_use/tool_result content blocks.
pub struct AnthropicBackend {
    api_key: String,
    /// Model identifier (e.g. `"claude-sonnet-4-6"`).
    pub model: String,
    /// Base URL for the API endpoint (no trailing slash).
    pub endpoint: String,
}

impl AnthropicBackend {
    /// Create a new Anthropic Messages API backend.
    ///
    /// - `model`: defaults to `"claude-sonnet-4-6"` if empty.
    /// - `endpoint`: defaults to `"https://api.anthropic.com"` if empty;
    ///   trailing slashes are stripped.
    pub fn new(api_key: &str, model: &str, endpoint: &str) -> Self {
        let model = if model.is_empty() {
            "claude-sonnet-4-6".to_string()
        } else {
            model.to_string()
        };

        let endpoint = if endpoint.is_empty() {
            "https://api.anthropic.com".to_string()
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

impl AgentBackend for AnthropicBackend {
    fn name(&self) -> &str {
        "Anthropic API"
    }

    fn spawn(
        &self,
        config: &BackendSpawnConfig,
        generation: u64,
    ) -> Result<AgentHandle, BackendError> {
        let (event_tx, event_rx) = mpsc::channel::<AgentEvent>();
        let (message_tx, message_rx) = mpsc::channel::<String>();

        let stop = Arc::new(AtomicBool::new(false));

        let conv_config = ConversationConfig {
            api_key: self.api_key.clone(),
            model: self.model.clone(),
            endpoint: self.endpoint.clone(),
            system_prompt: config.system_prompt.clone(),
            initial_message: config.initial_message.clone(),
            allowed_tools: config.allowed_tools.clone(),
            generation,
        };

        let stop_clone = Arc::clone(&stop);

        std::thread::Builder::new()
            .name("glass-anthropic-conversation".into())
            .spawn(move || {
                conversation_loop(conv_config, message_rx, event_tx, stop_clone);
            })
            .map_err(|e| BackendError::SpawnFailed(format!("failed to spawn thread: {e}")))?;

        tracing::info!(
            "AnthropicBackend: conversation thread spawned (model={}, generation={})",
            self.model,
            generation
        );

        Ok(AgentHandle {
            message_tx,
            event_rx,
            generation,
            shutdown_token: ShutdownToken::new(AnthropicShutdownState { stop }),
        })
    }

    fn shutdown(&self, token: ShutdownToken) {
        let Some(state) = token.downcast::<AnthropicShutdownState>() else {
            tracing::warn!("AnthropicBackend::shutdown: token type mismatch");
            return;
        };
        state.stop.store(true, Ordering::Relaxed);
    }
}

// ── Conversation loop ─────────────────────────────────────────────────────────

/// Main conversation loop running on a dedicated thread.
///
/// Manages the full message history, sends HTTP requests to the Anthropic API,
/// streams SSE responses, executes tool calls via IPC, and emits events.
fn conversation_loop(
    config: ConversationConfig,
    message_rx: mpsc::Receiver<String>,
    event_tx: mpsc::Sender<AgentEvent>,
    stop: Arc<AtomicBool>,
) {
    let ipc = crate::ipc_tools::SyncIpcClient::new();

    // Emit session init event.
    let session_id = format!("anthropic-{}", config.generation);
    let _ = event_tx.send(AgentEvent::Init {
        session_id: session_id.clone(),
    });

    // Anthropic uses a top-level `system` field, not a system message in the array.
    // Messages array starts empty (no system message).
    let mut messages: Vec<serde_json::Value> = Vec::new();

    // Send initial message if provided.
    if let Some(msg) = &config.initial_message {
        messages.push(serde_json::json!({"role": "user", "content": msg}));
        if !do_turn(&config, &mut messages, &ipc, &event_tx, &stop) {
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

        if !do_turn(&config, &mut messages, &ipc, &event_tx, &stop) {
            break;
        }
    }

    let _ = event_tx.send(AgentEvent::Crashed);
}

// ── Turn execution ────────────────────────────────────────────────────────────

/// Accumulated state for a single tool_use content block being streamed in.
#[derive(Default)]
struct AccumulatedToolUse {
    id: String,
    name: String,
    input_json: String,
}

/// Execute one complete conversation turn.
///
/// Sends the request, streams the SSE response, and handles tool call loops.
/// Returns `false` if the thread should exit (e.g. HTTP error, shutdown).
fn do_turn(
    config: &ConversationConfig,
    messages: &mut Vec<serde_json::Value>,
    ipc: &crate::ipc_tools::SyncIpcClient,
    event_tx: &mpsc::Sender<AgentEvent>,
    stop: &Arc<AtomicBool>,
) -> bool {
    let max_tool_rounds = 10;
    // Accumulates text across continuation rounds when max_tokens truncation occurs.
    let mut full_response = String::new();
    let mut continuations_remaining: u8 = 3;

    for _round in 0..max_tool_rounds {
        if stop.load(Ordering::Relaxed) {
            return false;
        }

        // Build request body (Anthropic Messages API format).
        let mut body = serde_json::json!({
            "model": config.model,
            "max_tokens": 16384,
            "system": config.system_prompt,
            "messages": messages,
            "stream": true,
        });

        // Add tool definitions if allowed_tools is non-empty.
        if !config.allowed_tools.is_empty() {
            let tools: Vec<serde_json::Value> = config.allowed_tools
                .iter()
                .map(|name| {
                    serde_json::json!({
                        "name": name,
                        "description": format!("Glass MCP tool: {}", name),
                        "input_schema": { "type": "object", "properties": {} }
                    })
                })
                .collect();
            body["tools"] = serde_json::json!(tools);
        }

        // Make HTTP request with Anthropic-specific headers.
        let url = format!("{}/v1/messages", config.endpoint);
        let body_str = serde_json::to_string(&body).unwrap_or_default();
        let response = match ureq::post(&url)
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .send(body_str.as_bytes())
        {
            Ok(resp) => resp,
            Err(e) => {
                tracing::error!("AnthropicBackend: HTTP request failed: {}", e);
                return false;
            }
        };

        // Read SSE stream line by line.
        let body_reader = response.into_body().into_reader();
        let reader = std::io::BufReader::new(body_reader);
        let mut accumulated_text = String::new();
        let mut tool_uses: Vec<AccumulatedToolUse> = Vec::new();
        let mut input_tokens: Option<u64> = None;
        let mut output_tokens: Option<u64> = None;
        let mut stop_reason: Option<String> = None;
        // Map content block index -> tool_uses vec index.
        let mut index_to_tool: std::collections::HashMap<usize, usize> =
            std::collections::HashMap::new();

        for line in std::io::BufRead::lines(reader) {
            if stop.load(Ordering::Relaxed) {
                return false;
            }

            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!("AnthropicBackend: SSE read error: {}", e);
                    break;
                }
            };

            if let Some(event) = parse_anthropic_sse_line(&line) {
                match event {
                    AnthropicSseEvent::MessageStart { input_tokens: it } => {
                        input_tokens = it;
                    }
                    AnthropicSseEvent::TextDelta(text) => {
                        accumulated_text.push_str(&text);
                    }
                    AnthropicSseEvent::ThinkingDelta(text) => {
                        let _ = event_tx.send(AgentEvent::Thinking { text });
                    }
                    AnthropicSseEvent::ToolUseStart { index, id, name } => {
                        let tool_idx = tool_uses.len();
                        tool_uses.push(AccumulatedToolUse {
                            id,
                            name,
                            input_json: String::new(),
                        });
                        index_to_tool.insert(index, tool_idx);
                    }
                    AnthropicSseEvent::InputJsonDelta {
                        index,
                        partial_json,
                    } => {
                        if let Some(&tool_idx) = index_to_tool.get(&index) {
                            if let Some(tc) = tool_uses.get_mut(tool_idx) {
                                tc.input_json.push_str(&partial_json);
                            }
                        }
                    }
                    AnthropicSseEvent::MessageDelta {
                        stop_reason: sr,
                        output_tokens: ot,
                    } => {
                        stop_reason = sr;
                        if ot.is_some() {
                            output_tokens = ot;
                        }
                    }
                    AnthropicSseEvent::MessageStop => {
                        break;
                    }
                }
            }
        }

        // Handle tool_use stop reason — execute tools and continue.
        if stop_reason.as_deref() == Some("tool_use") && !tool_uses.is_empty() {
            // Emit tool call events for the transcript.
            for tc in &tool_uses {
                let _ = event_tx.send(AgentEvent::ToolCall {
                    name: tc.name.clone(),
                    id: tc.id.clone(),
                    input: tc.input_json.clone(),
                });
            }

            // Build assistant message with content blocks (Anthropic format).
            let mut content_blocks: Vec<serde_json::Value> = Vec::new();
            if !accumulated_text.is_empty() {
                content_blocks.push(serde_json::json!({"type": "text", "text": accumulated_text}));
            }
            for tc in &tool_uses {
                let input: serde_json::Value =
                    serde_json::from_str(&tc.input_json).unwrap_or(serde_json::json!({}));
                content_blocks.push(serde_json::json!({
                    "type": "tool_use",
                    "id": tc.id,
                    "name": tc.name,
                    "input": input,
                }));
            }
            messages.push(serde_json::json!({"role": "assistant", "content": content_blocks}));

            // Execute each tool call via IPC and build tool_result content blocks.
            let mut tool_result_blocks: Vec<serde_json::Value> = Vec::new();
            for tc in &tool_uses {
                let params: serde_json::Value =
                    serde_json::from_str(&tc.input_json).unwrap_or(serde_json::json!({}));

                let result = match ipc.call_tool(&tc.name, params) {
                    Ok(v) => serde_json::to_string(&v).unwrap_or_else(|_| "null".to_string()),
                    Err(e) => format!("Tool error: {}", e),
                };

                let _ = event_tx.send(AgentEvent::ToolResult {
                    tool_use_id: tc.id.clone(),
                    content: result.clone(),
                });

                tool_result_blocks.push(serde_json::json!({
                    "type": "tool_result",
                    "tool_use_id": tc.id,
                    "content": result,
                }));
            }

            // Anthropic expects tool results in a user message with content blocks.
            messages.push(serde_json::json!({"role": "user", "content": tool_result_blocks}));

            // Continue to next round (send another request with tool results).
            continue;
        }

        // Handle max_tokens truncation — automatically continue the response.
        // Uses a separate counter so continuations don't starve tool call rounds.
        if stop_reason.as_deref() == Some("max_tokens") && !accumulated_text.is_empty() {
            full_response.push_str(&accumulated_text);
            if continuations_remaining > 0 {
                continuations_remaining -= 1;
                tracing::info!("AnthropicBackend: response truncated at max_tokens, requesting continuation ({} remaining)", continuations_remaining);
                messages
                    .push(serde_json::json!({"role": "assistant", "content": accumulated_text}));
                messages.push(serde_json::json!({"role": "user", "content": "Your previous response was truncated due to length. Please continue exactly where you left off."}));
                continue;
            }
            tracing::warn!(
                "AnthropicBackend: max continuations exhausted, emitting partial response"
            );
        }

        // No tool calls — this is the final response.
        full_response.push_str(&accumulated_text);
        if !full_response.is_empty() {
            let _ = event_tx.send(AgentEvent::AssistantText {
                text: full_response.clone(),
            });
        }

        // Add to history.
        messages.push(serde_json::json!({"role": "assistant", "content": full_response}));

        // Emit turn complete with cost estimate.
        // Sonnet pricing: ~$3/1M input, ~$15/1M output.
        let cost_usd = {
            let input_cost = input_tokens.unwrap_or(0) as f64 * 3.0 / 1_000_000.0;
            let output_cost = output_tokens.unwrap_or(0) as f64 * 15.0 / 1_000_000.0;
            input_cost + output_cost
        };
        let _ = event_tx.send(AgentEvent::TurnComplete { cost_usd });

        return true; // Turn complete, ready for next message.
    }

    tracing::warn!(
        "AnthropicBackend: max tool rounds ({}) exceeded",
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
        let line = r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
        match parse_anthropic_sse_line(line) {
            Some(AnthropicSseEvent::TextDelta(text)) => assert_eq!(text, "Hello"),
            other => panic!("expected TextDelta, got {other:?}"),
        }
    }

    #[test]
    fn parse_thinking_delta() {
        let line = r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Let me analyze..."}}"#;
        match parse_anthropic_sse_line(line) {
            Some(AnthropicSseEvent::ThinkingDelta(text)) => {
                assert_eq!(text, "Let me analyze...");
            }
            other => panic!("expected ThinkingDelta, got {other:?}"),
        }
    }

    #[test]
    fn parse_tool_use_start() {
        let line = r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"glass_query","input":{}}}"#;
        match parse_anthropic_sse_line(line) {
            Some(AnthropicSseEvent::ToolUseStart { index, id, name }) => {
                assert_eq!(index, 1);
                assert_eq!(id, "toolu_1");
                assert_eq!(name, "glass_query");
            }
            other => panic!("expected ToolUseStart, got {other:?}"),
        }
    }

    #[test]
    fn parse_input_json_delta() {
        let line = r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"text\":"}}"#;
        match parse_anthropic_sse_line(line) {
            Some(AnthropicSseEvent::InputJsonDelta {
                index,
                partial_json,
            }) => {
                assert_eq!(index, 1);
                assert_eq!(partial_json, r#"{"text":"#);
            }
            other => panic!("expected InputJsonDelta, got {other:?}"),
        }
    }

    #[test]
    fn parse_message_delta_end_turn() {
        let line = r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":15}}"#;
        match parse_anthropic_sse_line(line) {
            Some(AnthropicSseEvent::MessageDelta {
                stop_reason,
                output_tokens,
            }) => {
                assert_eq!(stop_reason.as_deref(), Some("end_turn"));
                assert_eq!(output_tokens, Some(15));
            }
            other => panic!("expected MessageDelta, got {other:?}"),
        }
    }

    #[test]
    fn parse_message_stop() {
        let line = r#"data: {"type":"message_stop"}"#;
        match parse_anthropic_sse_line(line) {
            Some(AnthropicSseEvent::MessageStop) => {}
            other => panic!("expected MessageStop, got {other:?}"),
        }
    }

    #[test]
    fn parse_message_start_with_input_tokens() {
        let line = r#"data: {"type":"message_start","message":{"id":"msg_1","model":"claude-sonnet-4-6","usage":{"input_tokens":25,"output_tokens":0}}}"#;
        match parse_anthropic_sse_line(line) {
            Some(AnthropicSseEvent::MessageStart { input_tokens }) => {
                assert_eq!(input_tokens, Some(25));
            }
            other => panic!("expected MessageStart, got {other:?}"),
        }
    }

    #[test]
    fn parse_non_data_returns_none() {
        assert!(parse_anthropic_sse_line("event: message_start").is_none());
        assert!(parse_anthropic_sse_line("").is_none());
        assert!(parse_anthropic_sse_line(": comment").is_none());
    }

    #[test]
    fn parse_content_block_start_text_returns_none() {
        let line = r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#;
        assert!(parse_anthropic_sse_line(line).is_none());
    }

    #[test]
    fn parse_empty_text_delta_returns_none() {
        let line = r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":""}}"#;
        assert!(parse_anthropic_sse_line(line).is_none());
    }

    // ── Backend struct tests ─────────────────────────────────────────────────

    #[test]
    fn backend_name() {
        let b = AnthropicBackend::new("k", "", "");
        assert_eq!(b.name(), "Anthropic API");
    }

    #[test]
    fn default_model() {
        let b = AnthropicBackend::new("k", "", "");
        assert_eq!(b.model, "claude-sonnet-4-6");
    }

    #[test]
    fn default_endpoint() {
        let b = AnthropicBackend::new("k", "", "");
        assert_eq!(b.endpoint, "https://api.anthropic.com");
    }

    #[test]
    fn custom_model() {
        let b = AnthropicBackend::new("k", "claude-opus-4-6", "");
        assert_eq!(b.model, "claude-opus-4-6");
    }

    #[test]
    fn custom_endpoint_strips_trailing_slash() {
        let b = AnthropicBackend::new("k", "", "https://custom.api.com/");
        assert_eq!(b.endpoint, "https://custom.api.com");
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
