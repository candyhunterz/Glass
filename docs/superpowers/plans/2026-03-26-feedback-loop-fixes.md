# Feedback Loop Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix three feedback loop gaps: ConfigTuning provisional lifecycle with regression rollback, prompt hints injection into agent system prompt, and script generation escalation trigger.

**Architecture:** Three independent fixes in `glass_feedback` crate + `main.rs` wiring. Fix 1 adds pending/cooldown state to `TuningHistoryFile` and evaluation logic to `on_run_end`. Fix 2 wires `prompt_hints()` into `build_system_prompt()` at all 4 spawn sites. Fix 3 replaces an unreachable condition with an escalation check.

**Tech Stack:** Rust, serde (TOML persistence), glass_feedback crate

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/glass_feedback/src/types.rs` | Modify | Add `PendingConfigChange`, `ConfigCooldown` structs |
| `crates/glass_feedback/src/analyzer.rs` | Modify | Add hard floor/ceiling clamping to 6 ConfigTuning detectors |
| `crates/glass_feedback/src/lib.rs` | Modify | Pending evaluation step, cooldown filtering, Tier 4 trigger, prompt_hints signature |
| `crates/glass_feedback/src/rules.rs` | Modify | Cap prompt_hints to 5, increment trigger_count |
| `src/main.rs` | Modify | Add hints param to build_system_prompt, wire at 4 call sites |
| `ORCHESTRATOR.md` | Modify | Update feedback lifecycle docs |

---

### Task 1: Add ConfigTuning data model

Add `PendingConfigChange` and `ConfigCooldown` types, extend `TuningHistoryFile`.

**Files:**
- Modify: `crates/glass_feedback/src/types.rs:217-228` (after `ConfigSnapshot`, in `TuningHistoryFile`)

- [ ] **Step 1: Add PendingConfigChange and ConfigCooldown structs**

In `crates/glass_feedback/src/types.rs`, add after `ConfigSnapshot` struct (after line 222):

```rust
/// A config change that has been applied but not yet evaluated.
/// Stored in tuning-history.toml. Cleared after next run evaluates it.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PendingConfigChange {
    /// Config field that was changed (e.g. "silence_timeout_secs").
    pub field: String,
    /// Value before the change.
    pub old_value: String,
    /// Value that was applied.
    pub new_value: String,
    /// Finding ID that triggered the change.
    pub finding_id: String,
    /// Run ID when the change was made.
    pub run_id: String,
}

/// Per-field cooldown after a ConfigTuning change is rejected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigCooldown {
    /// Config field under cooldown.
    pub field: String,
    /// Runs remaining before this field can be tuned again.
    pub remaining: u32,
}
```

- [ ] **Step 2: Add fields to TuningHistoryFile**

In the `TuningHistoryFile` struct (line 224), add after `snapshots`:

```rust
#[serde(default)]
pub pending: Option<PendingConfigChange>,
#[serde(default)]
pub cooldowns: Vec<ConfigCooldown>,
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p glass_feedback`
Expected: All existing tests pass (new types are just data, no logic yet).

- [ ] **Step 4: Commit**

```bash
git add crates/glass_feedback/src/types.rs
git commit -m "feat(feedback): add PendingConfigChange and ConfigCooldown types

Data model for ConfigTuning provisional lifecycle. Pending changes are
evaluated on the next run; cooldowns prevent re-tuning after rejection."
```

---

### Task 2: Add hard floors to ConfigTuning detectors

Clamp all 6 detector recommendations to prevent degenerate values.

**Files:**
- Modify: `crates/glass_feedback/src/analyzer.rs:35-202` (all 6 detect_* functions)

- [ ] **Step 1: Write failing tests**

In `crates/glass_feedback/src/analyzer.rs`, add to the `tests` module:

```rust
#[test]
fn silence_waste_respects_floor() {
    let mut data = make_run_data();
    data.config_silence_timeout = 5; // at floor
    data.avg_idle_between_iterations_secs = 100.0;
    data.iterations = 10;
    let findings = analyze(&data);
    let tuning = findings.iter().find(|f| f.id == "silence-waste");
    // 5 * 0.75 = 3.75 → rounds to 4, but floor is 5 → clamped to 5
    // Should NOT emit a finding since new == current
    assert!(tuning.is_none(), "should not emit finding when clamped to same value");
}

