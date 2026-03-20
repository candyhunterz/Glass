# Multi-Provider Backend Phase 2 — OpenAI-Compatible Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an OpenAI-compatible API backend that enables Glass orchestrator to use GPT, Gemini (OpenAI-compat mode), and any OpenAI-compatible local model server (vLLM, llama.cpp, LM Studio).

**Architecture:** New `OpenAiBackend` struct implements `AgentBackend` trait. Uses `ureq` for HTTP POST to `/v1/chat/completions` with SSE streaming. A dedicated reader thread parses SSE chunks into `AgentEvent`. A writer thread accumulates conversation history and sends HTTP requests. Tool calling uses IPC to execute Glass MCP tools via the running Glass GUI process. `resolve_backend()` factory routes config to the correct backend.

**Tech Stack:** Rust, ureq 3 (blocking HTTP), SSE line parsing, serde_json, Glass IPC client (Unix socket / named pipe)

**Spec:** `docs/superpowers/specs/2026-03-20-multi-provider-backend-design.md` — Phase 2 section

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/glass_agent_backend/src/openai.rs` | `OpenAiBackend` struct, `AgentBackend` impl, SSE parsing, conversation management, tool calling loop |
| `crates/glass_agent_backend/src/ipc_tools.rs` | Synchronous IPC client for calling Glass MCP tools from API backends (blocking version of glass_mcp's async IpcClient) |
| `crates/glass_agent_backend/src/model_cache.rs` | Fetch and cache model lists from provider APIs (24h file cache) |
| `crates/glass_agent_backend/src/lib.rs` | Add `resolve_backend()` factory, register new modules |
| `crates/glass_agent_backend/Cargo.toml` | Add `ureq` dependency |
| `src/main.rs` | Use `resolve_backend()` instead of hardcoded `ClaudeCliBackend::new()` |

---

### Task 1: Add `ureq` dependency and create module files

**Files:**
- Modify: `crates/glass_agent_backend/Cargo.toml`
- Create: `crates/glass_agent_backend/src/openai.rs`
- Create: `crates/glass_agent_backend/src/ipc_tools.rs`
- Create: `crates/glass_agent_backend/src/model_cache.rs`
- Modify: `crates/glass_agent_backend/src/lib.rs`

- [ ] **Step 1: Add `ureq` to `Cargo.toml`**

```toml
ureq = { workspace = true }
```

- [ ] **Step 2: Create placeholder module files**

`openai.rs`:
```rust
//! OpenAI-compatible API backend.
//!
//! Implements `AgentBackend` for any endpoint that speaks the OpenAI
//! `/v1/chat/completions` API with SSE streaming.
```

`ipc_tools.rs`:
```rust
//! Synchronous IPC client for calling Glass MCP tools from API backends.
//!
//! Provides a blocking interface to the Glass GUI's IPC listener,
//! suitable for use from the backend's writer thread (which is a regular
//! OS thread, not an async task).
```

`model_cache.rs`:
```rust
//! Model list caching — fetch from provider APIs and cache to disk.
```

- [ ] **Step 3: Register modules in `lib.rs`**

Add after `pub mod claude_cli;`:
```rust
pub mod ipc_tools;
pub mod model_cache;
pub mod openai;
```

- [ ] **Step 4: Verify compilation**

Run: `cargo build -p glass_agent_backend`

- [ ] **Step 5: Commit**

```
feat: scaffold Phase 2 modules (openai, ipc_tools, model_cache)
```

---

### Task 2: Implement synchronous IPC tool client

The existing `glass_mcp::ipc_client::IpcClient` is async (tokio). API backends run on OS threads, so we need a blocking version. This is a small, focused module.

**Files:**
- Modify: `crates/glass_agent_backend/src/ipc_tools.rs`

- [ ] **Step 1: Implement `SyncIpcClient`**

```rust
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, serde::Serialize)]
struct IpcRequest {
    id: u64,
    method: String,
    params: serde_json::Value,
}

#[derive(Debug, serde::Deserialize)]
struct IpcResponse {
    #[allow(dead_code)]
    id: u64,
    result: Option<serde_json::Value>,
    error: Option<String>,
}

/// Blocking IPC client for calling Glass MCP tools.
///
/// Each `call_tool()` opens a fresh connection to the Glass GUI's IPC
/// listener (Unix domain socket or Windows named pipe), sends a JSON-line
/// request, and reads the response. Connection-per-request handles GUI
/// restarts gracefully.
pub struct SyncIpcClient {
    next_id: AtomicU64,
}

