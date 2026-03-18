# Feedback Loop Data Wiring Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the 11 missing RunData fields and fix 3 structural bugs so all 11 analyzer detectors receive real data and the rule lifecycle works correctly.

**Architecture:** Add tracking fields to `OrchestratorState`, accumulate data during the orchestrator loop, and populate `build_feedback_run_data()`. Fix `trigger_count` increment in `check_rules`, fix `prd_items_completed` mapping, and fix incomplete run handling.

**Tech Stack:** Rust, glass_feedback crate, OrchestratorState in orchestrator.rs, Processor in main.rs

---

## File Structure

| File | Changes |
|------|---------|
| `src/orchestrator.rs` | Add 5 new tracking fields to `OrchestratorState` |
| `src/main.rs` | Accumulate data in event handlers, populate `build_feedback_run_data()`, increment `trigger_count` |
| `crates/glass_feedback/src/lib.rs` | Use incomplete run result, snapshot config fields |
| `crates/glass_feedback/src/lifecycle.rs` | Reset `trigger_count` per-run for accurate staleness |

---

### Task 1: Add Missing Tracking Fields to OrchestratorState

**Files:**
- Modify: `src/orchestrator.rs:480-515` (OrchestratorState fields)
- Modify: `src/orchestrator.rs:539-556` (OrchestratorState::new initializers)

We need 5 new fields. The other missing data can be derived from existing state at collection time.

- [ ] **Step 1: Add fields to OrchestratorState struct**

Add after line 490 (`feedback_iteration_timestamps`):

```rust
/// Feedback loop: count of stuck events during this run.
pub feedback_stuck_count: u32,
/// Feedback loop: count of checkpoint refreshes during this run.
pub feedback_checkpoint_count: u32,
/// Feedback loop: verify pass/fail sequence (true=pass, false=fail).
pub feedback_verify_sequence: Vec<bool>,
/// Feedback loop: agent response texts for instruction overload analysis.
pub feedback_agent_responses: Vec<String>,
/// Completion reason captured from GLASS_DONE or bounded stop.
pub feedback_completion_reason: String,
```

- [ ] **Step 2: Initialize new fields in OrchestratorState::new()**

Add to the `Self { ... }` block after `feedback_iteration_timestamps: Vec::new(),`:

```rust
feedback_stuck_count: 0,
feedback_checkpoint_count: 0,
feedback_verify_sequence: Vec::new(),
feedback_agent_responses: Vec::new(),
feedback_completion_reason: String::new(),
```

- [ ] **Step 3: Build and verify compilation**

Run: `cargo build --workspace`

- [ ] **Step 4: Commit**

```
feat(orchestrator): add feedback tracking fields for stuck, checkpoint, verify, responses
```

---

### Task 2: Accumulate Data in Event Handlers

**Files:**
- Modify: `src/main.rs` — OrchestratorResponse handler (~line 6974), stuck detection (~line 7037), checkpoint synthesis (~line 7108), VerifyComplete handler (~line 7910), Done handler (~line 7152)

- [ ] **Step 1: Increment stuck_count on stuck detection**

In the `OrchestratorResponse` handler, inside the `if stuck {` block (around line 7037), add:

```rust
self.orchestrator.feedback_stuck_count += 1;
```

- [ ] **Step 2: Increment checkpoint_count on checkpoint synthesis**

All checkpoints flow through `trigger_checkpoint_synthesis` (bounded stop, auto-checkpoint, agent-requested). Add the increment at the top of that method (`src/main.rs`, `fn trigger_checkpoint_synthesis`, around line 1949):

```rust
self.orchestrator.feedback_checkpoint_count += 1;
```

Do NOT also add it in the `AgentResponse::Checkpoint` match arm — that would double-count since it also calls `trigger_checkpoint_synthesis`.

- [ ] **Step 3: Record verify pass/fail in VerifyComplete handler**

In the `AppEvent::VerifyComplete` handler (around line 7893), add the push BEFORE the `if let Some(ref mut baseline)` block (around line 7936) to avoid use-after-move on the keep path:

```rust
// Record pass/fail for flaky verification detection
let all_passed = verify_results.iter().all(|r| r.exit_code == 0);
self.orchestrator.feedback_verify_sequence.push(all_passed);
```

Place this right after `let revert_commit = self.orchestrator.last_good_commit.clone();` (around line 7934) and before `if let Some(ref mut baseline) = self.orchestrator.metric_baseline {`.

- [ ] **Step 4: Record agent responses in OrchestratorResponse handler**

In the `AgentResponse::TypeText(text)` match arm (around line 7030), after stuck detection, add:

