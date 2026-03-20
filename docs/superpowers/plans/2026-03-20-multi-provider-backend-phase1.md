# Multi-Provider Agent Backend — Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract the agent subprocess lifecycle from `src/main.rs` into a `glass_agent_backend` crate behind an `AgentBackend` trait, with `ClaudeCliBackend` as the sole implementation. Zero behavior change.

**Architecture:** New crate `crates/glass_agent_backend/` defines `AgentEvent`, `AgentBackend` trait, `AgentHandle`, and `BackendSpawnConfig`. The existing ~580 lines of spawn/reader/writer logic move into `ClaudeCliBackend`. `main.rs` becomes a thin consumer that calls `backend.spawn()` and drains normalized `AgentEvent`s.

**Tech Stack:** Rust, std::sync::mpsc channels, std::process::Command (Claude CLI), parking_lot::Mutex, serde_json

**Spec:** `docs/superpowers/specs/2026-03-20-multi-provider-backend-design.md`

---

### Task 1: Create `glass_agent_backend` crate with core types

**Files:**
- Create: `crates/glass_agent_backend/Cargo.toml`
- Create: `crates/glass_agent_backend/src/lib.rs`
- Modify: `Cargo.toml` (workspace root — add to members, nothing else since `crates/*` glob already covers it)
- Modify: `Cargo.toml` (root `[dependencies]` — add `glass_agent_backend = { path = "crates/glass_agent_backend" }`)

- [ ] **Step 1: Create `Cargo.toml` for the new crate**

```toml
[package]
name = "glass_agent_backend"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
parking_lot = { workspace = true }
dirs = { workspace = true }
glass_core = { path = "../glass_core" }
glass_agent = { path = "../glass_agent" }
glass_coordination = { path = "../glass_coordination" }

# Platform-specific
[target.'cfg(target_os = "windows")'.dependencies]
windows-sys = { workspace = true }

[target.'cfg(target_os = "linux")'.dependencies]
libc = { workspace = true }

[target.'cfg(target_os = "macos")'.dependencies]
libc = { workspace = true }
```

- [ ] **Step 2: Create `lib.rs` with `AgentEvent`, `BackendSpawnConfig`, `AgentHandle`, `BackendError`, `AgentBackend` trait**

```rust
//! `glass_agent_backend` — pluggable agent backend trait and implementations.
//!
//! Defines the `AgentBackend` trait that normalizes different LLM providers
//! (Claude CLI, OpenAI API, Ollama, etc.) into a uniform `AgentEvent` stream.
//! `main.rs` consumes `AgentEvent`s without knowing which provider produced them.

pub mod claude_cli;

use glass_core::agent_runtime::AgentMode;
use std::sync::mpsc;
use std::time::Instant;

/// Normalized events emitted by any agent backend.
///
/// `main.rs` never sees provider-specific JSON — only these variants.
/// Application-level parsing (GLASS_PROPOSAL, GLASS_HANDOFF markers) happens
/// in `main.rs` on `AssistantText`, not here.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// Agent session initialized.
    Init { session_id: String },
    /// Agent produced text (may contain GLASS_WAIT, GLASS_DONE, etc.).
    AssistantText { text: String },
    /// Agent is thinking (extended thinking / reasoning tokens).
    Thinking { text: String },
    /// Agent called a tool.
    ToolCall {
        name: String,
        id: String,
        input: String,
    },
    /// Tool result returned to the agent.
    ToolResult {
        tool_use_id: String,
        content: String,
    },
    /// A conversation turn completed.
    TurnComplete { cost_usd: f64 },
    /// Agent process/connection died unexpectedly.
    Crashed,
}

/// Configuration passed to any backend at spawn time.
#[derive(Debug, Clone)]
pub struct BackendSpawnConfig {
    pub system_prompt: String,
    pub initial_message: Option<String>,
    pub project_root: String,
    pub mcp_config_path: String,
    pub allowed_tools: Vec<String>,
    pub mode: AgentMode,
    pub cooldown_secs: u64,
    pub restart_count: u32,
    pub last_crash: Option<Instant>,
}

/// Opaque shutdown token created per-spawn. Each backend stores
/// per-spawn state (child process, coordination IDs) inside this token.
/// Passed back to `shutdown()` so the backend can clean up the specific
/// spawn without conflicting with future spawns on the same backend instance.
pub struct ShutdownToken {
    inner: Box<dyn std::any::Any + Send>,
}

impl ShutdownToken {
    pub fn new<T: Send + 'static>(data: T) -> Self {
        Self {
            inner: Box::new(data),
        }
    }

    pub fn downcast<T: 'static>(&self) -> Option<&T> {
        self.inner.downcast_ref()
    }

    pub fn downcast_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.inner.downcast_mut()
    }
}

/// Handle returned by `AgentBackend::spawn()`.
///
/// Backend-agnostic: no subprocess-specific fields. The caller sends
/// messages via `message_tx` and receives normalized events via `event_rx`.
/// Per-spawn state lives in `shutdown_token`, which is passed to
/// `shutdown()` for cleanup.
pub struct AgentHandle {
    /// Send messages to the agent. The backend's internal writer thread
    /// reads from this channel and handles protocol-specific formatting.
    pub message_tx: mpsc::Sender<String>,
    /// Receives normalized `AgentEvent`s from the backend's reader thread.
    pub event_rx: mpsc::Receiver<AgentEvent>,
    /// Generation counter for stale-event detection.
    pub generation: u64,
    /// Per-spawn state for cleanup. Created by `spawn()`, consumed by `shutdown()`.
    pub shutdown_token: ShutdownToken,
}

/// Errors that can occur when resolving or spawning a backend.
#[derive(Debug)]
pub enum BackendError {
    /// The selected provider requires credentials that were not found.
    MissingCredentials {
        provider: String,
        env_var: String,
    },
    /// The agent binary was not found on PATH (CLI backends).
    BinaryNotFound {
        binary: String,
    },
    /// Subprocess spawn failed or HTTP connection failed.
    SpawnFailed(String),
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendError::MissingCredentials { provider, env_var } => {
                write!(
                    f,
                    "Missing credentials for {}. Set {} environment variable or add api_key to config.",
                    provider, env_var
                )
            }
            BackendError::BinaryNotFound { binary } => {
                write!(f, "'{}' not found on PATH", binary)
            }
            BackendError::SpawnFailed(msg) => write!(f, "Agent spawn failed: {}", msg),
        }
    }
}

impl std::error::Error for BackendError {}

/// Trait that every LLM provider backend implements.
///
/// Backends are fully decoupled from winit and `AppEvent`. They produce
/// `AgentEvent`s through a channel; `main.rs` maps those to `AppEvent`.
pub trait AgentBackend: Send + Sync {
    /// Human-readable name for logs and UI (e.g., "Claude CLI", "OpenAI API").
    fn name(&self) -> &str;

    /// Spawn the agent process/connection and wire up internal threads.
    ///
    /// Returns an `AgentHandle` with channels for bidirectional communication.
    /// The backend's reader thread sends `AgentEvent`s to `event_rx`.
    /// The backend's writer thread reads from `message_tx` and handles
    /// protocol-specific formatting and transmission.
    fn spawn(
        &self,
        config: &BackendSpawnConfig,
        generation: u64,
    ) -> Result<AgentHandle, BackendError>;

    /// Shut down the agent cleanly.
    ///
    /// Takes ownership of the `ShutdownToken` from `AgentHandle`.
    /// Each backend downcasts to its per-spawn state type and cleans up.
    /// CLI: kills child process, deregisters from coordination DB.
    /// Called on checkpoint respawn, orchestrator deactivation, and Drop.
    fn shutdown(&self, token: ShutdownToken);
}
```

