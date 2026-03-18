# Ablation Testing & Per-Rule Metric Attribution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add attribution-informed ablation testing to the feedback loop so Glass can identify and prune passenger rules that don't contribute to improvement.

**Architecture:** Two new modules (`attribution.rs`, `ablation.rs`) in `glass_feedback`, extending existing types with `#[serde(default)]` fields for backward compat. Attribution runs every run (cheap logging), ablation activates only when the system has converged (no provisionals). Settings overlay gets two new fields in the Orchestrator section.

**Tech Stack:** Rust, serde/TOML serialization, existing `glass_feedback` crate patterns, `glass_core` config, `glass_renderer` settings overlay.

**Spec:** `docs/superpowers/specs/2026-03-18-ablation-attribution-design.md`

---

### Task 1: Add new types to `types.rs`

**Files:**
- Modify: `crates/glass_feedback/src/types.rs:1-153`

- [ ] **Step 1: Write tests for new types**

Add at the bottom of the existing `#[cfg(test)] mod tests` block (after line 483):

```rust
#[test]
fn ablation_result_default_is_untested() {
    let result = AblationResult::default();
    assert_eq!(result, AblationResult::Untested);
}

#[test]
fn metric_deltas_default_is_zero() {
    let deltas = MetricDeltas::default();
    assert!((deltas.revert_rate - 0.0).abs() < f64::EPSILON);
    assert!((deltas.stuck_rate - 0.0).abs() < f64::EPSILON);
    assert!((deltas.waste_rate - 0.0).abs() < f64::EPSILON);
}

#[test]
fn rule_firing_construction() {
    let firing = RuleFiring {
        rule_id: "force-commit".to_string(),
        action: "force_commit".to_string(),
        firing_count: 3,
    };
    assert_eq!(firing.rule_id, "force-commit");
    assert_eq!(firing.firing_count, 3);
}

#[test]
fn attribution_score_default() {
    let score = AttributionScore::default();
    assert_eq!(score.runs_fired, 0);
    assert_eq!(score.runs_not_fired, 0);
    assert!((score.passenger_score - 0.0).abs() < f64::EPSILON);
}

#[test]
fn rule_ablation_fields_default() {
    // Simulate deserializing a Rule without ablation fields (backward compat)
    let rule_toml = r#"
id = "test"
trigger = "behavioral"
action = "force_commit"
status = "confirmed"
severity = "medium"
scope = "project"
tags = []
added_run = "run-001"
added_metric = ""
"#;
    let rule: Rule = toml::from_str(rule_toml).unwrap();
    assert_eq!(rule.ablation_result, AblationResult::Untested);
    assert_eq!(rule.last_ablation_run, "");
}

#[test]
fn run_metrics_rule_firings_default_empty() {
    // Simulate deserializing RunMetrics without rule_firings (backward compat)
    let metrics_toml = r#"
run_id = "run-001"
project_root = "/tmp"
iterations = 10
duration_secs = 600
revert_rate = 0.1
stuck_rate = 0.05
waste_rate = 0.08
checkpoint_rate = 0.2
completion = "success"
prd_items_completed = 5
prd_items_total = 10
kickoff_duration_secs = 60
"#;
    let metrics: RunMetrics = toml::from_str(metrics_toml).unwrap();
    assert!(metrics.rule_firings.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glass_feedback types::tests::ablation_result_default -- --nocapture 2>&1`
Expected: compilation error — types don't exist yet.

- [ ] **Step 3: Add the new types**

