# Orchestrator V2: Metric Guard, Artifact Completion, Bounded Iterations

**Date:** 2026-03-14
**Status:** Approved
**Scope:** Three new features for the orchestrator loop, inspired by agtx and autoresearch

## Motivation

Glass's orchestrator drives autonomous agent sessions. The improvements shipped today (SmartTrigger, StateFingerprint, SOI context windowing) made triggering smarter and stuck detection semantic. This next round addresses three remaining gaps:

1. **No regression prevention** — The orchestrator has no way to detect when agent changes break existing tests or build. It can detect "no progress" but not "going backwards."
2. **Completion is probabilistic** — Even with SmartTrigger, completion detection relies on silence/prompt patterns. Agents could signal completion deterministically by writing a file.
3. **No bounded runs** — Users can't say "run for 25 iterations then stop." Orchestration runs until manually stopped or GLASS_DONE.

These features are agent-agnostic and work with any LLM agent.

---

## Feature 1: Metric Guard

### Overview

After each orchestrator iteration, run verification commands to ensure the codebase hasn't regressed. Auto-revert if it has. Zero-config for most projects.

### Three-Layer Priority Chain

**1. User config (highest priority)** — If `verify_command` is set in config.toml, use exactly that. Skip auto-detect and agent discovery.

**2. Auto-detect (always runs unless user override)** — Glass checks for marker files at orchestration start. Pure Rust, no AI, instant:

| Marker File | Verify Command | SOI Parser |
|---|---|---|
| `Cargo.toml` | `cargo test` | RustTest |
| `package.json` with `"test"` script | `npm test` | Jest/Npm |
| `pyproject.toml` or `setup.py` | `pytest` | Pytest |
| `go.mod` | `go test ./...` | GoTest |
| `tsconfig.json` | `npx tsc --noEmit` | TypeScript |
| `Makefile` with `test` target | `make test` | Generic |
| Fallback | Build command (cargo build, npm run build, etc.) | Generic (exit code only) |

Detection logic lives in `src/orchestrator.rs` as a pure function `auto_detect_verify_commands(project_root: &str) -> Vec<VerifyCommand>`. Checks files in priority order, returns the first match.

**3. Agent discovery (extends, never removes)** — When the Glass Agent spawns, the system prompt includes instructions to report verification commands:

```
If you discover additional verification commands for this project, report them:
GLASS_VERIFY: {"commands": [{"name": "integration", "cmd": "./scripts/integration-test.sh"}]}
```

Agent-discovered commands are appended to auto-detected ones. The agent cannot remove or replace auto-detected commands — this prevents the agent from disabling its own safety net.

### GLASS_VERIFY Parsing

`GLASS_VERIFY` is parsed in `parse_agent_response()` in `src/orchestrator.rs`, same as existing GLASS_DONE, GLASS_WAIT, and GLASS_CHECKPOINT markers. Add a new variant:

```rust
pub enum AgentResponse {
    TypeText(String),
    Wait,
    Checkpoint { completed: String, next: String },
    Done { summary: String },
    Verify { commands: Vec<VerifyCommand> },  // NEW
}
```

When parsed, the verify commands are appended to `MetricBaseline.commands` on `OrchestratorState`. This happens once (agent reports commands early in the session), not every iteration.

### Baseline Timing

The baseline is captured at orchestration start after auto-detect runs and verify commands are established. The baseline run records the **initial state as-is**, including any pre-existing failures:

- If the project has 3 failing tests before orchestration, the baseline records 3 failures. The agent must not increase that number.
- If the project has no tests (0 passed, 0 failed), the floor is 0 — any added tests that later break will trigger revert.
- If the initial build fails (exit code != 0), the baseline records the failure. The agent is free to fix it (floor rises) but cannot make it worse.

### Data Structures