impl SyncIpcClient {
    pub fn new() -> Self {
        Self { next_id: AtomicU64::new(1) }
    }

    /// Call a Glass MCP tool by name with the given params.
    /// Returns the tool result as a JSON value, or an error string.
    pub fn call_tool(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = IpcRequest { id, method: method.to_string(), params };

        let mut stream = connect().map_err(|e| format!("Glass GUI not running: {}", e))?;

        // Write request
        let mut payload = serde_json::to_vec(&request).map_err(|e| format!("serialize: {}", e))?;
        payload.push(b'\n');
        stream.write_all(&payload).map_err(|e| format!("write: {}", e))?;
        stream.flush().map_err(|e| format!("flush: {}", e))?;

        // Read response with timeout
        // Set read timeout to 5 seconds
        set_read_timeout(&stream, std::time::Duration::from_secs(5));

        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| format!("read: {}", e))?;

        let resp: IpcResponse = serde_json::from_str(&line).map_err(|e| format!("parse: {}", e))?;
        if let Some(err) = resp.error {
            return Err(err);
        }
        Ok(resp.result.unwrap_or(serde_json::Value::Null))
    }
}

// Platform-specific connection — mirrors glass_mcp::ipc_client

#[cfg(unix)]
fn connect() -> Result<std::os::unix::net::UnixStream, String> {
    let path = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join(".glass")
        .join("glass.sock");
    std::os::unix::net::UnixStream::connect(&path)
        .map_err(|e| format!("{}: {}", path.display(), e))
}

#[cfg(unix)]
fn set_read_timeout(stream: &std::os::unix::net::UnixStream, timeout: std::time::Duration) {
    let _ = stream.set_read_timeout(Some(timeout));
}

#[cfg(windows)]
fn connect() -> Result<std::fs::File, String> {
    use std::fs::OpenOptions;
    let pipe_name = r"\\.\pipe\glass-terminal";
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(pipe_name)
        .map_err(|e| format!("{}: {}", pipe_name, e))
}

#[cfg(windows)]
fn set_read_timeout(_stream: &std::fs::File, _timeout: std::time::Duration) {
    // Named pipes don't support read timeouts directly;
    // the 5s timeout is best-effort on Windows.
}
```

- [ ] **Step 2: Add tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_creates_without_connecting() {
        let client = SyncIpcClient::new();
        assert_eq!(client.next_id.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn call_tool_returns_error_when_gui_not_running() {
        let client = SyncIpcClient::new();
        let result = client.call_tool("glass_ping", serde_json::json!({}));
        // Should fail because Glass GUI is not running in test
        assert!(result.is_err());
    }
}
```

- [ ] **Step 3: Verify and commit**

Run: `cargo test -p glass_agent_backend`

```
feat: synchronous IPC client for calling Glass MCP tools from API backends
```

---

### Task 3: Implement SSE stream parser

Parse OpenAI-style Server-Sent Events from an HTTP response body. This is the core I/O layer for the OpenAI backend.

**Files:**
- Modify: `crates/glass_agent_backend/src/openai.rs`

- [ ] **Step 1: Implement SSE line parser**