```rust
// Cap at 50 most recent responses for instruction overload analysis
if self.orchestrator.feedback_agent_responses.len() < 50 {
    self.orchestrator.feedback_agent_responses.push(text.clone());
}
```

- [ ] **Step 5: Record completion reason in Done handler**

In the `AgentResponse::Done { summary }` match arm (around line 7152), add:

```rust
self.orchestrator.feedback_completion_reason = if summary.is_empty() {
    "complete".to_string()
} else {
    format!("complete: {}", summary)
};
```

Also in the bounded stop handler (around line 7007):

```rust
self.orchestrator.feedback_completion_reason = "bounded_limit".to_string();
```

- [ ] **Step 6: Record iteration timestamps**

In the `OrchestratorResponse` handler, right after `self.orchestrator.iteration += 1` (around line 6998), add:

```rust
self.orchestrator.feedback_iteration_timestamps.push(std::time::Instant::now());
```

- [ ] **Step 7: Build and verify compilation**

Run: `cargo build --workspace`

- [ ] **Step 8: Commit**

```
feat(orchestrator): accumulate stuck, checkpoint, verify, response data during run
```

---

### Task 3: Populate build_feedback_run_data with Real Values

**Files:**
- Modify: `src/main.rs:1807-1865` — `build_feedback_run_data()` function

- [ ] **Step 1: Replace the 11 hardcoded fields**

Replace the entire `build_feedback_run_data` function body. The changes:

```rust
fn build_feedback_run_data(&self) -> glass_feedback::RunData {
    let root = &self.orchestrator.project_root;

    // Compute avg idle time from iteration timestamps
    let avg_idle = if self.orchestrator.feedback_iteration_timestamps.len() >= 2 {
        let ts = &self.orchestrator.feedback_iteration_timestamps;
        let total: f64 = ts.windows(2)
            .map(|w| w[1].duration_since(w[0]).as_secs_f64())
            .sum();
        total / (ts.len() - 1) as f64
    } else {
        0.0
    };

    // Collect fingerprint hashes for sequence analysis
    let fingerprint_seq: Vec<u64> = self.orchestrator.recent_fingerprints
        .iter()
        .map(|fp| fp.terminal_hash)
        .collect();

    // Read PRD content for scope creep detection
    let prd_content = self.config.agent.as_ref()
        .and_then(|a| a.orchestrator.as_ref())
        .and_then(|o| {
            let prd_path = std::path::Path::new(root).join(&o.prd_path);
            std::fs::read_to_string(prd_path).ok()
        });

    // Get git diff stat for scope creep detection
    let git_diff_stat = git_cmd()
        .args(["diff", "--stat"])
        .current_dir(root)
        .output()
        .ok()
        .and_then(|o| if o.status.success() {
            String::from_utf8(o.stdout).ok()
        } else {
            None
        });

    // Get git log for post-mortem
    let git_log = git_cmd()
        .args(["log", "--oneline", "-20"])
        .current_dir(root)
        .output()
        .ok()
        .and_then(|o| if o.status.success() {
            String::from_utf8(o.stdout).ok()
        } else {
            None
        });

    glass_feedback::RunData {
        project_root: root.clone(),
        iterations: self.orchestrator.iteration,
        duration_secs: self.orchestrator_activated_at
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0),
        kickoff_duration_secs: 0, // TODO: track kickoff phase duration
        iterations_tsv: std::fs::read_to_string(
            std::path::Path::new(root).join(".glass").join("iterations.tsv"),
        ).unwrap_or_default(),
        revert_count: self.orchestrator.metric_baseline.as_ref()
            .map(|m| m.revert_count).unwrap_or(0),
        keep_count: self.orchestrator.metric_baseline.as_ref()
            .map(|m| m.keep_count).unwrap_or(0),
        stuck_count: self.orchestrator.feedback_stuck_count,
        checkpoint_count: self.orchestrator.feedback_checkpoint_count,
        waste_count: self.orchestrator.feedback_waste_iterations,
        commit_count: self.orchestrator.feedback_commit_count,
        completion_reason: self.orchestrator.feedback_completion_reason.clone(),
        prd_content,
        git_log,
        git_diff_stat,
        reverted_files: self.orchestrator.feedback_reverted_files.clone(),
        verify_pass_fail_sequence: self.orchestrator.feedback_verify_sequence.clone(),
        agent_responses: self.orchestrator.feedback_agent_responses.clone(),
        silence_interruptions: 0, // TODO: track in SmartTrigger
        fast_trigger_during_output: self.orchestrator.feedback_fast_trigger_during_output,
        avg_idle_between_iterations_secs: avg_idle,
        fingerprint_sequence: fingerprint_seq,
        config_silence_timeout: self.config.agent.as_ref()
            .and_then(|a| a.orchestrator.as_ref())
            .map(|o| o.silence_timeout_secs)
            .unwrap_or(30),
        config_max_retries: self.config.agent.as_ref()
            .and_then(|a| a.orchestrator.as_ref())
            .map(|o| o.max_retries_before_stuck)
            .unwrap_or(3),
    }
}
```