```rust
/// A verification command with its name and command string.
#[derive(Debug, Clone)]
pub struct VerifyCommand {
    pub name: String,
    pub cmd: String,
}

/// Result of running a verification command.
#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub command_name: String,
    pub exit_code: i32,
    pub tests_passed: Option<u32>,
    pub tests_failed: Option<u32>,
    pub errors: Vec<String>,
}

/// Tracks verification baseline and results across iterations.
pub struct MetricBaseline {
    pub commands: Vec<VerifyCommand>,
    pub baseline_results: Vec<VerifyResult>,
    pub last_results: Vec<VerifyResult>,
    pub keep_count: u32,
    pub revert_count: u32,
}
```

`MetricBaseline` lives on `OrchestratorState`. Initialized at orchestration start after auto-detect runs.

### Verification Flow

```
Agent completes an iteration (SmartTrigger fires)
  → Glass captures terminal context + builds orchestrator context (existing)
  → Glass runs all verify commands sequentially
  → SOI parses each command's output (reuses existing SOI pipeline)
  → Compare results against baseline:
      │
      ├─ No regression (pass count >= baseline, fail count <= baseline, exit code 0):
      │   ├─ If tests were ADDED (pass count increased): update baseline (floor rises)
      │   ├─ Increment keep_count
      │   └─ Continue to send context to Glass Agent
      │
      └─ Regression detected (pass count dropped, fail count increased, or build broke):
          ├─ If snapshot engine enabled and has pre-iteration snapshot:
          │   └─ Restore via undo engine (file-level, granular)
          ├─ Else:
          │   └─ git revert: `git reset --hard HEAD~N` to last known good commit
          ├─ Increment revert_count
          └─ Send to Glass Agent:
              "[METRIC_GUARD] Your changes caused regression:
               Before: 45 passed, 0 failed
               After: 43 passed, 2 failed
               Errors: {SOI error records}
               Changes have been reverted. Try a different approach."
```

### Verification Timing

Verification runs AFTER the agent's response is processed (TypeText typed into PTY, command executes) and BEFORE the next context send to the Glass Agent. This means:

1. SmartTrigger fires → OrchestratorSilence event
2. Glass captures context, sends to Glass Agent
3. Glass Agent responds with TypeText (command to run)
4. Command executes in PTY, SmartTrigger fires again
5. **Before sending next context: run verify commands**
6. If regression: revert and include METRIC_GUARD message in context
7. If clean: include normal SOI context

Unlike `git log` or `git diff --stat` (which are sub-second), verification commands like `cargo test` or `npm test` can run for seconds to minutes. Running them synchronously on the main thread would freeze the UI. Instead, verification runs on a **background thread** with results sent back via a new event:

```rust
AppEvent::VerifyComplete {
    window_id: WindowId,
    session_id: u64,
    results: Vec<VerifyResult>,
}
```

Flow:
1. OrchestratorSilence handler detects it's time to verify
2. Spawns a thread that runs verify commands sequentially, captures output
3. Thread sends `AppEvent::VerifyComplete` with results
4. Main thread compares results against baseline, decides keep/revert
5. Sends context to Glass Agent (with or without METRIC_GUARD message)

During verification, the orchestrator sets `response_pending = true` to prevent duplicate context sends. SOI parsing of verification output reuses the existing async SOI pipeline.

### Revert Mechanism

**Primary: git-based revert.** At each iteration start, record the current commit SHA as `last_good_commit` on `OrchestratorState`. On regression, run `git reset --hard {last_good_commit}` to revert all changes. This is clean, reliable, and works regardless of how many files the agent changed.

**Secondary: snapshot-based revert (best-effort).** If the snapshot engine is enabled, the most recent command's pre-execution snapshot may be available via `UndoEngine::undo_latest()`. This can restore individual files from the last command only — it does NOT cover all changes across an entire iteration (which may span multiple commands). Use this as a supplement when available, not as the primary mechanism.

**Flow:**
1. Record `last_good_commit = git rev-parse HEAD` at iteration start
2. On regression: `git reset --hard {last_good_commit}`
3. If snapshot available for last command: also run `undo_latest()` to catch any untracked file changes git wouldn't cover

### Config

```toml
[agent.orchestrator]
# Optional — overrides auto-detect + agent discovery
# verify_command = "cargo test"
verify_mode = "floor"  # Default. "disabled" to turn off.
```

