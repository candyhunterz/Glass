# Orchestrator Core Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Get the basic orchestrator loop working — silence detection triggers the Glass Agent to read terminal context and type a response into the PTY.

**Architecture:** A new `orchestrator.rs` module in `src/` owns the silence timer and response routing. The existing `AgentRuntime` subprocess is reused with a new system prompt. The PTY loop emits a timestamp on each read. The orchestrator polls for silence, captures terminal context, sends it to the agent via stdin, and writes the agent's response to the PTY.

**Tech Stack:** Rust, winit event loop, existing PTY/Agent infrastructure

**Spec:** `docs/superpowers/specs/2026-03-14-agent-orchestrator-design.md`

---

## File Structure

```
src/orchestrator.rs          (CREATE) — Orchestrator state machine, silence check, response routing
crates/glass_core/src/config.rs    (MODIFY) — Add OrchestratorSection config
crates/glass_core/src/event.rs     (MODIFY) — Add OrchestratorResponse AppEvent
crates/glass_terminal/src/pty.rs   (MODIFY) — Emit last-output timestamp
src/main.rs                        (MODIFY) — Wire orchestrator into event loop, Ctrl+Shift+O, agent rewire
```

---

## Chunk 1: Config and Event Plumbing

### Task 1: Add OrchestratorSection config

**Files:**
- Modify: `crates/glass_core/src/config.rs`

- [ ] **Step 1: Write failing test for OrchestratorSection deserialization**

```rust
// In config.rs, inside #[cfg(test)] mod tests

#[test]
fn test_orchestrator_section_defaults() {
    let toml = "[agent]\nmode = \"autonomous\"\n[agent.orchestrator]\nenabled = true";
    let config = GlassConfig::load_from_str(toml);
    let orch = config
        .agent
        .expect("agent section")
        .orchestrator
        .expect("orchestrator section");
    assert!(orch.enabled);
    assert_eq!(orch.silence_timeout_secs, 30);
    assert_eq!(orch.prd_path, "PRD.md");
    assert_eq!(orch.checkpoint_path, ".glass/checkpoint.md");
    assert_eq!(orch.max_retries_before_stuck, 3);
}

#[test]
fn test_orchestrator_section_absent_is_none() {
    let toml = "[agent]\nmode = \"autonomous\"";
    let config = GlassConfig::load_from_str(toml);
    assert!(config.agent.unwrap().orchestrator.is_none());
}

#[test]
fn test_orchestrator_section_custom_values() {
    let toml = "[agent.orchestrator]\nenabled = true\nsilence_timeout_secs = 15\nprd_path = \"docs/plan.md\"";
    let config = GlassConfig::load_from_str(toml);
    let orch = config.agent.unwrap().orchestrator.unwrap();
    assert_eq!(orch.silence_timeout_secs, 15);
    assert_eq!(orch.prd_path, "docs/plan.md");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --package glass_core test_orchestrator`
Expected: FAIL — `OrchestratorSection` doesn't exist yet

- [ ] **Step 3: Implement OrchestratorSection**

Add to `crates/glass_core/src/config.rs`, after the `AgentSection` struct:

```rust
/// Orchestrator configuration in the `[agent.orchestrator]` TOML section.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OrchestratorSection {
    /// Whether the orchestrator loop is active. Default false.
    #[serde(default)]
    pub enabled: bool,
    /// Seconds of PTY silence before triggering the orchestrator. Default 30.
    #[serde(default = "default_orch_silence_timeout")]
    pub silence_timeout_secs: u64,
    /// Path to the project plan file (relative to CWD). Default "PRD.md".
    #[serde(default = "default_orch_prd_path")]
    pub prd_path: String,
    /// Path to the checkpoint file (relative to CWD). Default ".glass/checkpoint.md".
    #[serde(default = "default_orch_checkpoint_path")]
    pub checkpoint_path: String,
    /// Max identical responses before stuck detection triggers. Default 3.
    #[serde(default = "default_orch_max_retries")]
    pub max_retries_before_stuck: u32,
}

fn default_orch_silence_timeout() -> u64 { 30 }
fn default_orch_prd_path() -> String { "PRD.md".to_string() }
fn default_orch_checkpoint_path() -> String { ".glass/checkpoint.md".to_string() }
fn default_orch_max_retries() -> u32 { 3 }
```

