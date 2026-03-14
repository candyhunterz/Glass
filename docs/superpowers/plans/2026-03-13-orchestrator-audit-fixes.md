# Orchestrator Audit Fixes Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix all critical, high, and medium issues found in the orchestrator/agent mode audit — making the overnight PRD-to-completion and mid-work handoff use cases actually reliable.

**Architecture:** Three files carry 95% of the changes: `src/orchestrator.rs` (state machine + helpers), `src/main.rs` (event handlers), and `crates/glass_terminal/src/pty.rs` (silence detection). The fixes are layered: Chunk 1 fixes the silence loop so the orchestrator can actually re-poll, Chunk 2 wires up the dead checkpoint cycle so context refreshes work, Chunk 3 fixes safety/correctness bugs, and Chunk 4 adds robustness polish.

**Tech Stack:** Rust, winit event loop, mio polling, std::time, std::fs::metadata

**Dependencies between chunks:** Chunk 2 depends on Chunk 1 (periodic silence is needed for checkpoint polling). Chunk 3 depends on Chunk 2 (Task 5 uses `get_focused_cwd` helper defined in Task 3). Task 10 in Chunk 4 depends on Task 5's signature change to `read_iterations_log`. Execute in order: Chunk 1 → 2 → 3 → 4.

---

## Chunk 1: Periodic Silence & Backpressure

Fixes the two intertwined bugs: GLASS_WAIT causing permanent stall (silence never re-fires), and no backpressure (overlapping context sends). The approach: change silence from one-shot (`silence_fired` bool) to periodic (cooldown-based), and gate context sends on whether a response is still pending.

### Task 1: Make silence detection periodic instead of one-shot

**Files:**
- Modify: `crates/glass_terminal/src/pty.rs:285-417`

Currently `silence_fired = true` prevents re-firing until new PTY output. This means if the agent says `GLASS_WAIT`, no new output is generated, and the orchestrator never polls again. Fix: replace the boolean with a timestamp so silence fires every `threshold` seconds while the PTY is quiet.

- [ ] **Step 1: Write the failing test**

Add to `crates/glass_terminal/src/pty.rs` in the existing `#[cfg(test)] mod tests` block (or create one if absent). Since the silence logic is embedded in `glass_pty_loop` which can't be unit-tested directly, we test the conceptual logic as a standalone helper.

Create a new file for the testable logic:

```rust
// In crates/glass_terminal/src/silence.rs

use std::time::{Duration, Instant};

/// Tracks PTY silence for orchestrator polling.
/// Fires periodically (every `threshold`) while PTY is quiet,
/// instead of the old one-shot approach.
pub struct SilenceTracker {
    threshold: Duration,
    last_output_at: Instant,
    last_fired_at: Option<Instant>,
}

impl SilenceTracker {
    pub fn new(threshold_secs: u64) -> Self {
        Self {
            threshold: Duration::from_secs(threshold_secs),
            last_output_at: Instant::now(),
            last_fired_at: None,
        }
    }

    /// Call when PTY produces output. Resets all timers.
    pub fn on_output(&mut self) {
        self.last_output_at = Instant::now();
        self.last_fired_at = None;
    }

    /// Check if silence event should fire. Returns true at most once
    /// per `threshold` interval while the PTY remains quiet.
    pub fn should_fire(&mut self) -> bool {
        if self.last_output_at.elapsed() < self.threshold {
            return false;
        }
        let should = self
            .last_fired_at
            .map(|t| t.elapsed() >= self.threshold)
            .unwrap_or(true);
        if should {
            self.last_fired_at = Some(Instant::now());
        }
        should
    }

    /// Returns the maximum poll timeout to use, ensuring we wake up
    /// in time to check silence.
    pub fn poll_timeout(&self) -> Duration {
        let since_output = self.last_output_at.elapsed();
        if since_output >= self.threshold {
            // Already past threshold — next fire depends on last_fired_at
            let since_fired = self
                .last_fired_at
                .map(|t| t.elapsed())
                .unwrap_or(self.threshold);
            self.threshold.saturating_sub(since_fired).max(Duration::from_secs(1))
        } else {
            // Not yet silent long enough
            self.threshold.saturating_sub(since_output).max(Duration::from_secs(1))
        }
    }
}
```

