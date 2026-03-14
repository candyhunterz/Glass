# Orchestrator Improvements: Smart Trigger, State Fingerprint, SOI Context

**Date:** 2026-03-14
**Status:** Approved
**Scope:** Three targeted improvements to the orchestrator loop, all agent-agnostic

## Motivation

The orchestrator drives autonomous agent sessions by detecting when the agent is idle, checking if it's stuck, and sending terminal context. All three mechanisms have known limitations:

1. **Silence detection** uses a fixed 30s threshold — too slow when the agent finishes quickly, too aggressive during long-running commands.
2. **Stuck detection** compares response strings literally — agents that rephrase the same failed approach evade detection.
3. **Context windowing** grabs a hardcoded 100 terminal lines — wastes tokens on successful output, may miss errors in verbose output.

These improvements are designed to work with any LLM agent (Claude Code, Aider, local models, etc.), not just Claude Code.

---

## Improvement 1: Adaptive Silence + Configurable Prompt Detection

### Overview

Replace `SilenceTracker` with `SmartTrigger` that fires on the fastest available signal instead of always waiting for the fixed threshold.

### Detection Layers (priority order)

1. **Prompt regex match** (instant) — Optional regex from config. When new PTY output ends with a line matching the pattern, fire immediately. Examples: `^❯` for Claude Code, `^aider>` for Aider.

2. **Output velocity drop** (fast, 3-5s) — Track whether output was actively flowing (bytes received in the last ~2s). When output was flowing and then stops for `fast_trigger_secs` (default 5), fire once.

3. **Fixed silence threshold** (slow fallback) — Existing periodic-fire-after-N-seconds behavior. Handles edge cases like agents that produce no visible output.

4. **OSC 133;A detection** (shell prompt returned) — If the shell prompt returns while the orchestrator is active, the agent process exited. Fire immediately and flag as an exit event.

### SmartTrigger State

```rust
pub struct SmartTrigger {
    // Existing (from SilenceTracker)
    threshold: Duration,          // slow fallback threshold
    last_output_at: Instant,
    last_fired_at: Option<Instant>,
    // New
    fast_threshold: Duration,     // post-output quick detection
    prompt_regex: Option<Regex>,  // compiled from config pattern string
    was_output_flowing: bool,     // latch: set on output, cleared on fast-trigger fire
    prompt_detected: bool,        // regex matched end of output?
    shell_prompt_returned: bool,  // OSC 133;A while orchestrator active?
}
```

### should_fire() Logic

1. If `prompt_detected` -> fire, clear flag
2. If `shell_prompt_returned` -> fire, clear flag
3. If `was_output_flowing` and silence >= `fast_threshold` -> fire, clear `was_output_flowing`
4. If silence >= `threshold` -> periodic fire (existing behavior)

### poll_timeout() Update

Must account for `fast_threshold`: when `was_output_flowing` is set, return `min(fast_threshold - elapsed, threshold - elapsed)` so the poll loop wakes in time for the fast trigger.

### New Methods

- `on_output_bytes(&mut self, bytes: &[u8])` — Called from PTY reader thread (same thread as SmartTrigger). Sets `was_output_flowing = true`, resets timers. If `prompt_regex` is set, checks if the last line of `bytes` matches -> sets `prompt_detected`.
- `on_shell_prompt(&mut self)` — Called from PTY reader thread when `OscScanner` detects `OscEvent::PromptStart`. The `OscScanner` runs on the same PTY thread as `SmartTrigger`, so this is a direct call before the event is forwarded to the main thread via `EventProxy`. Sets `shell_prompt_returned = true`.

### `was_output_flowing` Semantics

This is a **latch**, not a sliding window. Set to `true` when any output is received via `on_output_bytes()`. Cleared only when the fast trigger fires (step 3 of `should_fire()`). This means: if output flows, stops, fast trigger fires, then silence continues — the slow fallback takes over for subsequent fires.

### Threading Model

`SmartTrigger` lives entirely on the PTY reader thread (same as current `SilenceTracker`). Both `on_output_bytes()` and `on_shell_prompt()` are called from this thread:

- `on_output_bytes()`: called in `glass_pty_loop()` when bytes are read from the PTY fd (existing output path).
- `on_shell_prompt()`: called when `OscScanner` produces `OscEvent::PromptStart` in the PTY loop, before the event is forwarded to the main thread. The `OscScanner` already runs on the PTY thread.

The `prompt_regex` is compiled from a pattern string passed as a parameter to `glass_pty_loop()` and `spawn_pty()`. The `Regex` is constructed on the PTY thread to avoid Send concerns.

### Config Changes

Add to `OrchestratorSection` in `config.rs`:

```rust
/// Seconds after output stops before fast-triggering. Default 5.
#[serde(default = "default_orch_fast_trigger")]
pub fast_trigger_secs: u64,

/// Optional regex pattern to detect the agent's prompt. Default None.
#[serde(default)]
pub agent_prompt_pattern: Option<String>,
```

TOML example:
```toml
[agent.orchestrator]
silence_timeout_secs = 30       # slow fallback
fast_trigger_secs = 5           # post-output quick detection
agent_prompt_pattern = "^❯"    # optional, agent-specific
```

### Settings Overlay Changes

Orchestrator section (index 6) new field layout:

| Index | Field | Type | Details |
|-------|-------|------|---------|
| 0 | Enabled | toggle | existing, in `handle_settings_activate` |
| 1 | Silence Timeout (sec) | +/- step 5 | existing, in `handle_settings_increment` |
| 2 | Fast Trigger (sec) | +/- step 1, default 5, min 1 | NEW, in `handle_settings_increment` |
| 3 | Agent Prompt Pattern | read-only display | NEW, display only (edit via config.toml) |
| 4 | PRD Path | read-only display | existing (renumbered from 2), display only |
| 5 | Max Retries | +/- step 1 | existing (renumbered from 3), in `handle_settings_increment` |

Note: Agent Prompt Pattern and PRD Path are displayed in the settings overlay but are read-only. Inline text editing infrastructure is not yet wired up in the settings overlay. These fields are editable only via `~/.glass/config.toml`.

Touch points:
- `config.rs`: Add fields to `OrchestratorSection`
- `settings_overlay.rs`: Add `orchestrator_fast_trigger_secs` and `orchestrator_prompt_pattern` to `SettingsConfigSnapshot`, update `fields_for_section()` index 6 to show 6 fields
- `main.rs` config snapshot builder: populate new fields from config
- `main.rs` `handle_settings_increment()`: add `(6, 2)` for fast_trigger (+/- step 1, min 1), renumber existing max_retries from `(6, 3)` to `(6, 5)`
- `main.rs` `handle_settings_activate()`: no changes needed (prompt pattern is read-only, PRD path has no activate handler)

### Files Modified

- `crates/glass_terminal/src/silence.rs` — Replace `SilenceTracker` with `SmartTrigger`
- `crates/glass_terminal/src/pty.rs` — Add `agent_prompt_pattern: Option<String>` parameter to `glass_pty_loop()` and `spawn_pty()`, pass to `SmartTrigger` constructor. Wire `OscEvent::PromptStart` to `smart_trigger.on_shell_prompt()`.
- `crates/glass_core/src/config.rs` — Add config fields
- `crates/glass_renderer/src/settings_overlay.rs` — Add settings fields
- `src/main.rs` — Update config snapshot, settings handlers, pass prompt pattern to PTY spawner

---

## Improvement 2: Multi-Signal State Fingerprint (Stuck Detection)

### Overview

Replace literal response string comparison with a composite state fingerprint that hashes the environment state, not the agent's words.

### StateFingerprint Struct

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct StateFingerprint {
    /// Hash of last 50 terminal lines
    terminal_hash: u64,
    /// Hash of SOI error records for last failed command, if any
    soi_error_hash: Option<u64>,
    /// Hash of `git diff --stat` output, if in a git repo
    git_diff_hash: Option<u64>,
}
```

### Signal Sources

1. **Terminal hash** (always available) — Hash last 50 lines from `extract_term_lines()`. Uses `std::hash::DefaultHasher`. Catches identical error output, same screen state.

2. **SOI error hash** (when command failed + SOI parsed) — Query `get_output_records(command_id, severity="Error")` from history DB. Hash serialized records. Catches same compiler errors, same test failures.

3. **Git diff hash** (when in git repo) — Run `git diff --stat` with a 2-second timeout (quick, avoids content diffing). Hash output. Catches agent making no code changes or reverting changes. If the command times out or fails, `git_diff_hash = None`.

### Changes to OrchestratorState

```rust
// Add alongside existing:
pub recent_fingerprints: Vec<StateFingerprint>,