- [ ] **Step 2: Build and verify compilation**

Run: `cargo build --workspace`

- [ ] **Step 3: Commit**

```
feat(feedback): populate all RunData fields with real orchestrator data
```

---

### Task 4: Clear New Fields on Orchestrator Activation

**Files:**
- Modify: `src/main.rs` — Ctrl+Shift+O handler (~line 4320-4328) and config reload handler (~line 6415-6422)

- [ ] **Step 1: Add clears for new fields at both activation sites**

Find both places where `feedback_waste_iterations = 0` is set and add:

```rust
self.orchestrator.feedback_stuck_count = 0;
self.orchestrator.feedback_checkpoint_count = 0;
self.orchestrator.feedback_verify_sequence.clear();
self.orchestrator.feedback_agent_responses.clear();
self.orchestrator.feedback_completion_reason.clear();
```

- [ ] **Step 2: Build and verify**

Run: `cargo build --workspace`

- [ ] **Step 3: Commit**

```
fix(orchestrator): clear new feedback fields on orchestrator activation
```

---

### Task 5: Fix trigger_count — Increment When Rules Fire

**Files:**
- Modify: `crates/glass_feedback/src/rules.rs` — `check_rules()` method
- Modify: `crates/glass_feedback/src/lifecycle.rs` — `update_staleness()`

The `trigger_count` field is never incremented, so staleness detection is broken. The rule engine should increment `trigger_count` when a rule fires, and `update_staleness` should check per-run triggering.

- [ ] **Step 1: Change `check_rules` to `&mut self` and use mutable iteration**

In `rules.rs`, change the method signature and loop (around line 53):

```rust
pub fn check_rules(&mut self, state: &RunState) -> Vec<RuleAction> {
    let mut actions = Vec::new();
    for rule in &mut self.rules {  // Changed from &self.rules to &mut self.rules
        // ... existing match logic stays the same ...
        // When a rule fires (before each push to actions), add:
        // rule.trigger_count += 1;
    }
    actions
}
```

Add `rule.trigger_count += 1;` inside each match arm that pushes an action, right before the `actions.push(...)` call. There are ~10 match arms in the method.

- [ ] **Step 2: Update `check_rules` caller in lib.rs**

In `crates/glass_feedback/src/lib.rs`, change the `check_rules` function (line 253):

```rust
pub fn check_rules(state: &mut FeedbackState, run_state: &RunState) -> Vec<RuleAction> {
    state.engine.check_rules(run_state)
}
```

- [ ] **Step 3: Update caller in main.rs**

In `src/main.rs`, change `if let Some(ref feedback_state) = self.feedback_state` to `if let Some(ref mut feedback_state) = self.feedback_state` at the `check_rules` call site (~line 7626).

- [ ] **Step 4: Update ~15 existing test callsites**

In `crates/glass_feedback/src/rules.rs` tests: every `let engine = RuleEngine { ... }` must become `let mut engine = RuleEngine { ... }` (approximately 15 occurrences).

In `crates/glass_feedback/src/lib.rs` test `check_rules_delegates` (line ~588): change `let actions = check_rules(&state, &run_state)` to `let actions = check_rules(&mut state, &run_state)` and add `let mut state = ...`.

- [ ] **Step 5: Reset trigger_count at the start of each run**

In `on_run_start` in `lib.rs`, after loading the engine (line 80), reset all trigger_counts:

```rust
for rule in &mut engine.rules {
    rule.trigger_count = 0;
}
```

This makes `trigger_count` track per-run firing, and `update_staleness` correctly detects rules that didn't fire during the run.

- [ ] **Step 6: Run tests**

Run: `cargo test -p glass_feedback`
Expected: All existing tests still pass (with `mut` bindings updated).

- [ ] **Step 7: Commit**

```
fix(feedback): increment trigger_count when rules fire, reset per-run
```

---

### Task 6: Fix prd_items_completed Mapping

**Files:**
- Modify: `crates/glass_feedback/src/lib.rs` — `metrics_from_run_data()` (~line 303)

- [ ] **Step 1: Fix the metric**

Change line ~303 from:
```rust
prd_items_completed: data.commit_count,
```

To count `- [x]` items in PRD content:
```rust
prd_items_completed: data.prd_content.as_deref()
    .map(|p| p.lines()
        .filter(|l| l.trim_start().starts_with("- [x]"))
        .count() as u32)
    .unwrap_or(0),
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p glass_feedback`