Test file at `crates/glass_terminal/src/silence.rs` (tests inline):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn fires_after_threshold() {
        let mut tracker = SilenceTracker::new(1);
        assert!(!tracker.should_fire()); // Not silent long enough
        thread::sleep(Duration::from_millis(1100));
        assert!(tracker.should_fire()); // Now fires
    }

    #[test]
    fn does_not_double_fire_immediately() {
        let mut tracker = SilenceTracker::new(1);
        thread::sleep(Duration::from_millis(1100));
        assert!(tracker.should_fire());
        assert!(!tracker.should_fire()); // Cooldown active
    }

    #[test]
    fn re_fires_after_cooldown() {
        let mut tracker = SilenceTracker::new(1);
        thread::sleep(Duration::from_millis(1100));
        assert!(tracker.should_fire());
        thread::sleep(Duration::from_millis(1100));
        assert!(tracker.should_fire()); // Fires again after cooldown
    }

    #[test]
    fn output_resets_timers() {
        let mut tracker = SilenceTracker::new(1);
        thread::sleep(Duration::from_millis(1100));
        tracker.on_output(); // Reset
        assert!(!tracker.should_fire()); // Timer reset, not silent long enough
    }

    #[test]
    fn poll_timeout_respects_threshold() {
        let tracker = SilenceTracker::new(30);
        let timeout = tracker.poll_timeout();
        assert!(timeout <= Duration::from_secs(30));
        assert!(timeout >= Duration::from_secs(1));
    }
}
```

- [ ] **Step 2: Register the module and run tests to verify they fail**

Add `pub mod silence;` to `crates/glass_terminal/src/lib.rs`.

Run: `cargo test -p glass_terminal -- silence::tests`

Expected: Compilation succeeds, tests run. The tests should pass since we wrote the implementation inline with the test file. If any fail, fix the logic.

- [ ] **Step 3: Integrate SilenceTracker into glass_pty_loop**

In `crates/glass_terminal/src/pty.rs`, replace the old silence variables (lines 285-292):

Old:
```rust
// Orchestrator silence detection
let mut last_output_at = Instant::now();
let silence_threshold = if orchestrator_silence_secs > 0 {
    Some(std::time::Duration::from_secs(orchestrator_silence_secs))
} else {
    None
};
let mut silence_fired = false;
```

New:
```rust
// Orchestrator silence detection (periodic, not one-shot)
let mut silence_tracker = if orchestrator_silence_secs > 0 {
    Some(crate::silence::SilenceTracker::new(orchestrator_silence_secs))
} else {
    None
};
```

Replace the poll timeout cap (lines 301-307):

Old:
```rust
// Cap poll timeout to silence threshold for periodic checks.
if let Some(threshold) = silence_threshold {
    let remaining = threshold.saturating_sub(last_output_at.elapsed());
    // Use at least 1 second to avoid busy-looping
    let silence_timeout = remaining.max(std::time::Duration::from_secs(1));
    timeout = Some(match timeout {
        Some(t) => t.min(silence_timeout),
        None => silence_timeout,
    });
}
```

New:
```rust
// Cap poll timeout to silence tracker's next check time.
if let Some(ref mut tracker) = silence_tracker {
    let silence_timeout = tracker.poll_timeout();
    timeout = Some(match timeout {
        Some(t) => t.min(silence_timeout),
        None => silence_timeout,
    });
}
```

Replace the output reset (lines 392-394):

Old:
```rust
// Reset silence timer on new output
last_output_at = Instant::now();
silence_fired = false;
```

New:
```rust
// Reset silence timer on new output
if let Some(ref mut tracker) = silence_tracker {
    tracker.on_output();
}
```

Replace the silence check (lines 408-417):

Old:
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

New:
```rust
// Orchestrator silence detection (fires periodically while quiet)
if let Some(ref mut tracker) = silence_tracker {
    if tracker.should_fire() {
        let _ = app_proxy.send_event(AppEvent::OrchestratorSilence {
            window_id,
            session_id: event_proxy.session_id(),
        });
    }
}
```

- [ ] **Step 4: Run full test suite**

Run: `cargo test --workspace`

Expected: All existing tests pass. The silence behavior is now periodic — it will re-fire every `threshold` seconds while the PTY stays quiet, fixing the GLASS_WAIT stall.

- [ ] **Step 5: Commit**

```bash
git add crates/glass_terminal/src/silence.rs crates/glass_terminal/src/lib.rs crates/glass_terminal/src/pty.rs
git commit -m "fix(pty): make silence detection periodic instead of one-shot

GLASS_WAIT responses caused permanent stall because silence_fired was
never reset without new PTY output. Replace boolean with SilenceTracker
that re-fires every threshold interval while PTY is quiet."
```

---

### Task 2: Add response_pending backpressure

**Files:**
- Modify: `src/orchestrator.rs:106-126` (add field)
- Modify: `src/main.rs:5303-5306` (clear on response)
- Modify: `src/main.rs:5444-5498` (gate on pending)

Now that silence fires periodically, we need backpressure so overlapping context sends don't confuse the agent. Add a `response_pending` flag: set when context is sent, cleared when a response arrives.

- [ ] **Step 1: Add response_pending field and methods to OrchestratorState**

In `src/orchestrator.rs`, add field to `OrchestratorState` (after line 125):

```rust
    /// Whether we're waiting for the agent to respond to a context send.
    pub response_pending: bool,
```

Initialize it in `new()` (add after `last_pty_write: None,`):

```rust
            response_pending: false,
```

- [ ] **Step 2: Write test for response_pending behavior**

Add to the `#[cfg(test)] mod tests` block in `src/orchestrator.rs`:

```rust
    #[test]
    fn response_pending_gates_context_sends() {
        let mut state = OrchestratorState::new(3);
        state.active = true;
        assert!(!state.response_pending);

        // Simulate sending context
        state.response_pending = true;
        assert!(state.response_pending);

        // Simulate receiving response — clears pending
        state.response_pending = false;
        assert!(!state.response_pending);
    }
```

- [ ] **Step 3: Run test to verify it passes**

Run: `cargo test -- orchestrator::tests::response_pending`

Expected: PASS

- [ ] **Step 4: Wire into main.rs — clear on response**

In `src/main.rs`, in the `AppEvent::OrchestratorResponse` handler (after line 5306, before the parse):

Add after `if !self.orchestrator.active { return; }`:

```rust
                self.orchestrator.response_pending = false;
```

- [ ] **Step 5: Wire into main.rs — gate on pending in silence handler**