Add field to `AgentSection`:

```rust
    #[serde(default)]
    pub orchestrator: Option<OrchestratorSection>,
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --package glass_core test_orchestrator`
Expected: PASS

- [ ] **Step 5: Run clippy**

Run: `cargo clippy --package glass_core -- -D warnings`
Expected: Clean

- [ ] **Step 6: Commit**

```bash
git add crates/glass_core/src/config.rs
git commit -m "feat(config): add [agent.orchestrator] section"
```

---

### Task 2: Add AppEvent variants for orchestrator

**Files:**
- Modify: `crates/glass_core/src/event.rs`

- [ ] **Step 1: Add new AppEvent variants**

Add after `AgentCrashed` in the `AppEvent` enum:

```rust
    /// Orchestrator: the Glass Agent produced a response to route.
    OrchestratorResponse {
        /// The raw text from the Glass Agent.
        response: String,
    },
    /// Orchestrator: PTY silence threshold reached.
    OrchestratorSilence {
        window_id: winit::window::WindowId,
        session_id: SessionId,
    },
```

- [ ] **Step 2: Run clippy to verify it compiles**

Run: `cargo clippy --workspace -- -D warnings`
Expected: Clean (new variants are unused for now, but enum variants don't trigger dead_code)

- [ ] **Step 3: Commit**

```bash
git add crates/glass_core/src/event.rs
git commit -m "feat(event): add OrchestratorResponse and OrchestratorSilence events"
```

---

## Chunk 2: Silence Detection in PTY Loop

### Task 3: Emit silence events from PTY loop

**Files:**
- Modify: `crates/glass_terminal/src/pty.rs`

The PTY loop runs in its own thread. We need it to notice when output stops for N seconds and emit an `OrchestratorSilence` event. The approach: track `Instant::now()` on each successful read. Use the poll timeout to check for silence.

- [ ] **Step 1: Add silence tracking state to glass_pty_loop**

In `glass_pty_loop` (line ~263), add new parameters and state. First, add an `orchestrator_silence_secs` parameter to the function signature:

Find the function signature `pub fn glass_pty_loop(` and add a new parameter:

```rust
pub fn glass_pty_loop(
    // ... existing params ...
    orchestrator_silence_secs: u64,  // 0 = disabled
)
```

Inside the function body, after `let mut output_buffer = OutputBuffer::new(...)` (line ~279):

```rust
    let mut last_output_at = std::time::Instant::now();
    let silence_threshold = if orchestrator_silence_secs > 0 {
        Some(std::time::Duration::from_secs(orchestrator_silence_secs))
    } else {
        None
    };
    let mut silence_fired = false; // prevent repeated firing
```

- [ ] **Step 2: Update last_output_at on successful reads**

After the `Ok(got)` arm of the PTY read (around line 423, inside `pty_read_with_scan` or in the read result handling), the data is processed. Find where `unprocessed += got` happens and add:

```rust
    last_output_at = std::time::Instant::now();
    silence_fired = false; // reset on new output
```

Note: this needs to be in the `glass_pty_loop` function where data is read from the PTY, NOT inside `pty_read_with_scan`. Find the outer loop where `pty.reader().read()` happens and data is dispatched.

- [ ] **Step 3: Check silence threshold in the poll loop**

Inside the main event loop of `glass_pty_loop`, after the poll timeout handling and before the next iteration, add a silence check:

```rust
    // Orchestrator silence detection
    if let Some(threshold) = silence_threshold {
        if !silence_fired && last_output_at.elapsed() >= threshold {
            silence_fired = true;
            let _ = app_proxy.send_event(AppEvent::OrchestratorSilence {
                window_id,
                session_id: event_proxy.session_id(),
            });
        }
    }
```

Place this at the end of each iteration of the `'event_loop` loop, after `pty_read_with_scan` returns and events are dispatched.

- [ ] **Step 4: Update call site in src/main.rs**

Find where `glass_pty_loop` is called in `src/main.rs` (search for `glass_pty_loop(`). Add the new parameter. Read from config:

```rust
let orchestrator_silence_secs = self
    .config
    .agent
    .as_ref()
    .and_then(|a| a.orchestrator.as_ref())
    .filter(|o| o.enabled)
    .map(|o| o.silence_timeout_secs)
    .unwrap_or(0);
```

Pass `orchestrator_silence_secs` to `glass_pty_loop`.

- [ ] **Step 5: Update any other call sites of glass_pty_loop**

Search for all call sites of `glass_pty_loop` across the workspace. There may be test helpers or multi-pane spawners. Add `0` (disabled) for all non-primary call sites.

- [ ] **Step 6: Build and verify**

Run: `cargo build`
Expected: Clean build

- [ ] **Step 7: Commit**

```bash
git add crates/glass_terminal/src/pty.rs src/main.rs
git commit -m "feat(pty): emit OrchestratorSilence after configurable silence threshold"
```

---

## Chunk 3: Orchestrator Module

### Task 4: Create orchestrator.rs with core state machine

**Files:**
- Create: `src/orchestrator.rs`
- Modify: `src/main.rs` (add `mod orchestrator;`)

- [ ] **Step 1: Create the orchestrator module with state and response parsing**

Create `src/orchestrator.rs`:

```rust
//! Orchestrator: drives Claude Code sessions autonomously via the Glass Agent.
//!
//! Owns the silence-triggered loop that captures terminal context, sends it
//! to the Glass Agent, and routes the response (type into PTY, wait, or checkpoint).

/// Parsed response from the Glass Agent.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentResponse {
    /// Type this text into the terminal.
    TypeText(String),
    /// Claude Code is still working; reset silence timer and check again later.
    Wait,
    /// Feature complete; trigger context refresh cycle.
    Checkpoint {
        completed: String,
        next: String,
    },
}

/// Parse a raw Glass Agent response into a structured action.
pub fn parse_agent_response(raw: &str) -> AgentResponse {
    let trimmed = raw.trim();

    if trimmed == "GLASS_WAIT" {
        return AgentResponse::Wait;
    }

    let checkpoint_marker = "GLASS_CHECKPOINT:";
    if let Some(start) = trimmed.find(checkpoint_marker) {
        let after = trimmed[start + checkpoint_marker.len()..].trim();
        if let Some(json_start) = after.find('{') {
            let json_slice = &after[json_start..];
            // Find matching closing brace
            let mut depth = 0usize;
            let mut end = None;
            for (i, ch) in json_slice.char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            end = Some(i + 1);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(end_idx) = end {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_slice[..end_idx]) {
                    let completed = val
                        .get("completed")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let next = val
                        .get("next")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    return AgentResponse::Checkpoint { completed, next };
                }
            }
        }
    }

    // Default: type the text into the terminal
    AgentResponse::TypeText(trimmed.to_string())
}

/// Orchestrator state, lives on Processor in main.rs.
pub struct OrchestratorState {
    /// Whether orchestration is active (toggled by Ctrl+Shift+O).
    pub active: bool,
    /// Iteration counter (for status bar display and logging).
    pub iteration: u32,
    /// Last N responses for stuck detection (ring buffer).
    pub recent_responses: Vec<String>,
    /// Max identical responses before stuck triggers.
    pub max_retries: u32,
}

impl OrchestratorState {
    pub fn new(max_retries: u32) -> Self {
        Self {
            active: false,
            iteration: 0,
            max_retries,
            recent_responses: Vec::new(),
        }
    }

    /// Record a response and check if we're stuck (N identical consecutive responses).
    /// Returns true if stuck.
    pub fn record_response(&mut self, response: &str) -> bool {
        self.recent_responses.push(response.to_string());
        if self.recent_responses.len() > self.max_retries as usize {
            self.recent_responses
                .drain(..self.recent_responses.len() - self.max_retries as usize);
        }
        if self.recent_responses.len() >= self.max_retries as usize {
            self.recent_responses
                .iter()
                .all(|r| r == &self.recent_responses[0])
        } else {
            false
        }
    }

    /// Reset stuck detection (e.g., after a successful verification).
    pub fn reset_stuck(&mut self) {
        self.recent_responses.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plain_text() {
        let resp = parse_agent_response("continue with the next feature");
        assert_eq!(
            resp,
            AgentResponse::TypeText("continue with the next feature".to_string())
        );
    }

    #[test]
    fn parse_wait() {
        assert_eq!(parse_agent_response("GLASS_WAIT"), AgentResponse::Wait);
        assert_eq!(parse_agent_response("  GLASS_WAIT  "), AgentResponse::Wait);
    }

    #[test]
    fn parse_checkpoint() {
        let raw = r#"GLASS_CHECKPOINT: {"completed": "auth module", "next": "database layer"}"#;
        match parse_agent_response(raw) {
            AgentResponse::Checkpoint { completed, next } => {
                assert_eq!(completed, "auth module");
                assert_eq!(next, "database layer");
            }
            other => panic!("Expected Checkpoint, got {:?}", other),
        }
    }

    #[test]
    fn parse_checkpoint_with_extra_text() {
        let raw = r#"Some preamble GLASS_CHECKPOINT: {"completed": "x", "next": "y"} trailing"#;
        match parse_agent_response(raw) {
            AgentResponse::Checkpoint { completed, next } => {
                assert_eq!(completed, "x");
                assert_eq!(next, "y");
            }
            other => panic!("Expected Checkpoint, got {:?}", other),
        }
    }

    #[test]
    fn parse_malformed_checkpoint_falls_back_to_text() {
        let raw = "GLASS_CHECKPOINT: not json";
        match parse_agent_response(raw) {
            AgentResponse::TypeText(_) => {} // expected fallback
            other => panic!("Expected TypeText fallback, got {:?}", other),
        }
    }

    #[test]
    fn stuck_detection_triggers_after_n_identical() {
        let mut state = OrchestratorState::new(3);
        assert!(!state.record_response("fix the test"));
        assert!(!state.record_response("fix the test"));
        assert!(state.record_response("fix the test")); // 3rd identical
    }

    #[test]
    fn stuck_detection_resets_on_different_response() {
        let mut state = OrchestratorState::new(3);
        state.record_response("fix the test");
        state.record_response("fix the test");
        assert!(!state.record_response("try a different approach")); // different
    }

    #[test]
    fn stuck_detection_reset_clears() {
        let mut state = OrchestratorState::new(3);
        state.record_response("fix the test");
        state.record_response("fix the test");
        state.reset_stuck();
        assert!(!state.record_response("fix the test")); // reset, only 1 now
    }
}
```

- [ ] **Step 2: Add `mod orchestrator;` to main.rs**

At the top of `src/main.rs`, after the other `mod` declarations (or `use` block), add:

```rust
mod orchestrator;
```

- [ ] **Step 3: Run tests**

Run: `cargo test --bin glass orchestrator`
Expected: All orchestrator tests pass

- [ ] **Step 4: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`
Expected: Clean

- [ ] **Step 5: Commit**

```bash
git add src/orchestrator.rs src/main.rs
git commit -m "feat: add orchestrator module with response parsing and stuck detection"
```

---

## Chunk 4: Wire Into Event Loop

### Task 5: Handle OrchestratorSilence — capture context and send to agent

**Files:**
- Modify: `src/main.rs`
- Modify: `src/orchestrator.rs`

This is the core wiring: when `OrchestratorSilence` fires, capture terminal context, format it as a user message, and send it to the Glass Agent's stdin.

- [ ] **Step 1: Add OrchestratorState to Processor struct**

Add field to Processor (after `activity_overlay_visible`):

```rust
    /// Orchestrator state for autonomous Claude Code collaboration.
    orchestrator: orchestrator::OrchestratorState,
```

Initialize in `Processor::new()` or wherever Processor fields are initialized:

```rust
    orchestrator: orchestrator::OrchestratorState::new(
        config.agent.as_ref()
            .and_then(|a| a.orchestrator.as_ref())
            .map(|o| o.max_retries_before_stuck)
            .unwrap_or(3),
    ),
```

- [ ] **Step 2: Add a channel for orchestrator→agent stdin writes**

The existing writer thread reads from `activity_rx` and writes to claude's stdin. For the orchestrator, we need a separate way to write to the agent's stdin. Add a field to `AgentRuntime`:

```rust
    /// Sender for writing orchestrator messages to the agent's stdin.
    /// The writer thread reads from the paired receiver.
    pub orchestrator_tx: Option<std::sync::mpsc::SyncSender<String>>,
```

In `try_spawn_agent`, create a bounded channel and pass it to the writer thread. When orchestrator mode is active, the writer thread reads from BOTH the activity channel (for non-orchestrator modes) and the orchestrator channel.

The simplest approach: create a unified message enum:

In `src/orchestrator.rs`, add:

```rust
/// Message types the writer thread can receive.
pub enum AgentWriterMsg {
    /// Activity event (existing error-watching mode).
    Activity(glass_core::activity_stream::ActivityEvent),
    /// Orchestrator query (terminal context for the agent to respond to).
    OrchestratorQuery(String),
    /// Shutdown signal.
    Shutdown,
}
```

- [ ] **Step 3: Modify try_spawn_agent to use unified channel**

Replace the `activity_rx: Receiver<ActivityEvent>` parameter with a `writer_rx: Receiver<AgentWriterMsg>`. Create the channel in the calling code. The writer thread reads `AgentWriterMsg` variants and handles each:

```rust
for msg in writer_rx.iter() {
    match msg {
        AgentWriterMsg::Activity(event) => {
            if !should_send_in_mode(mode, &event.severity) { continue; }
            if let Some(last) = last_sent {
                if last.elapsed() < cooldown { continue; }
            }
            let formatted = format_activity_as_user_message(&event);
            if writeln!(writer, "{formatted}").is_err() || writer.flush().is_err() { break; }
            last_sent = Some(Instant::now());
        }
        AgentWriterMsg::OrchestratorQuery(json_msg) => {
            if writeln!(writer, "{json_msg}").is_err() || writer.flush().is_err() { break; }
        }
        AgentWriterMsg::Shutdown => break,
    }
}
```

Store the `SyncSender<AgentWriterMsg>` on AgentRuntime so the orchestrator can send queries.

- [ ] **Step 4: Handle OrchestratorSilence event in main event loop**

Add a match arm in the main event handler:

```rust
AppEvent::OrchestratorSilence { window_id, session_id } => {
    if !self.orchestrator.active {
        return; // orchestrator not enabled
    }
    if self.agent_runtime.is_none() {
        return; // no agent running
    }

    // Capture terminal context
    if let Some(ctx) = self.windows.get(&window_id) {
        if let Some(session) = ctx.session_mux.session(session_id) {
            let lines = extract_term_lines(&session.terminal, 100);
            let context = lines.join("\n");

            // Format as stream-json user message
            let content = format!(
                "[TERMINAL_CONTEXT]\n{}",
                context
            );
            let msg = serde_json::json!({
                "type": "user",
                "message": {
                    "role": "user",
                    "content": content
                }
            }).to_string();

            // Send to agent via writer thread
            if let Some(ref runtime) = self.agent_runtime {
                if let Some(ref tx) = runtime.orchestrator_tx {
                    let _ = tx.try_send(
                        orchestrator::AgentWriterMsg::OrchestratorQuery(msg)
                    );
                }
            }
        }
    }
}
```

- [ ] **Step 5: Handle OrchestratorResponse in main event loop**

The reader thread already parses "assistant" messages. Extend it to emit `OrchestratorResponse` when orchestrator mode is active. In the reader thread's `Some("assistant")` arm, after extracting `full_text`:

```rust
// If orchestrator is active, route as orchestrator response
let _ = proxy_reader.send_event(AppEvent::OrchestratorResponse {
    response: full_text.clone(),
});
```

Then handle `OrchestratorResponse` in the main event loop:

```rust
AppEvent::OrchestratorResponse { response } => {
    if !self.orchestrator.active {
        return;
    }

    let parsed = orchestrator::parse_agent_response(&response);
    self.orchestrator.iteration += 1;

    match parsed {
        orchestrator::AgentResponse::Wait => {
            // Do nothing — silence timer will fire again
            tracing::debug!("Orchestrator: agent says WAIT");
        }
        orchestrator::AgentResponse::TypeText(text) => {
            // Check for stuck loop
            if self.orchestrator.record_response(&text) {
                tracing::warn!("Orchestrator: stuck detected after {} identical responses", self.orchestrator.max_retries);
                // TODO: handle stuck (Plan 3)
                return;
            }

            // Type the text into the active PTY
            if let Some(ctx) = self.windows.values().next() {
                if let Some(session) = ctx.session_mux.focused_session() {
                    let bytes = format!("{}\n", text).into_bytes();
                    let _ = session.pty_sender.send(
                        glass_terminal::PtyMsg::Input(std::borrow::Cow::Owned(bytes))
                    );
                }
            }
        }
        orchestrator::AgentResponse::Checkpoint { completed, next } => {
            tracing::info!("Orchestrator: checkpoint — completed={}, next={}", completed, next);
            // TODO: handle checkpoint cycle (Plan 3)
        }
    }

    // Request redraw for status bar update
    for ctx in self.windows.values() {
        ctx.window.request_redraw();
    }
}
```

- [ ] **Step 6: Build and verify**

Run: `cargo build`
Expected: Clean build

- [ ] **Step 7: Commit**

```bash
git add src/main.rs src/orchestrator.rs
git commit -m "feat: wire orchestrator silence → context capture → agent → PTY response loop"
```

---

### Task 6: Add Ctrl+Shift+O toggle and status bar indicator

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add Ctrl+Shift+O keyboard shortcut**

In the Ctrl+Shift keyboard handler block (around line 2506), add after the existing shortcuts:

```rust
// Ctrl+Shift+O: Toggle orchestrator on/off.
Key::Character(c) if c.as_str().eq_ignore_ascii_case("o") => {
    self.orchestrator.active = !self.orchestrator.active;
    if self.orchestrator.active {
        tracing::info!("Orchestrator: enabled by user");
        self.orchestrator.reset_stuck();
    } else {
        tracing::info!("Orchestrator: disabled by user");
    }
    ctx.window.request_redraw();
    return;
}
```

- [ ] **Step 2: Add auto-pause on user keyboard input**

In the keyboard input handler (where keystrokes are sent to the PTY, around line 3098), add before the PTY send:

```rust
// Auto-pause orchestrator if user types while it's active
if self.orchestrator.active {
    self.orchestrator.active = false;
    tracing::info!("Orchestrator: auto-paused (user typing detected)");
}
```

- [ ] **Step 3: Update status bar to show orchestrator state**

In the status bar rendering section (where `agent_mode_text` is computed), modify to include orchestrator state:

```rust
let agent_mode_text = self.agent_runtime.as_ref().map(|_r| {
    let mode = self.config.agent.as_ref()
        .map(|a| format!("{:?}", a.mode))
        .unwrap_or_else(|| "off".to_string())
        .to_lowercase();
    if self.orchestrator.active {
        format!("[orchestrating | iter #{}]", self.orchestrator.iteration)
    } else {
        format!("[agent: {}]", mode)
    }
});
```

- [ ] **Step 4: Build and run manual test**

Run: `cargo build --release`
Launch Glass, press Ctrl+Shift+O. Status bar should show `[orchestrating | iter #0]`. Press again to toggle off.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat: add Ctrl+Shift+O orchestrator toggle with auto-pause on user input"
```

---

### Task 7: Update agent system prompt for orchestrator mode

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Load PRD content for system prompt**

In `try_spawn_agent`, after the system prompt string, add PRD loading:

```rust
let orchestrator_enabled = config
    .orchestrator
    .as_ref()
    .map(|o| o.enabled)
    .unwrap_or(false);

let system_prompt = if orchestrator_enabled {
    let prd_path = config
        .orchestrator
        .as_ref()
        .map(|o| o.prd_path.clone())
        .unwrap_or_else(|| "PRD.md".to_string());

    let prd_content = std::fs::read_to_string(&prd_path)
        .unwrap_or_else(|_| format!("(PRD not found at {})", prd_path));

    // Truncate to ~4000 words
    let prd_truncated: String = prd_content
        .split_whitespace()
        .take(4000)
        .collect::<Vec<_>>()
        .join(" ");

    let checkpoint_path = config
        .orchestrator
        .as_ref()
        .map(|o| o.checkpoint_path.clone())
        .unwrap_or_else(|| ".glass/checkpoint.md".to_string());

    let checkpoint_content = std::fs::read_to_string(&checkpoint_path)
        .unwrap_or_else(|_| "Fresh start — no previous checkpoint.".to_string());

    format!(r#"You are the Glass Agent, collaborating with Claude Code to build a project.
Claude Code is the implementer — it writes code, runs commands, builds features.
You are the reviewer and guide — you make product decisions, ensure quality,
and keep the project moving against the plan.

PROJECT PLAN:
{prd_truncated}

CURRENT STATUS:
{checkpoint_content}

ITERATION PROTOCOL:
For each feature, guide Claude Code through this cycle:
1. PLAN: Tell Claude Code what to build next and define acceptance criteria
2. IMPLEMENT: Let Claude Code work. Answer its questions with clear decisions.
3. COMMIT: Tell Claude Code to commit before verification
4. VERIFY: Tell Claude Code to write tests and run them
5. DECIDE: Tests pass → move to next feature. Tests fail → tell Claude Code to fix.
   Stuck after 3 attempts → tell Claude Code to revert and try different approach.

CONTEXT REFRESH:
When you've completed 2-3 features and context is getting heavy, emit:
GLASS_CHECKPOINT: {{"completed": "<summary>", "next": "<next PRD item>"}}

RESPONSE FORMAT:
Respond with ONLY one of:
1. Text to type into the terminal (sent as-is to Claude Code)
2. GLASS_WAIT (Claude Code is still working, check again later)
3. GLASS_CHECKPOINT: {{"completed": "...", "next": "..."}}

No explanations, no meta-commentary. Just the response."#)
} else {
    // Existing error-watching system prompt
    r#"You are Glass Agent, an AI assistant integrated into the Glass terminal emulator.
..."#.to_string() // keep existing prompt
};
```

- [ ] **Step 2: Write updated prompt to disk and pass to claude**

The existing code writes the prompt to `~/.glass/agent-system-prompt.txt`. This doesn't need to change — just update the content.

- [ ] **Step 3: Pass OrchestratorSection to try_spawn_agent**

The `AgentRuntimeConfig` needs the orchestrator section. Either add it to the config struct or pass it separately. Simplest: add `orchestrator: Option<OrchestratorSection>` to `AgentRuntimeConfig`:

```rust
pub orchestrator: Option<OrchestratorSection>,
```

And populate it when building the config from `AgentSection`.

- [ ] **Step 4: Build and verify**

Run: `cargo build`
Expected: Clean build

- [ ] **Step 5: Commit**

```bash
git add src/main.rs crates/glass_core/src/agent_runtime.rs crates/glass_core/src/config.rs
git commit -m "feat: orchestrator system prompt with PRD and checkpoint loading"
```

---

## Summary

After completing these 7 tasks, the core orchestrator loop works:

1. User enables orchestration (`Ctrl+Shift+O`)
2. PTY silence timer fires after 30 seconds of no output
3. Orchestrator captures last 100 lines of terminal context
4. Sends it to the Glass Agent as a `[TERMINAL_CONTEXT]` message
5. Glass Agent responds with text to type (or WAIT/CHECKPOINT)
6. Orchestrator types the response into the PTY
7. Claude Code receives it and continues working
8. Status bar shows `[orchestrating | iter #N]`
9. User typing auto-pauses orchestration
10. Stuck detection prevents infinite loops

**Not yet implemented** (Plans 2-4):
- OAuth usage tracking and auto-pause/resume
- Iteration logging (.glass/iterations.tsv)
- Context refresh cycle (GLASS_CHECKPOINT handling)
- Git-as-memory (commit before verify, revert on failure)
- Claude Code crash recovery