```rust
/// A single parsed SSE chunk from the OpenAI streaming response.
#[derive(Debug, Clone)]
pub(crate) enum SseChunk {
    /// A delta content chunk (text fragment).
    TextDelta(String),
    /// A tool call chunk (accumulated across multiple deltas).
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        function_name: Option<String>,
        arguments_delta: String,
    },
    /// The stream is done — includes optional usage data.
    Done { usage: Option<Usage> },
    /// A reasoning/thinking content delta (for models that support it).
    ReasoningDelta(String),
}

#[derive(Debug, Clone)]
pub(crate) struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

/// Parse a single SSE `data:` line into an `SseChunk`.
///
/// Returns `None` for non-data lines, empty data, or unparseable JSON.
/// Returns `Some(SseChunk::Done)` for `data: [DONE]`.
pub(crate) fn parse_sse_line(line: &str) -> Option<SseChunk> {
    let data = line.strip_prefix("data: ")?.trim();
    if data == "[DONE]" {
        return Some(SseChunk::Done { usage: None });
    }
    let val: serde_json::Value = serde_json::from_str(data).ok()?;

    // Check for usage in the final chunk
    let usage = val.get("usage").and_then(|u| {
        Some(Usage {
            prompt_tokens: u.get("prompt_tokens")?.as_u64()?,
            completion_tokens: u.get("completion_tokens")?.as_u64()?,
        })
    });

    let choices = val.get("choices")?.as_array()?;
    let choice = choices.first()?;
    let delta = choice.get("delta")?;

    // Check finish_reason
    if choice.get("finish_reason").and_then(|f| f.as_str()) == Some("stop") {
        return Some(SseChunk::Done { usage });
    }

    // Tool calls
    if let Some(tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
        for tc in tool_calls {
            let index = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
            let id = tc.get("id").and_then(|i| i.as_str()).map(|s| s.to_string());
            let func = tc.get("function")?;
            let name = func.get("name").and_then(|n| n.as_str()).map(|s| s.to_string());
            let args = func.get("arguments").and_then(|a| a.as_str()).unwrap_or("");
            return Some(SseChunk::ToolCallDelta {
                index,
                id,
                function_name: name,
                arguments_delta: args.to_string(),
            });
        }
    }

    // Reasoning content (o-series models)
    if let Some(reasoning) = delta.get("reasoning_content").and_then(|r| r.as_str()) {
        if !reasoning.is_empty() {
            return Some(SseChunk::ReasoningDelta(reasoning.to_string()));
        }
    }

    // Regular text content
    if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
        if !content.is_empty() {
            return Some(SseChunk::TextDelta(content.to_string()));
        }
    }

    // If we got a chunk with usage but no other content, it's a done signal
    if usage.is_some() {
        return Some(SseChunk::Done { usage });
    }

    None
}
```

- [ ] **Step 2: Write tests for SSE parsing**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_text_delta() {
        let line = r#"data: {"choices":[{"delta":{"content":"Hello"},"index":0}]}"#;
        match parse_sse_line(line) {
            Some(SseChunk::TextDelta(t)) => assert_eq!(t, "Hello"),
            other => panic!("expected TextDelta, got {:?}", other),
        }
    }

    #[test]
    fn parse_done_marker() {
        assert!(matches!(parse_sse_line("data: [DONE]"), Some(SseChunk::Done { .. })));
    }

    #[test]
    fn parse_tool_call_delta() {
        let line = r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"glass_query","arguments":"{\"text\":"}}]},"index":0}]}"#;
        match parse_sse_line(line) {
            Some(SseChunk::ToolCallDelta { index, id, function_name, arguments_delta }) => {
                assert_eq!(index, 0);
                assert_eq!(id.as_deref(), Some("call_1"));
                assert_eq!(function_name.as_deref(), Some("glass_query"));
                assert!(arguments_delta.contains("text"));
            }
            other => panic!("expected ToolCallDelta, got {:?}", other),
        }
    }

    #[test]
    fn parse_finish_stop() {
        let line = r#"data: {"choices":[{"delta":{},"finish_reason":"stop","index":0}]}"#;
        assert!(matches!(parse_sse_line(line), Some(SseChunk::Done { .. })));
    }

    #[test]
    fn parse_non_data_line_returns_none() {
        assert!(parse_sse_line("event: ping").is_none());
        assert!(parse_sse_line("").is_none());
        assert!(parse_sse_line(": comment").is_none());
    }

    #[test]
    fn parse_usage_in_final_chunk() {
        let line = r#"data: {"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":50}}"#;
        match parse_sse_line(line) {
            Some(SseChunk::Done { usage: Some(u) }) => {
                assert_eq!(u.prompt_tokens, 100);
                assert_eq!(u.completion_tokens, 50);
            }
            other => panic!("expected Done with usage, got {:?}", other),
        }
    }
}
```

- [ ] **Step 3: Verify and commit**

Run: `cargo test -p glass_agent_backend`

```
feat: SSE stream parser for OpenAI-compatible API responses
```

---

### Task 4: Implement `OpenAiBackend` struct and `AgentBackend` trait

The core backend implementation. Manages conversation history, makes HTTP requests, parses SSE streams, and handles the tool-calling loop.

**Files:**
- Modify: `crates/glass_agent_backend/src/openai.rs`

- [ ] **Step 1: Define `OpenAiBackend` struct and constructor**

```rust
/// Backend for any OpenAI-compatible API endpoint.
///
/// Manages conversation history in memory. Each `spawn()` starts a new
/// conversation. Messages arrive via `message_tx` and are appended as
/// user messages before sending the next request.
pub struct OpenAiBackend {
    api_key: String,
    model: String,
    endpoint: String,
}

