# Feedback Loop Fixes: ConfigTuning Lifecycle, Prompt Hints, Script Generation

**Date:** 2026-03-26
**Status:** Approved

## Problem

Three gaps in the glass_feedback system prevent the feedback loop from self-improving effectively:

1. **ConfigTuning (Tier 1) lacks self-correction.** Config changes are applied immediately to `config.toml` with no provisional stage, regression rollback, or cooldown. Bad recommendations persist and can ratchet values in the wrong direction (e.g., `silence-waste` repeatedly decreasing `silence_timeout_secs`).

2. **Prompt Hints (Tier 3) are never injected.** `apply_llm_findings()` saves hints to `rules.toml` and `prompt_hints()` retrieves them, but `prompt_hints()` is never called from `main.rs`. Hints are generated but sit unused.

3. **Script Generation (Tier 4) has an unreachable trigger.** The condition requires `findings.is_empty()` AND >33% stuck/waste, but any run with those rates always produces Tier 1-2 findings. The gate is never satisfied.

## Design

### Fix 1: ConfigTuning Provisional Lifecycle

Give ConfigTuning changes the same provisional lifecycle that BehavioralRules already have: record the old value, apply the change, evaluate on the next run via the existing regression guard, and revert if regressed.

#### Data Model

Add `PendingConfigChange` to `crates/glass_feedback/src/types.rs`:

```rust
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
```

Add `ConfigCooldown` for per-field cooldowns:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigCooldown {
    pub field: String,
    pub remaining: u32,
}
```

#### Persistence

Add to `TuningHistoryFile` in `types.rs` (the struct backing `tuning-history.toml`, line 223):

```rust
#[serde(default)]
pub pending: Option<PendingConfigChange>,
#[serde(default)]
pub cooldowns: Vec<ConfigCooldown>,
```

The `#[serde(default)]` attributes are required for backward compatibility — existing `tuning-history.toml` files that lack these keys must still deserialize correctly.

#### Flow Changes in `on_run_end`

Before Step 9 (extract ConfigTuning findings), insert a new step:

1. Load `tuning-history.toml`, check for `pending` config change from previous run.
2. If pending exists, evaluate using the regression guard result (already computed in Step 4):
   - **Regressed** → Revert: write `old_value` back to `config.toml`, set cooldown for that field (5 runs), log rejection. Skip new ConfigTuning findings this run.
   - **Improved/Neutral** → Confirm: clear `pending`, allow new findings.
3. When extracting new ConfigTuning findings (Step 9), skip findings whose `field` has an active cooldown.
4. If a new ConfigTuning finding is applied, record it as `pending` (not fire-and-forget). Return the config change via `FeedbackResult.config_changes` so the caller in `main.rs` writes it to disk (maintaining the existing pattern where `on_run_end` returns changes and the caller applies them).
5. Decrement all cooldowns by 1 each run, remove when 0.
6. When reverting a pending change (regression detected), include the revert in `FeedbackResult.config_changes` so the caller writes the old value back. The caller must also set `feedback_write_pending = true` to suppress the config-reload agent restart handler.

#### Hard Floors

In `analyzer.rs`, clamp all recommendations before emitting:

| Field | Min | Max |
|-------|-----|-----|
| `silence_timeout_secs` | 5 | 300 |
| `max_retries_before_stuck` | 2 | 10 |
| `checkpoint_interval` | 5 | 50 |

Apply clamping in each `detect_*` function when constructing the `new_value`.

#### Files Changed

| File | Change |
|------|--------|
| `crates/glass_feedback/src/types.rs` | Add `PendingConfigChange`, `ConfigCooldown` |
| `crates/glass_feedback/src/io.rs` | Update load/save for new `TuningHistoryFile` fields |
| `crates/glass_feedback/src/analyzer.rs` | Add hard floors to all 6 ConfigTuning detectors |
| `crates/glass_feedback/src/lib.rs` | Add pending evaluation step, cooldown filtering, pending recording |

### Fix 2: Prompt Hints Injection

Wire `prompt_hints()` into the orchestrator agent's system prompt so LLM-generated hints actually reach the agent.

#### Injection Point

In `src/main.rs`, modify `build_system_prompt()`:

1. Add parameter `hints: &[String]`.
2. If non-empty, append to the system prompt:
   ```
   [FEEDBACK_HINTS]
   These are learned insights from previous orchestrator runs. Follow them:
   - <hint 1>
   - <hint 2>
   ```

#### Call Sites

Four places call `build_system_prompt()`:

1. Initial agent spawn in session start (~line 2995) — pass hints from `feedback_state`.
2. `respawn_orchestrator_agent` (checkpoint respawn, ~line 2438) — pass hints from `feedback_state`.
3. Config-reload agent restart handler (AGTC-01, ~line 7586) — pass hints from `feedback_state`.
4. Crash restart handler (`AgentCrashed` event, ~line 8116) — pass hints from `feedback_state`.

All four must inject hints for consistent behavior. When `feedback_state` is `None`, pass empty slice.

Note: `build_system_prompt` is a free function, not a method on `self`. Callers extract hints from `self.feedback_state` before calling it. The hints are baked into the system prompt string before it reaches `try_spawn_agent` / `AgentSpawnParams`.

#### Injection Cap

Two separate caps serve different purposes:

- **Storage cap**: `max_prompt_hints` config (default 10) limits how many hint rules are created in `rules.toml`. This is unchanged.
- **Injection cap**: Hardcoded limit of 5 hints injected into the system prompt. Applied in `prompt_hints()` by sorting by `added_run` descending and taking 5. This bounds system prompt bloat regardless of storage cap.

#### Count Injection as Firing

When hints are retrieved via `prompt_hints()`, increment their `trigger_count`. This integrates hints into the staleness lifecycle:

- Hints being actively injected stay alive (trigger_count > 0 each run).
- Hints from old projects that stopped running naturally age out (trigger_count stays 0 → stale after 10 runs → archived after 15).

Implementation: `prompt_hints()` currently takes `&FeedbackState`. Change to `&mut FeedbackState` so it can increment trigger counts on the engine's rules. Update the call site in `check_rules` flow accordingly.

#### Files Changed

| File | Change |
|------|--------|
| `src/main.rs` | Add `hints` param to `build_system_prompt()`, call `prompt_hints()` at all 4 spawn/restart sites |
| `crates/glass_feedback/src/rules.rs` | Cap `prompt_hints()` to 5, increment trigger_count |
| `crates/glass_feedback/src/lib.rs` | Change `prompt_hints` signature to `&mut FeedbackState` |

### Fix 3: Script Generation Escalation Trigger

Change the Tier 4 trigger from "no findings exist" to "problems persist despite existing rules."

#### New Condition

Replace in `on_run_end` (Step 9c):

```rust
// Old: unreachable because findings are never empty when waste/stuck is high
let script_prompt = if script_generation
    && findings.is_empty()
    && (data.stuck_count > data.iterations / 3 || data.waste_count > data.iterations / 3)

// New: escalation when lower tiers have been tried but problems persist
let has_tried_lower_tiers = !project_rules_file.rules.is_empty();
let high_waste_or_stuck = data.stuck_count > data.iterations / 3
    || data.waste_count > data.iterations / 3;
let script_prompt = if script_generation
    && high_waste_or_stuck
    && has_tried_lower_tiers
```

This fires when:
- Waste or stuck rates are high (>33%) — same threshold as before.
- The system has tried lower tiers (rules exist in any state).

Tier 4 can now fire alongside Tier 1-2 findings, which is correct — scripts address structural problems that parameter tuning and behavioral rules couldn't fix.

#### Files Changed

| File | Change |
|------|--------|
| `crates/glass_feedback/src/lib.rs` | Replace Tier 4 trigger condition |

## Testing

Each fix should include unit tests:

1. **ConfigTuning lifecycle**: Test pending creation, regression revert, confirmation, cooldown countdown, hard floor clamping.
2. **Prompt hints**: Test injection cap (>5 hints → only 5 returned), trigger_count increment, empty hints pass-through.
3. **Script generation**: Test new trigger fires when rules exist + high waste, doesn't fire when no rules exist, doesn't fire when waste is low.

## Constraints

- `cargo test --workspace` must pass after each fix.
- `cargo clippy --workspace -- -D warnings` must pass.
- Each fix is a separate commit.
- No changes to config.toml schema (the `PendingConfigChange` lives in `tuning-history.toml`, not user config).