// Keep existing for secondary check:
pub recent_responses: Vec<String>,
```

New method:
```rust
pub fn record_fingerprint(&mut self, fp: StateFingerprint) -> bool {
    self.recent_fingerprints.push(fp);
    if self.recent_fingerprints.len() > self.max_retries as usize {
        self.recent_fingerprints.drain(
            ..self.recent_fingerprints.len() - self.max_retries as usize
        );
    }
    if self.recent_fingerprints.len() >= self.max_retries as usize {
        self.recent_fingerprints.iter().all(|f| f == &self.recent_fingerprints[0])
    } else {
        false
    }
}
```

Update `reset_stuck()` to clear both buffers:
```rust
pub fn reset_stuck(&mut self) {
    self.recent_responses.clear();
    self.recent_fingerprints.clear();
}
```

### Stuck Detection Logic

Stuck = `record_fingerprint(fp) || record_response(text)`. Either signal triggers intervention. Fingerprint catches semantic loops; response comparison catches verbatim repetition.

### Integration in main.rs

At the `OrchestratorSilence` handler (~line 5660), after capturing terminal context:

1. Hash the extracted terminal lines
2. Query SOI for the latest failed command via `session.last_command_id`
3. Run `git diff --stat` in the CWD
4. Build `StateFingerprint`, pass to `record_fingerprint()`

### Fallback

No SOI data -> `soi_error_hash = None`. No git repo -> `git_diff_hash = None`. Fingerprint still works with just terminal hash.

### Files Modified

- `src/orchestrator.rs` — Add `StateFingerprint`, `record_fingerprint()`, update `reset_stuck()`
- `src/main.rs` — Build fingerprint in OrchestratorSilence handler

---

## Improvement 3: SOI-Driven Context Windowing

### Overview

Replace hardcoded 100-line terminal grab with severity-based context selection that sends compact, focused context to the Glass Agent.

### Function

New in `src/orchestrator.rs`:

```rust
pub fn build_orchestrator_context(
    terminal_lines: &[String],
    last_exit_code: Option<i32>,
    soi_summary: Option<&str>,
    soi_error_records: &[String],
) -> String
```

### Three Branches

**1. Command failed (exit != 0) + SOI available:**
```
[COMMAND_FAILED] exit code: 1
[SOI_SUMMARY] cargo test: 3 tests failed, 42 passed
[SOI_ERRORS]
  src/main.rs:142:5 Error[E0277]: the trait bound `Foo: Display` is not satisfied
  src/lib.rs:89:12 Error[E0308]: mismatched types
[RECENT_OUTPUT] (last 30 lines)
  ... terminal lines ...
```

**2. Command succeeded (exit == 0) + SOI available:**
```
[COMMAND_OK]
[SOI_SUMMARY] cargo test: 45 tests passed
[RECENT_OUTPUT] (last 20 lines)
  ... terminal lines ...
```

**3. No SOI data:**
```
[RECENT_OUTPUT] (last 80 lines)
  ... terminal lines ...
```

### Data Sources

- `last_exit_code`: from most recent completed `Block` in `BlockManager` (`block.exit_code`)
- `soi_summary`: `glass_history::soi::get_output_summary(conn, command_id)` using `session.last_command_id`
- `soi_error_records`: `glass_history::soi::get_output_records(conn, command_id, Some("Error"), ...)`, formatted as `"{file}:{line} {message}"`

### Integration in main.rs

Replace both context capture sites:

```rust
// OrchestratorSilence handler (~line 5660):
let lines = extract_term_lines(&session.term, 80);
let (exit_code, soi_summary, soi_errors) = fetch_latest_soi_context(&session, &self.history_db);
let context = orchestrator::build_orchestrator_context(&lines, exit_code, soi_summary.as_deref(), &soi_errors);

// Checkpoint refresh (~line 5613):
// Same pattern
```

`fetch_latest_soi_context()` is a small helper in `main.rs` that gets the latest completed block's exit code and queries SOI.

### Line Count Constants

Defined in `orchestrator.rs`, not user-facing config:
```rust
const CONTEXT_LINES_ON_ERROR: usize = 30;
const CONTEXT_LINES_ON_SUCCESS: usize = 20;
const CONTEXT_LINES_FALLBACK: usize = 80;
```

### Files Modified

- `src/orchestrator.rs` — Add `build_orchestrator_context()` and line count constants
- `src/main.rs` — Replace `extract_term_lines(&session.term, 100)` calls with new context builder

---

## Cross-Cutting Concerns

### Dependencies

- `regex` crate: already in the dependency tree (used by SOI parsers). `SmartTrigger` compiles the prompt pattern from a string passed to `glass_pty_loop()`. Compilation happens once on the PTY thread at loop start.
- No new external dependencies.

### Testing

Each improvement has isolated, testable units:

1. **SmartTrigger**: Unit tests for each firing path (prompt match, velocity drop, slow fallback, OSC 133;A). Similar to existing `SilenceTracker` tests.
2. **StateFingerprint**: Unit tests for fingerprint equality/inequality. Test stuck detection with identical vs changing fingerprints.
3. **build_orchestrator_context()**: Unit tests for each branch (failed+SOI, success+SOI, no SOI). Assert output format and line counts.

### Backward Compatibility

- All new config fields have defaults matching current behavior (`fast_trigger_secs = 5`, `agent_prompt_pattern = None`).
- Existing `silence_timeout_secs` keeps working as the slow fallback.
- `record_response()` is kept alongside `record_fingerprint()` — no behavior regression.
- Users who don't configure anything get: faster trigger via velocity detection + smarter stuck detection + better context. No config changes required.