impl OpenAiBackend {
    pub fn new(api_key: &str, model: &str, endpoint: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            model: if model.is_empty() { "gpt-4o".to_string() } else { model.to_string() },
            endpoint: if endpoint.is_empty() {
                "https://api.openai.com".to_string()
            } else {
                endpoint.trim_end_matches('/').to_string()
            },
        }
    }
}
```

- [ ] **Step 2: Implement conversation and request building**

Build the JSON request body for `/v1/chat/completions`:
- System message from `config.system_prompt`
- Conversation history (accumulated messages)
- Tool definitions from `config.allowed_tools` (converted to OpenAI function format)
- `stream: true` for SSE

Tool definition format:
```json
{
    "type": "function",
    "function": {
        "name": "glass_query",
        "description": "Search structured output records",
        "parameters": { "type": "object", "properties": {} }
    }
}
```

Note: For Phase 2, use minimal tool schemas (name + empty parameters). Full schema generation from schemars can come later — the model will see tool names in the system prompt and can call them with reasonable parameters.

- [ ] **Step 3: Implement `AgentBackend::spawn()`**

The spawn method creates:
1. A conversation manager thread that:
   - Starts with system message + initial_message (if provided)
   - Sends HTTP POST to `{endpoint}/v1/chat/completions` with SSE streaming
   - Reads SSE chunks, accumulates text/tool_calls
   - On tool_calls: executes via `SyncIpcClient`, appends results, sends next request
   - On final text: emits `AgentEvent::AssistantText` + `AgentEvent::TurnComplete`
   - Reads next user message from `message_rx`, appends to history, sends next request
   - On connection error: emits `AgentEvent::Crashed`
   - Max 10 tool-call rounds per turn (prevents infinite loops)

2. Returns `AgentHandle` with `message_tx` and `event_rx`

The conversation thread owns the full conversation state — this is simpler than separate reader/writer threads because HTTP request-response is inherently sequential (unlike the Claude CLI's concurrent stdin/stdout).

- [ ] **Step 4: Implement `shutdown()`**

Shutdown sets an `Arc<AtomicBool>` flag that the conversation thread checks between requests. The thread exits cleanly on next check.

- [ ] **Step 5: Write integration tests**

Test the SSE-to-AgentEvent pipeline with mock SSE data (not hitting real APIs):

```rust
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
```

- [ ] **Step 6: Verify and commit**

Run: `cargo test -p glass_agent_backend`
Run: `cargo build`

```
feat: implement OpenAiBackend with SSE streaming and tool calling

Supports any OpenAI-compatible endpoint. Conversation managed in a
single thread. Tool calls executed via Glass IPC. Max 10 tool rounds
per turn.
```

---

### Task 5: Add `resolve_backend()` factory and wire into `main.rs`

**Files:**
- Modify: `crates/glass_agent_backend/src/lib.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add `resolve_backend()` to `lib.rs`**

```rust
/// Resolve the appropriate backend from config.
///
/// Checks env vars first, then config file api_key.
/// Returns `Err(BackendError::MissingCredentials)` if an API provider
/// is selected but no credentials are found.
pub fn resolve_backend(
    provider: &str,
    model: &str,
    api_key: Option<&str>,
    api_endpoint: Option<&str>,
) -> Result<Box<dyn AgentBackend>, BackendError> {
    let endpoint = api_endpoint.unwrap_or("");

    match provider {
        "claude-code" | "" => Ok(Box::new(claude_cli::ClaudeCliBackend::new())),
        "openai-api" => {
            let key = std::env::var("OPENAI_API_KEY").ok()
                .or_else(|| api_key.map(|s| s.to_string()))
                .ok_or_else(|| BackendError::MissingCredentials {
                    provider: "openai-api".into(),
                    env_var: "OPENAI_API_KEY".into(),
                })?;
            Ok(Box::new(openai::OpenAiBackend::new(&key, model, endpoint)))
        }
        "custom" => {
            let key = std::env::var("GLASS_API_KEY").ok()
                .or_else(|| api_key.map(|s| s.to_string()))
                .unwrap_or_default();
            Ok(Box::new(openai::OpenAiBackend::new(&key, model, endpoint)))
        }
        _ => Ok(Box::new(claude_cli::ClaudeCliBackend::new())),
    }
}
```

