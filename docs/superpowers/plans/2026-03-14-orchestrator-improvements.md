# Orchestrator Improvements Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the orchestrator loop agent-agnostic and more responsive: faster trigger detection, smarter stuck detection, and severity-based context windowing.

**Architecture:** Three isolated improvements wired into the existing orchestrator loop. SmartTrigger replaces SilenceTracker on the PTY thread. StateFingerprint and build_orchestrator_context are new functions in orchestrator.rs, consumed by the OrchestratorSilence handler in main.rs.

**Tech Stack:** Rust, regex crate (already in deps), std::hash::DefaultHasher, existing SOI/history DB infrastructure.

**Spec:** `docs/superpowers/specs/2026-03-14-orchestrator-improvements-design.md`

---

## Chunk 1: Config + SmartTrigger + PTY Wiring

### Task 1: Add Config Fields to OrchestratorSection

**Files:**
- Modify: `crates/glass_core/src/config.rs:117-146`

- [ ] **Step 1: Write tests for new config fields**

Add to the existing `tests` module in `config.rs`:

```rust
#[test]
fn test_orchestrator_section_new_fields_defaults() {
    let toml = "[agent.orchestrator]\nenabled = true";
    let config = GlassConfig::load_from_str(toml);
    let orch = config.agent.unwrap().orchestrator.unwrap();
    assert_eq!(orch.fast_trigger_secs, 5);
    assert!(orch.agent_prompt_pattern.is_none());
}

#[test]
fn test_orchestrator_section_new_fields_custom() {
    let toml = r#"[agent.orchestrator]
enabled = true
fast_trigger_secs = 3
agent_prompt_pattern = "^❯""#;
    let config = GlassConfig::load_from_str(toml);
    let orch = config.agent.unwrap().orchestrator.unwrap();
    assert_eq!(orch.fast_trigger_secs, 3);
    assert_eq!(orch.agent_prompt_pattern.as_deref(), Some("^❯"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --package glass_core test_orchestrator_section_new_fields`
Expected: FAIL — fields don't exist yet.

- [ ] **Step 3: Add fields to OrchestratorSection and default functions**

In `crates/glass_core/src/config.rs`, add to `OrchestratorSection` struct (after `max_retries_before_stuck`):

```rust
/// Seconds after output stops before fast-triggering the orchestrator. Default 5.
#[serde(default = "default_orch_fast_trigger")]
pub fast_trigger_secs: u64,
/// Optional regex pattern to detect the agent's prompt for instant triggering.
#[serde(default)]
pub agent_prompt_pattern: Option<String>,
```

Add the default function (after `default_orch_max_retries`):

```rust
fn default_orch_fast_trigger() -> u64 {
    5
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --package glass_core test_orchestrator_section`
Expected: All orchestrator config tests PASS (including existing ones).

- [ ] **Step 5: Run clippy**