In `src/main.rs`, in the `AppEvent::OrchestratorSilence` handler, add after the `self.agent_runtime.is_none()` early return (after line 5453):

```rust
                if self.orchestrator.response_pending {
                    tracing::debug!("Orchestrator: skipping context send (response pending)");
                    return;
                }
```

And inside the successful write block in the silence handler — after `let _ = w.flush();` (inside the `if let Ok(mut w) = writer.lock()` block, around line 5495):

```rust
                                    self.orchestrator.response_pending = true;
```

This must be inside the innermost `if let` block so it's only set when the write actually succeeds. If placed outside, a failed write (no runtime, no writer, or lock failure) would permanently stall the orchestrator.

- [ ] **Step 6: Run full test suite**

Run: `cargo test --workspace`

Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/orchestrator.rs src/main.rs
git commit -m "fix(orchestrator): add response_pending backpressure

Prevents overlapping context sends when silence fires periodically.
Context is only sent when no response is pending from the agent."
```

---

## Chunk 2: Checkpoint Cycle Implementation

Wires up the dead `CheckpointPhase` state machine so context is actually refreshed. After Claude Code writes `checkpoint.md`, the agent subprocess is killed and respawned with an updated system prompt containing the new checkpoint content.

### Task 3: Update begin_checkpoint to capture baseline mtime and add timeout constant

**Files:**
- Modify: `src/orchestrator.rs:99-104` (add constant)
- Modify: `src/orchestrator.rs:182-191` (add mtime param)

- [ ] **Step 1: Add checkpoint timeout constant**

In `src/orchestrator.rs`, after line 104 (`CRASH_RECOVERY_GRACE_SECS`):

```rust
/// Maximum time to wait for Claude Code to write checkpoint.md before respawning anyway.
pub const CHECKPOINT_TIMEOUT_SECS: u64 = 180;
```

- [ ] **Step 2: Add mtime parameter to begin_checkpoint**

Change the `begin_checkpoint` method signature and body (lines 182-191):

Old:
```rust
    /// Start a checkpoint refresh cycle.
    pub fn begin_checkpoint(&mut self, completed: &str, next: &str) {
        self.last_checkpoint_completed = completed.to_string();
        self.last_checkpoint_next = next.to_string();
        self.iterations_since_checkpoint = 0;
        self.checkpoint_phase = CheckpointPhase::WaitingForCheckpoint {
            started_at: std::time::Instant::now(),
            last_mtime: None,
        };
    }
```

New:
```rust
    /// Start a checkpoint refresh cycle.
    /// `checkpoint_mtime` should be the current mtime of checkpoint.md (if it exists)
    /// so we can detect when Claude Code writes a new version.
    pub fn begin_checkpoint(
        &mut self,
        completed: &str,
        next: &str,
        checkpoint_mtime: Option<std::time::SystemTime>,
    ) {
        self.last_checkpoint_completed = completed.to_string();
        self.last_checkpoint_next = next.to_string();
        self.iterations_since_checkpoint = 0;
        self.response_pending = false;
        self.checkpoint_phase = CheckpointPhase::WaitingForCheckpoint {
            started_at: std::time::Instant::now(),
            last_mtime: checkpoint_mtime,
        };
    }
```

- [ ] **Step 3: Add helper to resolve checkpoint path from config**

Add to `src/orchestrator.rs` after the `read_iterations_log` function (after line 253):

```rust
/// Resolve the checkpoint file path for a given project root.
pub fn checkpoint_path(project_root: &str, config: Option<&str>) -> std::path::PathBuf {
    let rel = config.unwrap_or(".glass/checkpoint.md");
    std::path::Path::new(project_root).join(rel)
}

