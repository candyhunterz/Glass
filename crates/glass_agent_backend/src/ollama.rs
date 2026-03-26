//! Ollama backend — native `/api/chat` with JSON line streaming.
//!
//! Implements [`AgentBackend`](crate::AgentBackend) for a running Ollama
//! instance. Unlike the OpenAI backend this uses newline-delimited JSON
//! (not SSE), and does not require authentication.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use crate::{
    AgentBackend, AgentEvent, AgentHandle, BackendError, BackendSpawnConfig, ShutdownToken,
};

// ── Types ─────────────────────────────────────────────────────────────────────

/// A parsed chunk from one JSON line in an Ollama streaming response.
#[derive(Debug, Clone)]
pub(crate) enum OllamaChunk {
    /// A fragment of assistant text content.
    TextDelta(String),
    /// One or more complete tool calls (Ollama sends them in a single line).
    ToolCalls(Vec<OllamaToolCall>),
    /// The stream is complete. Carries optional token count (tokens generated).
    Done { _eval_count: Option<u64> },
}

/// A single tool call returned by Ollama.
#[derive(Debug, Clone)]
pub(crate) struct OllamaToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

// ── JSON line parser ──────────────────────────────────────────────────────────

/// Parse a single JSON line from an Ollama streaming response into an
/// [`OllamaChunk`].
///
/// Returns `None` for empty lines, whitespace-only lines, or malformed JSON.
pub(crate) fn parse_ollama_line(line: &str) -> Option<OllamaChunk> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    let v: serde_json::Value = serde_json::from_str(line).ok()?;

    // Check for `done: true` first — the terminal line.
    if v.get("done").and_then(|d| d.as_bool()) == Some(true) {
        let eval_count = v.get("eval_count").and_then(|e| e.as_u64());
        return Some(OllamaChunk::Done {
            _eval_count: eval_count,
        });
    }

    let message = v.get("message")?;

    // Tool calls — arrive as a complete array in a single line.
    if let Some(tool_calls) = message.get("tool_calls").and_then(|tc| tc.as_array()) {
        let calls: Vec<OllamaToolCall> = tool_calls
            .iter()
            .filter_map(|tc| {
                let func = tc.get("function")?;
                let name = func.get("name")?.as_str()?.to_owned();
                let arguments = func
                    .get("arguments")
                    .cloned()
                    .unwrap_or(serde_json::json!({}));
                Some(OllamaToolCall { name, arguments })
            })
            .collect();

        if !calls.is_empty() {
            return Some(OllamaChunk::ToolCalls(calls));
        }
    }

    // Text content delta.
    if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
        if !content.is_empty() {
            return Some(OllamaChunk::TextDelta(content.to_owned()));
        }
    }

    None
}

// ── Ollama Backend ────────────────────────────────────────────────────────────

/// Per-spawn state needed to cleanly shut down an Ollama session.
struct OllamaShutdownState {
    stop: Arc<AtomicBool>,
}

/// Native Ollama backend.
///
/// Connects to a running Ollama instance (default `http://localhost:11434`)
/// and streams responses via `/api/chat` with newline-delimited JSON.
/// No authentication is required.
pub struct OllamaBackend {
    /// Model identifier (e.g. `"llama3"`, `"llama3:70b"`).
    pub model: String,
    /// Base URL for the Ollama instance (no trailing slash).
    pub endpoint: String,
}

impl OllamaBackend {
    /// Create a new Ollama backend.
    ///
    /// - `model`: defaults to `"llama3"` if empty.
    /// - `endpoint`: defaults to `"http://localhost:11434"` if empty; trailing
    ///   slashes are stripped.
    pub fn new(model: &str, endpoint: &str) -> Self {
        Self {
            model: if model.is_empty() {
                "llama3".to_string()
            } else {
                model.to_string()
            },
            endpoint: if endpoint.is_empty() {
                "http://localhost:11434".to_string()
            } else {
                endpoint.trim_end_matches('/').to_string()
            },
        }
    }
}