### Files Modified

- `src/orchestrator.rs` — `VerifyCommand`, `VerifyResult`, `MetricBaseline`, `auto_detect_verify_commands()`, GLASS_VERIFY parsing
- `src/main.rs` — Run verification in OrchestratorSilence handler, handle revert, include METRIC_GUARD in context
- `crates/glass_core/src/config.rs` — `verify_command: Option<String>`, `verify_mode: String`
- `crates/glass_renderer/src/settings_overlay.rs` — Verify Mode and Verify Command fields

---

## Feature 2: Artifact-Based Completion Signal

### Overview

An optional file path that, when created or modified, triggers the orchestrator immediately. More deterministic than silence or prompt detection.

### How It Works

1. Config field `completion_artifact` (default: `.glass/done`) — relative to project root
2. At orchestration start, Glass sets up a `notify::RecommendedWatcher` on this path
3. When the file is created or modified, the watcher callback sends `AppEvent::OrchestratorSilence` to the main event loop immediately
4. The artifact path is included in the Glass Agent's system prompt so it can instruct the implementer agent to write the file when done
5. After the orchestrator processes the event, the artifact file is deleted so it can be used again for the next cycle

### Threading Model

The `notify` watcher runs on its own thread (same as existing config hot-reload and snapshot watchers). When it detects the artifact file, it sends an `AppEvent::OrchestratorSilence` event to the main thread via the `EventLoopProxy`. This bypasses `SmartTrigger` entirely — no cross-thread coordination needed.

The existing `OrchestratorSilence` handler in main.rs processes it identically to a silence-triggered event. The artifact just provides a faster, more deterministic trigger.

### Watcher Setup

Follow the existing pattern in `crates/glass_core/src/config_watcher.rs`: spawn a named thread, create the watcher inside it, filter events to the target filename, and park the thread to keep the watcher alive.

```rust
// In main.rs, when orchestrator is enabled:
let artifact_thread = {
    let full_path = PathBuf::from(&cwd).join(&artifact_path);
    let target_filename = full_path.file_name().unwrap().to_owned();
    let proxy = event_loop_proxy.clone();
    std::thread::Builder::new()
        .name("Glass artifact watcher".into())
        .spawn(move || {
            let mut watcher = notify::recommended_watcher(move |event: Result<notify::Event, _>| {
                if let Ok(ev) = event {
                    if matches!(ev.kind, notify::EventKind::Create(_) | notify::EventKind::Modify(_)) {
                        // Filter to only our target file
                        if ev.paths.iter().any(|p| p.file_name() == Some(&target_filename)) {
                            let _ = proxy.send_event(AppEvent::OrchestratorSilence { window_id, session_id });
                        }
                    }
                }
            }).expect("Failed to create artifact watcher");
            let parent = full_path.parent().unwrap_or(&full_path);
            let _ = watcher.watch(parent, notify::RecursiveMode::NonRecursive);
            std::thread::park(); // Keep watcher alive until thread is unparked
        })
        .expect("Failed to spawn artifact watcher thread")
};
// Store `artifact_thread` handle on Processor for lifecycle management
```

### Watcher Lifecycle

- **Start:** When orchestrator is enabled (Ctrl+Shift+O) and `completion_artifact` is non-empty, spawn the watcher thread. Store the `JoinHandle` on `Processor` (e.g., `artifact_watcher_thread: Option<JoinHandle<()>>`).
- **Stop:** When orchestrator is disabled, unpark the thread (`handle.thread().unpark()`) and join it. The watcher is dropped when the thread exits.
- **Config change:** When config is hot-reloaded with a different `completion_artifact` path, stop the old watcher and start a new one with the updated path.

### Artifact Cleanup

After the `OrchestratorSilence` handler processes an artifact-triggered event, it checks if the artifact file exists and deletes it:

```rust
if artifact_path.exists() {
    let _ = std::fs::remove_file(&artifact_path);
}
```

This ensures the artifact is a one-shot signal per cycle.

