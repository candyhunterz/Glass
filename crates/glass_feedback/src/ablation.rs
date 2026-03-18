//! Ablation engine — when the system converges (no provisionals), disable one
//! confirmed rule per run and measure impact to identify passengers.

use crate::types::{AblationResult, AttributionScore, Rule, RuleStatus, RunMetrics};

/// Select the next confirmed rule to ablate (disable for one run).
///
/// Returns `None` when:
/// - No confirmed rules exist
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
            r.last_ablation_run.is_empty() || r.last_ablation_run.as_str() <= last_sweep_run
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
            && (r.last_ablation_run.is_empty() || r.last_ablation_run.as_str() <= last_sweep_run)
    })
}

/// Evaluate whether removing a rule caused regression by comparing current
/// metrics against the rolling average of the last N runs.
///
/// Uses the same thresholds as the main regression guard:
/// - revert_rate increase > 0.10
/// - stuck_rate increase > 0.05
/// - waste_rate increase > 0.10
pub fn evaluate(metrics_history: &[RunMetrics], current_metrics: &RunMetrics) -> AblationResult {
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
    use crate::types::{AblationResult, Scope, Severity};

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

        // r1 was tested at run-100, which is > last_sweep "run-050", so skip
        let target = select_target(&rules, &[], "run-050");
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

    #[test]
    fn passenger_rule_can_be_demoted_to_stale() {
        let rule = make_rule("r1", RuleStatus::Confirmed);
        // Verify the transition is valid
        assert!(rule.status.can_transition_to(&RuleStatus::Stale));
    }

    #[test]
    fn needed_rule_stays_confirmed_with_updated_ablation_run() {
        let mut rule = make_rule("r1", RuleStatus::Confirmed);
        rule.last_ablation_run = "run-100".to_string();
        rule.ablation_result = AblationResult::Needed;
        assert_eq!(rule.status, RuleStatus::Confirmed);
        assert_eq!(rule.last_ablation_run, "run-100");
    }
}