impl AgentBackend for OllamaBackend {
    fn name(&self) -> &str {
        "Ollama"
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
        let model = self.model.clone();
        let endpoint = self.endpoint.clone();
        let system_prompt = config.system_prompt.clone();
        let initial_message = config.initial_message.clone();
        let allowed_tools = config.allowed_tools.clone();

        let stop_clone = Arc::clone(&stop);

        std::thread::Builder::new()
            .name("glass-ollama-conversation".into())
            .spawn(move || {
                conversation_loop(
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
            "OllamaBackend: conversation thread spawned (model={}, generation={})",
            self.model,
            generation
        );

        Ok(AgentHandle {
            message_tx,
            event_rx,
            generation,
            shutdown_token: ShutdownToken::new(OllamaShutdownState { stop }),
        })
    }

    fn shutdown(&self, token: ShutdownToken) {
        let Some(state) = token.downcast::<OllamaShutdownState>() else {
            tracing::warn!("OllamaBackend::shutdown: token type mismatch");
            return;
        };
        state.stop.store(true, Ordering::Relaxed);
    }
}

// ── Conversation loop ─────────────────────────────────────────────────────────

/// Main conversation loop running on a dedicated thread.
///
/// Manages the full message history, sends HTTP requests to the Ollama API,
/// streams JSON line responses, executes tool calls via IPC, and emits events.
#[allow(clippy::too_many_arguments)]
fn conversation_loop(
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
    let session_id = format!("ollama-{}", generation);
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

/// Execute one complete conversation turn.
///
/// Sends the request, streams the JSON line response, and handles tool call
/// loops. Returns `false` if the thread should exit (e.g. HTTP error, shutdown).
#[allow(clippy::too_many_arguments)]
fn do_turn(
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

        // Make HTTP request to Ollama's /api/chat endpoint.
        let url = format!("{}/api/chat", endpoint);
        let body_str = serde_json::to_string(&body).unwrap_or_default();
        let response = match ureq::post(&url)
            .header("Content-Type", "application/json")
            .send(body_str.as_bytes())
        {
            Ok(resp) => resp,
            Err(e) => {
                tracing::error!("OllamaBackend: HTTP request failed: {}", e);
                return false;
            }
        };

        // Read JSON lines from response body.
        let body_reader = response.into_body().into_reader();
        let reader = std::io::BufReader::new(body_reader);
        let mut accumulated_text = String::new();
        let mut tool_calls: Vec<OllamaToolCall> = Vec::new();

        for line in std::io::BufRead::lines(reader) {
            if stop.load(Ordering::Relaxed) {
                return false;
            }

            let line = match line {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!("OllamaBackend: read error: {}", e);
                    break;
                }
            };

            if let Some(chunk) = parse_ollama_line(&line) {
                match chunk {
                    OllamaChunk::TextDelta(text) => {
                        accumulated_text.push_str(&text);
                    }
                    OllamaChunk::ToolCalls(calls) => {
                        tool_calls.extend(calls);
                    }
                    OllamaChunk::Done { .. } => {
                        break;
                    }
                }
            }
        }

        // If we got tool calls, execute them and continue the loop.
        if !tool_calls.is_empty() {
            // Emit tool call events for the transcript.
            for (i, tc) in tool_calls.iter().enumerate() {
                let call_id = format!("ollama-call-{}-{}", _round, i);
                let _ = event_tx.send(AgentEvent::ToolCall {
                    name: tc.name.clone(),
                    id: call_id.clone(),
                    input: serde_json::to_string(&tc.arguments).unwrap_or_default(),
                });
            }

            // Add assistant message with tool_calls to history.
            let tool_calls_json: Vec<serde_json::Value> = tool_calls
                .iter()
                .map(|tc| {
                    serde_json::json!({
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
            } else {
                assistant_msg["content"] = serde_json::json!("");
            }
            assistant_msg["tool_calls"] = serde_json::json!(tool_calls_json);
            messages.push(assistant_msg);

            // Execute each tool call via IPC and add results to history.
            for tc in &tool_calls {
                let result = match ipc.call_tool(&tc.name, tc.arguments.clone()) {
                    Ok(v) => serde_json::to_string(&v).unwrap_or_else(|_| "null".to_string()),
                    Err(e) => format!("Tool error: {}", e),
                };

                let call_id = "ollama-tool-result".to_string();
                let _ = event_tx.send(AgentEvent::ToolResult {
                    tool_use_id: call_id,
                    content: result.clone(),
                });

                // Ollama expects tool results as role: "tool".
                messages.push(serde_json::json!({
                    "role": "tool",
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

        // Emit turn complete — local models are free, cost is always 0.
        let _ = event_tx.send(AgentEvent::TurnComplete { cost_usd: 0.0 });

        return true; // Turn complete, ready for next message.
    }

    tracing::warn!(
        "OllamaBackend: max tool rounds ({}) exceeded",
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

// ── Model list fetching ───────────────────────────────────────────────────────

/// Fetch available models from a running Ollama instance.
///
/// Queries `GET {endpoint}/api/tags` and maps each model to a
/// [`CachedModel`](crate::model_cache::CachedModel) with provider `"ollama"`.
///
/// Returns an empty `Vec` if the Ollama instance is unreachable or the
/// response cannot be parsed.
pub fn fetch_ollama_models(endpoint: &str) -> Vec<crate::model_cache::CachedModel> {
    let url = format!("{}/api/tags", endpoint.trim_end_matches('/'));

    match ureq::get(&url).call() {
        Ok(resp) => {
            let body = match resp.into_body().read_to_string() {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!("fetch_ollama_models: failed to read response body: {}", e);
                    return Vec::new();
                }
            };

            let v: serde_json::Value = match serde_json::from_str(&body) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("fetch_ollama_models: failed to parse JSON: {}", e);
                    return Vec::new();
                }
            };

            let Some(models) = v.get("models").and_then(|m| m.as_array()) else {
                tracing::warn!("fetch_ollama_models: response missing 'models' array");
                return Vec::new();
            };

            models
                .iter()
                .filter_map(|entry| {
                    let name = entry.get("name").and_then(|n| n.as_str())?;
                    Some(crate::model_cache::CachedModel {
                        id: name.to_string(),
                        display_name: name.to_string(),
                        provider: "ollama".to_string(),
                    })
                })
                .collect()
        }
        Err(e) => {
            tracing::warn!("fetch_ollama_models: HTTP request to {} failed: {}", url, e);
            Vec::new()
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── JSON line parser tests ────────────────────────────────────────────────

    #[test]
    fn parse_text_delta() {
        let line =
            r#"{"model":"llama3","message":{"role":"assistant","content":"Hi"},"done":false}"#;
        match parse_ollama_line(line) {
            Some(OllamaChunk::TextDelta(text)) => assert_eq!(text, "Hi"),
            other => panic!("expected TextDelta, got {other:?}"),
        }
    }

    #[test]
    fn parse_done() {
        let line = r#"{"model":"llama3","message":{"role":"assistant","content":""},"done":true,"eval_count":50}"#;
        match parse_ollama_line(line) {
            Some(OllamaChunk::Done { _eval_count }) => assert_eq!(_eval_count, Some(50)),
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn parse_done_without_eval_count() {
        let line = r#"{"done":true}"#;
        match parse_ollama_line(line) {
            Some(OllamaChunk::Done { _eval_count }) => assert_eq!(_eval_count, None),
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn parse_tool_calls() {
        let line = r#"{"model":"llama3","message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"glass_query","arguments":{"text":"test"}}}]},"done":false}"#;
        match parse_ollama_line(line) {
            Some(OllamaChunk::ToolCalls(calls)) => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].name, "glass_query");
                assert_eq!(calls[0].arguments, serde_json::json!({"text": "test"}));
            }
            other => panic!("expected ToolCalls, got {other:?}"),
        }
    }

    #[test]
    fn parse_multiple_tool_calls() {
        let line = r#"{"model":"llama3","message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":"glass_query","arguments":{"text":"a"}}},{"function":{"name":"glass_history","arguments":{}}}]},"done":false}"#;
        match parse_ollama_line(line) {
            Some(OllamaChunk::ToolCalls(calls)) => {
                assert_eq!(calls.len(), 2);
                assert_eq!(calls[0].name, "glass_query");
                assert_eq!(calls[1].name, "glass_history");
            }
            other => panic!("expected ToolCalls, got {other:?}"),
        }
    }

    #[test]
    fn parse_empty_line_returns_none() {
        assert!(parse_ollama_line("").is_none());
        assert!(parse_ollama_line("   ").is_none());
    }

    #[test]
    fn parse_invalid_json_returns_none() {
        assert!(parse_ollama_line("not json at all").is_none());
        assert!(parse_ollama_line("{broken").is_none());
    }

    #[test]
    fn parse_empty_content_returns_none() {
        let line = r#"{"model":"llama3","message":{"role":"assistant","content":""},"done":false}"#;
        assert!(parse_ollama_line(line).is_none());
    }

    // ── Backend struct tests ──────────────────────────────────────────────────

    #[test]
    fn backend_name() {
        assert_eq!(OllamaBackend::new("", "").name(), "Ollama");
    }

    #[test]
    fn default_model() {
        assert_eq!(OllamaBackend::new("", "").model, "llama3");
    }

    #[test]
    fn default_endpoint() {
        assert_eq!(
            OllamaBackend::new("", "").endpoint,
            "http://localhost:11434"
        );
    }

    #[test]
    fn custom_model() {
        assert_eq!(OllamaBackend::new("llama3:70b", "").model, "llama3:70b");
    }

    #[test]
    fn custom_endpoint_strips_slash() {
        assert_eq!(
            OllamaBackend::new("", "http://myhost:11434/").endpoint,
            "http://myhost:11434"
        );
    }

    #[test]
    fn custom_endpoint_no_slash() {
        assert_eq!(
            OllamaBackend::new("", "http://myhost:11434").endpoint,
            "http://myhost:11434"
        );
    }

    // ── Message extraction tests ──────────────────────────────────────────────

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