- [ ] **Step 2: Update `main.rs:try_spawn_agent()` to use `resolve_backend()`**

Replace the hardcoded `ClaudeCliBackend::new()` at line ~1144 with:

```rust
let provider = config.orchestrator.as_ref()
    .and_then(|_| None::<&str>) // provider comes from GlassConfig, not AgentRuntimeConfig
    .unwrap_or("claude-code");
// For now, pass provider info through. Full config plumbing in next step.
let backend = glass_agent_backend::resolve_backend(
    provider, "", None, None,
).unwrap_or_else(|e| {
    tracing::warn!("resolve_backend failed: {}, falling back to Claude CLI", e);
    Box::new(glass_agent_backend::claude_cli::ClaudeCliBackend::new())
});
```

Actually, the proper approach: pass `provider`, `model`, `api_key`, `api_endpoint` from the `GlassConfig` through to `try_spawn_agent`. These fields are on `AgentSection` (added in Phase 1 Task 4). The `try_spawn_agent` function receives `AgentRuntimeConfig` which doesn't have these fields. So either:
- Add them to `AgentRuntimeConfig`, or
- Pass them separately

The simplest: pass the `GlassConfig` agent section fields through `try_spawn_agent()` as additional parameters.

Add parameters to `try_spawn_agent()`:
```rust
fn try_spawn_agent(
    config: glass_core::agent_runtime::AgentRuntimeConfig,
    activity_rx: ...,
    proxy: ...,
    restart_count: u32,
    last_crash: Option<std::time::Instant>,
    project_root: String,
    initial_message: Option<String>,
    system_prompt: String,
    generation: u64,
    provider: &str,        // NEW
    model: &str,           // NEW
    api_key: Option<&str>, // NEW
    api_endpoint: Option<&str>, // NEW
) -> Option<AgentRuntime> {
    let backend = glass_agent_backend::resolve_backend(provider, model, api_key, api_endpoint)
        .unwrap_or_else(|e| {
            tracing::warn!("resolve_backend: {}, falling back to Claude CLI", e);
            Box::new(glass_agent_backend::claude_cli::ClaudeCliBackend::new())
        });
    // ... rest unchanged
}
```

Update all 4 call sites to pass the provider info from `self.config.agent`.

- [ ] **Step 3: Verify and commit**

Run: `cargo build`
Run: `cargo test --workspace`

```
feat: add resolve_backend() factory, wire provider selection into main.rs

Provider config field now routes to ClaudeCliBackend or OpenAiBackend.
Defaults to claude-code. Falls back gracefully on errors.
```

---

### Task 6: Implement model list caching

**Files:**
- Modify: `crates/glass_agent_backend/src/model_cache.rs`

- [ ] **Step 1: Implement model list fetching and caching**