- [ ] **Step 3: Add glass_agent_backend dependency to root `Cargo.toml`**

In the root `Cargo.toml`, add to the `[dependencies]` section:
```toml
glass_agent_backend = { path = "crates/glass_agent_backend" }
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build -p glass_agent_backend`
Expected: Successful compilation with no errors.

- [ ] **Step 5: Commit**

```bash
git add crates/glass_agent_backend/ Cargo.toml Cargo.lock
git commit -m "feat: create glass_agent_backend crate with core types

AgentEvent, AgentBackend trait, AgentHandle, BackendSpawnConfig,
BackendError. No implementations yet — ClaudeCliBackend comes next."
```

---

### Task 2: Write regression tests for Claude CLI JSON parsing

These tests lock in the current behavior BEFORE any code moves. They test the parsing logic that will be extracted.

**Files:**
- Create: `crates/glass_agent_backend/src/claude_cli.rs` (initially just test module)

- [ ] **Step 1: Create `claude_cli.rs` with a placeholder struct and test module**

```rust
//! Claude CLI backend — spawns `claude` as a subprocess with stream-json I/O.

/// Backend that spawns the Claude Code CLI as a subprocess.
pub struct ClaudeCliBackend;

#[cfg(test)]
mod tests {
    use crate::AgentEvent;

    /// Simulate parsing a Claude CLI stream-json "system" init line.
    /// This matches the logic at main.rs:1337-1343.
    fn parse_stream_json_line(line: &str) -> Option<AgentEvent> {
        let val: serde_json::Value = serde_json::from_str(line).ok()?;
        match val.get("type").and_then(|t| t.as_str()) {
            Some("system") => {
                if val.get("subtype").and_then(|s| s.as_str()) == Some("init") {
                    let session_id = val
                        .get("session_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    Some(AgentEvent::Init { session_id })
                } else {
                    None
                }
            }
            Some("assistant") => {
                let mut full_text = String::new();
                let mut thinking_text: Option<String> = None;
                let mut tool_calls: Vec<AgentEvent> = Vec::new();
                if let Some(content) = val.get("message").and_then(|m| m.get("content")) {
                    if let Some(arr) = content.as_array() {
                        for block in arr {
                            match block.get("type").and_then(|t| t.as_str()) {
                                Some("text") => {
                                    if let Some(text) =
                                        block.get("text").and_then(|t| t.as_str())
                                    {
                                        full_text.push_str(text);
                                    }
                                }
                                Some("thinking") => {
                                    if let Some(text) =
                                        block.get("thinking").and_then(|t| t.as_str())
                                    {
                                        thinking_text = Some(text.to_string());
                                    }
                                }
                                Some("tool_use") => {
                                    let name = block
                                        .get("name")
                                        .and_then(|n| n.as_str())
                                        .unwrap_or("?")
                                        .to_string();
                                    let id = block
                                        .get("id")
                                        .and_then(|i| i.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    let input = block
                                        .get("input")
                                        .map(|i| i.to_string())
                                        .unwrap_or_default();
                                    tool_calls.push(AgentEvent::ToolCall { name, id, input });
                                }
                                _ => {}
                            }
                        }
                    }
                }
                // Return the first significant event found.
                // In the real reader thread, all events are emitted.
                // For testing, we return them in priority: thinking, tool_call, text.
                if let Some(t) = thinking_text {
                    return Some(AgentEvent::Thinking { text: t });
                }
                if let Some(tc) = tool_calls.into_iter().next() {
                    return Some(tc);
                }
                if !full_text.is_empty() {
                    return Some(AgentEvent::AssistantText { text: full_text });
                }
                None
            }
            Some("result") => {
                let cost_usd =
                    glass_core::agent_runtime::parse_cost_from_result(line).unwrap_or(0.0);
                Some(AgentEvent::TurnComplete { cost_usd })
            }
            Some("user") => {
                // Tool results — check for tool_result blocks
                if let Some(content) = val.get("message").and_then(|m| m.get("content")) {
                    if let Some(arr) = content.as_array() {
                        for block in arr {
                            if block.get("type").and_then(|t| t.as_str())
                                == Some("tool_result")
                            {
                                let tool_use_id = block
                                    .get("tool_use_id")
                                    .and_then(|t| t.as_str())
                                    .unwrap_or("?")
                                    .to_string();
                                let content_text = match block.get("content") {
                                    Some(c) if c.is_string() => {
                                        c.as_str().unwrap_or("").to_string()
                                    }
                                    Some(c) if c.is_array() => c
                                        .as_array()
                                        .unwrap()
                                        .iter()
                                        .filter_map(|b| {
                                            b.get("text").and_then(|t| t.as_str())
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n"),
                                    _ => String::new(),
                                };
                                return Some(AgentEvent::ToolResult {
                                    tool_use_id,
                                    content: content_text,
                                });
                            }
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    #[test]
    fn parse_system_init() {
        let line = r#"{"type":"system","subtype":"init","session_id":"sess-123"}"#;
        let event = parse_stream_json_line(line).unwrap();
        match event {
            AgentEvent::Init { session_id } => assert_eq!(session_id, "sess-123"),
            other => panic!("expected Init, got {:?}", other),
        }
    }

    #[test]
    fn parse_system_non_init_ignored() {
        let line = r#"{"type":"system","subtype":"heartbeat"}"#;
        assert!(parse_stream_json_line(line).is_none());
    }

    #[test]
    fn parse_assistant_text() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"GLASS_WAIT"}]}}"#;
        let event = parse_stream_json_line(line).unwrap();
        match event {
            AgentEvent::AssistantText { text } => assert_eq!(text, "GLASS_WAIT"),
            other => panic!("expected AssistantText, got {:?}", other),
        }
    }

    #[test]
    fn parse_assistant_thinking() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"Let me analyze..."}]}}"#;
        let event = parse_stream_json_line(line).unwrap();
        match event {
            AgentEvent::Thinking { text } => assert_eq!(text, "Let me analyze..."),
            other => panic!("expected Thinking, got {:?}", other),
        }
    }

    #[test]
    fn parse_assistant_tool_use() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"glass_query","id":"tool-1","input":{"query":"test"}}]}}"#;
        let event = parse_stream_json_line(line).unwrap();
        match event {
            AgentEvent::ToolCall { name, id, input } => {
                assert_eq!(name, "glass_query");
                assert_eq!(id, "tool-1");
                assert!(input.contains("test"));
            }
            other => panic!("expected ToolCall, got {:?}", other),
        }
    }

    #[test]
    fn parse_result_with_cost() {
        let line = r#"{"type":"result","cost_usd":0.0042,"result":"done"}"#;
        let event = parse_stream_json_line(line).unwrap();
        match event {
            AgentEvent::TurnComplete { cost_usd } => {
                assert!((cost_usd - 0.0042).abs() < 0.0001);
            }
            other => panic!("expected TurnComplete, got {:?}", other),
        }
    }

    #[test]
    fn parse_result_without_cost() {
        let line = r#"{"type":"result","result":"done"}"#;
        let event = parse_stream_json_line(line).unwrap();
        match event {
            AgentEvent::TurnComplete { cost_usd } => {
                assert_eq!(cost_usd, 0.0);
            }
            other => panic!("expected TurnComplete, got {:?}", other),
        }
    }

    #[test]
    fn parse_user_tool_result_string() {
        let line = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tool-1","content":"result text"}]}}"#;
        let event = parse_stream_json_line(line).unwrap();
        match event {
            AgentEvent::ToolResult {
                tool_use_id,
                content,
            } => {
                assert_eq!(tool_use_id, "tool-1");
                assert_eq!(content, "result text");
            }
            other => panic!("expected ToolResult, got {:?}", other),
        }
    }

    #[test]
    fn parse_user_tool_result_array() {
        let line = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tool-2","content":[{"type":"text","text":"line 1"},{"type":"text","text":"line 2"}]}]}}"#;
        let event = parse_stream_json_line(line).unwrap();
        match event {
            AgentEvent::ToolResult {
                tool_use_id,
                content,
            } => {
                assert_eq!(tool_use_id, "tool-2");
                assert_eq!(content, "line 1\nline 2");
            }
            other => panic!("expected ToolResult, got {:?}", other),
        }
    }

    #[test]
    fn parse_empty_line_returns_none() {
        assert!(parse_stream_json_line("").is_none());
        assert!(parse_stream_json_line("   ").is_none());
    }

    #[test]
    fn parse_invalid_json_returns_none() {
        assert!(parse_stream_json_line("not json at all").is_none());
    }

    #[test]
    fn parse_unknown_type_returns_none() {
        let line = r#"{"type":"unknown","data":"something"}"#;
        assert!(parse_stream_json_line(line).is_none());
    }

    #[test]
    fn parse_assistant_multiple_text_blocks_concatenates() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hello "},{"type":"text","text":"world"}]}}"#;
        let event = parse_stream_json_line(line).unwrap();
        match event {
            AgentEvent::AssistantText { text } => assert_eq!(text, "hello world"),
            other => panic!("expected AssistantText, got {:?}", other),
        }
    }
}
```