- [ ] **Step 3: Commit**

```
fix(feedback): count PRD checkboxes for prd_items_completed instead of commit_count
```

---

### Task 7: Fix RunState stuck_rate and verify_alternations

**Files:**
- Modify: `src/main.rs` — `check_rules` call site (~line 7627-7656)

- [ ] **Step 1: Compute stuck_rate from actual data**

Change `stuck_rate: 0.0` to:
```rust
stuck_rate: if self.orchestrator.iteration > 0 {
    self.orchestrator.feedback_stuck_count as f64
        / self.orchestrator.iteration as f64
} else {
    0.0
},
```

- [ ] **Step 2: Compute verify_alternations from sequence**

Change `verify_alternations: 0` to:
```rust
verify_alternations: self.orchestrator.feedback_verify_sequence
    .windows(2)
    .filter(|w| w[0] != w[1])
    .count() as u32,
```

- [ ] **Step 3: Build and verify**

Run: `cargo build --workspace`

- [ ] **Step 4: Commit**

```
fix(feedback): compute stuck_rate and verify_alternations from real data
```

---

### Task 8: Snapshot Additional Config Fields

**Files:**
- Modify: `crates/glass_feedback/src/lib.rs` — `on_run_start()` config snapshot (~line 85-90)

- [ ] **Step 1: Add fields to FeedbackConfig in types.rs**

In `crates/glass_feedback/src/types.rs`, add to `FeedbackConfig` struct (around line 188):

```rust
pub silence_timeout_secs: Option<u64>,
pub max_retries_before_stuck: Option<u32>,
```

Update the `Default` impl (around line 192) to add:
```rust
silence_timeout_secs: None,
max_retries_before_stuck: None,
```

- [ ] **Step 2: Populate at both call sites in main.rs**

At both `FeedbackConfig` construction sites in main.rs (~lines 4330-4346 and ~6425-6441), add:

```rust
silence_timeout_secs: self.config.agent.as_ref()
    .and_then(|a| a.orchestrator.as_ref())
    .map(|o| o.silence_timeout_secs),
max_retries_before_stuck: self.config.agent.as_ref()
    .and_then(|a| a.orchestrator.as_ref())
    .map(|o| o.max_retries_before_stuck),
```

- [ ] **Step 3: Snapshot the values in on_run_start**

In `crates/glass_feedback/src/lib.rs`, after line 90, add:

```rust
if let Some(v) = config.silence_timeout_secs {
    config_values.insert("silence_timeout_secs".to_string(), v.to_string());
}
if let Some(v) = config.max_retries_before_stuck {
    config_values.insert("max_retries_before_stuck".to_string(), v.to_string());
}
```

- [ ] **Step 4: Update test `feedback_config_default_has_correct_values` in types.rs**

If the test asserts on `FeedbackConfig::default()`, add assertions for the new fields:
```rust
assert!(config.silence_timeout_secs.is_none());
assert!(config.max_retries_before_stuck.is_none());
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p glass_feedback`

- [ ] **Step 6: Commit**

```
feat(feedback): snapshot silence_timeout and max_retries in config for regression detection
```

---

### Task 9: Final Integration Test

**Files:**
- None new — validation only

- [ ] **Step 1: Run full test suite**

Run: `cargo test --workspace`
Expected: All tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`
Expected: No warnings

- [ ] **Step 3: Manual smoke test**

Launch Glass, navigate to a project with a PRD, enable orchestrator, let it run 2-3 iterations, disable it. Check:
- `.glass/run-metrics.toml` has non-zero `stuck_rate`, `checkpoint_rate`, etc.
- `.glass/rules.toml` has rules if any detectors fired
- `~/.glass/global-rules.toml` has global-scoped rules

- [ ] **Step 4: Commit**

```
test: verify feedback loop data wiring end-to-end
```

---

## Summary

| Task | What It Fixes | Detectors Unblocked |
|------|--------------|-------------------|
| 1-2 | Add tracking fields + accumulate data | — (infrastructure) |
| 3 | Populate RunData with real values | silence_waste, stuck_sensitivity, stuck_leniency, checkpoint_overhead, checkpoint_frequency, instruction_overload, flaky_verification, scope_creep |
| 4 | Clear fields on activation | Prevents stale data from prior runs |
| 5 | Fix trigger_count | Staleness detection, rule archival |
| 6 | Fix prd_items_completed | PRD completion tracking, regression detection |
| 7 | Fix RunState fields | verify_progress rule, run_verify_twice rule |
| 8 | Snapshot config fields | Config-drift regression detection |
