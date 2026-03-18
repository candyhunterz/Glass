# Ablation Testing & Per-Rule Metric Attribution

**Date:** 2026-03-18
**Status:** Design
**Scope:** `glass_feedback` crate + settings overlay

## Problem

The self-improvement feedback loop accumulates confirmed rules and scripts over time. Single-run before/after comparison can't establish causation — rules that happen to be present during a good run get promoted even if they didn't contribute. Over many runs, the confirmed set bloats with "passengers" that never cause regression but add overhead without benefit.

## Solution

Two complementary systems:

1. **Per-rule metric attribution** — every run, record which rules fired and correlate with metric deltas. Accumulates weak but growing signal over time identifying likely passengers.
2. **Ablation testing** — when the system has converged (no provisionals), disable one confirmed rule per run and measure impact. Attribution scores order the sweep (most suspicious first). Only ablation can demote.

## Design Principle

Attribution informs, ablation confirms. Attribution is too noisy to act on alone, but useful for prioritizing ablation targets. Ablation is the single source of truth for demotion decisions.

---

## Data Model

### RunMetrics extension

Add `rule_firings` to the existing `RunMetrics` struct in `types.rs`:

```rust
pub struct RuleFiring {
    pub rule_id: String,
    pub action: String,
    pub firing_count: u32,
}
```

Serialized in `run-metrics.toml`:

```toml
[[runs]]
run_id = "run-1709715424"
# ... existing fields ...

[[runs.rule_firings]]
rule_id = "force-commit-drift"
action = "force_commit"
firing_count = 3
```

### Attribution scores

New file: `.glass/rule-attribution.toml`

```toml
[[scores]]
rule_id = "force-commit-drift"
runs_fired = 12
runs_not_fired = 3
avg_delta_when_fired = { revert_rate = -0.04, stuck_rate = -0.01, waste_rate = -0.02 }
avg_delta_when_not_fired = { revert_rate = 0.01, stuck_rate = 0.0, waste_rate = 0.03 }
passenger_score = 0.15
last_updated_run = "run-1709716000"
```

`passenger_score`: 0.0 = definitely helpful, 1.0 = definitely passenger. Computed from difference in metric improvement between "fired" and "didn't fire" buckets.

### Attribution score struct

Add to `types.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AttributionScore {
    pub rule_id: String,
    pub runs_fired: u32,
    pub runs_not_fired: u32,
    pub avg_delta_when_fired: MetricDeltas,
    pub avg_delta_when_not_fired: MetricDeltas,
    #[serde(default)]
    pub passenger_score: f64,
    #[serde(default)]
    pub last_updated_run: String,
}
```

### Rule extensions

Add to `Rule` struct in `types.rs`. Both fields use `#[serde(default)]` for backward compatibility with existing `rules.toml` files:

```rust
#[serde(default)]
pub last_ablation_run: String,          // run_id when last ablation-tested
#[serde(default)]
pub ablation_result: AblationResult,    // Untested, Needed, or Passenger
```

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub enum AblationResult {
    #[default]
    Untested,
    Needed,
    Passenger,
}
```

### FeedbackState extensions

`FeedbackState` is defined in `lib.rs` (not `types.rs`). Add:

```rust
pub ablation_target: Option<String>,  // rule_id being ablated this run
pub last_sweep_run: String,           // run_id when last full sweep completed
pub attribution_scores: Vec<AttributionScore>,
```

---

## Attribution Engine

New module: `crates/glass_feedback/src/attribution.rs`

### Public API

```rust
pub fn update(
    scores: &mut Vec<AttributionScore>,
    rule_firings: &[RuleFiring],
    metric_deltas: &MetricDeltas,
    run_id: &str,
) -> ()
```

### Computation

Called at end of every run after metrics are computed:

1. For each rule present this run, bucket into "fired" (`firing_count > 0`) or "didn't fire"
2. Update rolling averages for metric deltas in each bucket
3. For rules with 5+ data points, recompute passenger score. Since metric deltas use the convention negative = improved, a helpful rule has more negative deltas when fired:
   ```
   benefit = avg_delta_when_not_fired - avg_delta_when_fired
   ```
   `benefit` is positive when the rule helps (firing correlates with better metrics). Aggregate across all three rates (average), clamp to [0.0, 1.0]:
   ```
   passenger_score = 1.0 - benefit.clamp(0.0, 1.0)
   ```
   So: `passenger_score` near 0.0 = rule clearly helps, near 1.0 = no detectable benefit.

Below 5 data points, `passenger_score` stays at 0.0 (benefit of the doubt).

### MetricDeltas

```rust
pub struct MetricDeltas {
    pub revert_rate: f64,   // current - previous (negative = improved)
    pub stuck_rate: f64,
    pub waste_rate: f64,
}
```

### Cleanup

When a rule is archived, its attribution score is removed. No point tracking dead rules.

---

## Ablation Engine

New module: `crates/glass_feedback/src/ablation.rs`

### Public API

```rust
pub fn select_target(
    rules: &[Rule],
    scores: &[AttributionScore],
    last_sweep_run: &str,
) -> Option<String>  // rule_id to ablate

