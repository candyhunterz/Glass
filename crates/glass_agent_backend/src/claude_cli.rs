//! Claude CLI backend.
//!
//! Implements the [`AgentBackend`] trait for the Claude CLI (`claude` binary).
//! Handles process spawning, reader/writer threads, coordination DB registration,
//! and parsing of Claude CLI's `--output-format stream-json` line-by-line output
//! into the provider-agnostic [`AgentEvent`] enum.
//!
//! Each line emitted by the Claude CLI is a JSON object with a `"type"` field.
//! This module maps those objects to zero or more [`AgentEvent`]s.

use std::io::{BufRead, BufReader, BufWriter, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;

use crate::{
    AgentBackend, AgentEvent, AgentHandle, BackendError, BackendSpawnConfig, ShutdownToken,
};

// ── Shutdown state ────────────────────────────────────────────────────────────

/// Per-spawn state needed to cleanly shut down a Claude CLI session.
struct ClaudeCliShutdownState {
    child: Option<std::process::Child>,
    coord_agent_id: Option<String>,
    coord_nonce: Option<String>,
}

// ── Backend implementation ───────────────────────────────────────────────────

/// Claude CLI backend — spawns `claude` as a subprocess with `stream-json` I/O.
pub struct ClaudeCliBackend;

impl ClaudeCliBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClaudeCliBackend {
    fn default() -> Self {
        Self::new()
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
        // ── (a) Write diagnostic file ────────────────────────────────────────
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

        // ── (b) Write system prompt ──────────────────────────────────────────
        let prompt_path = glass_dir.join("agent-system-prompt.txt");
        if let Err(e) = std::fs::write(&prompt_path, &config.system_prompt) {
            tracing::warn!("ClaudeCliBackend: failed to write system prompt: {}", e);
            return Err(BackendError::SpawnFailed(format!(
                "failed to write system prompt: {e}"
            )));
        }

        // ── (c) Generate MCP config JSON ─────────────────────────────────────
        let mcp_config_path = if config.mcp_config_path.is_empty() {
            // Generate default MCP config pointing at Glass's own MCP server
            (|| -> Option<String> {
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
                        tracing::warn!("ClaudeCliBackend: failed to write MCP config JSON: {}", e);
                        None
                    }
                }
            })()
            .unwrap_or_default()
        } else {
            config.mcp_config_path.clone()
        };

        // ── (d) Build CLI args ───────────────────────────────────────────────
        let args = build_claude_args(
            &config.allowed_tools,
            &prompt_path.to_string_lossy(),
            &mcp_config_path,
        );

        // ── (e) Spawn `claude` process ───────────────────────────────────────
        let mut cmd = Command::new("claude");
        cmd.args(&args);
        // Pipe stdout for stream-json. Stderr MUST be null -- piping stderr causes
        // a deadlock: the Claude CLI writes to stderr during initialization, fills
        // the pipe buffer before Glass starts reading, and blocks forever.
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        // Windows: suppress the console window for the claude subprocess.
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        // Linux: set PR_SET_PDEATHSIG so child is killed when parent dies.
        // (prctl is Linux-specific; macOS does not have it)
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

        // macOS: spawn a watchdog thread in the child process that polls getppid().
        // When the parent (Glass) exits, the child is reparented to launchd (PID 1).
        // The watchdog detects this and calls process::exit so the child does not
        // become a long-running orphan.
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                cmd.pre_exec(|| {
                    std::thread::Builder::new()
                        .name("glass-orphan-watchdog".into())
                        .spawn(|| loop {
                            std::thread::sleep(std::time::Duration::from_secs(2));
                            // getppid() returns 1 when reparented to launchd after parent death
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
                tracing::warn!("ClaudeCliBackend: failed to spawn claude process: {}", e);
                return Err(BackendError::SpawnFailed(format!(
                    "failed to spawn claude: {e}"
                )));
            }
        };

        // Extract stdin/stdout before storing child (stderr is null).
        let stdout = child.stdout.take().expect("stdout was piped");
        let mut stdin = child.stdin.take().expect("stdin was piped");

        // ── (f) Write initial stdin message for CLI 2.1.77+ compat ───────────
        {
            // Always send an initial message -- Claude CLI 2.1.77+ won't complete
            // initialization without one. Use the initial_message if provided, else GLASS_WAIT.
            let content = config.initial_message.as_deref().unwrap_or("GLASS_WAIT");
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
                    tracing::warn!("ClaudeCliBackend: failed to write initial message: {}", e);
                }
            }
        }

        // ── (g) Load prior handoff from AgentSessionDb ───────────────────────
        let prior_handoff_msg = {
            match glass_agent::AgentSessionDb::open_default() {
                Ok(db) => {
                    let canonical = std::fs::canonicalize(&config.project_root)
                        .unwrap_or_else(|_| std::path::PathBuf::from(&config.project_root));
                    let canonical_str = canonical.to_string_lossy().to_string();
                    match db.load_prior_handoff(&canonical_str) {
                        Ok(Some(record)) => {
                            let handoff_data = glass_core::agent_runtime::AgentHandoffData {
                                work_completed: record.handoff.work_completed,
                                work_remaining: record.handoff.work_remaining,
                                key_decisions: record.handoff.key_decisions,
                                previous_session_id: record.previous_session_id.clone(),
                            };
                            let msg = glass_core::agent_runtime::format_handoff_as_user_message(
                                &record.session_id,
                                &handoff_data,
                            );
                            tracing::info!(
                                "ClaudeCliBackend: injecting prior session handoff (session_id={})",
                                record.session_id
                            );
                            Some(msg)
                        }
                        Ok(None) => {
                            tracing::debug!("ClaudeCliBackend: no prior handoff found for project");
                            None
                        }
                        Err(e) => {
                            tracing::warn!("ClaudeCliBackend: failed to load prior handoff: {}", e);
                            None
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("ClaudeCliBackend: failed to open session db: {}", e);
                    None
                }
            }
        };

        // ── (h) Create channels ──────────────────────────────────────────────
        let (event_tx, event_rx) = mpsc::channel::<AgentEvent>();
        let (message_tx, message_rx) = mpsc::channel::<String>();

        // ── (i) Spawn reader thread ──────────────────────────────────────────
        std::thread::Builder::new()
            .name("glass-agent-reader".into())
            .spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    for event in parse_stream_json_line(&line) {
                        if event_tx.send(event).is_err() {
                            return; // receiver dropped
                        }
                    }
                }
                // stdout closed -- signal crash
                let _ = event_tx.send(AgentEvent::Crashed);
            })
            .ok();

        // ── (j) Spawn writer thread ─────────────────────────────────────────
        let shared_writer = std::sync::Arc::new(parking_lot::Mutex::new(BufWriter::new(stdin)));
        let writer_clone = std::sync::Arc::clone(&shared_writer);

        std::thread::Builder::new()
            .name("glass-agent-writer".into())
            .spawn(move || {
                // Inject prior session handoff as first message
                if let Some(ref msg) = prior_handoff_msg {
                    let mut w = writer_clone.lock();
                    let _ = writeln!(w, "{msg}");
                    let _ = w.flush();
                }

                for content in message_rx.iter() {
                    let mut w = writer_clone.lock();
                    if writeln!(w, "{content}").is_err() || w.flush().is_err() {
                        break; // BrokenPipe: child died
                    }
                }
            })
            .ok();

        tracing::info!(
            "ClaudeCliBackend: claude subprocess spawned (mode={:?}, restart_count={})",
            config.mode,
            config.restart_count
        );

        // ── (k) Register with coordination DB ───────────────────────────────
        let (coord_agent_id, coord_nonce) = {
            let canonical_str =
                glass_coordination::canonicalize_path(std::path::Path::new(&config.project_root))
                    .unwrap_or_else(|_| config.project_root.clone());
            match glass_coordination::CoordinationDb::open_default() {
                Ok(mut db) => {
                    // Prune stale agents (dead PIDs or expired heartbeats) before registering.
                    // Timeout of 120s: agents that haven't heartbeated in 2 minutes are stale.
                    match db.prune_stale(120) {
                        Ok(pruned) if !pruned.is_empty() => {
                            tracing::info!(
                                "ClaudeCliBackend: pruned {} stale agent(s): {:?}",
                                pruned.len(),
                                pruned
                            );
                        }
                        Err(e) => {
                            tracing::warn!("ClaudeCliBackend: prune_stale failed (soft): {}", e);
                        }
                        _ => {}
                    }
                    let cwd = std::env::current_dir()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| canonical_str.clone());
                    match db.register("glass-agent", "claude-code", &canonical_str, &cwd, None) {
                        Ok((agent_id, nonce)) => {
                            // Advisory lock on the project root directory
                            let lock_path = std::path::PathBuf::from(&canonical_str);
                            match db.lock_files(
                                &agent_id,
                                &[lock_path],
                                Some("agent session"),
                                &nonce,
                            ) {
                                Ok(_) => tracing::info!(
                                    "ClaudeCliBackend: registered with coordination (id={})",
                                    agent_id
                                ),
                                Err(e) => tracing::warn!(
                                    "ClaudeCliBackend: coordination lock failed (soft): {}",
                                    e
                                ),
                            }
                            (Some(agent_id), Some(nonce))
                        }
                        Err(e) => {
                            tracing::warn!(
                                "ClaudeCliBackend: coordination registration failed (soft): {}",
                                e
                            );
                            (None, None)
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "ClaudeCliBackend: failed to open coordination DB (soft): {}",
                        e
                    );
                    (None, None)
                }
            }
        };

        // ── (l) Return AgentHandle ───────────────────────────────────────────
        Ok(AgentHandle {
            message_tx,
            event_rx,
            generation,
            shutdown_token: ShutdownToken::new(ClaudeCliShutdownState {
                child: Some(child),
                coord_agent_id,
                coord_nonce,
            }),
        })
    }

    fn shutdown(&self, mut token: ShutdownToken) {
        let Some(state) = token.downcast_mut::<ClaudeCliShutdownState>() else {
            tracing::warn!("ClaudeCliBackend::shutdown: token type mismatch");
            return;
        };

        // Kill the child process
        if let Some(ref mut child) = state.child {
            match child.try_wait() {
                Ok(Some(_status)) => {
                    // Already exited
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
        state.child = None;

        // Deregister from coordination DB
        if let (Some(ref agent_id), Some(ref nonce)) = (&state.coord_agent_id, &state.coord_nonce) {
            if let Ok(mut db) = glass_coordination::CoordinationDb::open_default() {
                let _ = db.unlock_all(agent_id, nonce);
                let _ = db.deregister(agent_id, nonce);
            }
        }
    }
}

// ── CLI argument builder ─────────────────────────────────────────────────────

/// Build the argument list for spawning the `claude` CLI binary.
///
/// Extracted from `glass_core::agent_runtime::build_agent_command_args` but
/// simplified: the caller has already resolved which tools are allowed, so
/// we just pass them through.
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

// ── Stream-JSON parser ───────────────────────────────────────────────────────

/// Parse a single JSON line from Claude CLI's `stream-json` output into
/// zero or more [`AgentEvent`]s.
///
/// Returns an empty `Vec` for:
/// - empty / whitespace-only lines
/// - lines that are not valid JSON
/// - lines whose `"type"` value is not handled
///
/// A single line can produce multiple events (e.g. a `"thinking"` block
/// followed by a `"text"` block in the same `"assistant"` message).
pub(crate) fn parse_stream_json_line(line: &str) -> Vec<AgentEvent> {
    if line.trim().is_empty() {
        return vec![];
    }
    let val: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    match val.get("type").and_then(|t| t.as_str()) {
        // -- system ----------------------------------------------------------------
        Some("system") => {
            if val.get("subtype").and_then(|s| s.as_str()) == Some("init") {
                let session_id = val
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                vec![AgentEvent::Init { session_id }]
            } else {
                vec![]
            }
        }

        // -- assistant -------------------------------------------------------------
        Some("assistant") => {
            let mut events: Vec<AgentEvent> = Vec::new();
            let mut accumulated_text = String::new();

            if let Some(arr) = val
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for block in arr {
                    match block.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                accumulated_text.push_str(text);
                            }
                        }
                        Some("thinking") => {
                            if let Some(text) = block.get("thinking").and_then(|t| t.as_str()) {
                                events.push(AgentEvent::Thinking {
                                    text: text.to_string(),
                                });
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
                            events.push(AgentEvent::ToolCall { name, id, input });
                        }
                        _ => {}
                    }
                }
            }

            if !accumulated_text.is_empty() {
                events.push(AgentEvent::AssistantText {
                    text: accumulated_text,
                });
            }

            events
        }

        // -- result ----------------------------------------------------------------
        Some("result") => {
            let cost_usd = glass_core::agent_runtime::parse_cost_from_result(line).unwrap_or(0.0);
            vec![AgentEvent::TurnComplete { cost_usd }]
        }

        // -- user (tool results) ---------------------------------------------------
        Some("user") => {
            let mut events: Vec<AgentEvent> = Vec::new();

            if let Some(arr) = val
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for block in arr {
                    if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                        let tool_use_id = block
                            .get("tool_use_id")
                            .and_then(|t| t.as_str())
                            .unwrap_or("?")
                            .to_string();
                        let content = match block.get("content") {
                            Some(c) if c.is_string() => c.as_str().unwrap_or("").to_string(),
                            Some(c) if c.is_array() => c
                                .as_array()
                                .unwrap()
                                .iter()
                                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                                .collect::<Vec<_>>()
                                .join("\n"),
                            _ => String::new(),
                        };
                        events.push(AgentEvent::ToolResult {
                            tool_use_id,
                            content,
                        });
                    }
                }
            }

            events
        }

        _ => vec![],
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: assert exactly one event is returned and return it.
    fn single(line: &str) -> AgentEvent {
        let mut events = parse_stream_json_line(line);
        assert_eq!(
            events.len(),
            1,
            "expected exactly 1 event, got {:?}",
            events
        );
        events.remove(0)
    }

    // ── system ────────────────────────────────────────────────────────────────

    #[test]
    fn parse_system_init() {
        let line = r#"{"type":"system","subtype":"init","session_id":"sess-123"}"#;
        match single(line) {
            AgentEvent::Init { session_id } => assert_eq!(session_id, "sess-123"),
            other => panic!("expected Init, got {:?}", other),
        }
    }

    #[test]
    fn parse_system_non_init_ignored() {
        let line = r#"{"type":"system","subtype":"heartbeat"}"#;
        assert!(parse_stream_json_line(line).is_empty());
    }

    // ── assistant ─────────────────────────────────────────────────────────────

    #[test]
    fn parse_assistant_text() {
        let line =
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello!"}]}}"#;
        match single(line) {
            AgentEvent::AssistantText { text } => assert_eq!(text, "Hello!"),
            other => panic!("expected AssistantText, got {:?}", other),
        }
    }

    #[test]
    fn parse_assistant_thinking() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"Let me think..."}]}}"#;
        match single(line) {
            AgentEvent::Thinking { text } => assert_eq!(text, "Let me think..."),
            other => panic!("expected Thinking, got {:?}", other),
        }
    }

    #[test]
    fn parse_assistant_tool_use() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","id":"tool-abc","input":{"command":"ls"}}]}}"#;
        match single(line) {
            AgentEvent::ToolCall { name, id, input } => {
                assert_eq!(name, "Bash");
                assert_eq!(id, "tool-abc");
                // input is the JSON-serialised representation
                assert!(
                    input.contains("ls"),
                    "input should contain 'ls', got: {input}"
                );
            }
            other => panic!("expected ToolCall, got {:?}", other),
        }
    }

    #[test]
    fn parse_assistant_multiple_text_blocks_concatenates() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"foo"},{"type":"text","text":"bar"}]}}"#;
        match single(line) {
            AgentEvent::AssistantText { text } => assert_eq!(text, "foobar"),
            other => panic!("expected AssistantText, got {:?}", other),
        }
    }

    #[test]
    fn parse_assistant_mixed_blocks() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"done"}]}}"#;
        let events = parse_stream_json_line(line);
        assert_eq!(events.len(), 2, "expected 2 events, got {:?}", events);
        match &events[0] {
            AgentEvent::Thinking { text } => assert_eq!(text, "hmm"),
            other => panic!("expected Thinking first, got {:?}", other),
        }
        match &events[1] {
            AgentEvent::AssistantText { text } => assert_eq!(text, "done"),
            other => panic!("expected AssistantText second, got {:?}", other),
        }
    }

    // ── result ────────────────────────────────────────────────────────────────

    #[test]
    fn parse_result_with_cost() {
        let line = r#"{"type":"result","cost_usd":0.0042}"#;
        match single(line) {
            AgentEvent::TurnComplete { cost_usd } => {
                assert!(
                    (cost_usd - 0.0042).abs() < 1e-9,
                    "cost mismatch: {cost_usd}"
                )
            }
            other => panic!("expected TurnComplete, got {:?}", other),
        }
    }

    #[test]
    fn parse_result_without_cost() {
        let line = r#"{"type":"result"}"#;
        match single(line) {
            AgentEvent::TurnComplete { cost_usd } => {
                assert_eq!(cost_usd, 0.0)
            }
            other => panic!("expected TurnComplete, got {:?}", other),
        }
    }

    // ── user / tool_result ────────────────────────────────────────────────────

    #[test]
    fn parse_user_tool_result_string() {
        let line = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tid-1","content":"output text"}]}}"#;
        match single(line) {
            AgentEvent::ToolResult {
                tool_use_id,
                content,
            } => {
                assert_eq!(tool_use_id, "tid-1");
                assert_eq!(content, "output text");
            }
            other => panic!("expected ToolResult, got {:?}", other),
        }
    }

    #[test]
    fn parse_user_tool_result_array() {
        let line = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tid-2","content":[{"type":"text","text":"line1"},{"type":"text","text":"line2"}]}]}}"#;
        match single(line) {
            AgentEvent::ToolResult {
                tool_use_id,
                content,
            } => {
                assert_eq!(tool_use_id, "tid-2");
                assert_eq!(content, "line1\nline2");
            }
            other => panic!("expected ToolResult, got {:?}", other),
        }
    }

    // ── edge cases ────────────────────────────────────────────────────────────

    #[test]
    fn parse_empty_line_returns_empty() {
        assert!(parse_stream_json_line("").is_empty());
        assert!(parse_stream_json_line("   ").is_empty());
    }

    #[test]
    fn parse_invalid_json_returns_empty() {
        assert!(parse_stream_json_line("not json").is_empty());
    }

    #[test]
    fn parse_unknown_type_returns_empty() {
        let line = r#"{"type":"unknown"}"#;
        assert!(parse_stream_json_line(line).is_empty());
    }

    // ── build_claude_args ─────────────────────────────────────────────────────

    #[test]
    fn build_args_with_tools_and_mcp() {
        let args = build_claude_args(
            &["Bash".to_string(), "Read".to_string()],
            "/tmp/prompt.txt",
            "/tmp/mcp.json",
        );
        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"--verbose".to_string()));
        assert!(args.contains(&"stream-json".to_string()));
        assert!(args.contains(&"--system-prompt-file".to_string()));
        assert!(args.contains(&"/tmp/prompt.txt".to_string()));
        assert!(args.contains(&"--mcp-config".to_string()));
        assert!(args.contains(&"/tmp/mcp.json".to_string()));
        assert!(args.contains(&"Bash,Read".to_string()));
        assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
        assert!(args.contains(&"--disable-slash-commands".to_string()));
    }

    #[test]
    fn build_args_empty_tools_uses_defaults() {
        let args = build_claude_args(&[], "/tmp/prompt.txt", "");
        assert!(args.contains(&"glass_query,glass_context".to_string()));
        // No --mcp-config when path is empty
        assert!(!args.contains(&"--mcp-config".to_string()));
    }

    #[test]
    fn backend_name() {
        let backend = ClaudeCliBackend::new();
        assert_eq!(backend.name(), "Claude CLI");
    }
}