- [ ] **Step 2: Run the regression tests**

Run: `cargo test -p glass_agent_backend`
Expected: All 13 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/glass_agent_backend/src/claude_cli.rs
git commit -m "test: regression tests for Claude CLI stream-json parsing

13 tests covering: system init, assistant text, thinking, tool_use,
result/cost, tool_result (string and array), edge cases. These lock
in current behavior before extracting the reader thread."
```

---

### Task 3: Implement `ClaudeCliBackend::spawn()` — extract process spawning and reader/writer threads

This is the core extraction. Move the spawn logic from `main.rs:963-1619` into `ClaudeCliBackend`.

**Files:**
- Modify: `crates/glass_agent_backend/src/claude_cli.rs` — add full implementation
- Modify: `crates/glass_agent_backend/src/lib.rs` — re-export `ClaudeCliBackend`

- [ ] **Step 1: Implement `ClaudeCliBackend` with internal state and `AgentBackend` trait**

In `claude_cli.rs`, replace the placeholder struct with the full implementation. The code below is extracted line-for-line from `main.rs:963-1619` with these changes:
- Reader thread sends `AgentEvent` via `event_tx` instead of `AppEvent` via `proxy`
- Writer thread reads from `message_tx` channel instead of `orchestrator_writer` directly
- Child process stored in `Arc<Mutex<Option<Child>>>` for `shutdown()`
- `build_agent_command_args()` moved from `glass_core::agent_runtime` into this file
- System prompt writing and MCP config generation stay here (they're Claude CLI specific)

Key sections to move:
1. Diagnostic file writing (`main.rs:975-983`) → `spawn()`
2. System prompt file write (`main.rs:985-1137`) → **NO, stays in main.rs** (system prompt is provider-agnostic, assembled before spawn)
3. MCP config generation (`main.rs:1139-1160`) → `spawn()` (Claude CLI specific)
4. `build_agent_command_args()` call (`main.rs:1162-1166`) → `spawn()`
5. `Command::new("claude")` + platform setup (`main.rs:1168-1242`) → `spawn()`
6. Initial stdin write (`main.rs:1248-1268`) → `spawn()`
7. Prior handoff loading (`main.rs:1271-1312`) → `spawn()`
8. Reader thread (`main.rs:1318-1491`) → `spawn()`, emit `AgentEvent` instead of `AppEvent`
9. Writer thread (`main.rs:1493-1538`) → `spawn()`, read from `message_tx`
10. Coordination DB registration (`main.rs:1546-1604`) → `spawn()`

The `BackendSpawnConfig.system_prompt` is already assembled by `main.rs` — `spawn()` writes it to `~/.glass/agent-system-prompt.txt` and passes the path to `build_agent_command_args()`.

```rust
use crate::{AgentBackend, AgentEvent, AgentHandle, BackendError, BackendSpawnConfig};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::sync::Arc;

/// Per-spawn state stored in `ShutdownToken` for cleanup.
struct ClaudeCliShutdownState {
    child: Option<Child>,
    coord_agent_id: Option<String>,
    coord_nonce: Option<String>,
}

/// Backend that spawns the Claude Code CLI (`claude`) as a subprocess.
///
/// Communicates via stream-json on stdin/stdout. The reader thread parses
/// Claude CLI JSON and normalizes to `AgentEvent`. The writer thread reads
/// from `message_tx` and formats as stream-json user messages.
///
/// Per-spawn state (child process, coordination IDs) is stored in the
/// `ShutdownToken` returned inside `AgentHandle`, not on the struct itself.
/// This allows safe sequential spawn/shutdown cycles.
pub struct ClaudeCliBackend;

impl ClaudeCliBackend {
    pub fn new() -> Self {
        Self
    }
}

impl AgentBackend for ClaudeCliBackend {
    fn name(&self) -> &str {
        "Claude CLI"
    }

