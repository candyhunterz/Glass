//! Regression guard — snapshot config before a run, compare metrics after,
//! detect incomplete runs (crashes), and signal Improved / Neutral / Regressed.

use std::collections::HashMap;
use std::path::Path;

use crate::io::{load_metrics_file, load_tuning_history, save_tuning_history};
use crate::types::{ConfigSnapshot, RunMetrics, TuningHistoryFile};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Outcome of comparing a run's metrics against a baseline.
#[derive(Debug, Clone)]
pub enum RegressionResult {
    /// At least one key rate improved by more than 5 pp.
    Improved,
    /// No significant change in any direction.
    Neutral,
    /// One or more regressions detected.
    Regressed { reasons: Vec<String> },
}

// ---------------------------------------------------------------------------
// Snapshot
// ---------------------------------------------------------------------------

/// Capture the current config values and provisional rule IDs as a
/// [`ConfigSnapshot`], append it to the tuning-history file on disk, and
/// return the snapshot for later comparison.
///
/// # Arguments
/// * `config_values` — key/value pairs from the active config (e.g.
///   `silence_timeout → "45"`).
/// * `provisional_rules` — IDs of rules currently in the `Provisional` state.
/// * `run_id` — identifier for the run that is about to start.
/// * `history_path` — path to `tuning-history.toml`.
pub fn take_snapshot(
    config_values: HashMap<String, String>,
    provisional_rules: Vec<String>,
    run_id: &str,
    history_path: &Path,
) -> ConfigSnapshot {
    let snapshot = ConfigSnapshot {
        run_id: run_id.to_string(),
        config_values,
        provisional_rules,
    };

    let mut history: TuningHistoryFile = load_tuning_history(history_path);
    history.snapshots.push(snapshot.clone());
    if let Err(err) = save_tuning_history(history_path, &history) {
        tracing::warn!(
            error = %err,
            path = %history_path.display(),
            "glass_feedback: failed to persist tuning history snapshot"
        );
    }

    snapshot
}

// ---------------------------------------------------------------------------
// Compare
// ---------------------------------------------------------------------------

/// Compare `current` metrics against an optional `baseline`.
///
/// Returns `None` on cold start (when `baseline` is `None`).
///
/// ## Regression criteria (any one triggers Regressed)
/// * `current.revert_rate − baseline.revert_rate > 0.10`
/// * `current.stuck_rate  − baseline.stuck_rate  > 0.05`
/// * `current.waste_rate  − baseline.waste_rate  > 0.10`
/// * `baseline.completion == "complete"` and `current.completion != "complete"`
///
/// ## Improved criteria (any one triggers Improved, provided no regression)
/// * Any of the three rates decreased by more than 0.05
pub fn compare(current: &RunMetrics, baseline: Option<&RunMetrics>) -> Option<RegressionResult> {
    let baseline = baseline?; // cold start → None

    let mut reasons: Vec<String> = Vec::new();

    // --- regression checks ---
    let revert_delta = current.revert_rate - baseline.revert_rate;
    if revert_delta > 0.10 {
        reasons.push(format!(
            "revert_rate increased by {:.3} (baseline {:.3} → current {:.3})",
            revert_delta, baseline.revert_rate, current.revert_rate
        ));
    }

    let stuck_delta = current.stuck_rate - baseline.stuck_rate;
    if stuck_delta > 0.05 {
        reasons.push(format!(
            "stuck_rate increased by {:.3} (baseline {:.3} → current {:.3})",
            stuck_delta, baseline.stuck_rate, current.stuck_rate
        ));
    }

    let waste_delta = current.waste_rate - baseline.waste_rate;
    if waste_delta > 0.10 {
        reasons.push(format!(
            "waste_rate increased by {:.3} (baseline {:.3} → current {:.3})",
            waste_delta, baseline.waste_rate, current.waste_rate
        ));
    }

    if baseline.completion == "complete" && current.completion != "complete" {
        reasons.push(format!(
            "completion regressed: baseline was \"complete\", current is \"{}\"",
            current.completion
        ));
    }

    if !reasons.is_empty() {
        return Some(RegressionResult::Regressed { reasons });
    }

    // --- improvement checks ---
    let improved = (-revert_delta > 0.05) || (-stuck_delta > 0.05) || (-waste_delta > 0.05);
    if improved {
        return Some(RegressionResult::Improved);
    }

    Some(RegressionResult::Neutral)
}