#[test]
fn silence_mismatch_respects_ceiling() {
    let mut data = make_run_data();
    data.config_silence_timeout = 250;
    data.fast_trigger_during_output = 3;
    let findings = analyze(&data);
    let tuning = findings.iter().find(|f| f.id == "silence-mismatch");
    assert!(tuning.is_some());
    if let FindingAction::ConfigTuning { new_value, .. } = &tuning.unwrap().action {
        let val: u64 = new_value.parse().unwrap();
        assert!(val <= 300, "silence_timeout should be capped at 300");
    }
}

#[test]
fn stuck_leniency_respects_floor() {
    let mut data = make_run_data();
    data.config_max_retries = 2; // at floor
    data.fingerprint_sequence = vec![1, 1, 1, 1, 1, 1];
    let findings = analyze(&data);
    let tuning = findings.iter().find(|f| f.id == "stuck-leniency");
    // 2 - 1 = 1, but floor is 2 → clamped, no change → no finding
    assert!(tuning.is_none(), "should not emit finding when clamped to same value");
}

#[test]
fn checkpoint_frequency_respects_floor() {
    let mut data = make_run_data();
    data.config_checkpoint_interval = 5; // at floor
    data.iterations = 25;
    data.checkpoint_count = 0;
    let findings = analyze(&data);
    let tuning = findings.iter().find(|f| f.id == "checkpoint-frequency");
    // 5 * 0.75 = 3.75 → rounds to 4, but floor is 5 → clamped, no change
    assert!(tuning.is_none(), "should not emit finding when clamped to same value");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glass_feedback -- silence_waste_respects_floor silence_mismatch_respects_ceiling stuck_leniency_respects_floor checkpoint_frequency_respects_floor`
Expected: FAIL — detectors emit findings even when values hit floor/ceiling.

- [ ] **Step 3: Implement clamping in all 6 detectors**

Add a helper function at the top of `analyzer.rs` (before `analyze`):

```rust
/// Clamp a ConfigTuning value to its hard floor/ceiling.
/// Returns None if clamped value equals current (no change needed).
fn clamp_config_value(field: &str, value: u64, current: u64) -> Option<u64> {
    let (min, max) = match field {
        "silence_timeout_secs" => (5, 300),
        "max_retries_before_stuck" => (2, 10),
        "checkpoint_interval" => (5, 50),
        _ => (0, u64::MAX),
    };
    let clamped = value.clamp(min, max);
    if clamped == current { None } else { Some(clamped) }
}
```

Then in each detector, replace the `new_value` computation + finding push with a pattern like:

**detect_silence_waste (line 69):**
```rust
let raw = ((current as f64) * 0.75).round() as u64;
if let Some(new_value) = clamp_config_value("silence_timeout_secs", raw, current) {
    // ... push finding with new_value.to_string() ...
}
```

Apply the same pattern to all 6 detectors:
- `detect_silence_mismatch` (line 41): `clamp_config_value("silence_timeout_secs", raw, current)`
- `detect_silence_waste` (line 69): `clamp_config_value("silence_timeout_secs", raw, current)`
- `detect_stuck_sensitivity` (line 101): `clamp_config_value("max_retries_before_stuck", raw, current as u64)` (current is u32, cast)
- `detect_stuck_leniency` (line 132): `clamp_config_value("max_retries_before_stuck", raw, current as u64)` — remove existing `.max(2)` since clamp handles it
- `detect_checkpoint_overhead` (line 159): `clamp_config_value("checkpoint_interval", raw, current as u64)`
- `detect_checkpoint_frequency` (line 185): `clamp_config_value("checkpoint_interval", raw, current as u64)`

Each detector: wrap the finding push in `if let Some(new_value) = clamp_config_value(...)`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p glass_feedback`
Expected: All tests pass including 4 new ones.

- [ ] **Step 5: Commit**

```bash
git add crates/glass_feedback/src/analyzer.rs
git commit -m "fix(feedback): add hard floor/ceiling clamping to ConfigTuning detectors

Prevents degenerate values: silence_timeout [5,300], max_retries [2,10],
checkpoint_interval [5,50]. Detectors skip emission when clamped value
equals current."
```

---

### Task 3: Add ConfigTuning provisional lifecycle to on_run_end

Add pending evaluation, cooldown filtering, and pending recording.

**Files:**
- Modify: `crates/glass_feedback/src/lib.rs:208-396` (on_run_end Steps 4-9)

- [ ] **Step 1: Write failing tests**

In `crates/glass_feedback/src/lib.rs`, add to the `tests` module:

```rust
#[test]
fn config_tuning_records_pending() {
    let dir = TempDir::new().unwrap();
    let project_root = dir.path().to_str().unwrap();
    let state = make_state_in_dir(&dir);
    // Trigger silence-waste: avg_idle > 2x timeout, iterations >= 5
    let mut data = make_run_data(project_root);
    data.config_silence_timeout = 30;
    data.avg_idle_between_iterations_secs = 100.0;
    data.iterations = 10;
    let result = on_run_end(state, data);
    // Should have a config change
    assert!(!result.config_changes.is_empty());
    // Should be recorded as pending in tuning-history
    let history = io::load_tuning_history(&dir.path().join(".glass").join("tuning-history.toml"));
    assert!(history.pending.is_some());
    assert_eq!(history.pending.unwrap().field, "silence_timeout_secs");
}

#[test]
fn config_tuning_reverts_on_regression() {
    let dir = TempDir::new().unwrap();
    let project_root = dir.path().to_str().unwrap();
    // Set up a pending change from a "previous run"
    let history_path = dir.path().join(".glass").join("tuning-history.toml");
    std::fs::create_dir_all(dir.path().join(".glass")).unwrap();
    let mut history = types::TuningHistoryFile::default();
    history.pending = Some(types::PendingConfigChange {
        field: "silence_timeout_secs".to_string(),
        old_value: "30".to_string(),
        new_value: "23".to_string(),
        finding_id: "silence-waste".to_string(),
        run_id: "prev-run".to_string(),
    });
    io::save_tuning_history(&history_path, &history).unwrap();

    // Run with regression (high revert rate)
    let state = make_state_in_dir(&dir);
    let mut data = make_run_data(project_root);
    data.iterations = 10;
    data.revert_count = 5; // 50% revert rate → regression
    let result = on_run_end(state, data);

    // Should revert: config_changes should contain the old value
    let revert = result.config_changes.iter().find(|(f, _, _)| f == "silence_timeout_secs");
    assert!(revert.is_some());
    let (_, _, new_val) = revert.unwrap();
    assert_eq!(new_val, "30"); // reverted to old value

    // Pending should be cleared, cooldown should be set
    let history = io::load_tuning_history(&history_path);
    assert!(history.pending.is_none());
    assert!(history.cooldowns.iter().any(|c| c.field == "silence_timeout_secs" && c.remaining == 5));
}

#[test]
fn config_tuning_confirms_on_improvement() {
    let dir = TempDir::new().unwrap();
    let project_root = dir.path().to_str().unwrap();
    let history_path = dir.path().join(".glass").join("tuning-history.toml");
    std::fs::create_dir_all(dir.path().join(".glass")).unwrap();
    let mut history = types::TuningHistoryFile::default();
    history.pending = Some(types::PendingConfigChange {
        field: "silence_timeout_secs".to_string(),
        old_value: "30".to_string(),
        new_value: "23".to_string(),
        finding_id: "silence-waste".to_string(),
        run_id: "prev-run".to_string(),
    });
    io::save_tuning_history(&history_path, &history).unwrap();

    // Run with no regression
    let state = make_state_in_dir(&dir);
    let data = make_run_data(project_root); // default data has low rates
    let result = on_run_end(state, data);

    // Pending cleared, no revert in config_changes, no cooldown
    let history = io::load_tuning_history(&history_path);
    assert!(history.pending.is_none());
    assert!(history.cooldowns.is_empty());
}

#[test]
fn config_tuning_skips_field_in_cooldown() {
    let dir = TempDir::new().unwrap();
    let project_root = dir.path().to_str().unwrap();
    let history_path = dir.path().join(".glass").join("tuning-history.toml");
    std::fs::create_dir_all(dir.path().join(".glass")).unwrap();
    let mut history = types::TuningHistoryFile::default();
    history.cooldowns.push(types::ConfigCooldown {
        field: "silence_timeout_secs".to_string(),
        remaining: 3,
    });
    io::save_tuning_history(&history_path, &history).unwrap();

    let state = make_state_in_dir(&dir);
    let mut data = make_run_data(project_root);
    data.config_silence_timeout = 30;
    data.avg_idle_between_iterations_secs = 100.0;
    data.iterations = 10;
    let result = on_run_end(state, data);

    // silence-waste would fire, but silence_timeout_secs is in cooldown
    assert!(result.config_changes.is_empty());

    // Cooldown should be decremented
    let history = io::load_tuning_history(&history_path);
    assert_eq!(history.cooldowns[0].remaining, 2);
}
```

Note: `make_state_in_dir` and `make_run_data(project_root)` are existing test helpers. If `make_run_data` constructs a baseline `RunData` with no pre-filled metrics, the preceding `data.iterations = 10` etc. overrides are sufficient for triggering detectors. Check the existing `make_run_data` implementation and adapt field assignments if needed.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glass_feedback -- config_tuning_records config_tuning_reverts config_tuning_confirms config_tuning_skips`
Expected: FAIL — no pending logic exists yet.

- [ ] **Step 3: Implement pending evaluation in on_run_end**

In `crates/glass_feedback/src/lib.rs`, in `on_run_end` between Step 4 (regression comparison, line 250) and Step 9 (extract ConfigTuning, line 358), add:

```rust
// --- Step 8b: evaluate pending ConfigTuning change ---
let tuning_history_path = state.history_path.clone(); // already points to tuning-history.toml
let mut tuning_history = io::load_tuning_history(&tuning_history_path);
let mut pending_revert: Option<(String, String, String)> = None;
let mut suppress_config_tuning = false;

if let Some(pending) = tuning_history.pending.take() {
    match &regression {
        Some(regression::RegressionResult::Regressed { .. }) => {
            // Revert: return old value as a config change
            pending_revert = Some((
                pending.field.clone(),
                pending.new_value.clone(), // "old" from caller's perspective
                pending.old_value.clone(), // revert to this
            ));
            tuning_history.cooldowns.push(types::ConfigCooldown {
                field: pending.field,
                remaining: 5,
            });
            suppress_config_tuning = true;
            tracing::info!("ConfigTuning: reverted pending change (regression detected)");
        }
        _ => {
            tracing::info!("ConfigTuning: confirmed pending change (no regression)");
        }
    }
}

// Decrement cooldowns
tuning_history.cooldowns.retain_mut(|c| {
    c.remaining = c.remaining.saturating_sub(1);
    c.remaining > 0
});
```

Then modify Step 9 to filter by cooldowns and record as pending:

```rust
// --- Step 9: extract ConfigTuning findings (max 1 per run) ---
let cooled_fields: Vec<String> = tuning_history.cooldowns.iter().map(|c| c.field.clone()).collect();
let config_changes: Vec<(String, String, String)> = if suppress_config_tuning {
    vec![] // skip new tuning this run — just reverted a bad one
} else {
    findings
        .iter()
        .filter_map(|f| {
            if let FindingAction::ConfigTuning { field, current_value, new_value } = &f.action {
                if cooled_fields.contains(field) {
                    None // field in cooldown
                } else {
                    // Record as pending
                    tuning_history.pending = Some(types::PendingConfigChange {
                        field: field.clone(),
                        old_value: current_value.clone(),
                        new_value: new_value.clone(),
                        finding_id: f.id.clone(),
                        run_id: state.snapshot.run_id.clone(),
                    });
                    Some((field.clone(), current_value.clone(), new_value.clone()))
                }
            } else {
                None
            }
        })
        .take(1)
        .collect()
};

// Include revert in config_changes if needed
let mut all_config_changes = config_changes;
if let Some(revert) = pending_revert {
    all_config_changes.push(revert);
}

// Save tuning history
let _ = io::save_tuning_history(&tuning_history_path, &tuning_history);
```

Use `all_config_changes` in the `FeedbackResult` instead of `config_changes`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p glass_feedback`
Expected: All tests pass including 4 new ones.

- [ ] **Step 5: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`

- [ ] **Step 6: Commit**

```bash
git add crates/glass_feedback/src/lib.rs
git commit -m "feat(feedback): add ConfigTuning provisional lifecycle

Config changes are now recorded as pending and evaluated on the next run.
If regression is detected, the change is reverted and the field enters
a 5-run cooldown. If improved/neutral, the change is confirmed."
```

---

### Task 4: Wire prompt hints into agent system prompt

**Files:**
- Modify: `crates/glass_feedback/src/rules.rs:166-180` (prompt_hints method)
- Modify: `crates/glass_feedback/src/lib.rs:501` (prompt_hints function)
- Modify: `src/main.rs:1015` (build_system_prompt), ~lines 2438, 2995, 7586, 8116 (4 call sites)

- [ ] **Step 1: Write failing test for injection cap and trigger_count**

In `crates/glass_feedback/src/rules.rs`, add to tests:

```rust
#[test]
fn prompt_hints_caps_at_5() {
    let mut rules = Vec::new();
    for i in 0..8 {
        let mut r = make_rule(&format!("hint-{i}"), "prompt_hint", RuleStatus::Confirmed);
        r.action_params.insert("text".to_string(), format!("Hint {i}"));
        r.added_run = format!("run-{i:03}");
        rules.push(r);
    }
    let mut engine = RuleEngine { rules };
    let hints = engine.prompt_hints_mut();
    assert_eq!(hints.len(), 5, "should cap at 5 hints");
}

#[test]
fn prompt_hints_increments_trigger_count() {
    let mut r = make_rule("hint-1", "prompt_hint", RuleStatus::Confirmed);
    r.action_params.insert("text".to_string(), "Keep PRs small".to_string());
    r.trigger_count = 0;
    let mut engine = RuleEngine { rules: vec![r] };
    let _ = engine.prompt_hints_mut();
    assert_eq!(engine.rules[0].trigger_count, 1);
}
```

Note: `make_rule(id, action, status)` is an existing test helper. If it doesn't exist in `rules.rs` tests, import it from the `lib.rs` test module or create a local version.

- [ ] **Step 2: Implement prompt_hints_mut on RuleEngine**

In `crates/glass_feedback/src/rules.rs`, add alongside existing `prompt_hints` (line 166):

```rust
/// Return hint texts, cap at 5 most recent, and increment trigger_count.
pub fn prompt_hints_mut(&mut self) -> Vec<String> {
    let mut hints: Vec<&mut Rule> = self.rules
        .iter_mut()
        .filter(|r| r.action == "prompt_hint")
        .filter(|r| matches!(r.status, RuleStatus::Confirmed | RuleStatus::Provisional))
        .collect();
    // Sort by added_run descending (most recent first)
    hints.sort_by(|a, b| b.added_run.cmp(&a.added_run));
    hints.truncate(5);
    hints.iter_mut().map(|r| {
        r.trigger_count += 1;
        r.action_params.get("text").cloned().unwrap_or_default()
    }).collect()
}
```

- [ ] **Step 3: Update lib.rs prompt_hints to use mutable version**

In `crates/glass_feedback/src/lib.rs`, change `prompt_hints` (line 501):

```rust
pub fn prompt_hints(state: &mut FeedbackState) -> Vec<String> {
    state.engine.prompt_hints_mut()
}
```

- [ ] **Step 4: Run glass_feedback tests**

Run: `cargo test -p glass_feedback`
Expected: All tests pass.

- [ ] **Step 5: Add hints parameter to build_system_prompt**

In `src/main.rs`, modify `build_system_prompt` (line 1015) signature:

```rust
fn build_system_prompt(
    config: &glass_core::agent_runtime::AgentRuntimeConfig,
    project_root: &str,
    hints: &[String],
) -> String {
```

At the end of the function, before returning the prompt string, append:

```rust
if !hints.is_empty() {
    prompt.push_str("\n[FEEDBACK_HINTS]\nThese are learned insights from previous orchestrator runs. Follow them:\n");
    for hint in hints {
        prompt.push_str(&format!("- {}\n", hint));
    }
}
```

- [ ] **Step 6: Wire hints at all 4 call sites**

At each call site, extract hints before calling `build_system_prompt`:

```rust
let hints = if let Some(ref mut fs) = self.feedback_state {
    glass_feedback::prompt_hints(fs)
} else {
    vec![]
};
let system_prompt = build_system_prompt(&agent_config, cwd, &hints);
```

Apply at:
1. ~line 2438 (respawn_orchestrator_agent)
2. ~line 2995 (initial spawn)
3. ~line 7586 (config-reload restart)
4. ~line 8116 (crash restart)

For call sites where `self.feedback_state` may not be accessible (e.g., inside a different borrow scope), extract hints earlier and pass as a local variable.

- [ ] **Step 7: Run full test suite**

Run: `cargo test --workspace && cargo clippy --workspace -- -D warnings`

- [ ] **Step 8: Commit**

```bash
git add crates/glass_feedback/src/rules.rs crates/glass_feedback/src/lib.rs src/main.rs
git commit -m "feat(feedback): inject prompt hints into agent system prompt

Tier 3 prompt hints from LLM analysis are now injected into the
orchestrator agent's system prompt at all spawn/restart sites.
Capped at 5 most recent hints. Injection increments trigger_count
so hints participate in the staleness lifecycle."
```

---

### Task 5: Fix script generation trigger

**Files:**
- Modify: `crates/glass_feedback/src/lib.rs:383-396` (Step 9c)

- [ ] **Step 1: Write failing test**

In `crates/glass_feedback/src/lib.rs`, add to tests:

```rust
#[test]
fn script_generation_fires_with_rules_and_high_waste() {
    let dir = TempDir::new().unwrap();
    let mut state = make_state_in_dir(&dir);
    // Add a rule so has_tried_lower_tiers is true
    state.engine.rules.push(make_rule("r1", "force_commit", RuleStatus::Confirmed));
    let mut data = make_run_data();
    data.iterations = 9;
    data.waste_count = 4; // > 9/3 = 3
    let result = on_run_end(state, data);
    assert!(result.script_prompt.is_some(), "Tier 4 should fire with active rules + high waste");
}

#[test]
fn script_generation_does_not_fire_without_rules() {
    let dir = TempDir::new().unwrap();
    let state = make_state_in_dir(&dir); // no rules
    let mut data = make_run_data();
    data.iterations = 9;
    data.waste_count = 4;
    let result = on_run_end(state, data);
    assert!(result.script_prompt.is_none(), "Tier 4 should not fire without any rules");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glass_feedback -- script_generation_fires script_generation_does_not`
Expected: FAIL — current condition requires `findings.is_empty()`.

- [ ] **Step 3: Replace trigger condition**

In `crates/glass_feedback/src/lib.rs`, replace Step 9c (line 383-396):

```rust
// --- Step 9c: Tier 4 script generation prompt ---
// Escalation: fire when lower tiers have been tried but problems persist.
let has_tried_lower_tiers = !project_rules_file.rules.is_empty();
let high_waste_or_stuck = data.stuck_count > data.iterations / 3
    || data.waste_count > data.iterations / 3;
let script_prompt = if script_generation && high_waste_or_stuck && has_tried_lower_tiers {
    Some(build_script_prompt(&data))
} else {
    None
};
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p glass_feedback`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/glass_feedback/src/lib.rs
git commit -m "fix(feedback): make Tier 4 script generation trigger reachable

Replace unreachable condition (findings.is_empty() + high waste) with
escalation trigger (rules exist + high waste). Tier 4 now fires when
lower tiers have been tried but problems persist."
```

---

### Task 6: Update ORCHESTRATOR.md

**Files:**
- Modify: `ORCHESTRATOR.md:384-468` (Feedback Lifecycle section)

- [ ] **Step 1: Update on_run_end documentation**

In `ORCHESTRATOR.md`, update the `on_run_end` section (line 412) to reflect the new flow. Specifically:

Replace step 8:
```
8. **Config tuning** — extract Tier 1 findings → write to config.toml (max 1 per run)
```

With:
```
8. **Pending ConfigTuning evaluation** — if a config change from the previous run is pending:
   - Regressed → revert config value, set 5-run cooldown on that field
   - Improved/Neutral → confirm change, clear pending
8b. **Config tuning** — extract Tier 1 findings (max 1 per run, skip fields in cooldown) → write to config.toml and record as pending for next-run evaluation
```

Update step 10 (script generation):
```
10. **Build script prompt** — if `script_generation = true` and lower tiers have been tried (rules exist) but waste/stuck rates exceed 33%, build Tier 4 prompt
```

- [ ] **Step 2: Update Tier 3 documentation**

In the "Feedback LLM (Tier 3)" section (line 434), update step 5:
```
5. These Tier 3 rules are injected into the orchestrator agent's system prompt (up to 5 most recent) via `prompt_hints()` at every spawn/restart. Injection increments `trigger_count` so hints participate in the staleness lifecycle.
```

- [ ] **Step 3: Update Tier 4 documentation**

In the "Script Generation (Tier 4)" section (line 452), update the trigger description:
```
When `script_generation = true` in config (default) and lower tiers have been tried (rules exist in any state) but waste/stuck rates exceed 33%:
```

- [ ] **Step 4: Commit**

```bash
git add ORCHESTRATOR.md
git commit -m "docs: update ORCHESTRATOR.md for feedback loop fixes

Document ConfigTuning provisional lifecycle (pending evaluation,
cooldowns), prompt hints injection into system prompt, and updated
Tier 4 escalation trigger."
```

---

### Task 7: Integration verification

- [ ] **Step 1: Full build and test**

```bash
cargo build --release
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

- [ ] **Step 2: Verify backward compatibility**

Confirm that an empty `tuning-history.toml` (without `pending` or `cooldowns` keys) still deserializes correctly due to `#[serde(default)]`.

## Execution Order

1. Task 1 — Data model (types only, no logic)
2. Task 2 — Hard floors (isolated to analyzer.rs)
3. Task 3 — ConfigTuning lifecycle (depends on Task 1)
4. Task 4 — Prompt hints injection (independent of Tasks 1-3)
5. Task 5 — Script generation trigger (independent)
6. Task 6 — ORCHESTRATOR.md update (after all code changes)
7. Task 7 — Integration verification

## Constraints

- `cargo test --workspace` must pass after every task
- `cargo clippy --workspace -- -D warnings` must pass after every task
- Each task is a separate commit
- No changes to user-facing config.toml schema