    fn spawn(
        &self,
        config: &BackendSpawnConfig,
        generation: u64,
    ) -> Result<AgentHandle, BackendError> {
        // --- Diagnostic file ---
        let glass_dir = dirs::home_dir()
            .map(|h| h.join(".glass"))
            .unwrap_or_else(|| std::path::PathBuf::from(".glass"));
        let _ = std::fs::create_dir_all(&glass_dir);

        let diag_path = glass_dir.join("agent-diag.txt");
        let mut diag = format!(
            "timestamp: {:?}\nPATH: {}\n",
            std::time::SystemTime::now(),
            std::env::var("PATH").unwrap_or_else(|_| "NOT SET".to_string()),
        );

        // --- Write system prompt to file ---
        let prompt_path = glass_dir.join("agent-system-prompt.txt");
        if let Err(e) = std::fs::write(&prompt_path, &config.system_prompt) {
            tracing::warn!("ClaudeCliBackend: failed to write system prompt: {}", e);
            return Err(BackendError::SpawnFailed(format!(
                "failed to write system prompt: {}",
                e
            )));
        }

        // --- Generate MCP config JSON ---
        let mcp_config_path = (|| -> Option<String> {
            let exe_path = std::env::current_exe().ok()?;
            let mcp_json_path = glass_dir.join("agent-mcp.json");
            let mcp_json = serde_json::json!({
                "mcpServers": {
                    "glass": {
                        "command": exe_path.to_string_lossy(),
                        "args": ["mcp", "serve"]
                    }
                }
            });
            match std::fs::write(&mcp_json_path, mcp_json.to_string()) {
                Ok(()) => Some(mcp_json_path.to_string_lossy().to_string()),
                Err(e) => {
                    tracing::warn!("ClaudeCliBackend: failed to write MCP config: {}", e);
                    None
                }
            }
        })()
        .unwrap_or_default();

        // --- Build command args ---
        // (move build_agent_command_args logic here from glass_core::agent_runtime)
        let args = build_claude_args(
            &config.allowed_tools,
            &prompt_path.to_string_lossy(),
            &mcp_config_path,
        );

        // --- Spawn process ---
        let mut cmd = Command::new("claude");
        cmd.args(&args);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                cmd.pre_exec(|| {
                    libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                    Ok(())
                });
            }
        }

        #[cfg(target_os = "macos")]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                cmd.pre_exec(|| {
                    std::thread::Builder::new()
                        .name("glass-orphan-watchdog".into())
                        .spawn(|| loop {
                            std::thread::sleep(std::time::Duration::from_secs(2));
                            if unsafe { libc::getppid() } == 1 {
                                std::process::exit(1);
                            }
                        })
                        .ok();
                    Ok(())
                });
            }
        }

        let args_str = args.join(" ");
        diag.push_str(&format!("spawn args: claude {}\n", args_str));
        let mut child = match cmd.spawn() {
            Ok(c) => {
                diag.push_str(&format!("spawn SUCCESS pid={}\n", c.id()));
                let _ = std::fs::write(&diag_path, &diag);
                c
            }
            Err(e) => {
                diag.push_str(&format!("spawn FAILED: {}\n", e));
                let _ = std::fs::write(&diag_path, &diag);
                tracing::warn!("ClaudeCliBackend: failed to spawn claude: {}", e);
                return Err(BackendError::BinaryNotFound {
                    binary: "claude".to_string(),
                });
            }
        };

        // --- Extract stdin/stdout ---
        let stdout = child.stdout.take().expect("stdout was piped");
        let mut stdin = child.stdin.take().expect("stdin was piped");

        // --- Initial stdin message (CLI 2.1.77+ compat) ---
        {
            let content = config
                .initial_message
                .as_deref()
                .unwrap_or("GLASS_WAIT");
            let json_msg = serde_json::json!({
                "type": "user",
                "message": { "role": "user", "content": content }
            })
            .to_string();
            match writeln!(stdin, "{json_msg}") {
                Ok(()) => {
                    let _ = stdin.flush();
                }
                Err(e) => {
                    tracing::warn!(
                        "ClaudeCliBackend: failed to write initial message: {}",
                        e
                    );
                }
            }
        }

        // Child process stored in shutdown state (moved into ShutdownToken at end)

        // --- Prior handoff loading ---
        let project_root = config.project_root.clone();
        let prior_handoff_msg = {
            match glass_agent::AgentSessionDb::open_default() {
                Ok(db) => {
                    let canonical = std::fs::canonicalize(&project_root)
                        .unwrap_or_else(|_| std::path::PathBuf::from(&project_root));
                    let canonical_str = canonical.to_string_lossy().to_string();
                    match db.load_prior_handoff(&canonical_str) {
                        Ok(Some(record)) => {
                            let handoff_data =
                                glass_core::agent_runtime::AgentHandoffData {
                                    work_completed: record.handoff.work_completed,
                                    work_remaining: record.handoff.work_remaining,
                                    key_decisions: record.handoff.key_decisions,
                                    previous_session_id: record
                                        .previous_session_id
                                        .clone(),
                                };
                            let msg =
                                glass_core::agent_runtime::format_handoff_as_user_message(
                                    &record.session_id,
                                    &handoff_data,
                                );
                            tracing::info!(
                                "ClaudeCliBackend: injecting prior handoff (session_id={})",
                                record.session_id
                            );
                            Some(msg)
                        }
                        Ok(None) => None,
                        Err(e) => {
                            tracing::warn!(
                                "ClaudeCliBackend: failed to load prior handoff: {}",
                                e
                            );
                            None
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "ClaudeCliBackend: failed to open session db: {}",
                        e
                    );
                    None
                }
            }
        };

        // --- Channels ---
        let (event_tx, event_rx) = mpsc::channel::<AgentEvent>();
        let (message_tx, message_rx) = mpsc::channel::<String>();

        // --- Reader thread ---
        let event_tx_reader = event_tx.clone();
        std::thread::Builder::new()
            .name("glass-agent-reader".into())
            .spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    if line.trim().is_empty() {
                        continue;
                    }
                    let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) else {
                        continue;
                    };
                    match val.get("type").and_then(|t| t.as_str()) {
                        Some("system") => {
                            if val.get("subtype").and_then(|s| s.as_str()) == Some("init") {
                                if let Some(id) =
                                    val.get("session_id").and_then(|v| v.as_str())
                                {
                                    let _ = event_tx_reader.send(AgentEvent::Init {
                                        session_id: id.to_string(),
                                    });
                                }
                            }
                        }
                        Some("result") => {
                            let cost_usd =
                                glass_core::agent_runtime::parse_cost_from_result(&line)
                                    .unwrap_or(0.0);
                            let _ = event_tx_reader
                                .send(AgentEvent::TurnComplete { cost_usd });
                        }
                        Some("assistant") => {
                            let mut full_text = String::new();
                            if let Some(content) =
                                val.get("message").and_then(|m| m.get("content"))
                            {
                                if let Some(arr) = content.as_array() {
                                    for block in arr {
                                        match block
                                            .get("type")
                                            .and_then(|t| t.as_str())
                                        {
                                            Some("text") => {
                                                if let Some(text) = block
                                                    .get("text")
                                                    .and_then(|t| t.as_str())
                                                {
                                                    full_text.push_str(text);
                                                }
                                            }
                                            Some("thinking") => {
                                                if let Some(text) = block
                                                    .get("thinking")
                                                    .and_then(|t| t.as_str())
                                                {
                                                    let _ = event_tx_reader.send(
                                                        AgentEvent::Thinking {
                                                            text: text.to_string(),
                                                        },
                                                    );
                                                }
                                            }
                                            Some("tool_use") => {
                                                let name = block
                                                    .get("name")
                                                    .and_then(|n| n.as_str())
                                                    .unwrap_or("?")
                                                    .to_string();
                                                let id = block
                                                    .get("id")
                                                    .and_then(|i| i.as_str())
                                                    .unwrap_or("")
                                                    .to_string();
                                                let input = block
                                                    .get("input")
                                                    .map(|i| i.to_string())
                                                    .unwrap_or_default();
                                                let _ = event_tx_reader.send(
                                                    AgentEvent::ToolCall {
                                                        name,
                                                        id,
                                                        input,
                                                    },
                                                );
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            if !full_text.is_empty() {
                                let _ = event_tx_reader.send(
                                    AgentEvent::AssistantText { text: full_text },
                                );
                            }
                        }
                        Some("user") => {
                            if let Some(content) =
                                val.get("message").and_then(|m| m.get("content"))
                            {
                                if let Some(arr) = content.as_array() {
                                    for block in arr {
                                        if block
                                            .get("type")
                                            .and_then(|t| t.as_str())
                                            == Some("tool_result")
                                        {
                                            let tool_use_id = block
                                                .get("tool_use_id")
                                                .and_then(|t| t.as_str())
                                                .unwrap_or("?")
                                                .to_string();
                                            let content_text =
                                                match block.get("content") {
                                                    Some(c) if c.is_string() => c
                                                        .as_str()
                                                        .unwrap_or("")
                                                        .to_string(),
                                                    Some(c) if c.is_array() => c
                                                        .as_array()
                                                        .unwrap()
                                                        .iter()
                                                        .filter_map(|b| {
                                                            b.get("text")
                                                                .and_then(|t| t.as_str())
                                                        })
                                                        .collect::<Vec<_>>()
                                                        .join("\n"),
                                                    _ => String::new(),
                                                };
                                            let _ = event_tx_reader.send(
                                                AgentEvent::ToolResult {
                                                    tool_use_id,
                                                    content: content_text,
                                                },
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                // stdout closed — signal crash
                let _ = event_tx_reader.send(AgentEvent::Crashed);
            })
            .ok();

        // --- Shared stdin writer ---
        let shared_writer =
            Arc::new(parking_lot::Mutex::new(BufWriter::new(stdin)));
        let writer_clone = Arc::clone(&shared_writer);

        // --- Writer thread ---
        let cooldown_secs = config.cooldown_secs;
        std::thread::Builder::new()
            .name("glass-agent-writer".into())
            .spawn(move || {
                let mut last_sent: Option<std::time::Instant> = None;
                let cooldown = std::time::Duration::from_secs(cooldown_secs);

                // Inject prior session handoff first
                if let Some(ref msg) = prior_handoff_msg {
                    let mut w = writer_clone.lock();
                    let _ = writeln!(w, "{msg}");
                    let _ = w.flush();
                }

                // Read messages from the channel and write to stdin
                for content in message_rx.iter() {
                    // Cooldown gate
                    if let Some(last) = last_sent {
                        if last.elapsed() < cooldown {
                            continue;
                        }
                    }

                    let mut w = writer_clone.lock();
                    if writeln!(w, "{content}").is_err() || w.flush().is_err() {
                        break; // BrokenPipe: child died
                    }
                    last_sent = Some(std::time::Instant::now());
                }
            })
            .ok();

        tracing::info!(
            "ClaudeCliBackend: spawned (mode={:?}, restart_count={})",
            config.mode,
            config.restart_count
        );

        // --- Coordination DB registration ---
        let mut coord_agent_id: Option<String> = None;
        let mut coord_nonce: Option<String> = None;
        {
            let canonical_str =
                glass_coordination::canonicalize_path(std::path::Path::new(
                    &config.project_root,
                ))
                .unwrap_or_else(|_| config.project_root.clone());
            match glass_coordination::CoordinationDb::open_default() {
                Ok(mut db) => {
                    match db.prune_stale(120) {
                        Ok(pruned) if !pruned.is_empty() => {
                            tracing::info!(
                                "ClaudeCliBackend: pruned {} stale agent(s)",
                                pruned.len()
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                "ClaudeCliBackend: prune_stale failed: {}",
                                e
                            );
                        }
                        _ => {}
                    }
                    let cwd = std::env::current_dir()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| canonical_str.clone());
                    match db.register(
                        "glass-agent",
                        "claude-code",
                        &canonical_str,
                        &cwd,
                        None,
                    ) {
                        Ok((id, nonce_val)) => {
                            let lock_path =
                                std::path::PathBuf::from(&canonical_str);
                            let _ = db.lock_files(
                                &id,
                                &[lock_path],
                                Some("agent session"),
                                &nonce_val,
                            );
                            tracing::info!(
                                "ClaudeCliBackend: registered (id={})",
                                id
                            );
                            coord_agent_id = Some(id);
                            coord_nonce = Some(nonce_val);
                        }
                        Err(e) => {
                            tracing::warn!(
                                "ClaudeCliBackend: registration failed: {}",
                                e
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "ClaudeCliBackend: failed to open coordination DB: {}",
                        e
                    );
                }
            }
        }

        // Build per-spawn shutdown state
        let shutdown_state = ClaudeCliShutdownState {
            child: Some(child),
            coord_agent_id,
            coord_nonce,
        };

        Ok(AgentHandle {
            message_tx,
            event_rx,
            generation,
            shutdown_token: crate::ShutdownToken::new(shutdown_state),
        })
    }

    fn shutdown(&self, mut token: ShutdownToken) {
        if let Some(state) = token.downcast_mut::<ClaudeCliShutdownState>() {
            // Kill child process
            if let Some(ref mut child) = state.child {
                match child.try_wait() {
                    Ok(Some(_)) => {} // already exited
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                    }
                }
            }
            state.child = None;

            // Deregister from coordination DB
            if let (Some(ref agent_id), Some(ref nonce)) =
                (&state.coord_agent_id, &state.coord_nonce)
            {
                if let Ok(mut db) =
                    glass_coordination::CoordinationDb::open_default()
                {
                    let _ = db.unlock_all(agent_id, nonce);
                    let _ = db.deregister(agent_id, nonce);
                }
            }
        }
    }
}

/// Build CLI arguments for the `claude` subprocess.
///
/// Moved from `glass_core::agent_runtime::build_agent_command_args()`.
fn build_claude_args(
    allowed_tools: &[String],
    prompt_path: &str,
    mcp_config_path: &str,
) -> Vec<String> {
    let mut args = vec![
        "-p".to_string(),
        "--verbose".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--input-format".to_string(),
        "stream-json".to_string(),
        "--system-prompt-file".to_string(),
        prompt_path.to_string(),
    ];
    if !mcp_config_path.is_empty() {
        args.push("--mcp-config".to_string());
        args.push(mcp_config_path.to_string());
    }
    let tools = if allowed_tools.is_empty() {
        "glass_query,glass_context".to_string()
    } else {
        allowed_tools.join(",")
    };
    args.push("--allowedTools".to_string());
    args.push(tools);
    args.push("--dangerously-skip-permissions".to_string());
    args.push("--disable-slash-commands".to_string());
    args
}
```

Note: The actual implementation should carefully preserve every detail from `main.rs` — the code above is the blueprint. During implementation, diff against the original line-for-line.

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p glass_agent_backend`
Expected: Successful compilation.

- [ ] **Step 3: Run regression tests still pass**

Run: `cargo test -p glass_agent_backend`
Expected: All 13 tests pass (parsing logic unchanged).

- [ ] **Step 4: Commit**

```bash
git add crates/glass_agent_backend/src/claude_cli.rs crates/glass_agent_backend/src/lib.rs
git commit -m "feat: implement ClaudeCliBackend with spawn/shutdown

Extract process spawning, reader thread, writer thread, coordination
registration, and MCP config generation from main.rs into ClaudeCliBackend.
Reader thread emits AgentEvent instead of AppEvent."
```

---

### Task 4: Add new config fields to `glass_core::config`

**Files:**
- Modify: `crates/glass_core/src/config.rs:120-204` — add fields to `AgentSection` and `OrchestratorSection`
- Modify: `config.example.toml:62-94` — document new fields

- [ ] **Step 1: Add `provider`, `model`, `api_key`, `api_endpoint` to `AgentSection`**

In `crates/glass_core/src/config.rs`, add after the `quiet_rules` field (line 138):

```rust
    /// LLM provider for the agent backend. Default: "claude-code".
    /// Options: "claude-code", "anthropic-api", "openai-api", "ollama", "custom".
    #[serde(default = "default_agent_provider")]
    pub provider: String,
    /// Model ID override. Empty string = provider default.
    #[serde(default)]
    pub model: Option<String>,
    /// API key for API-based providers. Env var takes precedence.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Custom API endpoint URL. Only used with "custom" or self-hosted providers.
    #[serde(default)]
    pub api_endpoint: Option<String>,
```

Add the default function:
```rust
fn default_agent_provider() -> String {
    "claude-code".to_string()
}
```

- [ ] **Step 2: Add `implementer`, `implementer_command`, `implementer_name`, `persona` to `OrchestratorSection`**

In `crates/glass_core/src/config.rs`, add after the `agent_instructions` field (line 203):

```rust
    /// Which CLI to launch as the implementer. Default: "claude-code".
    /// Options: "claude-code", "codex", "aider", "gemini", "custom".
    #[serde(default = "default_implementer")]
    pub implementer: String,
    /// Custom launch command when implementer = "custom".
    #[serde(default)]
    pub implementer_command: Option<String>,
    /// Display name for the implementer in system prompts. Default: "Claude Code".
    #[serde(default = "default_implementer_name")]
    pub implementer_name: String,
    /// Custom persona for the orchestrator agent. Inline string or path to .md file.
    #[serde(default)]
    pub persona: Option<String>,
```

Add the default functions:
```rust
fn default_implementer() -> String {
    "claude-code".to_string()
}
fn default_implementer_name() -> String {
    "Claude Code".to_string()
}
```

- [ ] **Step 3: Update `config.example.toml`**

Add the new fields (commented out) to the `[agent]` and `[agent.orchestrator]` sections:

```toml
# [agent]
# provider = "claude-code"    # "claude-code", "anthropic-api", "openai-api", "ollama", "custom"
# model = ""                  # empty = provider default
# api_key = ""                # optional — env var used if empty
# api_endpoint = ""           # optional — for custom endpoints

# [agent.orchestrator]
# implementer = "claude-code"       # "claude-code", "codex", "aider", "gemini", "custom"
# implementer_command = ""          # custom launch command
# implementer_name = "Claude Code"  # display name in system prompt
# persona = ""                      # inline string or path to .md file
```

- [ ] **Step 4: Verify it compiles and existing config tests pass**

Run: `cargo test -p glass_core`
Expected: All existing tests pass. New fields have defaults so existing configs remain valid.

- [ ] **Step 5: Commit**

```bash
git add crates/glass_core/src/config.rs config.example.toml
git commit -m "feat: add multi-provider config fields to AgentSection and OrchestratorSection

provider, model, api_key, api_endpoint in [agent].
implementer, implementer_command, implementer_name, persona in [agent.orchestrator].
All default to current behavior."
```

---

### Task 5: Wire `main.rs` to use `ClaudeCliBackend` via the trait

This is the critical integration task. Replace the direct spawn logic with the backend trait.

**Files:**
- Modify: `src/main.rs:304-332` — update `AgentRuntime` struct
- Modify: `src/main.rs:963-1619` — replace `try_spawn_agent()` with thin wrapper
- Modify: `src/main.rs:2413-2505` — update `respawn_orchestrator_agent()`

- [ ] **Step 1: Update `AgentRuntime` struct to hold `AgentHandle` + backend**

Replace `AgentRuntime` at `main.rs:304-332` with:

```rust
/// Encapsulates the agent subprocess lifecycle.
struct AgentRuntime {
    /// The agent handle returned by the backend.
    handle: glass_agent_backend::AgentHandle,
    /// The backend implementation (for shutdown/respawn).
    backend: Box<dyn glass_agent_backend::AgentBackend>,
    /// Rate-limit gate.
    #[allow(dead_code)]
    cooldown: glass_core::agent_runtime::CooldownTracker,
    /// Accumulated cost gate.
    budget: glass_core::agent_runtime::BudgetTracker,
    /// Runtime configuration.
    config: glass_core::agent_runtime::AgentRuntimeConfig,
    /// Number of crash-restart attempts.
    restart_count: u32,
    /// Timestamp of last crash.
    last_crash: Option<std::time::Instant>,
    /// Generation counter for stale-event filtering.
    generation: u64,
    /// Session ID captured from AgentEvent::Init (for handoff tracking).
    session_id: String,
    /// Project root path (for handoff tracking).
    project_root: String,
}

/// Drop calls backend.shutdown() to kill child process and deregister
/// from coordination DB. This preserves the existing behavior where
/// `self.agent_runtime = None` triggers cleanup.
impl Drop for AgentRuntime {
    fn drop(&mut self) {
        // Take the shutdown token and pass to backend for cleanup.
        // We need to move it out of the handle — use a dummy replacement.
        let token = std::mem::replace(
            &mut self.handle.shutdown_token,
            glass_agent_backend::ShutdownToken::new(()),
        );
        self.backend.shutdown(token);
    }
}
```

- [ ] **Step 2: Replace `try_spawn_agent()` with backend-based version**

Replace the ~660-line function with a thin wrapper:

```rust
fn try_spawn_agent(
    config: glass_core::agent_runtime::AgentRuntimeConfig,
    _activity_rx: std::sync::mpsc::Receiver<glass_core::activity_stream::ActivityEvent>,
    _proxy: winit::event_loop::EventLoopProxy<glass_core::event::AppEvent>,
    restart_count: u32,
    last_crash: Option<std::time::Instant>,
    project_root: String,
    initial_message: Option<String>,
    system_prompt: String,
    generation: u64,
) -> Option<AgentRuntime> {
    // Resolve backend from config
    let backend: Box<dyn glass_agent_backend::AgentBackend> =
        Box::new(glass_agent_backend::claude_cli::ClaudeCliBackend::new());

    // Compute allowed tools based on orchestrator mode (preserves existing logic
    // from glass_core::agent_runtime::build_agent_command_args lines 395-458).
    // Orchestrator active + audit mode → all MCP tools.
    // Orchestrator active + build/general → observation only.
    // Non-orchestrator → use config.allowed_tools as-is.
    let allowed_tools = {
        let orchestrator_active = config.orchestrator.as_ref().map(|o| o.enabled).unwrap_or(false);
        let orch_mode = config.orchestrator.as_ref()
            .map(|o| o.orchestrator_mode.as_str()).unwrap_or("build");
        if orchestrator_active && orch_mode == "audit" {
            vec![
                "Read", "glass_history", "glass_context", "glass_undo",
                "glass_file_diff", "glass_pipe_inspect", "glass_tab_create",
                "glass_tab_list", "glass_tab_send", "glass_tab_output",
                "glass_tab_close", "glass_cache_check", "glass_command_diff",
                "glass_compressed_context", "glass_extract_errors",
                "glass_has_running_command", "glass_cancel_command",
                "glass_query", "glass_query_trend", "glass_query_drill",
                "glass_agent_register", "glass_agent_deregister",
                "glass_agent_list", "glass_agent_status", "glass_agent_heartbeat",
                "glass_agent_lock", "glass_agent_unlock", "glass_agent_locks",
                "glass_agent_broadcast", "glass_agent_send", "glass_agent_messages",
                "glass_ping",
            ].into_iter().map(|s| s.to_string()).collect()
        } else if orchestrator_active {
            vec!["glass_query".to_string(), "glass_context".to_string()]
        } else {
            config.allowed_tools.split(',').map(|s| s.trim().to_string()).collect()
        }
    };

    let spawn_config = glass_agent_backend::BackendSpawnConfig {
        system_prompt,
        initial_message,
        project_root: project_root.clone(),
        mcp_config_path: String::new(), // handled internally by ClaudeCliBackend
        allowed_tools,
        mode: config.mode,
        cooldown_secs: config.cooldown_secs,
        restart_count,
        last_crash,
    };

    match backend.spawn(&spawn_config, generation) {
        Ok(handle) => {
            tracing::info!("AgentRuntime: {} backend spawned", backend.name());
            Some(AgentRuntime {
                handle,
                backend,
                cooldown: glass_core::agent_runtime::CooldownTracker::new(config.cooldown_secs),
                budget: glass_core::agent_runtime::BudgetTracker::new(config.max_budget_usd),
                config,
                restart_count,
                last_crash,
                generation,
                session_id: String::new(), // populated when AgentEvent::Init arrives
                project_root: project_root.clone(),
            })
        }
        Err(e) => {
            tracing::warn!("AgentRuntime: backend spawn failed: {}", e);
            None
        }
    }
}
```

Note: The system prompt assembly logic (lines 996-1132) stays in `main.rs` — it's passed to `try_spawn_agent()` as a `system_prompt` parameter. All call sites must be updated to build the system prompt before calling.

- [ ] **Step 3: Add event routing loop**

Add a function that drains `event_rx` and maps `AgentEvent` → `AppEvent`. This replaces what the reader thread used to do directly:

```rust
/// Drain pending AgentEvents from the backend and route to AppEvents.
/// Called from the winit event loop on each iteration.
fn drain_agent_events(
    runtime: &mut AgentRuntime,
    proxy: &winit::event_loop::EventLoopProxy<glass_core::event::AppEvent>,
    buffered_response: &mut Option<String>,
    tool_id_to_name: &mut std::collections::HashMap<String, String>,
) {
    while let Ok(event) = runtime.handle.event_rx.try_recv() {
        match event {
            glass_agent_backend::AgentEvent::Init { session_id } => {
                tracing::info!("AgentRuntime: session_id={}", session_id);
                runtime.session_id = session_id;
            }
            glass_agent_backend::AgentEvent::AssistantText { text } => {
                // Extract proposals/handoffs (application-level, provider-agnostic)
                if let Some(proposal) = glass_core::agent_runtime::extract_proposal(&text) {
                    let _ = proxy.send_event(glass_core::event::AppEvent::AgentProposal(proposal));
                }
                if let Some((handoff, raw_json)) = glass_core::agent_runtime::extract_handoff(&text) {
                    let _ = proxy.send_event(glass_core::event::AppEvent::AgentHandoff {
                        session_id: runtime.session_id.clone(),
                        handoff,
                        project_root: runtime.project_root.clone(),
                        raw_json,
                    });
                }
                // Buffer for orchestrator response
                if !text.is_empty() {
                    *buffered_response = Some(text);
                }
            }
            glass_agent_backend::AgentEvent::Thinking { text } => {
                let _ = proxy.send_event(glass_core::event::AppEvent::OrchestratorThinking { text });
            }
            glass_agent_backend::AgentEvent::ToolCall { name, id, input } => {
                let summary = orchestrator_events::truncate_display(&input, 200);
                tool_id_to_name.insert(id, name.clone());
                let _ = proxy.send_event(glass_core::event::AppEvent::OrchestratorToolCall {
                    name,
                    params_summary: summary,
                });
            }
            glass_agent_backend::AgentEvent::ToolResult { tool_use_id, content } => {
                let tool_name = tool_id_to_name
                    .remove(&tool_use_id)
                    .unwrap_or_else(|| tool_use_id);
                let summary = orchestrator_events::truncate_display(&content, 200);
                let _ = proxy.send_event(glass_core::event::AppEvent::OrchestratorToolResult {
                    name: tool_name,
                    output_summary: summary,
                });
            }
            glass_agent_backend::AgentEvent::TurnComplete { cost_usd } => {
                let _ = proxy.send_event(glass_core::event::AppEvent::AgentQueryResult { cost_usd });
                if let Some(response) = buffered_response.take() {
                    let _ = proxy.send_event(glass_core::event::AppEvent::OrchestratorResponse { response });
                }
            }
            glass_agent_backend::AgentEvent::Crashed => {
                let _ = proxy.send_event(glass_core::event::AppEvent::AgentCrashed);
            }
        }
    }
}
```

- [ ] **Step 4: Update `respawn_orchestrator_agent()` to use backend**

At `main.rs:2413`, the existing `self.agent_runtime = None` already triggers `Drop`, which calls `backend.shutdown(token)`. No explicit shutdown call needed — just ensure the `Drop` impl is in place:

```rust
fn respawn_orchestrator_agent(&mut self, cwd: &str, handoff_content: String) {
    // ... existing event buffer push ...

    // Drop triggers backend.shutdown() via Drop impl
    self.agent_runtime = None;
    self.agent_generation += 1;

    // ... rest of existing logic, using try_spawn_agent() with system_prompt param ...
}
```
```

- [ ] **Step 5: Update all `try_spawn_agent()` call sites to pass `system_prompt`**

Search for all calls to `try_spawn_agent()` in `main.rs` and update them to build the system prompt first and pass it as a parameter. The system prompt assembly code (lines 996-1132) moves into a helper function.

- [ ] **Step 6: Bridge the activity stream to `message_tx`**

The current writer thread in `try_spawn_agent()` (lines 1497-1538) reads from `activity_rx` and writes to stdin with mode/cooldown gating. This powers the non-orchestrator agent modes (Watch, Assist, Autonomous). With the new design, `activity_rx` is no longer consumed by the backend — it must be bridged to `handle.message_tx`.

Add a bridging thread that receives from `activity_rx`, applies mode/cooldown filtering, formats as stream-json, and sends through `message_tx`:

```rust
// Bridge activity stream to the agent backend's message channel.
// This preserves Watch/Assist/Autonomous agent modes.
let mode = config.mode;
let cooldown_secs = config.cooldown_secs;
let bridge_tx = handle.message_tx.clone();
std::thread::Builder::new()
    .name("glass-agent-activity-bridge".into())
    .spawn(move || {
        let mut last_sent: Option<std::time::Instant> = None;
        let cooldown = std::time::Duration::from_secs(cooldown_secs);
        for event in activity_rx.iter() {
            if !glass_core::agent_runtime::should_send_in_mode(mode, &event.severity) {
                continue;
            }
            if let Some(last) = last_sent {
                if last.elapsed() < cooldown {
                    continue;
                }
            }
            let msg = glass_core::agent_runtime::format_activity_as_user_message(&event);
            if bridge_tx.send(msg).is_err() {
                break; // Agent died
            }
            last_sent = Some(std::time::Instant::now());
        }
    })
    .ok();
```

This thread is spawned in `try_spawn_agent()` after `backend.spawn()` returns. It runs alongside the backend's internal writer thread, which reads from `message_tx` and writes to stdin.

- [ ] **Step 7: Update message sending to use `message_tx`**

Replace all `orchestrator_writer` usage with `handle.message_tx.send()`. Search for `orchestrator_writer` in `main.rs` and replace each:

```rust
// Before:
if let Some(ref writer) = rt.orchestrator_writer {
    let mut w = writer.lock();
    let _ = writeln!(w, "{json_msg}");
    let _ = w.flush();
}

// After:
let _ = rt.handle.message_tx.send(json_msg);
```

- [ ] **Step 8: Remove old `AgentRuntime` fields and update `Drop`**

Remove: `child`, `agent_id`, `coord_nonce`, `orchestrator_writer` from the old struct. These are now internal to `ClaudeCliBackend` via `ShutdownToken`. The new `Drop` impl (from Step 1) replaces the old `Drop` at `main.rs:466`.

Also update all code paths that set `self.agent_runtime = None` (lines 2422, 7019, 7552) — `Drop` is now called automatically and handles shutdown. Remove any explicit child-kill or coordination-deregister code at those sites since `Drop` covers it.

- [ ] **Step 9: Verify it compiles**

Run: `cargo build`
Expected: Successful compilation.

- [ ] **Step 10: Run all tests**

Run: `cargo test --workspace`
Expected: All existing tests pass.

- [ ] **Step 11: Commit**

```bash
git add src/main.rs
git commit -m "refactor: wire main.rs to use AgentBackend trait

AgentRuntime holds AgentHandle + backend. try_spawn_agent() is a thin
wrapper around backend.spawn(). Event routing drains AgentEvent channel
and maps to AppEvent. All orchestrator_writer usage replaced with
message_tx. Zero behavior change."
```

---

### Task 6: Update crash recovery to use `implementer_launch_command()`

**Files:**
- Modify: `src/main.rs:6312` — use configurable implementer command

- [ ] **Step 1: Add `implementer_launch_command()` helper**

```rust
/// Get the command to launch the implementer CLI for crash recovery.
fn implementer_launch_command(config: &glass_core::config::GlassConfig) -> String {
    let implementer = config
        .agent
        .as_ref()
        .and_then(|a| a.orchestrator.as_ref())
        .map(|o| o.implementer.as_str())
        .unwrap_or("claude-code");

    match implementer {
        "claude-code" => "claude --dangerously-skip-permissions -p".to_string(),
        "codex" => "codex --full-auto".to_string(),
        "aider" => "aider --yes-always".to_string(),
        "gemini" => "gemini".to_string(),
        "custom" => config
            .agent
            .as_ref()
            .and_then(|a| a.orchestrator.as_ref())
            .and_then(|o| o.implementer_command.clone())
            .unwrap_or_default(),
        _ => "claude --dangerously-skip-permissions -p".to_string(),
    }
}
```

- [ ] **Step 2: Replace hardcoded crash recovery at line 6312**

```rust
// Before:
let restart_msg = format!(
    "claude --dangerously-skip-permissions -p \"Read {} and continue...\"\r",
    cp_rel,
);

// After:
let impl_cmd = implementer_launch_command(&self.config);
let restart_msg = format!(
    "{} \"Read {} and continue the project from where you left off. Follow the iteration protocol: plan, implement, commit, verify, decide.\"\r",
    impl_cmd, cp_rel,
);
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: Successful compilation.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: configurable implementer CLI for crash recovery

Replace hardcoded 'claude --dangerously-skip-permissions' with
implementer_launch_command() that reads from config. Supports
claude-code, codex, aider, gemini, and custom commands."
```

---

### Task 7: Parameterize system prompt with `implementer_name` and `persona`

**Files:**
- Modify: `src/main.rs` — system prompt assembly function

- [ ] **Step 1: Extract system prompt assembly into a helper function**

Move the prompt construction logic (currently in `try_spawn_agent` lines 996-1132) into a dedicated function:

```rust
fn build_orchestrator_system_prompt(
    config: &glass_core::config::OrchestratorSection,
    project_root: &str,
) -> String {
    let implementer_name = &config.implementer_name;
    let artifact_path = &config.completion_artifact;
    let orch_mode = &config.orchestrator_mode;

    // Load persona
    let persona = match &config.persona {
        Some(p) if p.ends_with(".md") => std::fs::read_to_string(p).unwrap_or_default(),
        Some(p) => p.clone(),
        None => String::new(),
    };

    let mode_instructions = if orch_mode == "audit" {
        // ... existing audit prompt with "Claude Code" replaced by {implementer_name} ...
    } else if orch_mode == "general" {
        // ... existing general prompt with "Claude Code" replaced by {implementer_name} ...
    } else {
        // ... existing build prompt with "Claude Code" replaced by {implementer_name} ...
    };

    // Protocol + mode + persona + critical rules
    let prompt = format!(
        r#"You are the Glass Agent, collaborating with {implementer_name} to build a project.
{implementer_name} is the implementer — it writes code, runs commands, builds features.
You are the reviewer and guide — you make product decisions, ensure quality,
and keep the project moving against the plan.

PROJECT DIRECTORY: {project_root}

{mode_instructions}

{persona}

CRITICAL RULES:
- You CANNOT write code yourself. Instruct {implementer_name} to do all implementation.
..."#
    );

    prompt
}
```

- [ ] **Step 2: Replace all hardcoded "Claude Code" references with `{implementer_name}`**

In the mode instruction strings, replace every occurrence of "Claude Code" with the `implementer_name` variable. There are ~20 occurrences across the audit, general, and build mode prompts.

- [ ] **Step 3: Verify it compiles and prompts are identical when using defaults**

Run: `cargo build`
Expected: Successful compilation. When `implementer_name` defaults to "Claude Code" and `persona` is empty, the generated prompt is byte-for-byte identical to the current hardcoded prompt.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: parameterize system prompt with implementer_name and persona

Replace hardcoded 'Claude Code' with configurable implementer_name.
Add persona layer between mode instructions and critical rules.
Defaults produce identical prompts to current behavior."
```

---

### Task 8: Update settings overlay with new fields

**Files:**
- Modify: `crates/glass_renderer/src/settings_overlay.rs:72-106` — add fields to `SettingsConfigSnapshot`
- Modify: `crates/glass_renderer/src/settings_overlay.rs:940-1020` — add Persona row to orchestrator section

- [ ] **Step 1: Add `orchestrator_persona` and `agent_provider` to `SettingsConfigSnapshot`**

Add to the struct (after `orchestrator_ablation_sweep_interval`):

```rust
    pub orchestrator_persona: String,
    pub agent_provider: String,
    pub agent_model: String,
```

Add defaults:
```rust
    orchestrator_persona: "(default)".to_string(),
    agent_provider: "claude-code".to_string(),
    agent_model: "(default)".to_string(),
```

- [ ] **Step 2: Add Persona row to orchestrator section in the overlay**

In the orchestrator section (section index 5), add the Persona row:

```rust
("Persona", config.orchestrator_persona.clone(), false, false),
```

- [ ] **Step 3: Verify it compiles and existing overlay tests pass**

Run: `cargo test -p glass_renderer`
Expected: All existing tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/glass_renderer/src/settings_overlay.rs
git commit -m "feat: add provider, model, persona to settings overlay

Display orchestrator persona and agent provider/model in settings.
Read-only display for Phase 1 — interactive model picker comes in Phase 2."
```

---

### Task 9: Clean up `glass_core::agent_runtime` — remove moved code

**Files:**
- Modify: `crates/glass_core/src/agent_runtime.rs` — remove `build_agent_command_args()` (moved to `claude_cli.rs`)

- [ ] **Step 1: Remove `build_agent_command_args()` from `glass_core::agent_runtime`**

This function (lines 376-466) has been moved to `glass_agent_backend::claude_cli::build_claude_args()`. Remove it and its tests. Keep all other functions (`extract_proposal`, `extract_handoff`, `format_activity_as_user_message`, etc.) — they're provider-agnostic and still used by `main.rs`.

- [ ] **Step 2: Remove any tests that specifically tested `build_agent_command_args`**

- [ ] **Step 3: Verify no compilation errors from the removal**

Run: `cargo build --workspace`
Expected: No code references the removed function.

Run: `cargo test --workspace`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/glass_core/src/agent_runtime.rs
git commit -m "refactor: remove build_agent_command_args from glass_core

Moved to glass_agent_backend::claude_cli::build_claude_args().
All provider-agnostic helpers (extract_proposal, extract_handoff, etc.)
remain in glass_core::agent_runtime."
```

---

### Task 10: Full regression verification

**Files:** None — verification only.

- [ ] **Step 1: Run full test suite**

Run: `cargo test --workspace`
Expected: All ~420 tests pass.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`
Expected: No warnings.

- [ ] **Step 3: Run fmt check**

Run: `cargo fmt --all -- --check`
Expected: No formatting issues.

- [ ] **Step 4: Manual end-to-end test**

1. Build release: `cargo build --release`
2. Launch Glass
3. Open a project with a PRD.md
4. Press Ctrl+Shift+O to activate orchestrator
5. Verify: agent spawns, activity overlay shows events, silence detection works
6. Let it run 2-3 iterations
7. Verify: checkpoint/respawn works
8. Press Ctrl+Shift+O to deactivate
9. Verify: clean shutdown, no orphan processes

- [ ] **Step 5: Commit final state if any fixups were needed**

```bash
git add -A
git commit -m "fix: Phase 1 regression fixups from end-to-end testing"
```