```rust
/// A cached model entry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedModel {
    pub id: String,
    pub display_name: String,
    pub provider: String,
}

/// Fetch models from an OpenAI-compatible `/v1/models` endpoint.
/// Returns cached results if cache is less than 24h old.
pub fn fetch_models(
    provider: &str,
    endpoint: &str,
    api_key: &str,
) -> Vec<CachedModel> {
    let cache_dir = dirs::home_dir()
        .map(|h| h.join(".glass").join("cache").join("models"))
        .unwrap_or_else(|| std::path::PathBuf::from(".glass/cache/models"));
    let _ = std::fs::create_dir_all(&cache_dir);

    let cache_file = cache_dir.join(format!("{}.json", provider));

    // Check cache age
    if let Ok(meta) = std::fs::metadata(&cache_file) {
        if let Ok(modified) = meta.modified() {
            if modified.elapsed().unwrap_or_default() < std::time::Duration::from_secs(86400) {
                if let Ok(data) = std::fs::read_to_string(&cache_file) {
                    if let Ok(models) = serde_json::from_str::<Vec<CachedModel>>(&data) {
                        return models;
                    }
                }
            }
        }
    }

    // Fetch from API
    let url = format!("{}/v1/models", endpoint.trim_end_matches('/'));
    let models = match ureq::get(&url)
        .header("Authorization", &format!("Bearer {}", api_key))
        .call()
    {
        Ok(resp) => {
            let body: serde_json::Value = resp.body_mut().read_json().unwrap_or_default();
            parse_model_list(provider, &body)
        }
        Err(e) => {
            tracing::warn!("model_cache: fetch failed for {}: {}", provider, e);
            return load_fallback_cache(&cache_file);
        }
    };

    // Write cache
    if let Ok(json) = serde_json::to_string_pretty(&models) {
        let _ = std::fs::write(&cache_file, json);
    }

    models
}

fn parse_model_list(provider: &str, body: &serde_json::Value) -> Vec<CachedModel> {
    let data = body.get("data").and_then(|d| d.as_array()).cloned().unwrap_or_default();
    data.iter()
        .filter_map(|m| {
            let id = m.get("id")?.as_str()?.to_string();
            // Filter out non-chat models
            if id.contains("embed") || id.contains("tts") || id.contains("whisper")
                || id.contains("dall-e") || id.contains("moderation")
            {
                return None;
            }
            let display_name = friendly_model_name(&id);
            Some(CachedModel { id, display_name, provider: provider.to_string() })
        })
        .collect()
}

fn friendly_model_name(id: &str) -> String {
    match id {
        "gpt-4o" => "GPT-4o".to_string(),
        "gpt-4o-mini" => "GPT-4o mini".to_string(),
        "o3" => "o3".to_string(),
        "o3-mini" => "o3 mini".to_string(),
        id if id.contains("opus") => "Claude Opus".to_string(),
        id if id.contains("sonnet") => "Claude Sonnet".to_string(),
        id if id.contains("haiku") => "Claude Haiku".to_string(),
        _ => id.to_string(),
    }
}

fn load_fallback_cache(cache_file: &std::path::Path) -> Vec<CachedModel> {
    std::fs::read_to_string(cache_file).ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}
```

- [ ] **Step 2: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friendly_name_known_models() {
        assert_eq!(friendly_model_name("gpt-4o"), "GPT-4o");
        assert_eq!(friendly_model_name("gpt-4o-mini"), "GPT-4o mini");
    }

    #[test]
    fn friendly_name_unknown_model_returns_id() {
        assert_eq!(friendly_model_name("custom-model-v1"), "custom-model-v1");
    }

    #[test]
    fn parse_model_list_filters_embeddings() {
        let body = serde_json::json!({
            "data": [
                {"id": "gpt-4o"},
                {"id": "text-embedding-ada-002"},
                {"id": "tts-1"},
                {"id": "gpt-4o-mini"},
            ]
        });
        let models = parse_model_list("openai", &body);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "gpt-4o");
        assert_eq!(models[1].id, "gpt-4o-mini");
    }
}
```

- [ ] **Step 3: Verify and commit**

```
feat: model list caching with 24h file cache and friendly display names
```

---

### Task 7: Update settings overlay with dynamic model picker

**Files:**
- Modify: `crates/glass_renderer/src/settings_overlay.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Make Provider and Model fields cycleable in the settings overlay**

Update the orchestrator section to make the Provider row a dropdown (cycleable with arrow keys). When the provider changes, write the new value to config and trigger hot-reload.

The available provider list is static for now: `["Claude Code", "OpenAI API", "Custom"]`. Dynamic model cycling within a provider comes when the model cache integration is wired in.

- [ ] **Step 2: Wire model cache into settings overlay population**

When the settings overlay is opened and the Orchestrator section is active, fetch the cached model list for the current provider. Populate the Model field's cycle list with the cached models.

- [ ] **Step 3: Verify and commit**

```
feat: cycleable provider and model selection in settings overlay
```

---

### Task 8: Full regression verification

**Files:** None — verification only.

- [ ] **Step 1: Run full test suite**

Run: `cargo test --workspace`

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`

- [ ] **Step 3: Run fmt**

Run: `cargo fmt --all -- --check`

- [ ] **Step 4: Manual verification**

1. Build and launch Glass
2. Verify default behavior unchanged (Claude Code provider)
3. Set `provider = "openai-api"` and `OPENAI_API_KEY` env var
4. Press Ctrl+Shift+O — verify orchestrator works with OpenAI
5. Check settings overlay shows provider/model correctly

- [ ] **Step 5: Commit if fixups needed**

```
fix: Phase 2 regression fixups
```