Run: `cargo clippy --package glass_core -- -D warnings`
Expected: No warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/glass_core/src/config.rs
git commit -m "feat(config): add fast_trigger_secs and agent_prompt_pattern to orchestrator config"
```

---

### Task 2: Replace SilenceTracker with SmartTrigger

**Files:**
- Modify: `crates/glass_terminal/src/silence.rs` (full rewrite)

- [ ] **Step 1: Write failing tests for SmartTrigger**

Replace the entire test module in `silence.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn fires_on_prompt_detected() {
        let mut trigger = SmartTrigger::new(30, 5, Some("^❯".to_string()));
        assert!(!trigger.should_fire());
        trigger.on_output_bytes(b"some output\n\xe2\x9d\xaf ");
        assert!(trigger.should_fire());
        // Clears after firing
        assert!(!trigger.should_fire());
    }

    #[test]
    fn fires_on_shell_prompt() {
        let mut trigger = SmartTrigger::new(30, 5, None);
        trigger.on_shell_prompt();
        assert!(trigger.should_fire());
        assert!(!trigger.should_fire());
    }

    #[test]
    fn fires_on_fast_trigger_after_output_stops() {
        let mut trigger = SmartTrigger::new(30, 1, None);
        trigger.on_output_bytes(b"output");
        assert!(!trigger.should_fire());
        thread::sleep(Duration::from_millis(1100));
        assert!(trigger.should_fire());
        // was_output_flowing cleared — fast trigger won't fire again
        assert!(!trigger.should_fire());
    }

    #[test]
    fn fires_on_slow_fallback() {
        let mut trigger = SmartTrigger::new(1, 5, None);
        assert!(!trigger.should_fire());
        thread::sleep(Duration::from_millis(1100));
        assert!(trigger.should_fire());
    }

    #[test]
    fn output_resets_timers() {
        let mut trigger = SmartTrigger::new(1, 1, None);
        thread::sleep(Duration::from_millis(1100));
        trigger.on_output_bytes(b"output");
        assert!(!trigger.should_fire());
    }

    #[test]
    fn no_prompt_regex_means_no_prompt_detection() {
        let mut trigger = SmartTrigger::new(30, 5, None);
        trigger.on_output_bytes(b"\xe2\x9d\xaf ");
        // No regex configured, so prompt detection doesn't fire
        assert!(!trigger.should_fire());
    }

    #[test]
    fn poll_timeout_respects_fast_threshold() {
        let mut trigger = SmartTrigger::new(30, 5, None);
        trigger.on_output_bytes(b"output");
        let timeout = trigger.poll_timeout();
        // Should be <= fast_threshold (5s), not the slow 30s
        assert!(timeout <= Duration::from_secs(5));
        assert!(timeout >= Duration::from_secs(1));
    }

    #[test]
    fn slow_fallback_fires_periodically() {
        let mut trigger = SmartTrigger::new(1, 5, None);
        thread::sleep(Duration::from_millis(1100));
        assert!(trigger.should_fire());
        // Shouldn't double-fire immediately
        assert!(!trigger.should_fire());
        // Should fire again after another threshold
        thread::sleep(Duration::from_millis(1100));
        assert!(trigger.should_fire());
    }

    #[test]
    fn invalid_regex_is_ignored() {
        // Invalid regex pattern should not panic, just disable prompt detection
        let mut trigger = SmartTrigger::new(30, 5, Some("[invalid".to_string()));
        trigger.on_output_bytes(b"some output");
        assert!(!trigger.should_fire());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --package glass_terminal silence`
Expected: FAIL — `SmartTrigger` doesn't exist.

- [ ] **Step 3: Implement SmartTrigger**

Replace the entire contents of `crates/glass_terminal/src/silence.rs` with:

```rust
use regex::Regex;
use std::time::{Duration, Instant};

/// Smart orchestrator trigger that fires on the fastest available signal.
///
/// Priority order:
/// 1. Prompt regex match (instant)
/// 2. Shell prompt returned / OSC 133;A (instant)
/// 3. Output velocity drop (fast_threshold seconds after output stops)
/// 4. Fixed silence threshold (slow fallback, periodic)
pub struct SmartTrigger {
    /// Slow fallback threshold (existing behavior).
    threshold: Duration,
    /// Fast trigger threshold (after output stops).
    fast_threshold: Duration,
    /// Last time PTY produced output.
    last_output_at: Instant,
    /// Last time should_fire() returned true (for periodic slow fallback).
    last_fired_at: Option<Instant>,
    /// Compiled prompt regex (None if not configured or invalid).
    prompt_regex: Option<Regex>,
    /// Latch: set on output, cleared when fast trigger fires.
    was_output_flowing: bool,
    /// Set when prompt regex matches end of output.
    prompt_detected: bool,
    /// Set when OSC 133;A (shell prompt) received.
    shell_prompt_returned: bool,
}

impl SmartTrigger {
    pub fn new(
        threshold_secs: u64,
        fast_trigger_secs: u64,
        prompt_pattern: Option<String>,
    ) -> Self {
        let prompt_regex = prompt_pattern.and_then(|p| {
            Regex::new(&p)
                .map_err(|e| tracing::warn!("Invalid orchestrator prompt regex '{p}': {e}"))
                .ok()
        });
        Self {
            threshold: Duration::from_secs(threshold_secs),
            fast_threshold: Duration::from_secs(fast_trigger_secs),
            last_output_at: Instant::now(),
            last_fired_at: None,
            prompt_regex,
            was_output_flowing: false,
            prompt_detected: false,
            shell_prompt_returned: false,
        }
    }

    /// Call when PTY produces output bytes. Resets timers, checks prompt regex.
    pub fn on_output_bytes(&mut self, bytes: &[u8]) {
        self.last_output_at = Instant::now();
        self.last_fired_at = None;
        self.was_output_flowing = true;

        // Check prompt regex against the last line of output
        if let Some(ref regex) = self.prompt_regex {
            if let Ok(text) = std::str::from_utf8(bytes) {
                // Check the last non-empty line
                if let Some(last_line) = text.lines().rev().find(|l| !l.is_empty()) {
                    if regex.is_match(last_line) {
                        self.prompt_detected = true;
                    }
                }
            }
        }
    }

    /// Call when OSC 133;A (shell prompt start) is detected on the PTY thread.
    pub fn on_shell_prompt(&mut self) {
        self.shell_prompt_returned = true;
    }

    /// Check if the orchestrator should be triggered. Returns true at most once
    /// per signal, then clears the signal.
    pub fn should_fire(&mut self) -> bool {
        // Priority 1: Prompt regex matched
        if self.prompt_detected {
            self.prompt_detected = false;
            self.last_fired_at = Some(Instant::now());
            return true;
        }

        // Priority 2: Shell prompt returned (agent exited)
        if self.shell_prompt_returned {
            self.shell_prompt_returned = false;
            self.last_fired_at = Some(Instant::now());
            return true;
        }

        let silence = self.last_output_at.elapsed();

        // Priority 3: Output was flowing and stopped for fast_threshold
        if self.was_output_flowing && silence >= self.fast_threshold {
            self.was_output_flowing = false;
            self.last_fired_at = Some(Instant::now());
            return true;
        }

        // Priority 4: Slow fallback (periodic fire after threshold)
        if silence >= self.threshold {
            let should = self
                .last_fired_at
                .map(|t| t.elapsed() >= self.threshold)
                .unwrap_or(true);
            if should {
                self.last_fired_at = Some(Instant::now());
            }
            return should;
        }

        false
    }

    /// Returns the maximum poll timeout, ensuring we wake in time for the next check.
    pub fn poll_timeout(&self) -> Duration {
        let since_output = self.last_output_at.elapsed();

        // If instant signals are pending, wake immediately
        if self.prompt_detected || self.shell_prompt_returned {
            return Duration::from_millis(50);
        }

        // If output was flowing, use fast threshold
        if self.was_output_flowing {
            let fast_remaining = self.fast_threshold.saturating_sub(since_output);
            let slow_remaining = self.threshold.saturating_sub(since_output);
            return fast_remaining
                .min(slow_remaining)
                .max(Duration::from_secs(1));
        }

        // Otherwise use slow threshold
        if since_output >= self.threshold {
            let since_fired = self
                .last_fired_at
                .map(|t| t.elapsed())
                .unwrap_or(self.threshold);
            self.threshold
                .saturating_sub(since_fired)
                .max(Duration::from_secs(1))
        } else {
            self.threshold
                .saturating_sub(since_output)
                .max(Duration::from_secs(1))
        }
    }
}
```

- [ ] **Step 4: Add regex dependency to glass_terminal's Cargo.toml**

Check if `regex` is already a dependency of `glass_terminal`. If not, add it:

```toml
regex = "1"
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --package glass_terminal silence`
Expected: All 9 SmartTrigger tests PASS.

- [ ] **Step 6: Run clippy**

Run: `cargo clippy --package glass_terminal -- -D warnings`
Expected: No warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/glass_terminal/src/silence.rs crates/glass_terminal/Cargo.toml
git commit -m "feat(terminal): replace SilenceTracker with SmartTrigger

SmartTrigger fires on prompt regex, OSC 133;A, output velocity drop,
or slow silence fallback — whichever comes first."
```

---

### Task 3: Wire SmartTrigger into PTY Loop

**Files:**
- Modify: `crates/glass_terminal/src/pty.rs:158-167` (spawn_pty signature)
- Modify: `crates/glass_terminal/src/pty.rs:239-252` (glass_pty_loop call)
- Modify: `crates/glass_terminal/src/pty.rs:264-292` (glass_pty_loop signature + SmartTrigger init)
- Modify: `crates/glass_terminal/src/pty.rs:375-393` (on_output_bytes call)
- Modify: `crates/glass_terminal/src/pty.rs:484-496` (on_shell_prompt call)

- [ ] **Step 1: Add `agent_prompt_pattern` parameter to `spawn_pty`**

In `crates/glass_terminal/src/pty.rs`, modify `spawn_pty` signature at line 158:

Change:
```rust
pub fn spawn_pty(
    event_proxy: EventProxy,
    proxy: winit::event_loop::EventLoopProxy<AppEvent>,
    window_id: WindowId,
    shell_override: Option<&str>,
    working_directory: Option<&std::path::Path>,
    max_output_capture_kb: u32,
    pipes_enabled: bool,
    orchestrator_silence_secs: u64,
) -> (PtySender, Arc<FairMutex<Term<EventProxy>>>) {
```

To:
```rust
pub fn spawn_pty(
    event_proxy: EventProxy,
    proxy: winit::event_loop::EventLoopProxy<AppEvent>,
    window_id: WindowId,
    shell_override: Option<&str>,
    working_directory: Option<&std::path::Path>,
    max_output_capture_kb: u32,
    pipes_enabled: bool,
    orchestrator_silence_secs: u64,
    orchestrator_fast_trigger_secs: u64,
    orchestrator_prompt_pattern: Option<String>,
) -> (PtySender, Arc<FairMutex<Term<EventProxy>>>) {
```

- [ ] **Step 2: Pass new params to `glass_pty_loop`**

In `spawn_pty`, update the thread spawn closure (around line 242) to pass the new parameters:

Change:
```rust
glass_pty_loop(
    pty,
    term_clone,
    event_proxy,
    proxy,
    window_id,
    rx,
    poll,
    max_output_capture_kb,
    orchestrator_silence_secs,
);
```

To:
```rust
glass_pty_loop(
    pty,
    term_clone,
    event_proxy,
    proxy,
    window_id,
    rx,
    poll,
    max_output_capture_kb,
    orchestrator_silence_secs,
    orchestrator_fast_trigger_secs,
    orchestrator_prompt_pattern,
);
```

- [ ] **Step 3: Update `glass_pty_loop` signature and SmartTrigger init**

Update the `glass_pty_loop` function signature (around line 264) to accept new params:

Change:
```rust
fn glass_pty_loop(
    mut pty: tty::Pty,
    terminal: Arc<FairMutex<Term<EventProxy>>>,
    event_proxy: EventProxy,
    app_proxy: winit::event_loop::EventLoopProxy<AppEvent>,
    window_id: WindowId,
    rx: Receiver<PtyMsg>,
    poll: Arc<polling::Poller>,
    max_output_capture_kb: u32,
    orchestrator_silence_secs: u64,
) {
```

To:
```rust
fn glass_pty_loop(
    mut pty: tty::Pty,
    terminal: Arc<FairMutex<Term<EventProxy>>>,
    event_proxy: EventProxy,
    app_proxy: winit::event_loop::EventLoopProxy<AppEvent>,
    window_id: WindowId,
    rx: Receiver<PtyMsg>,
    poll: Arc<polling::Poller>,
    max_output_capture_kb: u32,
    orchestrator_silence_secs: u64,
    orchestrator_fast_trigger_secs: u64,
    orchestrator_prompt_pattern: Option<String>,
) {
```

Replace the SilenceTracker init block (lines 286-292):

Change:
```rust
let mut silence_tracker = if orchestrator_silence_secs > 0 {
    Some(crate::silence::SilenceTracker::new(
        orchestrator_silence_secs,
    ))
} else {
    None
};
```

To:
```rust
let mut smart_trigger = if orchestrator_silence_secs > 0 {
    Some(crate::silence::SmartTrigger::new(
        orchestrator_silence_secs,
        orchestrator_fast_trigger_secs,
        orchestrator_prompt_pattern,
    ))
} else {
    None
};
```

- [ ] **Step 4: Pass SmartTrigger into `pty_read_with_scan` for byte-level access**

SmartTrigger needs access to raw PTY bytes for prompt regex matching and OSC 133;A events for shell prompt detection. Both are available inside `pty_read_with_scan`, so pass SmartTrigger as an optional parameter.

**4a. Update `pty_read_with_scan` signature** to accept SmartTrigger:

```rust
fn pty_read_with_scan(
    pty: &mut tty::Pty,
    terminal: &Arc<FairMutex<Term<EventProxy>>>,
    event_proxy: &EventProxy,
    app_proxy: &winit::event_loop::EventLoopProxy<AppEvent>,
    window_id: WindowId,
    scanner: &mut OscScanner,
    parser: &mut ansi::Processor,
    buf: &mut [u8],
    output_buffer: &mut OutputBuffer,
    smart_trigger: Option<&mut crate::silence::SmartTrigger>,
) -> io::Result<()> {
```

**4b. Inside `pty_read_with_scan`**, add two things after the `let data = &buf[..unprocessed];` line:

First, feed raw bytes to SmartTrigger (after the existing `output_buffer.append(data)` call):
```rust
// Feed raw bytes to SmartTrigger for prompt detection and timer reset
if let Some(ref mut trigger) = smart_trigger {
    trigger.on_output_bytes(data);
}
```

Second, inside the existing `for osc_event in &osc_events` loop (the one that already forwards events to the main thread), add shell prompt detection:
```rust
// Inside the existing OscEvent forwarding loop, after the send_event call:
if matches!(osc_event, crate::osc_scanner::OscEvent::PromptStart) {
    if let Some(ref mut trigger) = smart_trigger {
        trigger.on_shell_prompt();
    }
}
```

**4c. Update all call sites** of `pty_read_with_scan` to pass `smart_trigger.as_mut()`:
- Child exit drain (line 354): add `smart_trigger.as_mut()` as last argument
- Readable event (line 376): add `smart_trigger.as_mut()` as last argument

**4d. Remove the old separate output tracking** after the readable event (lines 391-393). Delete:
```rust
if let Some(ref mut tracker) = silence_tracker {
    tracker.on_output();
}
```
This is now handled inside `pty_read_with_scan` via `on_output_bytes`.

**4e. Update poll timeout reference** (around line 302):
```rust
if let Some(ref mut trigger) = smart_trigger {
    let silence_timeout = trigger.poll_timeout();
```

- [ ] **Step 5: Update silence check to use smart_trigger**

Replace (around line 408):
```rust
if let Some(ref mut tracker) = silence_tracker {
    if tracker.should_fire() {
```
With:
```rust
if let Some(ref mut trigger) = smart_trigger {
    if trigger.should_fire() {
```

- [ ] **Step 6: Build to verify compilation**

Run: `cargo build --package glass_terminal`
Expected: Build succeeds.

- [ ] **Step 7: Fix all callers of `spawn_pty` in main.rs**

Search for `spawn_pty(` in `src/main.rs` and add the two new parameters at each call site. Extract from config:

```rust
let fast_trigger = self.config.agent.as_ref()
    .and_then(|a| a.orchestrator.as_ref())
    .map(|o| o.fast_trigger_secs)
    .unwrap_or(5);
let prompt_pattern = self.config.agent.as_ref()
    .and_then(|a| a.orchestrator.as_ref())
    .and_then(|o| o.agent_prompt_pattern.clone());
```

Pass `fast_trigger` and `prompt_pattern` as the last two arguments to `spawn_pty`.

- [ ] **Step 8: Build full workspace**

Run: `cargo build`
Expected: Full build succeeds.

- [ ] **Step 9: Run all tests**

Run: `cargo test --workspace`
Expected: All tests pass.

- [ ] **Step 10: Commit**

```bash
git add crates/glass_terminal/src/pty.rs src/main.rs
git commit -m "feat(terminal): wire SmartTrigger into PTY loop

Pass prompt pattern and fast trigger config to glass_pty_loop.
SmartTrigger receives raw bytes for prompt detection and
OSC 133;A events for shell prompt detection."
```

---

### Task 4: Settings Overlay Changes

**Files:**
- Modify: `crates/glass_renderer/src/settings_overlay.rs:72-100` (SettingsConfigSnapshot)
- Modify: `crates/glass_renderer/src/settings_overlay.rs:885-907` (fields_for_section index 6)
- Modify: `src/main.rs:2279-2395` (config snapshot builder)
- Modify: `src/main.rs:6733-6762` (handle_settings_increment)

- [ ] **Step 1: Add fields to SettingsConfigSnapshot**

In `crates/glass_renderer/src/settings_overlay.rs`, add to `SettingsConfigSnapshot` after `orchestrator_max_retries`:

```rust
pub orchestrator_fast_trigger_secs: u64,
pub orchestrator_prompt_pattern: String,
```

Update `Default` impl to include:
```rust
orchestrator_fast_trigger_secs: 5,
orchestrator_prompt_pattern: String::new(),
```

- [ ] **Step 2: Update fields_for_section for Orchestrator (index 6)**

Replace the index 6 match arm in `fields_for_section` (around line 885):

```rust
6 => vec![
    // Orchestrator
    (
        "Enabled",
        if config.orchestrator_enabled {
            "ON".to_string()
        } else {
            "OFF".to_string()
        },
        true,
    ),
    (
        "Silence Timeout (sec)",
        format!("{}", config.orchestrator_silence_secs),
        false,
    ),
    (
        "Fast Trigger (sec)",
        format!("{}", config.orchestrator_fast_trigger_secs),
        false,
    ),
    (
        "Prompt Pattern",
        if config.orchestrator_prompt_pattern.is_empty() {
            "(none)".to_string()
        } else {
            config.orchestrator_prompt_pattern.clone()
        },
        false,
    ),
    ("PRD Path", config.orchestrator_prd_path.clone(), false),
    (
        "Max Retries",
        format!("{}", config.orchestrator_max_retries),
        false,
    ),
],
```

- [ ] **Step 3: Update config snapshot builder in main.rs**

In `src/main.rs` where `SettingsConfigSnapshot` is constructed (around line 2279), add the two new fields:

```rust
orchestrator_fast_trigger_secs: self
    .config
    .agent
    .as_ref()
    .and_then(|a| a.orchestrator.as_ref())
    .map(|o| o.fast_trigger_secs)
    .unwrap_or(5),
orchestrator_prompt_pattern: self
    .config
    .agent
    .as_ref()
    .and_then(|a| a.orchestrator.as_ref())
    .and_then(|o| o.agent_prompt_pattern.clone())
    .unwrap_or_default(),
```

- [ ] **Step 4: Update handle_settings_increment for new field indices**

In `src/main.rs` `handle_settings_increment()`, add the fast trigger handler and renumber max_retries:

Add new case for `(6, 2)` — Fast Trigger:
```rust
// Orchestrator fast_trigger_secs: step 1
(6, 2) => {
    let current = config
        .agent
        .as_ref()
        .and_then(|a| a.orchestrator.as_ref())
        .map(|o| o.fast_trigger_secs)
        .unwrap_or(5) as i64;
    let new_val = (current + delta).max(1);
    Some((
        Some("agent.orchestrator"),
        "fast_trigger_secs",
        new_val.to_string(),
    ))
}
```

Change existing max_retries from `(6, 3)` to `(6, 5)`:
```rust
// Orchestrator max_retries: step 1
(6, 5) => {
```

- [ ] **Step 5: Build and run tests**

Run: `cargo build && cargo test --workspace`
Expected: Build succeeds, all tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/glass_renderer/src/settings_overlay.rs src/main.rs
git commit -m "feat(settings): add Fast Trigger and Prompt Pattern to orchestrator settings"
```

---

## Chunk 2: StateFingerprint + SOI Context Windowing

### Task 5: Add StateFingerprint to Orchestrator

**Files:**
- Modify: `src/orchestrator.rs`

- [ ] **Step 1: Write tests for StateFingerprint and record_fingerprint**

Add to the existing `tests` module in `src/orchestrator.rs`:

```rust
#[test]
fn fingerprint_stuck_after_n_identical() {
    let mut state = OrchestratorState::new(3);
    let fp = StateFingerprint {
        terminal_hash: 12345,
        soi_error_hash: Some(67890),
        git_diff_hash: None,
    };
    assert!(!state.record_fingerprint(fp.clone()));
    assert!(!state.record_fingerprint(fp.clone()));
    assert!(state.record_fingerprint(fp)); // 3rd identical
}

#[test]
fn fingerprint_not_stuck_when_different() {
    let mut state = OrchestratorState::new(3);
    let fp1 = StateFingerprint {
        terminal_hash: 111,
        soi_error_hash: None,
        git_diff_hash: None,
    };
    let fp2 = StateFingerprint {
        terminal_hash: 222,
        soi_error_hash: None,
        git_diff_hash: None,
    };
    assert!(!state.record_fingerprint(fp1.clone()));
    assert!(!state.record_fingerprint(fp1));
    assert!(!state.record_fingerprint(fp2)); // different
}

#[test]
fn fingerprint_reset_clears() {
    let mut state = OrchestratorState::new(3);
    let fp = StateFingerprint {
        terminal_hash: 111,
        soi_error_hash: None,
        git_diff_hash: None,
    };
    state.record_fingerprint(fp.clone());
    state.record_fingerprint(fp.clone());
    state.reset_stuck();
    assert!(!state.record_fingerprint(fp)); // reset, only 1
}

#[test]
fn compute_fingerprint_hashes_lines() {
    let lines1 = vec!["hello".to_string(), "world".to_string()];
    let lines2 = vec!["hello".to_string(), "world".to_string()];
    let lines3 = vec!["different".to_string()];
    let fp1 = StateFingerprint::compute(&lines1, None, None);
    let fp2 = StateFingerprint::compute(&lines2, None, None);
    let fp3 = StateFingerprint::compute(&lines3, None, None);
    assert_eq!(fp1.terminal_hash, fp2.terminal_hash);
    assert_ne!(fp1.terminal_hash, fp3.terminal_hash);
}

#[test]
fn compute_fingerprint_with_soi_and_git() {
    let lines = vec!["output".to_string()];
    let soi = vec!["Error[E0277]".to_string()];
    let git = "1 file changed";
    let fp = StateFingerprint::compute(&lines, Some(&soi), Some(git));
    assert!(fp.soi_error_hash.is_some());
    assert!(fp.git_diff_hash.is_some());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --package glass orchestrator::tests::fingerprint`
Expected: FAIL — `StateFingerprint` doesn't exist.

- [ ] **Step 3: Implement StateFingerprint**

Add to `src/orchestrator.rs`, before `OrchestratorState`:

```rust
use std::hash::{Hash, Hasher};

/// Composite environment state fingerprint for semantic stuck detection.
/// Hashes terminal content, SOI errors, and git diff instead of comparing
/// agent response strings literally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateFingerprint {
    /// Hash of recent terminal lines.
    pub terminal_hash: u64,
    /// Hash of SOI error records (if a command failed with SOI data).
    pub soi_error_hash: Option<u64>,
    /// Hash of `git diff --stat` output (if in a git repo).
    pub git_diff_hash: Option<u64>,
}

impl StateFingerprint {
    /// Compute a fingerprint from available signals.
    pub fn compute(
        terminal_lines: &[String],
        soi_errors: Option<&[String]>,
        git_diff_stat: Option<&str>,
    ) -> Self {
        let terminal_hash = {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            for line in terminal_lines {
                line.hash(&mut hasher);
            }
            hasher.finish()
        };

        let soi_error_hash = soi_errors.map(|errors| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            for error in errors {
                error.hash(&mut hasher);
            }
            hasher.finish()
        });

        let git_diff_hash = git_diff_stat.map(|diff| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            diff.hash(&mut hasher);
            hasher.finish()
        });

        Self {
            terminal_hash,
            soi_error_hash,
            git_diff_hash,
        }
    }
}
```

- [ ] **Step 4: Add `recent_fingerprints` and `record_fingerprint` to OrchestratorState**

Add fields to `OrchestratorState`:
```rust
/// Last N environment fingerprints for semantic stuck detection.
pub recent_fingerprints: Vec<StateFingerprint>,
/// Whether the last fingerprint check detected stuck (consumed by response handler).
pub fingerprint_stuck: bool,
```

Initialize in `OrchestratorState::new`:
```rust
recent_fingerprints: Vec::new(),
fingerprint_stuck: false,
```

Add method:
```rust
/// Record an environment fingerprint and check if stuck (N identical consecutive).
/// Returns true if stuck.
pub fn record_fingerprint(&mut self, fp: StateFingerprint) -> bool {
    self.recent_fingerprints.push(fp);
    if self.recent_fingerprints.len() > self.max_retries as usize {
        self.recent_fingerprints
            .drain(..self.recent_fingerprints.len() - self.max_retries as usize);
    }
    if self.recent_fingerprints.len() >= self.max_retries as usize {
        self.recent_fingerprints
            .iter()
            .all(|f| f == &self.recent_fingerprints[0])
    } else {
        false
    }
}
```

Update `reset_stuck`:
```rust
pub fn reset_stuck(&mut self) {
    self.recent_responses.clear();
    self.recent_fingerprints.clear();
    self.fingerprint_stuck = false;
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --package glass orchestrator::tests`
Expected: All orchestrator tests PASS (existing + new).

- [ ] **Step 6: Run clippy**

Run: `cargo clippy --package glass -- -D warnings`
Expected: No warnings.

- [ ] **Step 7: Commit**

```bash
git add src/orchestrator.rs
git commit -m "feat(orchestrator): add StateFingerprint for semantic stuck detection

Hashes terminal content, SOI errors, and git diff to detect when the
agent is stuck in a loop, even if it rephrases its responses."
```

---

### Task 6: Add build_orchestrator_context

**Files:**
- Modify: `src/orchestrator.rs`

- [ ] **Step 1: Write tests for build_orchestrator_context**

Add to `tests` module in `src/orchestrator.rs`:

```rust
#[test]
fn context_failed_with_soi() {
    let lines: Vec<String> = (0..50).map(|i| format!("line {i}")).collect();
    let context = build_orchestrator_context(
        &lines,
        Some(1),
        Some("cargo test: 3 failed"),
        &["src/main.rs:10 Error[E0277]: trait bound".to_string()],
    );
    assert!(context.contains("[COMMAND_FAILED]"));
    assert!(context.contains("exit code: 1"));
    assert!(context.contains("[SOI_SUMMARY]"));
    assert!(context.contains("cargo test: 3 failed"));
    assert!(context.contains("[SOI_ERRORS]"));
    assert!(context.contains("Error[E0277]"));
    assert!(context.contains("[RECENT_OUTPUT]"));
    // Should include last CONTEXT_LINES_ON_ERROR lines, not all 50
    assert!(!context.contains("line 0\n"));
    assert!(context.contains("line 49"));
}

#[test]
fn context_success_with_soi() {
    let lines: Vec<String> = (0..50).map(|i| format!("line {i}")).collect();
    let context = build_orchestrator_context(
        &lines,
        Some(0),
        Some("cargo test: 45 passed"),
        &[],
    );
    assert!(context.contains("[COMMAND_OK]"));
    assert!(context.contains("[SOI_SUMMARY]"));
    assert!(context.contains("45 passed"));
    assert!(context.contains("[RECENT_OUTPUT]"));
    // Should include fewer lines on success
    assert!(!context.contains("line 0\n"));
}

#[test]
fn context_no_soi() {
    let lines: Vec<String> = (0..100).map(|i| format!("line {i}")).collect();
    let context = build_orchestrator_context(&lines, None, None, &[]);
    assert!(!context.contains("[COMMAND_FAILED]"));
    assert!(!context.contains("[COMMAND_OK]"));
    assert!(!context.contains("[SOI_SUMMARY]"));
    assert!(context.contains("[RECENT_OUTPUT]"));
    // Should include CONTEXT_LINES_FALLBACK lines
    assert!(context.contains("line 99"));
    assert!(context.contains("line 20"));
}

#[test]
fn context_empty_terminal() {
    let context = build_orchestrator_context(&[], Some(1), None, &[]);
    assert!(context.contains("[COMMAND_FAILED]"));
    assert!(context.contains("[RECENT_OUTPUT]"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --package glass orchestrator::tests::context`
Expected: FAIL — `build_orchestrator_context` doesn't exist.

- [ ] **Step 3: Implement build_orchestrator_context**

Add to `src/orchestrator.rs`:

```rust
/// Line counts for SOI-driven context windowing.
const CONTEXT_LINES_ON_ERROR: usize = 30;
const CONTEXT_LINES_ON_SUCCESS: usize = 20;
const CONTEXT_LINES_FALLBACK: usize = 80;

/// Build context string for the Glass Agent based on command outcome and SOI data.
///
/// Uses severity-based selection:
/// - Failed command + SOI: structured errors + 30 terminal lines
/// - Succeeded command + SOI: one-line summary + 20 terminal lines
/// - No SOI: 80 terminal lines (generous fallback)
pub fn build_orchestrator_context(
    terminal_lines: &[String],
    last_exit_code: Option<i32>,
    soi_summary: Option<&str>,
    soi_error_records: &[String],
) -> String {
    let mut context = String::new();

    let has_soi = soi_summary.is_some();
    let failed = last_exit_code.is_some_and(|c| c != 0);

    if failed && has_soi {
        // Branch 1: Command failed with SOI data
        context.push_str(&format!(
            "[COMMAND_FAILED] exit code: {}\n",
            last_exit_code.unwrap_or(-1)
        ));
        if let Some(summary) = soi_summary {
            context.push_str(&format!("[SOI_SUMMARY] {summary}\n"));
        }
        if !soi_error_records.is_empty() {
            context.push_str("[SOI_ERRORS]\n");
            for record in soi_error_records {
                context.push_str(&format!("  {record}\n"));
            }
        }
        let n = CONTEXT_LINES_ON_ERROR;
        let start = terminal_lines.len().saturating_sub(n);
        context.push_str(&format!("[RECENT_OUTPUT] (last {n} lines)\n"));
        for line in &terminal_lines[start..] {
            context.push_str(line);
            context.push('\n');
        }
    } else if !failed && has_soi {
        // Branch 2: Command succeeded with SOI data
        context.push_str("[COMMAND_OK]\n");
        if let Some(summary) = soi_summary {
            context.push_str(&format!("[SOI_SUMMARY] {summary}\n"));
        }
        let n = CONTEXT_LINES_ON_SUCCESS;
        let start = terminal_lines.len().saturating_sub(n);
        context.push_str(&format!("[RECENT_OUTPUT] (last {n} lines)\n"));
        for line in &terminal_lines[start..] {
            context.push_str(line);
            context.push('\n');
        }
    } else {
        // Branch 3: No SOI data
        if failed {
            context.push_str(&format!(
                "[COMMAND_FAILED] exit code: {}\n",
                last_exit_code.unwrap_or(-1)
            ));
        }
        let n = CONTEXT_LINES_FALLBACK;
        let start = terminal_lines.len().saturating_sub(n);
        context.push_str(&format!("[RECENT_OUTPUT] (last {n} lines)\n"));
        for line in &terminal_lines[start..] {
            context.push_str(line);
            context.push('\n');
        }
    }

    context
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --package glass orchestrator::tests`
Expected: All orchestrator tests PASS.

- [ ] **Step 5: Run clippy**

Run: `cargo clippy --package glass -- -D warnings`
Expected: No warnings.

- [ ] **Step 6: Commit**

```bash
git add src/orchestrator.rs
git commit -m "feat(orchestrator): add SOI-driven context windowing

build_orchestrator_context selects context based on command outcome:
30 lines + SOI errors on failure, 20 lines + summary on success,
80 lines fallback when no SOI data available."
```

---

## Chunk 3: Main.rs Integration

### Task 7: Wire Fingerprint + Context into OrchestratorSilence Handler

**Files:**
- Modify: `src/main.rs:5560-5707` (OrchestratorSilence handler)

- [ ] **Step 1: Add `fetch_latest_soi_context` helper**

Add a helper function near the `extract_term_lines` function in `src/main.rs`:

```rust
/// Fetch SOI context for the most recent command in a session.
/// Returns (exit_code, soi_summary, soi_error_strings).
fn fetch_latest_soi_context(
    session: &glass_mux::session::Session,
) -> (Option<i32>, Option<String>, Vec<String>) {
    // Get exit code from most recent completed block
    let exit_code = session
        .block_manager
        .blocks()
        .iter()
        .rev()
        .find(|b| b.state == glass_terminal::block_manager::BlockState::Complete)
        .and_then(|b| b.exit_code);

    let command_id = match session.last_command_id {
        Some(id) => id,
        None => return (exit_code, None, Vec::new()),
    };

    let db = match session.history_db.as_ref() {
        Some(db) => db,
        None => return (exit_code, None, Vec::new()),
    };

    let conn = db.conn();

    let soi_summary = glass_history::soi::get_output_summary(conn, command_id)
        .ok()
        .flatten()
        .map(|s| s.one_line);

    let soi_errors = glass_history::soi::get_output_records(
        &conn,
        command_id,
        Some("Error"),
        None,
        None,
        100,
    )
    .ok()
    .unwrap_or_default()
    .into_iter()
    .map(|r| {
        let file = r.file_path.as_deref().unwrap_or("");
        let data_preview = r.data.chars().take(200).collect::<String>();
        if file.is_empty() {
            data_preview
        } else {
            format!("{file} {data_preview}")
        }
    })
    .collect();

    (exit_code, soi_summary, soi_errors)
}
```

- [ ] **Step 2: Replace context capture in OrchestratorSilence handler**

In the OrchestratorSilence handler (around line 5657-5680), replace the context capture block:

Change:
```rust
let lines = extract_term_lines(&session.term, 100);
let context = lines.join("\n");
```

To:
```rust
let lines = extract_term_lines(&session.term, 80);
let (exit_code, soi_summary, soi_errors) =
    fetch_latest_soi_context(session);
let context = orchestrator::build_orchestrator_context(
    &lines,
    exit_code,
    soi_summary.as_deref(),
    &soi_errors,
);
```

- [ ] **Step 3: Add fingerprint computation before context send**

After the context is built (before the JSON message construction), add fingerprint computation:

Note: `git diff --stat` is fast (avoids content diffing). The existing code already runs `git log --oneline -10` synchronously in the same handler, so this is consistent.

```rust
// Build environment fingerprint for stuck detection
let git_diff = std::process::Command::new("git")
    .args(["diff", "--stat"])
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

let fp_lines = extract_term_lines(&session.term, 50);
let soi_for_fp = if exit_code.is_some_and(|c| c != 0) {
    Some(soi_errors.as_slice())
} else {
    None
};
let fingerprint = orchestrator::StateFingerprint::compute(
    &fp_lines,
    soi_for_fp,
    git_diff.as_deref(),
);
self.orchestrator.fingerprint_stuck = self.orchestrator.record_fingerprint(fingerprint);
```

- [ ] **Step 4: Wire fingerprint stuck detection into the response handler**

Find where `record_response` is called in main.rs (in the OrchestratorResponse handler). The `fingerprint_stuck` flag was set in the OrchestratorSilence handler (Step 3) and stored on `self.orchestrator` (field added in Task 5). Wire it into the stuck check:

```rust
let text_stuck = self.orchestrator.record_response(&response_text);
let stuck = text_stuck || self.orchestrator.fingerprint_stuck;
if stuck {
    self.orchestrator.fingerprint_stuck = false;
    // ... existing stuck handling ...
}
```

- [ ] **Step 5: Update checkpoint refresh context**

Find the checkpoint refresh context capture (around line 5610-5613):

Change:
```rust
.map(|s| extract_term_lines(&s.term, 100).join("\n"))
```

To:
```rust
.map(|s| {
    let lines = extract_term_lines(&s.term, 80);
    let (exit_code, soi_summary, soi_errors) = fetch_latest_soi_context(s);
    orchestrator::build_orchestrator_context(
        &lines, exit_code, soi_summary.as_deref(), &soi_errors
    )
})
```

- [ ] **Step 6: Build full workspace**

Run: `cargo build`
Expected: Build succeeds.

- [ ] **Step 7: Run all tests**

Run: `cargo test --workspace`
Expected: All tests pass.

- [ ] **Step 8: Run clippy and fmt**

Run: `cargo fmt --all -- --check && cargo clippy --workspace -- -D warnings`
Expected: No formatting issues, no clippy warnings.

- [ ] **Step 9: Commit**

```bash
git add src/main.rs src/orchestrator.rs
git commit -m "feat(orchestrator): integrate fingerprint and SOI context into main loop

OrchestratorSilence handler now:
- Uses build_orchestrator_context for severity-based context selection
- Computes StateFingerprint for semantic stuck detection
- Checks both fingerprint and response text for stuck loops"
```

---

## Final Verification

- [ ] **Run full test suite**: `cargo test --workspace`
- [ ] **Run clippy**: `cargo clippy --workspace -- -D warnings`
- [ ] **Run fmt check**: `cargo fmt --all -- --check`
- [ ] **Manual smoke test**: Start Glass, open settings overlay (Ctrl+Shift+,), verify Orchestrator section shows all 6 fields including new Fast Trigger and Prompt Pattern.
