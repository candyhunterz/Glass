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
        let fired = firing.is_some_and(|f| f.firing_count > 0);

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
/// Uses the online mean formula: avg = avg + (new - avg) / n
fn update_rolling_avg(avg: &mut MetricDeltas, new: &MetricDeltas, count: u32) {
    let n = count as f64;
    avg.revert_rate += (new.revert_rate - avg.revert_rate) / n;
    avg.stuck_rate += (new.stuck_rate - avg.stuck_rate) / n;
    avg.waste_rate += (new.waste_rate - avg.waste_rate) / n;
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
    // Scale by 5 so small-but-consistent improvements map to low scores.
    // A benefit of ~0.15 (e.g. 0.1 rate improvement per metric) yields ~0.25.
    1.0 - (avg_benefit * 5.0).clamp(0.0, 1.0)
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