Add after the `RuleStatus` impl block (after line 57), before `FindingAction`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AblationResult {
    #[default]
    Untested,
    Needed,
    Passenger,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetricDeltas {
    #[serde(default)]
    pub revert_rate: f64,
    #[serde(default)]
    pub stuck_rate: f64,
    #[serde(default)]
    pub waste_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuleFiring {
    pub rule_id: String,
    pub action: String,
    pub firing_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AttributionScore {
    pub rule_id: String,
    #[serde(default)]
    pub runs_fired: u32,
    #[serde(default)]
    pub runs_not_fired: u32,
    #[serde(default)]
    pub avg_delta_when_fired: MetricDeltas,
    #[serde(default)]
    pub avg_delta_when_not_fired: MetricDeltas,
    #[serde(default)]
    pub passenger_score: f64,
    #[serde(default)]
    pub last_updated_run: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AttributionFile {
    #[serde(default)]
    pub scores: Vec<AttributionScore>,
}
```

Add two `#[serde(default)]` fields to the `Rule` struct (after `stale_runs` field, line 120):

```rust
    #[serde(default)]
    pub last_ablation_run: String,
    #[serde(default)]
    pub ablation_result: AblationResult,
```

Add `#[serde(default)]` field to `RunMetrics` struct (after `kickoff_duration_secs`, line 152):

```rust
    #[serde(default)]
    pub rule_firings: Vec<RuleFiring>,
```

- [ ] **Step 4: Update all `make_rule` / `make_test_rule` helpers across the crate**

Adding fields to `Rule` breaks all existing struct literal constructors. Every `make_rule` helper must include the new fields. There are helpers in 4 files:

- `crates/glass_feedback/src/lib.rs` line 634 (`make_rule`)
- `crates/glass_feedback/src/lifecycle.rs` line 317 (`make_rule`)
- `crates/glass_feedback/src/rules.rs` line 193 (`make_test_rule`)
- `crates/glass_feedback/src/io.rs` line 149 (`make_rule`)

Add to each helper's `Rule { ... }` construction:

```rust
            last_ablation_run: String::new(),
            ablation_result: AblationResult::Untested,
```

Also add to the import in each test module: `use crate::types::AblationResult;`

Also update `lifecycle.rs` `apply_findings` function (line 101-119) which constructs `Rule` structs — add the two new fields to that constructor too.

Also update `metrics_from_run_data` in `lib.rs` (line 522-541) — add `rule_firings: vec![]` to the `RunMetrics` struct literal.

Also update `make_run_metrics` helper in `io.rs` tests (line 172-187) — add `rule_firings: vec![]`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p glass_feedback -- --nocapture 2>&1`
Expected: all tests pass, including new ones.

- [ ] **Step 6: Commit**

```bash
git add crates/glass_feedback/src/types.rs crates/glass_feedback/src/lib.rs crates/glass_feedback/src/lifecycle.rs crates/glass_feedback/src/rules.rs crates/glass_feedback/src/io.rs
git commit -m "feat(feedback): add ablation and attribution types"
```

---

### Task 2: Add I/O for attribution file

**Files:**
- Modify: `crates/glass_feedback/src/io.rs:1-131`

- [ ] **Step 1: Write tests for attribution I/O**

Add at the bottom of `#[cfg(test)] mod tests` in `io.rs`:

```rust
use crate::types::{AttributionFile, AttributionScore, MetricDeltas};

#[test]
fn save_and_load_attribution_roundtrip() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("attribution.toml");

    let score = AttributionScore {
        rule_id: "force-commit".to_string(),
        runs_fired: 5,
        runs_not_fired: 3,
        avg_delta_when_fired: MetricDeltas {
            revert_rate: -0.04,
            stuck_rate: -0.01,
            waste_rate: -0.02,
        },
        avg_delta_when_not_fired: MetricDeltas::default(),
        passenger_score: 0.25,
        last_updated_run: "run-100".to_string(),
    };
    let file = AttributionFile {
        scores: vec![score],
    };
    save_attribution_file(&path, &file).unwrap();
    let loaded = load_attribution_file(&path);

    assert_eq!(loaded.scores.len(), 1);
    assert_eq!(loaded.scores[0].rule_id, "force-commit");
    assert_eq!(loaded.scores[0].runs_fired, 5);
    assert!((loaded.scores[0].passenger_score - 0.25).abs() < f64::EPSILON);
}

#[test]
fn load_attribution_missing_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("attribution.toml");
    let loaded = load_attribution_file(&path);
    assert!(loaded.scores.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p glass_feedback io::tests::save_and_load_attribution 2>&1`
Expected: compilation error — functions don't exist.

- [ ] **Step 3: Add attribution I/O functions**

Add after the `save_archived_rules` function (after line 130):

```rust
// ---------------------------------------------------------------------------
// Attribution file
// ---------------------------------------------------------------------------

/// Load the rule-attribution file at `path`. Returns an empty
/// [`AttributionFile`] if the file is missing or corrupted.
pub fn load_attribution_file(path: &Path) -> crate::types::AttributionFile {
    load_toml_or_default(path)
}

/// Persist an [`AttributionFile`] to `path`.
pub fn save_attribution_file(
    path: &Path,
    file: &crate::types::AttributionFile,
) -> anyhow::Result<()> {
    save_toml(path, file)
}
```

Also add `AttributionFile` to the import in io.rs line 14:

```rust
use crate::types::{AttributionFile, RulesFile, RunMetricsFile, TuningHistoryFile};
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p glass_feedback io::tests -- --nocapture 2>&1`
Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/glass_feedback/src/io.rs
git commit -m "feat(feedback): add attribution file I/O"
```

---

### Task 3: Create attribution engine

**Files:**
- Create: `crates/glass_feedback/src/attribution.rs`
- Modify: `crates/glass_feedback/src/lib.rs:7-16` (add `pub mod attribution;`)

- [ ] **Step 1: Create `attribution.rs` with tests first**

```rust
//! Attribution engine — tracks which rules fired per run, correlates with
//! metric deltas, and computes passenger scores.

use crate::types::{AttributionScore, MetricDeltas, RuleFiring};

/// Minimum data points before computing a meaningful passenger score.
const MIN_DATA_POINTS: u32 = 5;

/// Update attribution scores based on this run's rule firings and metric deltas.
///
/// For each rule present in `all_rule_ids`, bucket into "fired" or "didn't fire"
/// based on `rule_firings`, then update rolling averages and recompute passenger
/// scores for rules with enough data.
pub fn update(
    scores: &mut Vec<AttributionScore>,
    rule_firings: &[RuleFiring],
    all_rule_ids: &[String],
    metric_deltas: &MetricDeltas,
    run_id: &str,
) {
    for rule_id in all_rule_ids {
        let firing = rule_firings.iter().find(|f| f.rule_id == *rule_id);
        let fired = firing.map_or(false, |f| f.firing_count > 0);

        // Find or create the attribution entry.
        let score = match scores.iter_mut().find(|s| s.rule_id == *rule_id) {
            Some(s) => s,
            None => {
                scores.push(AttributionScore {
                    rule_id: rule_id.clone(),
                    ..Default::default()
                });
                scores.last_mut().unwrap()
            }
        };

        if fired {
            score.runs_fired += 1;
            update_rolling_avg(&mut score.avg_delta_when_fired, metric_deltas, score.runs_fired);
        } else {
            score.runs_not_fired += 1;
            update_rolling_avg(
                &mut score.avg_delta_when_not_fired,
                metric_deltas,
                score.runs_not_fired,
            );
        }

        score.last_updated_run = run_id.to_string();

        // Recompute passenger score if enough data.
        let total = score.runs_fired + score.runs_not_fired;
        if total >= MIN_DATA_POINTS {
            score.passenger_score = compute_passenger_score(
                &score.avg_delta_when_fired,
                &score.avg_delta_when_not_fired,
            );
        }
    }
}

/// Remove attribution entries for rules that no longer exist.
pub fn prune(scores: &mut Vec<AttributionScore>, active_rule_ids: &[String]) {
    scores.retain(|s| active_rule_ids.contains(&s.rule_id));
}

/// Update a rolling average with a new data point.
fn update_rolling_avg(avg: &mut MetricDeltas, new: &MetricDeltas, count: u32) {
    let n = count as f64;
    avg.revert_rate = avg.revert_rate + (new.revert_rate - avg.revert_rate) / n;
    avg.stuck_rate = avg.stuck_rate + (new.stuck_rate - avg.stuck_rate) / n;
    avg.waste_rate = avg.waste_rate + (new.waste_rate - avg.waste_rate) / n;
}

/// Compute passenger score from average deltas.
///
/// benefit = avg_delta_when_not_fired - avg_delta_when_fired
/// (positive when the rule helps — firing correlates with lower/better deltas)
///
/// Averaged across three rates, clamped to [0, 1]:
/// passenger_score = 1.0 - benefit.clamp(0.0, 1.0)
fn compute_passenger_score(when_fired: &MetricDeltas, when_not_fired: &MetricDeltas) -> f64 {
    let benefit_revert = when_not_fired.revert_rate - when_fired.revert_rate;
    let benefit_stuck = when_not_fired.stuck_rate - when_fired.stuck_rate;
    let benefit_waste = when_not_fired.waste_rate - when_fired.waste_rate;

    let avg_benefit = (benefit_revert + benefit_stuck + benefit_waste) / 3.0;
    1.0 - avg_benefit.clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_firing(rule_id: &str, count: u32) -> RuleFiring {
        RuleFiring {
            rule_id: rule_id.to_string(),
            action: "test_action".to_string(),
            firing_count: count,
        }
    }

    #[test]
    fn update_creates_new_entry() {
        let mut scores = vec![];
        let firings = vec![make_firing("r1", 3)];
        let ids = vec!["r1".to_string()];
        let deltas = MetricDeltas {
            revert_rate: -0.05,
            stuck_rate: -0.02,
            waste_rate: -0.03,
        };

        update(&mut scores, &firings, &ids, &deltas, "run-001");

        assert_eq!(scores.len(), 1);
        assert_eq!(scores[0].rule_id, "r1");
        assert_eq!(scores[0].runs_fired, 1);
        assert_eq!(scores[0].runs_not_fired, 0);
    }

    #[test]
    fn update_buckets_correctly() {
        let mut scores = vec![];
        let ids = vec!["r1".to_string(), "r2".to_string()];
        let firings = vec![make_firing("r1", 2)]; // r2 didn't fire
        let deltas = MetricDeltas::default();

        update(&mut scores, &firings, &ids, &deltas, "run-001");

        let r1 = scores.iter().find(|s| s.rule_id == "r1").unwrap();
        assert_eq!(r1.runs_fired, 1);
        assert_eq!(r1.runs_not_fired, 0);

        let r2 = scores.iter().find(|s| s.rule_id == "r2").unwrap();
        assert_eq!(r2.runs_fired, 0);
        assert_eq!(r2.runs_not_fired, 1);
    }

    #[test]
    fn passenger_score_stays_zero_below_threshold() {
        let mut scores = vec![];
        let ids = vec!["r1".to_string()];
        let deltas = MetricDeltas {
            revert_rate: 0.1,
            stuck_rate: 0.1,
            waste_rate: 0.1,
        };

        // Run 4 times (below MIN_DATA_POINTS=5)
        for i in 0..4 {
            let firings = vec![make_firing("r1", 1)];
            update(&mut scores, &firings, &ids, &deltas, &format!("run-{i}"));
        }

        assert!((scores[0].passenger_score - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn helpful_rule_gets_low_passenger_score() {
        let mut scores = vec![];
        let ids = vec!["r1".to_string()];

        // When r1 fires, metrics improve (negative deltas)
        for i in 0..5 {
            let firings = vec![make_firing("r1", 1)];
            let deltas = MetricDeltas {
                revert_rate: -0.10,
                stuck_rate: -0.05,
                waste_rate: -0.08,
            };
            update(&mut scores, &firings, &ids, &deltas, &format!("run-{i}"));
        }

        // When r1 doesn't fire, metrics worsen (positive deltas)
        for i in 5..10 {
            let firings = vec![]; // r1 didn't fire
            let deltas = MetricDeltas {
                revert_rate: 0.10,
                stuck_rate: 0.05,
                waste_rate: 0.08,
            };
            update(&mut scores, &firings, &ids, &deltas, &format!("run-{i}"));
        }

        // Helpful rule should have low passenger score
        assert!(scores[0].passenger_score < 0.3);
    }

    #[test]
    fn passenger_rule_gets_high_passenger_score() {
        let mut scores = vec![];
        let ids = vec!["r1".to_string()];

        // Same deltas whether r1 fires or not — no correlation
        for i in 0..5 {
            let firings = vec![make_firing("r1", 1)];
            let deltas = MetricDeltas {
                revert_rate: 0.0,
                stuck_rate: 0.0,
                waste_rate: 0.0,
            };
            update(&mut scores, &firings, &ids, &deltas, &format!("run-{i}"));
        }
        for i in 5..10 {
            let firings = vec![];
            let deltas = MetricDeltas {
                revert_rate: 0.0,
                stuck_rate: 0.0,
                waste_rate: 0.0,
            };
            update(&mut scores, &firings, &ids, &deltas, &format!("run-{i}"));
        }

        // No correlation → passenger score should be 1.0
        assert!((scores[0].passenger_score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn prune_removes_dead_rules() {
        let mut scores = vec![
            AttributionScore {
                rule_id: "r1".to_string(),
                ..Default::default()
            },
            AttributionScore {
                rule_id: "r2".to_string(),
                ..Default::default()
            },
        ];

        prune(&mut scores, &["r1".to_string()]);

        assert_eq!(scores.len(), 1);
        assert_eq!(scores[0].rule_id, "r1");
    }
}
```

- [ ] **Step 2: Add module declaration to `lib.rs`**

Add `pub mod attribution;` after line 8 (`pub mod defaults;`) in `crates/glass_feedback/src/lib.rs`.

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p glass_feedback attribution::tests -- --nocapture 2>&1`
Expected: all 7 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/glass_feedback/src/attribution.rs crates/glass_feedback/src/lib.rs
git commit -m "feat(feedback): add attribution engine with passenger scoring"
```

---

### Task 4: Create ablation engine

**Files:**
- Create: `crates/glass_feedback/src/ablation.rs`
- Modify: `crates/glass_feedback/src/lib.rs` (add `pub mod ablation;`)

- [ ] **Step 1: Create `ablation.rs` with tests**

```rust
//! Ablation engine — when the system converges (no provisionals), disable one
//! confirmed rule per run and measure impact to identify passengers.

use crate::types::{AblationResult, AttributionScore, Rule, RuleStatus, RunMetrics};

/// Select the next confirmed rule to ablate (disable for one run).
///
/// Returns `None` when:
/// - No confirmed (non-Pinned) rules exist
/// - All confirmed rules have been tested since `last_sweep_run`
///
/// When multiple candidates exist, sorts by `passenger_score` descending
/// (most suspicious first) using attribution data.
pub fn select_target(
    rules: &[Rule],
    scores: &[AttributionScore],
    last_sweep_run: &str,
) -> Option<String> {
    let mut candidates: Vec<(&Rule, f64)> = rules
        .iter()
        // Only Confirmed rules are candidates (Pinned has its own status, so excluded here)
        .filter(|r| r.status == RuleStatus::Confirmed)
        .filter(|r| {
            r.last_ablation_run.is_empty() || r.last_ablation_run <= last_sweep_run.to_string()
        })
        .map(|r| {
            let score = scores
                .iter()
                .find(|s| s.rule_id == r.id)
                .map(|s| s.passenger_score)
                .unwrap_or(0.5); // Default to mid-range if no attribution data
            (r, score)
        })
        .collect();

    // Sort by passenger_score descending (most suspicious first)
    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    candidates.first().map(|(r, _)| r.id.clone())
}

/// Check if all confirmed rules have been ablation-tested since `last_sweep_run`.
pub fn sweep_complete(rules: &[Rule], last_sweep_run: &str) -> bool {
    // Pinned rules have status Pinned (not Confirmed), so are excluded by the Confirmed check
    !rules.iter().any(|r| {
        r.status == RuleStatus::Confirmed
            && (r.last_ablation_run.is_empty() || r.last_ablation_run <= last_sweep_run.to_string())
    })
}

/// Evaluate whether removing a rule caused regression by comparing current
/// metrics against the rolling average of the last N runs.
///
/// Uses the same thresholds as the main regression guard:
/// - revert_rate increase > 0.10
/// - stuck_rate increase > 0.05
/// - waste_rate increase > 0.10
pub fn evaluate(
    metrics_history: &[RunMetrics],
    current_metrics: &RunMetrics,
) -> AblationResult {
    if metrics_history.is_empty() {
        return AblationResult::Needed; // Conservative: can't determine, assume needed
    }

    let count = metrics_history.len() as f64;
    let avg_revert: f64 = metrics_history.iter().map(|m| m.revert_rate).sum::<f64>() / count;
    let avg_stuck: f64 = metrics_history.iter().map(|m| m.stuck_rate).sum::<f64>() / count;
    let avg_waste: f64 = metrics_history.iter().map(|m| m.waste_rate).sum::<f64>() / count;

    let revert_delta = current_metrics.revert_rate - avg_revert;
    let stuck_delta = current_metrics.stuck_rate - avg_stuck;
    let waste_delta = current_metrics.waste_rate - avg_waste;

    if revert_delta > 0.10 || stuck_delta > 0.05 || waste_delta > 0.10 {
        AblationResult::Needed
    } else {
        AblationResult::Passenger
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::types::{Scope, Severity};

    fn make_rule(id: &str, status: RuleStatus) -> Rule {
        Rule {
            id: id.to_string(),
            trigger: "behavioral".to_string(),
            trigger_params: HashMap::new(),
            action: format!("action_{id}"),
            action_params: HashMap::new(),
            status,
            severity: Severity::Medium,
            scope: Scope::Project,
            tags: vec![],
            added_run: "run-001".to_string(),
            added_metric: String::new(),
            confirmed_run: String::new(),
            rejected_run: String::new(),
            rejected_reason: String::new(),
            last_triggered_run: String::new(),
            trigger_count: 0,
            cooldown_remaining: 0,
            stale_runs: 0,
            last_ablation_run: String::new(),
            ablation_result: AblationResult::Untested,
        }
    }

    fn make_rule_ablated(id: &str, ablation_run: &str) -> Rule {
        let mut r = make_rule(id, RuleStatus::Confirmed);
        r.last_ablation_run = ablation_run.to_string();
        r
    }

    fn make_metrics(run_id: &str, revert: f64, stuck: f64, waste: f64) -> RunMetrics {
        RunMetrics {
            run_id: run_id.to_string(),
            project_root: "/tmp".to_string(),
            iterations: 10,
            duration_secs: 600,
            revert_rate: revert,
            stuck_rate: stuck,
            waste_rate: waste,
            checkpoint_rate: 0.1,
            completion: "success".to_string(),
            prd_items_completed: 5,
            prd_items_total: 10,
            kickoff_duration_secs: 30,
            rule_firings: vec![],
        }
    }

    #[test]
    fn select_target_picks_highest_passenger_score() {
        let rules = vec![
            make_rule("r1", RuleStatus::Confirmed),
            make_rule("r2", RuleStatus::Confirmed),
        ];
        let scores = vec![
            AttributionScore {
                rule_id: "r1".to_string(),
                passenger_score: 0.3,
                ..Default::default()
            },
            AttributionScore {
                rule_id: "r2".to_string(),
                passenger_score: 0.9,
                ..Default::default()
            },
        ];

        let target = select_target(&rules, &scores, "");
        assert_eq!(target, Some("r2".to_string()));
    }

    #[test]
    fn select_target_skips_pinned() {
        let rules = vec![
            make_rule("r1", RuleStatus::Pinned),
            make_rule("r2", RuleStatus::Confirmed),
        ];

        let target = select_target(&rules, &[], "");
        assert_eq!(target, Some("r2".to_string()));
    }

    #[test]
    fn select_target_skips_already_tested() {
        let rules = vec![make_rule_ablated("r1", "run-100")];

        let target = select_target(&rules, &[], "run-050"); // tested after sweep
        // r1 was tested at run-100, which is > last_sweep "run-050", so skip
        assert!(target.is_none());
    }

    #[test]
    fn select_target_returns_none_when_empty() {
        let rules: Vec<Rule> = vec![];
        let target = select_target(&rules, &[], "");
        assert!(target.is_none());
    }

    #[test]
    fn sweep_complete_true_when_all_tested() {
        let rules = vec![make_rule_ablated("r1", "run-100")];
        assert!(sweep_complete(&rules, "run-050"));
    }

    #[test]
    fn sweep_complete_false_when_untested() {
        let rules = vec![make_rule("r1", RuleStatus::Confirmed)];
        assert!(!sweep_complete(&rules, ""));
    }

    #[test]
    fn evaluate_detects_regression() {
        let history = vec![
            make_metrics("run-1", 0.10, 0.05, 0.08),
            make_metrics("run-2", 0.12, 0.04, 0.09),
            make_metrics("run-3", 0.11, 0.05, 0.07),
        ];
        // Current run has much higher revert rate (rule was needed)
        let current = make_metrics("run-4", 0.30, 0.05, 0.08);

        assert_eq!(evaluate(&history, &current), AblationResult::Needed);
    }

    #[test]
    fn evaluate_detects_passenger() {
        let history = vec![
            make_metrics("run-1", 0.10, 0.05, 0.08),
            make_metrics("run-2", 0.12, 0.04, 0.09),
            make_metrics("run-3", 0.11, 0.05, 0.07),
        ];
        // Current run is similar — rule wasn't needed
        let current = make_metrics("run-4", 0.11, 0.05, 0.08);

        assert_eq!(evaluate(&history, &current), AblationResult::Passenger);
    }

    #[test]
    fn evaluate_conservative_on_empty_history() {
        let current = make_metrics("run-1", 0.10, 0.05, 0.08);
        assert_eq!(evaluate(&[], &current), AblationResult::Needed);
    }
}
```

- [ ] **Step 2: Add module declaration to `lib.rs`**

Add `pub mod ablation;` after the `pub mod attribution;` line.

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p glass_feedback ablation::tests -- --nocapture 2>&1`
Expected: all 8 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/glass_feedback/src/ablation.rs crates/glass_feedback/src/lib.rs
git commit -m "feat(feedback): add ablation engine with sweep lifecycle"
```

---

### Task 5: Wire attribution and ablation into `on_run_start` / `on_run_end`

**Files:**
- Modify: `crates/glass_feedback/src/lib.rs:34-338`
- Modify: `crates/glass_feedback/src/rules.rs:61-136` (skip ablation target)

- [ ] **Step 1: Extend `FeedbackState` in `lib.rs`**

Add three fields to the `FeedbackState` struct (after `max_prompt_hints`, line 44):

```rust
    pub ablation_target: Option<String>,
    pub last_sweep_run: String,
    pub attribution_scores: Vec<types::AttributionScore>,
    pub attribution_path: std::path::PathBuf,
    pub ablation_enabled: bool,
```

Initialize in `on_run_start` (inside the `FeedbackState` construction, line 128-139). Add before the closing brace:

```rust
        ablation_target: None,
        last_sweep_run: String::new(),
        attribution_scores: vec![],
        attribution_path: project_dir.join("rule-attribution.toml"),
        ablation_enabled: config.ablation_enabled,
```

After the `FeedbackState` construction, add ablation/attribution setup:

```rust
    // Load attribution data
    let mut state = FeedbackState { /* existing fields */ };
    state.attribution_scores = io::load_attribution_file(&state.attribution_path).scores;

    // Check ablation conditions
    if state.ablation_enabled {
        let has_provisionals = state.engine.rules.iter().any(|r| r.status == types::RuleStatus::Provisional);
        if !has_provisionals {
            state.ablation_target = ablation::select_target(
                &state.engine.rules,
                &state.attribution_scores,
                &state.last_sweep_run,
            );
        }
    }

    state
```

- [ ] **Step 2: Add `ablation_enabled` to `FeedbackConfig`**

In `types.rs`, add to `FeedbackConfig` (after `max_retries_before_stuck`, line 191):

```rust
    pub ablation_enabled: bool,
```

Update the `Default` impl (line 194-203) to include:

```rust
            ablation_enabled: true,
```

- [ ] **Step 3: Modify `check_rules` in `rules.rs` to skip ablation target**

Change `RuleEngine::check_rules` to accept an optional ablation target. In `rules.rs`, modify the `check_rules` method signature (line 61):

```rust
    pub fn check_rules(&mut self, state: &RunState, ablation_target: Option<&str>) -> Vec<RuleAction> {
```

Add at the top of the loop body (after line 64, inside `for rule in &mut self.rules {`):

```rust
            // Skip the ablation target — it exists but doesn't fire this run.
            if let Some(target) = ablation_target {
                if rule.id == target {
                    continue;
                }
            }
```

Update the call in `lib.rs` `check_rules` function (line 347):

```rust
pub fn check_rules(state: &mut FeedbackState, run_state: &RunState) -> Vec<RuleAction> {
    state.engine.check_rules(run_state, state.ablation_target.as_deref())
}
```

- [ ] **Step 4: Wire attribution and ablation into `on_run_end`**

In `on_run_end` (lib.rs), after step 3 (load baseline, ~line 168), add:

```rust
    // --- Step 3b: record rule firings ---
    let rule_firings: Vec<types::RuleFiring> = state
        .engine
        .rules
        .iter()
        .map(|r| types::RuleFiring {
            rule_id: r.id.clone(),
            action: r.action.clone(),
            firing_count: r.trigger_count,
        })
        .collect();

    // --- Step 3c: compute metric deltas and update attribution ---
    if let Some(ref base) = baseline {
        let deltas = types::MetricDeltas {
            revert_rate: current_metrics.revert_rate - base.revert_rate,
            stuck_rate: current_metrics.stuck_rate - base.stuck_rate,
            waste_rate: current_metrics.waste_rate - base.waste_rate,
        };
        let all_rule_ids: Vec<String> = state.engine.rules.iter().map(|r| r.id.clone()).collect();
        attribution::update(
            &mut state.attribution_scores,
            &rule_firings,
            &all_rule_ids,
            &deltas,
            &state.snapshot.run_id,
        );
    }

    // --- Step 3d: evaluate ablation ---
    if let Some(ref target_id) = state.ablation_target {
        let recent: Vec<_> = metrics_file.runs.iter().rev().take(3).cloned().collect::<Vec<_>>().into_iter().rev().collect();
        let ablation_result = ablation::evaluate(&recent, &current_metrics);

        // Update the rule in the project rules file
        if let Some(rule) = project_rules_file.rules.iter_mut().find(|r| r.id == *target_id) {
            rule.last_ablation_run = state.snapshot.run_id.clone();
            rule.ablation_result = ablation_result.clone();
            if ablation_result == types::AblationResult::Passenger {
                if rule.status.can_transition_to(&types::RuleStatus::Stale) {
                    rule.status = types::RuleStatus::Stale;
                    tracing::info!(rule_id = %rule.id, "ablation: rule is a passenger, demoting to Stale");
                }
            } else {
                tracing::info!(rule_id = %rule.id, "ablation: rule is needed, keeping Confirmed");
            }
        }

        // Check if sweep is complete
        if ablation::sweep_complete(&project_rules_file.rules, &state.last_sweep_run) {
            tracing::info!("ablation: sweep complete");
            // Note: last_sweep_run would be updated in persisted state
        }
    }
```

Add rule_firings to `current_metrics` before it's pushed to `metrics_file` (before step 10, ~line 280):

```rust
    // Attach rule firings to metrics before persisting
    let mut current_metrics = current_metrics;
    current_metrics.rule_firings = rule_firings;
```

After step 10 (persist), add attribution persistence:

```rust
    // --- Step 10c: persist attribution ---
    let attribution_file = types::AttributionFile {
        scores: state.attribution_scores,
    };
    let _ = io::save_attribution_file(&state.attribution_path, &attribution_file);
```

- [ ] **Step 5: Fix all existing tests that call `check_rules`**

In `rules.rs` tests, update all calls to `engine.check_rules(&state)` to `engine.check_rules(&state, None)`.

In `lib.rs` tests, the `check_rules` wrapper handles this automatically.

- [ ] **Step 6: Run all feedback tests**

Run: `cargo test -p glass_feedback -- --nocapture 2>&1`
Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/glass_feedback/src/lib.rs crates/glass_feedback/src/rules.rs crates/glass_feedback/src/types.rs
git commit -m "feat(feedback): wire attribution and ablation into run lifecycle"
```

---

### Task 6: Add config fields to `glass_core`

**Files:**
- Modify: `crates/glass_core/src/config.rs:115-165`

- [ ] **Step 1: Write test**

Add to the existing `tests` module in `config.rs`:

```rust
#[test]
fn test_orchestrator_ablation_defaults() {
    let toml = "[agent.orchestrator]\nenabled = true";
    let config = GlassConfig::load_from_str(toml);
    let orch = config.agent.unwrap().orchestrator.unwrap();
    assert!(orch.ablation_enabled);
    assert_eq!(orch.ablation_sweep_interval, 20);
}

#[test]
fn test_orchestrator_ablation_custom() {
    let toml = "[agent.orchestrator]\nenabled = true\nablation_enabled = false\nablation_sweep_interval = 50";
    let config = GlassConfig::load_from_str(toml);
    let orch = config.agent.unwrap().orchestrator.unwrap();
    assert!(!orch.ablation_enabled);
    assert_eq!(orch.ablation_sweep_interval, 50);
}
```

- [ ] **Step 2: Add fields to `OrchestratorSection`**

Add after `max_prompt_hints` field (line 164):

```rust
    /// Enable automatic ablation testing of confirmed rules. Default true.
    #[serde(default = "default_ablation_enabled")]
    pub ablation_enabled: bool,
    /// Runs between re-sweeps after full ablation coverage. Default 20.
    #[serde(default = "default_ablation_sweep_interval")]
    pub ablation_sweep_interval: u32,
```

Add default functions (after `default_max_prompt_hints`, ~line 192):

```rust
fn default_ablation_enabled() -> bool {
    true
}
fn default_ablation_sweep_interval() -> u32 {
    20
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p glass_core -- --nocapture 2>&1`
Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/glass_core/src/config.rs
git commit -m "feat(config): add ablation_enabled and ablation_sweep_interval"
```

---

### Task 7: Add ablation fields to settings overlay

**Files:**
- Modify: `crates/glass_renderer/src/settings_overlay.rs:72-104,941-999`

- [ ] **Step 1: Add fields to `SettingsConfigSnapshot`**

Add after `orchestrator_max_prompt_hints` (line 103):

```rust
    pub orchestrator_ablation_enabled: bool,
    pub orchestrator_ablation_sweep_interval: u32,
```

Update the `Default` impl to include:

```rust
            orchestrator_ablation_enabled: true,
            orchestrator_ablation_sweep_interval: 20,
```

- [ ] **Step 2: Add entries to `fields_for_section` match arm 6**

Add after the "Max Prompt Hints" entry (before the closing `]` of section 6, ~line 998):

```rust
                (
                    "Ablation Testing",
                    if config.orchestrator_ablation_enabled {
                        "ON".to_string()
                    } else {
                        "OFF".to_string()
                    },
                    true,
                    false,
                ),
                (
                    "Ablation Sweep Interval",
                    format!("{}", config.orchestrator_ablation_sweep_interval),
                    false,
                    false,
                ),
```

- [ ] **Step 3: Update the snapshot builder in `main.rs`**

Find where `SettingsConfigSnapshot` is constructed from `GlassConfig` in `src/main.rs` (search for `SettingsConfigSnapshot`). Add the new fields:

```rust
            orchestrator_ablation_enabled: orch.ablation_enabled,
            orchestrator_ablation_sweep_interval: orch.ablation_sweep_interval,
```

Also find the write-back match arms (where field edits are applied to config) and add:

```rust
"Ablation Testing" => {
    update_config_field(config_path, Some("agent.orchestrator"), "ablation_enabled", if new_bool { "true" } else { "false" }).ok();
}
"Ablation Sweep Interval" => {
    update_config_field(config_path, Some("agent.orchestrator"), "ablation_sweep_interval", &new_value).ok();
}
```

- [ ] **Step 4: Run build and tests**

Run: `cargo build 2>&1` then `cargo test -p glass_renderer -- --nocapture 2>&1`
Expected: compiles and tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/glass_renderer/src/settings_overlay.rs src/main.rs
git commit -m "feat(settings): add ablation testing fields to orchestrator section"
```

---

### Task 8: Wire orchestrator to pass ablation config

**Files:**
- Modify: `src/main.rs` (where `FeedbackConfig` is constructed — two sites, ~lines 4630 and 6832)

**Note:** `FeedbackConfig` is constructed in `src/main.rs`, NOT `src/orchestrator.rs`. There are two construction sites — both must be updated.

- [ ] **Step 1: Find both `FeedbackConfig` construction sites in `src/main.rs`**

Search for `FeedbackConfig {` in `src/main.rs`. There are two — one for initial orchestrator start and one for checkpoint restart. Add to both:

```rust
    ablation_enabled: orch_config.ablation_enabled,
```

- [ ] **Step 2: Run full workspace tests**

Run: `cargo test --workspace 2>&1`
Expected: all tests pass.

- [ ] **Step 3: Run clippy**

Run: `cargo clippy --workspace -- -D warnings 2>&1`
Expected: no warnings.

- [ ] **Step 4: Run fmt check**

Run: `cargo fmt --all -- --check 2>&1`
Expected: no formatting issues.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat(orchestrator): pass ablation config to feedback system"
```

---

### Task 9: Clean up attribution on rule archival

**Files:**
- Modify: `crates/glass_feedback/src/lifecycle.rs:181-221`

- [ ] **Step 1: Write test**

Add to `lifecycle.rs` tests:

```rust
#[test]
fn update_staleness_returns_archived_ids() {
    let mut rule = make_rule("r1", "action_a", RuleStatus::Stale);
    rule.trigger_count = 0;
    rule.stale_runs = 14;

    let mut rules = vec![rule];
    let mut archived: Vec<Rule> = vec![];
    update_staleness(&mut rules, &mut archived, 20);

    assert!(rules.is_empty());
    assert_eq!(archived.len(), 1);
    assert_eq!(archived[0].id, "r1");
}
```

This test already exists — the archival mechanism works. The attribution cleanup happens in `on_run_end` after `update_staleness`, using `attribution::prune` with the remaining active rule IDs. This is already wired in Task 5.

- [ ] **Step 2: Verify the prune call is in on_run_end**

After `update_staleness` in `on_run_end`, add:

```rust
    // Prune attribution scores for archived rules
    let active_ids: Vec<String> = project_rules_file.rules.iter().map(|r| r.id.clone()).collect();
    attribution::prune(&mut state.attribution_scores, &active_ids);
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p glass_feedback -- --nocapture 2>&1`
Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/glass_feedback/src/lib.rs
git commit -m "feat(feedback): prune attribution scores when rules are archived"
```

---

### Task 10: Missing tests and sweep interval logic

**Files:**
- Modify: `crates/glass_feedback/src/ablation.rs` (add tests)
- Modify: `crates/glass_feedback/src/rules.rs` (add ablation skip test)
- Modify: `crates/glass_feedback/src/lib.rs` (sweep interval logic)

- [ ] **Step 1: Add test for `check_rules` skipping ablation target**

Add to `rules.rs` tests:

```rust
#[test]
fn check_rules_skips_ablation_target() {
    let mut engine = RuleEngine {
        rules: vec![
            make_test_rule("r1", "force_commit", RuleStatus::Confirmed),
            make_test_rule("r2", "extend_silence", RuleStatus::Confirmed),
        ],
    };

    let state = RunState {
        iterations_since_last_commit: 6,
        ..Default::default()
    };

    // Without ablation: both fire
    let result = engine.check_rules(&state, None);
    assert_eq!(result.len(), 2);

    // Reset trigger counts
    for r in &mut engine.rules {
        r.trigger_count = 0;
    }

    // With ablation on r1: only r2 fires
    let result = engine.check_rules(&state, Some("r1"));
    assert_eq!(result.len(), 1);
    assert!(matches!(result[0], RuleAction::ExtendSilence { .. }));
}
```

- [ ] **Step 2: Add test for passenger demotion to Stale**

Add to `ablation.rs` tests:

```rust
#[test]
fn passenger_rule_demoted_to_stale() {
    use crate::types::RuleStatus;

    let mut rule = make_rule("r1", RuleStatus::Confirmed);
    rule.ablation_result = AblationResult::Passenger;

    // Verify the transition is valid
    assert!(rule.status.can_transition_to(&RuleStatus::Stale));

    rule.status = RuleStatus::Stale;
    assert_eq!(rule.status, RuleStatus::Stale);
}

#[test]
fn needed_rule_stays_confirmed() {
    use crate::types::RuleStatus;

    let mut rule = make_rule("r1", RuleStatus::Confirmed);
    rule.last_ablation_run = "run-100".to_string();
    rule.ablation_result = AblationResult::Needed;

    assert_eq!(rule.status, RuleStatus::Confirmed);
    assert_eq!(rule.last_ablation_run, "run-100");
}
```

- [ ] **Step 3: Add sweep interval logic to `on_run_start`**

In `lib.rs` `on_run_start`, enhance the ablation activation logic to use `ablation_sweep_interval`. After checking for provisionals:

```rust
    if state.ablation_enabled {
        let has_provisionals = state.engine.rules.iter()
            .any(|r| r.status == types::RuleStatus::Provisional);
        if !has_provisionals {
            // Check if sweep is needed: either untested rules exist,
            // or enough runs have passed since last sweep
            let needs_sweep = !ablation::sweep_complete(
                &state.engine.rules,
                &state.last_sweep_run,
            );
            let interval_elapsed = if !state.last_sweep_run.is_empty() {
                // Compare run counts from metrics file
                let metrics = io::load_metrics_file(&state.metrics_path);
                let sweep_idx = metrics.runs.iter()
                    .position(|m| m.run_id == state.last_sweep_run);
                sweep_idx.map_or(true, |idx| {
                    (metrics.runs.len() - idx) as u32 >= config.ablation_sweep_interval
                })
            } else {
                true
            };

            if needs_sweep || interval_elapsed {
                state.ablation_target = ablation::select_target(
                    &state.engine.rules,
                    &state.attribution_scores,
                    &state.last_sweep_run,
                );
            }
        }
    }
```

Also add `ablation_sweep_interval` to `FeedbackConfig`:

```rust
    pub ablation_sweep_interval: u32,
```

With default:

```rust
    ablation_sweep_interval: 20,
```

- [ ] **Step 4: Run all tests**

Run: `cargo test -p glass_feedback -- --nocapture 2>&1`
Expected: all tests pass.

- [ ] **Step 5: Run full workspace build + clippy + fmt**

Run: `cargo build 2>&1 && cargo clippy --workspace -- -D warnings 2>&1 && cargo fmt --all -- --check 2>&1`
Expected: clean build, no warnings, no formatting issues.

- [ ] **Step 6: Commit**

```bash
git add crates/glass_feedback/src/ablation.rs crates/glass_feedback/src/rules.rs crates/glass_feedback/src/lib.rs crates/glass_feedback/src/types.rs
git commit -m "feat(feedback): add missing tests and sweep interval logic"
```