pub fn evaluate(
    metrics_history: &[RunMetrics],  // last 3 runs
    current_metrics: &RunMetrics,
) -> AblationResult  // Needed or Passenger
```

### Trigger conditions

Ablation activates when ALL are true:

1. `ablation_enabled = true` in config
2. No provisional rules exist
3. No provisional scripts exist
4. At least one confirmed rule has `last_ablation_run` empty or older than `last_sweep_run`

### Target selection

1. Gather Confirmed rules not yet tested this sweep (exclude Pinned rules — they are never ablated)
2. Sort by `passenger_score` descending (most suspicious first)
3. Pick top candidate
4. Store in `FeedbackState.ablation_target`

### Execution

During `check_rules()` in `rules.rs`, if `ablation_target` matches a rule's ID, skip its precondition evaluation. The rule exists but never fires this run.

### Result evaluation

Compare current metrics against **3-run rolling average** (not single previous run):

- Any metric regressed beyond threshold (same thresholds as regression guard) → `AblationResult::Needed`
- Metrics same or improved → `AblationResult::Passenger`

### Demotion

Passengers are demoted to **Stale** — not archived immediately. This gives 5 runs for the rule to resurrect if conditions change (e.g., different project phase). Normal staleness logic handles archival.

Rules marked "needed" stay Confirmed with `last_ablation_run` updated. They won't be re-tested until the next sweep.

### Sweep lifecycle

1. System converges (no provisionals) → ablation begins
2. One rule tested per run, ordered by passenger_score
3. After all confirmed rules tested → `last_sweep_run = current_run_id`
4. Ablation goes idle until re-sweep trigger:
   - New rule reaches Confirmed status
   - `ablation_sweep_interval` runs since last sweep (default 20)

### Edge case: concurrent regression

If an ablation run regresses due to something unrelated (difficult task, bad LLM output), the ablated rule gets marked "needed" even if it's actually a passenger. This is the conservative choice — false positives (keeping a passenger) are cheaper than false negatives (demoting a needed rule). Next sweep re-tests it.

---

## Integration into on_run_start / on_run_end

### on_run_start additions

After existing rule engine load:

1. Load attribution data from `.glass/rule-attribution.toml`
2. Check ablation trigger conditions
3. If triggered, call `ablation::select_target()`, store in state
4. Pass ablation target ID to `RuleEngine` for skip-during-check

### on_run_end additions

After step 3 (load previous metrics / compute baseline), before step 4 (regression compare). The new steps need the baseline to compute deltas:

1. **Record rule firings** — collect each rule's `trigger_count` into `RunMetrics.rule_firings`
2. **Compute metric deltas** — diff current metrics against baseline: `MetricDeltas { revert_rate: current - baseline, ... }`
3. **Update attribution** — call `attribution::update()` with firings and metric deltas
4. **Evaluate ablation** — if `ablation_target` is set, call `ablation::evaluate()` using 3-run rolling average from metrics history, update rule's `ablation_result` and `last_ablation_run`, demote if passenger
5. **Save attribution** — persist updated scores to `.glass/rule-attribution.toml`

Steps 4-10 (original numbering) continue unchanged.

---

## Config

```toml
[agent.orchestrator]
# ... existing keys ...
ablation_enabled = true        # Enable automatic ablation testing (default: true)
ablation_sweep_interval = 20   # Runs between re-sweeps after full coverage
```

Added to `GlassConfig` in `crates/glass_core/src/config.rs`.

---

## Settings Overlay

Add two fields to the Orchestrator section (index 6) in `settings_overlay.rs`:

1. `SettingsConfigSnapshot`: add `orchestrator_ablation_enabled: bool` and `orchestrator_ablation_sweep_interval: u32`
2. `fields_for_section()` match arm 6: add entries using same tuple pattern:
   - `("Ablation Testing", "ON"/"OFF", true, false)` — boolean toggle
   - `("Ablation Sweep Interval", "N", false, false)` — numeric +/-

Same write-back pattern as existing orchestrator fields — changes go to `~/.glass/config.toml`, hot-reload picks them up.

---

## Files Changed

| File | Change |
|---|---|
| `crates/glass_feedback/src/attribution.rs` | **New** — attribution engine |
| `crates/glass_feedback/src/ablation.rs` | **New** — ablation engine |
| `crates/glass_feedback/src/types.rs` | Add `RuleFiring`, `AttributionScore`, `MetricDeltas`, `AblationResult` enum; extend `Rule` with `#[serde(default)]` ablation fields |
| `crates/glass_feedback/src/io.rs` | Add load/save for `rule-attribution.toml`; extend `RunMetrics` serialization with `rule_firings` |
| `crates/glass_feedback/src/lib.rs` | Extend `FeedbackState` with ablation/attribution fields; wire into `on_run_start` / `on_run_end`; export new modules |
| `crates/glass_feedback/src/lifecycle.rs` | Clean up attribution scores when rules are archived in `update_staleness()` |
| `crates/glass_feedback/src/rules.rs` | `check_rules` skips ablation target |
| `crates/glass_core/src/config.rs` | Add `ablation_enabled`, `ablation_sweep_interval` to orchestrator config |
| `crates/glass_renderer/src/settings_overlay.rs` | Add ablation fields to Orchestrator section in `SettingsConfigSnapshot` and `fields_for_section()` |
| `src/orchestrator.rs` | Pass ablation config to feedback init |

No changes to scripting, MCP, coordination, or terminal layers.

---

## Testing

- **Attribution accumulation**: feed synthetic run data across 10 runs, verify scores converge correctly for a known-helpful rule vs a known-passenger
- **Ablation trigger**: verify ablation only activates when no provisionals exist and untested rules remain
- **Target ordering**: verify highest passenger_score is selected first
- **Result evaluation**: verify 3-run rolling average comparison catches regressions and passes stable metrics
- **Demotion**: verify passenger → Stale transition, verify "needed" rules stay Confirmed
- **Sweep lifecycle**: verify sweep completes, goes idle, re-triggers on new confirmed rule or interval
- **Config**: verify settings overlay reads/writes ablation fields correctly
- **Edge case**: verify ablation during unrelated regression conservatively marks rule as needed