### Config

```toml
[agent.orchestrator]
completion_artifact = ".glass/done"  # Default. Set to "" to disable.
```

### Files Modified

- `src/main.rs` — Set up `notify` watcher with lifecycle management (start/stop/re-create), delete artifact after processing, store `JoinHandle` on `Processor`
- `crates/glass_core/src/config.rs` — `completion_artifact: String` field with default
- `crates/glass_renderer/src/settings_overlay.rs` — Completion Artifact field (read-only)

---

## Feature 3: Bounded Iteration Mode

### Overview

Optionally limit orchestration to N iterations, then gracefully checkpoint and stop with a summary.

### How It Works

1. Config field `max_iterations: Option<u32>` (default: `None` = unlimited)
2. After each iteration completes, check `iteration >= max_iterations`
3. When limit reached:
   - Let current iteration finish (don't interrupt mid-work)
   - Trigger checkpoint: send checkpoint request to agent (reuses existing `begin_checkpoint()` flow)
   - Wait for agent to commit + write checkpoint.md (existing checkpoint polling)
   - Print summary to terminal
   - Set `orchestrator.active = false`

### Summary Format

Written to terminal via PTY write and logged to `iterations.tsv`.

**With metric guard active:**
```
[GLASS_ORCHESTRATOR] Bounded run complete (25/25 iterations)
  Metric guard: 12 kept, 3 reverted
  Baseline: 45 tests → Current: 52 tests
  Last checkpoint: .glass/checkpoint.md
  To resume: enable orchestrator (Ctrl+Shift+O)
```

**Without metric guard (verify_mode = "disabled" or no commands detected):**
```
[GLASS_ORCHESTRATOR] Bounded run complete (25/25 iterations)
  Last checkpoint: .glass/checkpoint.md
  To resume: enable orchestrator (Ctrl+Shift+O)
```

Summary data sources:
- `iteration` count — exists on `OrchestratorState`
- Keep/revert counts — from `MetricBaseline` (added by Feature 1), omitted when no verify commands
- Metric baseline → current — from `MetricBaseline`, omitted when no verify commands
- Checkpoint path — from config

### Integration with Existing Checkpoint Cycle

The bounded stop reuses `begin_checkpoint()`. The new trigger condition "iteration count reached" is checked alongside the existing "15 iterations since last checkpoint" in `should_auto_checkpoint()`:

```rust
pub fn should_stop_bounded(&self) -> bool {
    self.max_iterations
        .map(|max| self.iteration >= max)
        .unwrap_or(false)
}
```

When `should_stop_bounded()` returns true, the handler triggers a checkpoint AND sets a flag to deactivate after the checkpoint completes.

### Resumability

When the user re-enables the orchestrator (Ctrl+Shift+O), it picks up from the checkpoint. The `iteration` counter is NOT reset — it continues from where it left off. The `max_iterations` limit applies to total iterations across the session, not per-resume. To run another bounded batch, the user updates `max_iterations` in config or settings.

### Config

```toml
[agent.orchestrator]
# max_iterations = 25  # Optional. Default: unlimited. 0 = unlimited.
```

### Files Modified

- `src/orchestrator.rs` — `max_iterations` field on `OrchestratorState`, `should_stop_bounded()`, summary builder
- `src/main.rs` — Check iteration limit after each iteration, trigger checkpoint stop, print summary
- `crates/glass_core/src/config.rs` — `max_iterations: Option<u32>` field
- `crates/glass_renderer/src/settings_overlay.rs` — Max Iterations field (+/- step 5)

---

## Settings Overlay

Updated Orchestrator section (index 6) with all new fields:

| Index | Field | Type | Handler | Details |
|---|---|---|---|---|
| 0 | Enabled | toggle | activate | existing |
| 1 | Silence Timeout (sec) | +/- step 5 | increment | existing |
| 2 | Fast Trigger (sec) | +/- step 1 | increment | existing |
| 3 | Prompt Pattern | read-only | — | existing |
| 4 | PRD Path | read-only | — | existing |
| 5 | Max Retries | +/- step 1 | increment | existing |
| 6 | Verify Mode | cycle: floor/disabled | activate | NEW |
| 7 | Verify Command | read-only | — | NEW (shows auto-detected or user override) |
| 8 | Completion Artifact | read-only | — | NEW (default `.glass/done`) |
| 9 | Max Iterations | +/- step 5, 0=unlimited | increment | NEW |

Touch points for settings:
- `config.rs`: Add 4 fields to `OrchestratorSection` (`verify_command`, `verify_mode`, `completion_artifact`, `max_iterations`)
- `settings_overlay.rs`: Add 4 fields to `SettingsConfigSnapshot`, update `fields_for_section()` index 6 to show 10 fields
- `main.rs` config snapshot builder: populate 4 new fields
- `main.rs` `handle_settings_activate()`: add `(6, 6)` for Verify Mode cycle — exactly two values: `"floor"` ↔ `"disabled"`. Toggle on Enter/Space.
- `main.rs` `handle_settings_increment()`: add `(6, 9)` for Max Iterations (+/- step 5, min 0). Renumber existing `(6, 5)` Max Retries stays at `(6, 5)` — no renumbering needed since new fields are appended after existing ones.

---

## Non-Feature: Regression Detection in Stuck Detection

Per design decision, regression detection is handled entirely by the metric guard. Stuck detection (StateFingerprint) stays focused on "no change" loops. No modifications to the stuck detection system.

- **Metric guard** → "things got worse" → revert
- **Stuck detection** → "nothing changed" → intervene
- **Both fire together** → agent makes same broken change repeatedly → metric guard reverts each time, fingerprint catches the loop

---

## Cross-Cutting Concerns

### Pre-Existing Bug: `update_config_field` Dotted Path

`update_config_field()` in `crates/glass_core/src/config.rs` treats section names like `"agent.orchestrator"` as flat keys rather than traversing the nested table path `agent -> orchestrator`. This affects all orchestrator settings writes (existing and new). The existing settings appear to work because the TOML file is re-parsed after hot-reload, but the write may create duplicate keys.

This is a pre-existing bug that affects the current orchestrator settings too. It should be fixed as a **prerequisite task** before this spec is implemented: update `update_config_field()` to split dotted section names and traverse nested tables.

### Dependencies

No new external dependencies. `notify` crate is already in the dependency tree (used by snapshot file watcher and config hot-reload).

### Testing

1. **auto_detect_verify_commands()**: Unit tests with temp directories containing marker files. Test each project type detection and fallback behavior.
2. **MetricBaseline**: Unit tests for regression detection logic (pass count dropped, fail count increased, build broke). Test floor-rising behavior.
3. **GLASS_VERIFY parsing**: Unit tests for the agent response parser, same pattern as existing `parse_agent_response()`.
4. **Bounded iteration**: Unit tests for `should_stop_bounded()` with various max_iterations values.
5. **Summary builder**: Unit test for summary string generation.
6. **Artifact completion**: Integration-level — harder to unit test file watching, but the trigger mechanism (emit OrchestratorSilence) is testable.

### Backward Compatibility

- All new config fields have defaults: `verify_mode = "floor"`, `completion_artifact = ".glass/done"`, `max_iterations = None`, `verify_command = None`.
- Users who don't configure anything get: auto-detected verification with floor mode, artifact completion watching, unlimited iterations.
- Metric guard's auto-detect may fail to find a verify command (no test framework detected). In that case, `commands` is empty and verification is skipped silently — no regression, no revert.
- Existing orchestrator behavior is fully preserved when verify_mode is "disabled" and max_iterations is None.

### Orchestrator System Prompt Updates

The Glass Agent's system prompt needs to include:
- The artifact path: "When the implementer is done, have it create the file `{completion_artifact}` to signal completion."
- The GLASS_VERIFY instruction: "If you discover additional verification commands, report them with GLASS_VERIFY."
- Metric guard awareness: "After each iteration, Glass will run verification commands. If your changes cause regressions, they will be automatically reverted."