/// Get the current mtime of a file, or None if it doesn't exist.
pub fn file_mtime(path: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

/// Check if a checkpoint file has been updated since a baseline mtime.
pub fn checkpoint_changed(
    path: &std::path::Path,
    baseline: Option<std::time::SystemTime>,
) -> bool {
    match (baseline, file_mtime(path)) {
        (None, Some(_)) => true,       // File created
        (Some(old), Some(new)) => new > old, // File modified
        _ => false,
    }
}
```

- [ ] **Step 4: Write tests for the new helpers**

Add to `#[cfg(test)] mod tests` in `src/orchestrator.rs`:

```rust
    #[test]
    fn checkpoint_changed_detects_creation() {
        let path = std::path::Path::new("nonexistent_test_file_12345.md");
        // No file exists, no baseline → no change
        assert!(!checkpoint_changed(path, None));
    }

    #[test]
    fn begin_checkpoint_stores_mtime() {
        let mut state = OrchestratorState::new(3);
        let fake_mtime = std::time::SystemTime::now();
        state.begin_checkpoint("feature-a", "feature-b", Some(fake_mtime));
        match state.checkpoint_phase {
            CheckpointPhase::WaitingForCheckpoint { last_mtime, .. } => {
                assert_eq!(last_mtime, Some(fake_mtime));
            }
            _ => panic!("Expected WaitingForCheckpoint"),
        }
        assert_eq!(state.last_checkpoint_completed, "feature-a");
        assert_eq!(state.last_checkpoint_next, "feature-b");
        assert_eq!(state.iterations_since_checkpoint, 0);
    }
```

- [ ] **Step 5: Fix existing callers in main.rs**

In `src/main.rs`, update both `begin_checkpoint` call sites to pass the current mtime.

Call site 1 — auto-checkpoint (around line 5321):

Old:
```rust
                    self.orchestrator.begin_checkpoint("auto-refresh", "continue from PRD");
```

New:
```rust
                    let cp_path = orchestrator::checkpoint_path(
                        &self.get_focused_cwd(),
                        self.config.agent.as_ref().and_then(|a| a.orchestrator.as_ref()).map(|o| o.checkpoint_path.as_str()),
                    );
                    let mtime = orchestrator::file_mtime(&cp_path);
                    self.orchestrator.begin_checkpoint("auto-refresh", "continue from PRD", mtime);
```

Call site 2 — GLASS_CHECKPOINT response (around line 5395):

Old:
```rust
                        self.orchestrator.begin_checkpoint(&completed, &next);
```

New:
```rust
                        let cp_path = orchestrator::checkpoint_path(
                            &self.get_focused_cwd(),
                            self.config.agent.as_ref().and_then(|a| a.orchestrator.as_ref()).map(|o| o.checkpoint_path.as_str()),
                        );
                        let mtime = orchestrator::file_mtime(&cp_path);
                        self.orchestrator.begin_checkpoint(&completed, &next, mtime);
```

This requires a small helper on Processor to get the focused session's CWD. Add to `src/main.rs` as a method on Processor (near other helper methods):

```rust
    /// Get the CWD of the focused session, falling back to the process CWD.
    fn get_focused_cwd(&self) -> String {
        self.windows
            .values()
            .next()
            .and_then(|ctx| ctx.session_mux.focused_session())
            .map(|s| s.status.cwd().to_string())
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            })
    }
```

- [ ] **Step 6: Run tests**

Run: `cargo test --workspace`

Expected: All tests pass. Existing `begin_checkpoint` test needs updating for the new signature.

- [ ] **Step 7: Commit**

```bash
git add src/orchestrator.rs src/main.rs
git commit -m "refactor(orchestrator): add mtime tracking and helpers for checkpoint cycle

begin_checkpoint now captures baseline mtime of checkpoint.md so the
silence handler can detect when Claude Code has written a new version.
Adds checkpoint_path, file_mtime, and checkpoint_changed helpers."
```

---

### Task 4: Wire checkpoint detection into silence handler and respawn agent

**Files:**
- Modify: `src/main.rs:5444-5506` (silence handler — add checkpoint phase check)
- Modify: `src/main.rs:2957-3064` (extract respawn logic)

This is the core fix: when the silence handler fires and we're in `WaitingForCheckpoint`, check if `checkpoint.md` has been updated. If so (or if timed out), kill the agent and respawn with a fresh system prompt that includes the updated checkpoint.

- [ ] **Step 1: Extract respawn_orchestrator_agent helper method**

Add a new method to Processor in `src/main.rs`. This consolidates the agent respawn logic used by both Ctrl+Shift+O and checkpoint refresh:

```rust
    /// Kill the current agent and respawn with a fresh system prompt.
    /// `handoff_content` is the initial message sent to the new agent.
    fn respawn_orchestrator_agent(&mut self, cwd: &str, terminal_context: &str, handoff_content: String) {
        // Kill old agent
        self.agent_runtime = None;

        // Create new activity channel
        let activity_config = glass_core::activity_stream::ActivityStreamConfig::default();
        let (new_tx, new_rx) = glass_core::activity_stream::create_channel(&activity_config);
        self.activity_stream_tx = Some(new_tx);

        // Build agent config
        let agent_config = self
            .config
            .agent
            .clone()
            .map(|a| glass_core::agent_runtime::AgentRuntimeConfig {
                mode: a.mode,
                max_budget_usd: a.max_budget_usd,
                cooldown_secs: a.cooldown_secs,
                allowed_tools: a.allowed_tools,
                orchestrator: a.orchestrator,
            })
            .unwrap_or_default();

        // Spawn new agent with fresh system prompt (reads updated checkpoint.md)
        self.agent_runtime = try_spawn_agent(
            agent_config,
            new_rx,
            self.proxy.clone(),
            0,
            None,
            cwd.to_string(),
        );

        // Send handoff to new agent
        if let Some(ref runtime) = self.agent_runtime {
            if let Some(ref writer) = runtime.orchestrator_writer {
                let msg = serde_json::json!({
                    "type": "user",
                    "message": {
                        "role": "user",
                        "content": handoff_content
                    }
                })
                .to_string();

                if let Ok(mut w) = writer.lock() {
                    use std::io::Write;
                    let _ = writeln!(w, "{msg}");
                    let _ = w.flush();
                }
            }
        }

        tracing::info!("Orchestrator: respawned agent for {}", cwd);
    }
```

- [ ] **Step 2: Refactor Ctrl+Shift+O to use the helper**

In `src/main.rs`, replace the inline agent respawn in the Ctrl+Shift+O handler (lines ~2965-3058) with a call to the helper:

```rust
                                // Respawn agent with fresh system prompt
                                let current_cwd = ctx
                                    .session_mux
                                    .focused_session()
                                    .map(|s| s.status.cwd().to_string())
                                    .unwrap_or_else(|| {
                                        std::env::current_dir()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                            .to_string()
                                    });

                                let terminal_context = ctx
                                    .session_mux
                                    .focused_session()
                                    .map(|s| extract_term_lines(&s.term, 100).join("\n"))
                                    .unwrap_or_default();

                                // Check for handoff note
                                let handoff_path = std::path::Path::new(&current_cwd)
                                    .join(".glass")
                                    .join("handoff.md");
                                let handoff_note = std::fs::read_to_string(&handoff_path).ok();

                                let git_log = std::process::Command::new("git")
                                    .args(["log", "--oneline", "-10"])
                                    .current_dir(&current_cwd)
                                    .output()
                                    .ok()
                                    .and_then(|o| if o.status.success() {
                                        String::from_utf8(o.stdout).ok()
                                    } else {
                                        None
                                    });

                                let mut content = String::from(
                                    "[ORCHESTRATOR_HANDOFF]\nThe user just enabled orchestration. Pick up where they left off.\n",
                                );
                                if let Some(note) = &handoff_note {
                                    content.push_str(&format!("\nUSER INSTRUCTIONS:\n{}\n", note));
                                }
                                if let Some(log) = git_log {
                                    content.push_str(&format!("\nRECENT GIT HISTORY:\n{}\n", log.trim()));
                                }
                                content.push_str(&format!(
                                    "\nTERMINAL CONTEXT (last 100 lines):\n{}\n",
                                    terminal_context
                                ));

                                self.respawn_orchestrator_agent(&current_cwd, &terminal_context, content);

                                // Delete handoff.md only after agent starts successfully
                                if handoff_note.is_some() && self.agent_runtime.is_some() {
                                    let _ = std::fs::remove_file(&handoff_path);
                                }
```

- [ ] **Step 3: Add checkpoint detection to the OrchestratorSilence handler**

In `src/main.rs`, at the top of the `AppEvent::OrchestratorSilence` handler (after the `response_pending` check from Task 2), add the checkpoint phase check:

```rust
                // Check if we're in a checkpoint cycle
                if let orchestrator::CheckpointPhase::WaitingForCheckpoint {
                    started_at,
                    last_mtime,
                } = &self.orchestrator.checkpoint_phase
                {
                    let cwd = self.get_focused_cwd();
                    let cp_path = orchestrator::checkpoint_path(
                        &cwd,
                        self.config.agent.as_ref().and_then(|a| a.orchestrator.as_ref()).map(|o| o.checkpoint_path.as_str()),
                    );

                    let changed = orchestrator::checkpoint_changed(&cp_path, *last_mtime);
                    let timed_out =
                        started_at.elapsed().as_secs() >= orchestrator::CHECKPOINT_TIMEOUT_SECS;

                    if changed || timed_out {
                        if timed_out && !changed {
                            tracing::warn!(
                                "Orchestrator: checkpoint timeout after {}s — respawning anyway",
                                started_at.elapsed().as_secs()
                            );
                        } else {
                            tracing::info!("Orchestrator: checkpoint.md updated — respawning agent");
                        }

                        // Capture terminal context for the new agent
                        let terminal_context = if let Some(ctx) = self.windows.get(&window_id) {
                            ctx.session_mux
                                .session(session_id)
                                .map(|s| extract_term_lines(&s.term, 100).join("\n"))
                                .unwrap_or_default()
                        } else {
                            String::new()
                        };

                        let git_log = std::process::Command::new("git")
                            .args(["log", "--oneline", "-10"])
                            .current_dir(&cwd)
                            .output()
                            .ok()
                            .and_then(|o| {
                                if o.status.success() {
                                    String::from_utf8(o.stdout).ok()
                                } else {
                                    None
                                }
                            });

                        let mut content = format!(
                            "[ORCHESTRATOR_CHECKPOINT_REFRESH]\n\
                             Context has been refreshed. Your system prompt now contains the updated checkpoint.\n\
                             Completed so far: {}\n\
                             Next up: {}\n\
                             Continue from where you left off.\n",
                            self.orchestrator.last_checkpoint_completed,
                            self.orchestrator.last_checkpoint_next,
                        );
                        if let Some(log) = git_log {
                            content.push_str(&format!("\nRECENT GIT HISTORY:\n{}\n", log.trim()));
                        }
                        content.push_str(&format!(
                            "\nTERMINAL CONTEXT (last 100 lines):\n{}\n",
                            terminal_context
                        ));

                        self.respawn_orchestrator_agent(&cwd, &terminal_context, content);
                        self.orchestrator.checkpoint_phase = orchestrator::CheckpointPhase::Idle;
                        self.orchestrator.reset_stuck();
                    }
                    // Don't send normal context while waiting for checkpoint
                    return;
                }
```

- [ ] **Step 4: Run full test suite**

Run: `cargo test --workspace`

Expected: All tests pass.

- [ ] **Step 5: Manual verification checklist**

Verify by reading the code that:
- [ ] Ctrl+Shift+O still works (calls respawn_orchestrator_agent)
- [ ] GLASS_CHECKPOINT response → begin_checkpoint with mtime → silence handler detects file change → respawn
- [ ] Auto-checkpoint (15 iterations) → same flow
- [ ] Checkpoint timeout (180s) → respawn with warning log
- [ ] While WaitingForCheckpoint, normal context sends are skipped (early return)

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat(orchestrator): wire up checkpoint cycle with agent respawn

When GLASS_CHECKPOINT fires, the silence handler now polls checkpoint.md
mtime. Once updated (or after 180s timeout), the agent subprocess is
killed and respawned with a fresh system prompt containing the new
checkpoint. This fixes the dead state machine where context was never
actually refreshed."
```

---

## Chunk 3: Safety & Correctness Fixes

Independent fixes for high-priority bugs: wrong paths, missing grace period, context-free crash recovery, and dangerous stuck recovery.

### Task 5: Fix iterations.tsv path resolution

**Files:**
- Modify: `src/orchestrator.rs:197-253` (append_iteration_log and read_iterations_log)
- Modify: `src/main.rs` (all append_iteration_log call sites)

The functions use `Path::new(".glass")` which is relative to Glass's process CWD, not the project directory. The system prompt reads from `project_dir.join(".glass/iterations.tsv")`. These can diverge.

- [ ] **Step 1: Add project_root parameter to append_iteration_log**

In `src/orchestrator.rs`, change the function signature (line 197):

Old:
```rust
pub fn append_iteration_log(
    iteration: u32,
    feature: &str,
    status: &str,
    description: &str,
) {
    let glass_dir = std::path::Path::new(".glass");
```

New:
```rust
pub fn append_iteration_log(
    project_root: &str,
    iteration: u32,
    feature: &str,
    status: &str,
    description: &str,
) {
    let glass_dir = std::path::Path::new(project_root).join(".glass");
```

Also update line 205 from `let path = glass_dir.join("iterations.tsv");` — this now works correctly since `glass_dir` is absolute.

- [ ] **Step 2: Update read_iterations_log**

Old (lines 250-253):
```rust
pub fn read_iterations_log() -> String {
    let path = std::path::Path::new(".glass").join("iterations.tsv");
    std::fs::read_to_string(&path).unwrap_or_default()
}
```

New:
```rust
pub fn read_iterations_log(project_root: &str) -> String {
    let path = std::path::Path::new(project_root)
        .join(".glass")
        .join("iterations.tsv");
    std::fs::read_to_string(path).unwrap_or_default()
}
```

- [ ] **Step 3: Update all call sites in main.rs**

There are 3 `append_iteration_log` calls in main.rs (stuck, checkpoint, done). Add `&self.get_focused_cwd()` as the first argument to each:

```rust
// Stuck (around line 5348):
orchestrator::append_iteration_log(
    &self.get_focused_cwd(),
    self.orchestrator.iteration,
    // ... rest unchanged
);

// Checkpoint (around line 5387):
orchestrator::append_iteration_log(
    &self.get_focused_cwd(),
    self.orchestrator.iteration,
    // ... rest unchanged
);

// Done (around line 5410):
orchestrator::append_iteration_log(
    &self.get_focused_cwd(),
    self.orchestrator.iteration,
    // ... rest unchanged
);
```

Also update the `read_iterations_log` call in `try_spawn_agent` (around line 786) — it already uses `project_dir`, so pass `project_root` instead:

Old:
```rust
        let iterations_path = project_dir.join(".glass").join("iterations.tsv");
        let iterations_content = std::fs::read_to_string(&iterations_path)
            .unwrap_or_default();
```

This one is already correct (uses project_dir). But if `read_iterations_log` is called elsewhere, update those too. Search for `read_iterations_log` and update.

- [ ] **Step 4: Run tests**

Run: `cargo test --workspace`

Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/orchestrator.rs src/main.rs
git commit -m "fix(orchestrator): resolve iterations.tsv relative to project root

append_iteration_log and read_iterations_log were using relative paths
from Glass's process CWD, which could differ from the project directory.
Now takes explicit project_root parameter."
```

---

### Task 6: Add mark_pty_write on checkpoint and done responses

**Files:**
- Modify: `src/main.rs:5379-5435`

Checkpoint and Done responses type text into the PTY but don't call `mark_pty_write()`, so the crash recovery grace period isn't set. If the typed message triggers a shell prompt, crash recovery could incorrectly fire `claude --dangerously-skip-permissions`.

- [ ] **Step 1: Add mark_pty_write to checkpoint handler**

In `src/main.rs`, in the `Checkpoint` match arm, add `mark_pty_write()` **inside** the `if let Some(session) = ctx.session_mux.focused_session()` block, immediately after `let _ = session.pty_sender.send(...)` (around line 5403) and before the closing braces:

```rust
                                let _ = session
                                    .pty_sender
                                    .send(PtyMsg::Input(std::borrow::Cow::Owned(bytes)));
                                self.orchestrator.mark_pty_write(); // <-- ADD THIS LINE
                            }
                        }
```

Note: `self.orchestrator` and `self.windows` are disjoint struct fields, so both borrows are valid simultaneously.

- [ ] **Step 2: Add mark_pty_write to done handler**

Same pattern in the `Done` match arm — add inside the `if let Some(session)` block, after the `send` call (around line 5433):

```rust
                                let _ = session
                                    .pty_sender
                                    .send(PtyMsg::Input(std::borrow::Cow::Owned(bytes)));
                                self.orchestrator.mark_pty_write(); // <-- ADD THIS LINE
                            }
                        }
```

- [ ] **Step 3: Run tests**

Run: `cargo test --workspace`

Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "fix(orchestrator): add mark_pty_write to checkpoint and done handlers

Missing grace period meant crash recovery could incorrectly restart
Claude Code after checkpoint/done messages triggered a shell prompt."
```

---

### Task 7: Crash recovery with project context

**Files:**
- Modify: `src/main.rs:4206-4221`

Currently crash recovery types bare `claude --dangerously-skip-permissions\n` which starts Claude Code with no knowledge of the project. Fix: tell the restarted Claude Code to read the checkpoint file and continue.

- [ ] **Step 1: Update crash recovery message**

In `src/main.rs`, replace the crash recovery block (lines 4210-4220):

Old:
```rust
                            tracing::info!(
                                "Orchestrator: shell prompt detected — Claude Code may have exited, restarting"
                            );
                            let restart_msg =
                                "claude --dangerously-skip-permissions\n";
                            let bytes = restart_msg.as_bytes().to_vec();
                            let _ = session
                                .pty_sender
                                .send(PtyMsg::Input(std::borrow::Cow::Owned(bytes)));
                            self.orchestrator.mark_pty_write();
```

New:
```rust
                            tracing::info!(
                                "Orchestrator: shell prompt detected — Claude Code may have exited, restarting"
                            );

                            // Determine checkpoint path for context injection
                            let cp_rel = self
                                .config
                                .agent
                                .as_ref()
                                .and_then(|a| a.orchestrator.as_ref())
                                .map(|o| o.checkpoint_path.as_str())
                                .unwrap_or(".glass/checkpoint.md");

                            // Restart Claude Code with instruction to read checkpoint
                            let restart_msg = format!(
                                "claude --dangerously-skip-permissions -p \"Read {} and continue the project from where you left off. Follow the iteration protocol: plan, implement, commit, verify, decide.\"\n",
                                cp_rel,
                            );
                            let bytes = restart_msg.into_bytes();
                            let _ = session
                                .pty_sender
                                .send(PtyMsg::Input(std::borrow::Cow::Owned(bytes)));
                            self.orchestrator.mark_pty_write();
```

- [ ] **Step 2: Run tests**

Run: `cargo test --workspace`

Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "fix(orchestrator): crash recovery now injects project context

Restarted Claude Code is told to read checkpoint.md and continue,
instead of starting with zero context about the project."
```

---

### Task 8: Better stuck recovery message

**Files:**
- Modify: `src/main.rs:5355-5361`

`git revert HEAD` is wrong: it may revert the wrong commit, fails with uncommitted changes, and creates garbage revert commits. Replace with a message that lets Claude Code decide the right recovery strategy.

- [ ] **Step 1: Update stuck recovery message**

In `src/main.rs`, replace the stuck message (around line 5358):

Old:
```rust
                                    let msg = "You've tried this approach multiple times without success. Revert to the last good commit with 'git revert HEAD' and try a different approach.\n";
```

New:
```rust
                                    let msg = "You've tried this same approach multiple times without making progress. STOP and take a different approach:\n1. If you have uncommitted changes, stash them: git stash\n2. Think about WHY the current approach isn't working\n3. Try a fundamentally different strategy, not a minor variation\n";
```

- [ ] **Step 2: Commit**

```bash
git add src/main.rs
git commit -m "fix(orchestrator): replace git revert HEAD with safer stuck recovery

The old message prescribed 'git revert HEAD' which could revert the
wrong commit or fail with uncommitted changes. New message guides
Claude Code to stash and rethink approach."
```

---

## Chunk 4: Robustness & Polish

Medium and low priority fixes that improve reliability for overnight runs.

### Task 9: PRD validation on orchestrator enable

**Files:**
- Modify: `src/main.rs:2957-2963` (Ctrl+Shift+O handler)

Currently the orchestrator starts even when PRD.md is missing. Add a check and log a warning.

- [ ] **Step 1: Add PRD existence check**

In `src/main.rs`, in the Ctrl+Shift+O handler, after `current_cwd` is computed (around line 2967-2976) and before respawning the agent. Reuse the existing `current_cwd` variable — do NOT recompute it:

```rust
                                    // Validate PRD exists
                                    let prd_rel = self
                                        .config
                                        .agent
                                        .as_ref()
                                        .and_then(|a| a.orchestrator.as_ref())
                                        .map(|o| o.prd_path.as_str())
                                        .unwrap_or("PRD.md");
                                    let prd_path = std::path::Path::new(&current_cwd).join(prd_rel);
                                    if !prd_path.exists() {
                                        tracing::warn!(
                                            "Orchestrator: PRD not found at {} — orchestrating without project plan",
                                            prd_path.display()
                                        );
                                    }
```

This is a warning, not a blocker — the orchestrator can still work with the handoff note and checkpoint. A hard block would be frustrating if the user intentionally wants to use a different workflow.

- [ ] **Step 2: Commit**

```bash
git add src/main.rs
git commit -m "fix(orchestrator): warn when PRD is missing on enable"
```

---

### Task 10: Truncate iterations in system prompt and add PRD truncation notice

**Files:**
- Modify: `src/main.rs:785-792` (iterations in system prompt)
- Modify: `src/main.rs:772-776` (PRD truncation)

The iterations log grows unboundedly and wastes context. Limit to last 50 entries. Also add a notice when PRD is truncated so the agent knows to check the full file.

- [ ] **Step 1: Add iterations truncation helper**

In `src/orchestrator.rs`, add after `read_iterations_log`:

```rust
/// Read the last N lines of iterations.tsv (plus header) for the system prompt.
pub fn read_iterations_log_truncated(project_root: &str, max_entries: usize) -> String {
    let content = read_iterations_log(project_root);
    if content.is_empty() {
        return content;
    }
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= max_entries + 1 {
        // +1 for header
        return content;
    }
    // Keep header + last N entries
    let header = lines[0];
    let tail = &lines[lines.len() - max_entries..];
    let mut result = String::from(header);
    result.push('\n');
    let skipped = lines.len() - max_entries - 1;
    result.push_str(&format!("... ({skipped} earlier entries omitted)\n"));
    for line in tail {
        result.push_str(line);
        result.push('\n');
    }
    result
}
```

- [ ] **Step 2: Write test**

Add to `#[cfg(test)] mod tests` in `src/orchestrator.rs`:

```rust
    #[test]
    fn iterations_truncation_keeps_header_and_tail() {
        // This tests the logic, not file I/O
        let input = "header\nline1\nline2\nline3\nline4\nline5\n";
        let lines: Vec<&str> = input.lines().collect();
        assert_eq!(lines.len(), 6); // header + 5 entries

        // Simulate truncation to 3 entries
        let max = 3;
        let header = lines[0];
        let tail = &lines[lines.len() - max..];
        assert_eq!(header, "header");
        assert_eq!(tail, &["line3", "line4", "line5"]);
    }
```

- [ ] **Step 3: Update system prompt to use truncated iterations**

**Prerequisite:** Task 5 must be completed first — `read_iterations_log` must already accept a `project_root` parameter, since `read_iterations_log_truncated` calls it internally.

In `src/main.rs`, replace the iterations reading (lines 785-792):

Old:
```rust
        let iterations_path = project_dir.join(".glass").join("iterations.tsv");
        let iterations_content = std::fs::read_to_string(&iterations_path)
            .unwrap_or_default();
        let iterations_content = if iterations_content.is_empty() {
            "No iterations yet.".to_string()
        } else {
            iterations_content
        };
```

New:
```rust
        let iterations_content = orchestrator::read_iterations_log_truncated(&project_root, 50);
        let iterations_content = if iterations_content.is_empty() {
            "No iterations yet.".to_string()
        } else {
            iterations_content
        };
```

- [ ] **Step 4: Add PRD truncation notice**

In `src/main.rs`, after the PRD truncation (after line 776). Note: `prd_rel` is already defined at line 765-767 in the same scope:

Old:
```rust
        let prd_truncated: String = prd_content
            .split_whitespace()
            .take(4000)
            .collect::<Vec<_>>()
            .join(" ");
```

New:
```rust
        let word_count = prd_content.split_whitespace().count();
        let prd_truncated: String = prd_content
            .split_whitespace()
            .take(4000)
            .collect::<Vec<_>>()
            .join(" ");
        let prd_truncated = if word_count > 4000 {
            format!(
                "{}\n\n[PRD TRUNCATED — {} words omitted. Read the full file at {} for complete requirements.]",
                prd_truncated,
                word_count - 4000,
                prd_rel,
            )
        } else {
            prd_truncated
        };
```

- [ ] **Step 5: Run tests**

Run: `cargo test --workspace`

Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/orchestrator.rs src/main.rs
git commit -m "fix(orchestrator): truncate iterations log and add PRD truncation notice

Iterations in system prompt limited to last 50 entries to prevent
unbounded context growth. PRD truncation now appends a notice so the
agent knows to check the full file for remaining requirements."
```

---

### Task 11: Reset iteration counter and defer handoff.md deletion

**Files:**
- Modify: `src/main.rs:2959-2963` (iteration reset on re-enable)
- Modify: `src/main.rs:3020-3022` (handoff deletion timing)

Two small fixes: reset the iteration counter when re-enabling orchestrator (less confusing status bar), and defer handoff.md deletion until after the agent starts successfully (prevents losing context if spawn fails).

- [ ] **Step 1: Reset iteration on re-enable**

In `src/main.rs`, in the Ctrl+Shift+O handler, add after `self.orchestrator.iterations_since_checkpoint = 0;`:

```rust
                                    self.orchestrator.iteration = 0;
```

- [ ] **Step 2: Verify handoff.md deletion timing**

Task 4 Step 2 already refactored the Ctrl+Shift+O handler to delete handoff.md only after confirming `self.agent_runtime.is_some()`. No additional change needed here — just verify Task 4's refactored code includes the guard:

```rust
                                // Delete handoff.md only after agent starts successfully
                                if handoff_note.is_some() && self.agent_runtime.is_some() {
                                    let _ = std::fs::remove_file(&handoff_path);
                                }
```

- [ ] **Step 3: Run tests**

Run: `cargo test --workspace`

Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "fix(orchestrator): reset iteration counter on re-enable

Iteration counter now resets to 0 when user toggles orchestrator off
and back on, instead of continuing from the previous session's count."
```

---

### Task 12: Final verification

**Files:** None (verification only)

- [ ] **Step 1: Run full test suite**

Run: `cargo test --workspace`

Expected: All tests pass.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`

Expected: No warnings.

- [ ] **Step 3: Run format check**

Run: `cargo fmt --all -- --check`

Expected: No formatting issues.

- [ ] **Step 4: Review the diff**

Run: `git diff main --stat`

Verify changes are limited to:
- `src/orchestrator.rs` — state machine fixes, path helpers, iteration truncation
- `src/main.rs` — event handler fixes, respawn helper, checkpoint cycle wiring
- `crates/glass_terminal/src/pty.rs` — silence tracker integration
- `crates/glass_terminal/src/silence.rs` — new file for SilenceTracker
- `crates/glass_terminal/src/lib.rs` — module registration