// ---------------------------------------------------------------------------
// Incomplete-run detection
// ---------------------------------------------------------------------------

/// Returns `true` if the last snapshot in the tuning-history file has no
/// corresponding entry in the metrics file — indicating the previous run
/// crashed or was killed before writing metrics.
///
/// Returns `false` when:
/// * The history file is empty (nothing to check).
/// * Every snapshot has a matching metrics entry.
pub fn check_incomplete_run(history_path: &Path, metrics_path: &Path) -> bool {
    let history = load_tuning_history(history_path);
    let last_snapshot = match history.snapshots.last() {
        Some(s) => s,
        None => return false,
    };

    let metrics = load_metrics_file(metrics_path);
    let found = metrics
        .runs
        .iter()
        .any(|r| r.run_id == last_snapshot.run_id);

    !found
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tempfile::TempDir;

    use super::*;
    use crate::io::{load_tuning_history, save_metrics_file};
    use crate::types::{RunMetrics, RunMetricsFile};

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn base_metrics(run_id: &str) -> RunMetrics {
        RunMetrics {
            run_id: run_id.to_string(),
            project_root: "/tmp/project".to_string(),
            iterations: 20,
            duration_secs: 1200,
            revert_rate: 0.10,
            stuck_rate: 0.05,
            waste_rate: 0.10,
            checkpoint_rate: 0.20,
            completion: "complete".to_string(),
            prd_items_completed: 8,
            prd_items_total: 10,
            kickoff_duration_secs: 60,
            rule_firings: vec![],
        }
    }

    fn config_values() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("silence_timeout".to_string(), "45".to_string());
        m.insert("max_retries".to_string(), "5".to_string());
        m
    }

    // -----------------------------------------------------------------------
    // take_snapshot tests
    // -----------------------------------------------------------------------

    #[test]
    fn take_snapshot_persists() {
        let dir = TempDir::new().unwrap();
        let history_path = dir.path().join("tuning-history.toml");

        let snapshot = take_snapshot(
            config_values(),
            vec!["rule-001".to_string()],
            "run-001",
            &history_path,
        );

        assert_eq!(snapshot.run_id, "run-001");

        let loaded = load_tuning_history(&history_path);
        assert_eq!(loaded.snapshots.len(), 1);
        assert_eq!(loaded.snapshots[0].run_id, "run-001");
        assert_eq!(
            loaded.snapshots[0]
                .config_values
                .get("silence_timeout")
                .map(String::as_str),
            Some("45")
        );
        assert_eq!(loaded.snapshots[0].provisional_rules, vec!["rule-001"]);
    }

    // -----------------------------------------------------------------------
    // compare tests
    // -----------------------------------------------------------------------

    #[test]
    fn compare_cold_start() {
        let current = base_metrics("run-001");
        let result = compare(&current, None);
        assert!(result.is_none(), "cold start must return None");
    }

    #[test]
    fn compare_regression() {
        let baseline = base_metrics("run-001");
        let mut current = base_metrics("run-002");
        // revert_rate increases by 0.15 — exceeds the 0.10 threshold
        current.revert_rate = baseline.revert_rate + 0.15;

        let result = compare(&current, Some(&baseline));
        match result {
            Some(RegressionResult::Regressed { reasons }) => {
                assert!(!reasons.is_empty(), "at least one reason must be reported");
                assert!(
                    reasons.iter().any(|r| r.contains("revert_rate")),
                    "reason must mention revert_rate"
                );
            }
            other => panic!("expected Regressed, got {other:?}"),
        }
    }

    #[test]
    fn compare_improved() {
        let baseline = base_metrics("run-001");
        let mut current = base_metrics("run-002");
        // revert_rate drops by 0.08 — exceeds the 0.05 improvement threshold
        current.revert_rate = baseline.revert_rate - 0.08;

        let result = compare(&current, Some(&baseline));
        match result {
            Some(RegressionResult::Improved) => {}
            other => panic!("expected Improved, got {other:?}"),
        }
    }

    #[test]
    fn compare_neutral() {
        let baseline = base_metrics("run-001");
        let mut current = base_metrics("run-002");
        // Small changes within both thresholds
        current.revert_rate = baseline.revert_rate + 0.02;
        current.stuck_rate = baseline.stuck_rate - 0.01;
        current.waste_rate = baseline.waste_rate + 0.03;

        let result = compare(&current, Some(&baseline));
        match result {
            Some(RegressionResult::Neutral) => {}
            other => panic!("expected Neutral, got {other:?}"),
        }
    }

    #[test]
    fn compare_completion_regression() {
        let baseline = base_metrics("run-001"); // completion = "complete"
        let mut current = base_metrics("run-002");
        current.completion = "partial".to_string();

        let result = compare(&current, Some(&baseline));
        match result {
            Some(RegressionResult::Regressed { reasons }) => {
                assert!(
                    reasons.iter().any(|r| r.contains("completion")),
                    "reason must mention completion"
                );
            }
            other => panic!("expected Regressed, got {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // check_incomplete_run tests
    // -----------------------------------------------------------------------

    #[test]
    fn incomplete_run_detected() {
        let dir = TempDir::new().unwrap();
        let history_path = dir.path().join("tuning-history.toml");
        let metrics_path = dir.path().join("metrics.toml");

        // Write a snapshot for "run-crash" but no matching metrics entry.
        take_snapshot(config_values(), vec![], "run-crash", &history_path);

        // metrics file has an entry for a different run
        let file = RunMetricsFile {
            runs: vec![base_metrics("run-001")],
        };
        save_metrics_file(&metrics_path, &file).unwrap();

        assert!(
            check_incomplete_run(&history_path, &metrics_path),
            "should detect incomplete run when snapshot has no matching metrics"
        );
    }

    #[test]
    fn complete_run_not_flagged() {
        let dir = TempDir::new().unwrap();
        let history_path = dir.path().join("tuning-history.toml");
        let metrics_path = dir.path().join("metrics.toml");

        // Write a snapshot for "run-001"
        take_snapshot(config_values(), vec![], "run-001", &history_path);

        // metrics file has a matching entry for "run-001"
        let file = RunMetricsFile {
            runs: vec![base_metrics("run-001")],
        };
        save_metrics_file(&metrics_path, &file).unwrap();

        assert!(
            !check_incomplete_run(&history_path, &metrics_path),
            "should not flag a complete run as incomplete"
        );
    }

    // -----------------------------------------------------------------------
    // Edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn take_snapshot_appends_to_existing_history() {
        let dir = TempDir::new().unwrap();
        let history_path = dir.path().join("tuning-history.toml");

        take_snapshot(config_values(), vec![], "run-001", &history_path);
        take_snapshot(config_values(), vec![], "run-002", &history_path);

        let loaded = load_tuning_history(&history_path);
        assert_eq!(loaded.snapshots.len(), 2);
        assert_eq!(loaded.snapshots[0].run_id, "run-001");
        assert_eq!(loaded.snapshots[1].run_id, "run-002");
    }

    #[test]
    fn check_incomplete_run_empty_history() {
        let dir = TempDir::new().unwrap();
        let history_path = dir.path().join("tuning-history.toml");
        let metrics_path = dir.path().join("metrics.toml");
        // No history written — should not flag incomplete
        assert!(!check_incomplete_run(&history_path, &metrics_path));
    }

    #[test]
    fn compare_multiple_regressions_all_reported() {
        let baseline = base_metrics("run-001");
        let mut current = base_metrics("run-002");
        current.revert_rate = baseline.revert_rate + 0.15;
        current.stuck_rate = baseline.stuck_rate + 0.10;
        current.waste_rate = baseline.waste_rate + 0.20;

        let result = compare(&current, Some(&baseline));
        match result {
            Some(RegressionResult::Regressed { reasons }) => {
                assert_eq!(
                    reasons.len(),
                    3,
                    "all three rate regressions must be reported"
                );
            }
            other => panic!("expected Regressed, got {other:?}"),
        }
    }
}
