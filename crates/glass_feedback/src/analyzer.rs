use crate::types::{Finding, FindingAction, FindingCategory, RunData, Scope, Severity};

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn analyze(data: &RunData) -> Vec<Finding> {
    let mut findings = Vec::new();
    findings.extend(detect_silence_mismatch(data));
    findings.extend(detect_silence_waste(data));
    findings.extend(detect_stuck_sensitivity(data));
    findings.extend(detect_stuck_leniency(data));
    findings.extend(detect_checkpoint_overhead(data));
    findings
}

// ---------------------------------------------------------------------------
// Detectors
// ---------------------------------------------------------------------------

/// If the silence timeout fired too early during active output (`fast_trigger_during_output >= 2`),
/// suggest increasing `silence_timeout_secs` by 50%.
fn detect_silence_mismatch(data: &RunData) -> Vec<Finding> {
    if data.fast_trigger_during_output < 2 {
        return vec![];
    }

    let current = data.config_silence_timeout;
    let new_value = ((current as f64) * 1.5).round() as u64;

    vec![Finding {
        id: "silence-mismatch".to_string(),
        category: FindingCategory::ConfigTuning,
        severity: Severity::High,
        action: FindingAction::ConfigTuning {
            field: "silence_timeout_secs".to_string(),
            current_value: current.to_string(),
            new_value: new_value.to_string(),
        },
        evidence: format!(
            "Silence timeout fired {} time(s) during active output; increasing timeout from {}s to {}s (+50%).",
            data.fast_trigger_during_output, current, new_value
        ),
        scope: Scope::Project,
    }]
}

/// If the average idle time between iterations is more than 2× the configured silence timeout
/// AND we have at least 5 iterations, suggest decreasing `silence_timeout_secs` by 25%.
fn detect_silence_waste(data: &RunData) -> Vec<Finding> {
    let threshold = data.config_silence_timeout as f64 * 2.0;
    if data.avg_idle_between_iterations_secs <= threshold || data.iterations < 5 {
        return vec![];
    }

    let current = data.config_silence_timeout;
    let new_value = ((current as f64) * 0.75).round() as u64;

    vec![Finding {
        id: "silence-waste".to_string(),
        category: FindingCategory::ConfigTuning,
        severity: Severity::Medium,
        action: FindingAction::ConfigTuning {
            field: "silence_timeout_secs".to_string(),
            current_value: current.to_string(),
            new_value: new_value.to_string(),
        },
        evidence: format!(
            "Average idle between iterations ({:.1}s) is more than 2× the silence timeout ({}s); decreasing timeout to {}s (-25%).",
            data.avg_idle_between_iterations_secs, current, new_value
        ),
        scope: Scope::Project,
    }]
}

/// If we got stuck at least once but waste is low (< 15% of iterations), the agent recovered
/// after being interrupted — suggest increasing `max_retries_before_stuck` by 1 to give more
/// room before declaring stuck.
fn detect_stuck_sensitivity(data: &RunData) -> Vec<Finding> {
    if data.stuck_count < 1 {
        return vec![];
    }
    let waste_ratio =
        data.waste_count as f64 / data.iterations.max(1) as f64;
    if waste_ratio >= 0.15 {
        return vec![];
    }

    let current = data.config_max_retries;
    let new_value = current + 1;

    vec![Finding {
        id: "stuck-sensitivity".to_string(),
        category: FindingCategory::ConfigTuning,
        severity: Severity::Medium,
        action: FindingAction::ConfigTuning {
            field: "max_retries_before_stuck".to_string(),
            current_value: current.to_string(),
            new_value: new_value.to_string(),
        },
        evidence: format!(
            "Agent was stuck {} time(s) but waste ratio was only {:.0}% (< 15%), suggesting progress after interruption; relaxing stuck threshold from {} to {}.",
            data.stuck_count,
            waste_ratio * 100.0,
            current,
            new_value
        ),
        scope: Scope::Project,
    }]
}

/// Count the maximum run of consecutive identical fingerprints. If that run is >= 5 AND
/// greater than `config_max_retries`, decrease `max_retries_before_stuck` by 1 (minimum 2).
fn detect_stuck_leniency(data: &RunData) -> Vec<Finding> {
    let max_run = max_consecutive_run(&data.fingerprint_sequence);
    if max_run < 5 || max_run <= data.config_max_retries as usize {
        return vec![];
    }

    let current = data.config_max_retries;
    let new_value = current.saturating_sub(1).max(2);

    vec![Finding {
        id: "stuck-leniency".to_string(),
        category: FindingCategory::ConfigTuning,
        severity: Severity::Medium,
        action: FindingAction::ConfigTuning {
            field: "max_retries_before_stuck".to_string(),
            current_value: current.to_string(),
            new_value: new_value.to_string(),
        },
        evidence: format!(
            "Fingerprint sequence had a run of {} identical values (> max_retries {}); decreasing stuck threshold from {} to {} (min 2).",
            max_run, current, current, new_value
        ),
        scope: Scope::Project,
    }]
}

/// If checkpoints are disproportionately frequent for a small run (>= 3 checkpoints and
/// < 15 iterations), flag it as overhead.
fn detect_checkpoint_overhead(data: &RunData) -> Vec<Finding> {
    if data.checkpoint_count < 3 || data.iterations >= 15 {
        return vec![];
    }

    vec![Finding {
        id: "checkpoint-overhead".to_string(),
        category: FindingCategory::ConfigTuning,
        severity: Severity::Low,
        action: FindingAction::ConfigTuning {
            field: "checkpoint_interval_iterations".to_string(),
            current_value: data.checkpoint_count.to_string(),
            new_value: "review".to_string(),
        },
        evidence: format!(
            "{} checkpoints in only {} iterations; checkpoint frequency may be too high for short runs, adding overhead.",
            data.checkpoint_count, data.iterations
        ),
        scope: Scope::Project,
    }]
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Returns the length of the longest run of consecutive identical values in `seq`.
fn max_consecutive_run(seq: &[u64]) -> usize {
    if seq.is_empty() {
        return 0;
    }
    let mut max_run = 1usize;
    let mut current_run = 1usize;
    for i in 1..seq.len() {
        if seq[i] == seq[i - 1] {
            current_run += 1;
            if current_run > max_run {
                max_run = current_run;
            }
        } else {
            current_run = 1;
        }
    }
    max_run
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- helpers ---

    fn make_config_tuning(f: &Finding) -> (&str, &str, &str) {
        if let FindingAction::ConfigTuning {
            field,
            current_value,
            new_value,
        } = &f.action
        {
            (field.as_str(), current_value.as_str(), new_value.as_str())
        } else {
            panic!("expected ConfigTuning action");
        }
    }

    // --- max_consecutive_run ---

    #[test]
    fn helper_max_consecutive_run_empty() {
        assert_eq!(max_consecutive_run(&[]), 0);
    }

    #[test]
    fn helper_max_consecutive_run_single() {
        assert_eq!(max_consecutive_run(&[42]), 1);
    }

    #[test]
    fn helper_max_consecutive_run_all_different() {
        assert_eq!(max_consecutive_run(&[1, 2, 3, 4]), 1);
    }

    #[test]
    fn helper_max_consecutive_run_all_same() {
        assert_eq!(max_consecutive_run(&[7, 7, 7, 7, 7]), 5);
    }

    #[test]
    fn helper_max_consecutive_run_mixed() {
        // 1,1,1,2,2,3 -> max run = 3
        assert_eq!(max_consecutive_run(&[1, 1, 1, 2, 2, 3]), 3);
    }

    // --- detect_silence_mismatch ---

    #[test]
    fn silence_mismatch_positive_produces_finding() {
        let data = RunData {
            fast_trigger_during_output: 2,
            config_silence_timeout: 30,
            ..Default::default()
        };
        let findings = detect_silence_mismatch(&data);
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.id, "silence-mismatch");
        let (field, cur, new) = make_config_tuning(f);
        assert_eq!(field, "silence_timeout_secs");
        assert_eq!(cur, "30");
        assert_eq!(new, "45"); // 30 * 1.5 = 45
    }

    #[test]
    fn silence_mismatch_rounding() {
        // 20 * 1.5 = 30.0 — exact
        let data = RunData {
            fast_trigger_during_output: 3,
            config_silence_timeout: 20,
            ..Default::default()
        };
        let findings = detect_silence_mismatch(&data);
        assert_eq!(findings.len(), 1);
        let (_, _, new) = make_config_tuning(&findings[0]);
        assert_eq!(new, "30");
    }

    #[test]
    fn silence_mismatch_negative_below_threshold() {
        let data = RunData {
            fast_trigger_during_output: 1,
            config_silence_timeout: 30,
            ..Default::default()
        };
        assert!(detect_silence_mismatch(&data).is_empty());
    }

    #[test]
    fn silence_mismatch_negative_zero() {
        let data = RunData {
            fast_trigger_during_output: 0,
            config_silence_timeout: 30,
            ..Default::default()
        };
        assert!(detect_silence_mismatch(&data).is_empty());
    }

    // --- detect_silence_waste ---

    #[test]
    fn silence_waste_positive_produces_finding() {
        // avg_idle = 70, timeout = 30, threshold = 60 → 70 > 60, iterations = 5 >= 5
        let data = RunData {
            avg_idle_between_iterations_secs: 70.0,
            config_silence_timeout: 30,
            iterations: 5,
            ..Default::default()
        };
        let findings = detect_silence_waste(&data);
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.id, "silence-waste");
        let (field, cur, new) = make_config_tuning(f);
        assert_eq!(field, "silence_timeout_secs");
        assert_eq!(cur, "30");
        assert_eq!(new, "23"); // 30 * 0.75 = 22.5 → rounds to 23
    }

    #[test]
    fn silence_waste_negative_idle_not_exceeding() {
        // avg_idle = 59, timeout = 30, threshold = 60 → not exceeded
        let data = RunData {
            avg_idle_between_iterations_secs: 59.0,
            config_silence_timeout: 30,
            iterations: 10,
            ..Default::default()
        };
        assert!(detect_silence_waste(&data).is_empty());
    }

    #[test]
    fn silence_waste_negative_too_few_iterations() {
        let data = RunData {
            avg_idle_between_iterations_secs: 100.0,
            config_silence_timeout: 30,
            iterations: 4,
            ..Default::default()
        };
        assert!(detect_silence_waste(&data).is_empty());
    }

    // --- detect_stuck_sensitivity ---

    #[test]
    fn stuck_sensitivity_positive_produces_finding() {
        // stuck=2, waste_count=1, iterations=10 → ratio=0.10 < 0.15
        let data = RunData {
            stuck_count: 2,
            waste_count: 1,
            iterations: 10,
            config_max_retries: 3,
            ..Default::default()
        };
        let findings = detect_stuck_sensitivity(&data);
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.id, "stuck-sensitivity");
        let (field, cur, new) = make_config_tuning(f);
        assert_eq!(field, "max_retries_before_stuck");
        assert_eq!(cur, "3");
        assert_eq!(new, "4");
    }

    #[test]
    fn stuck_sensitivity_negative_no_stuck() {
        let data = RunData {
            stuck_count: 0,
            waste_count: 0,
            iterations: 20,
            config_max_retries: 3,
            ..Default::default()
        };
        assert!(detect_stuck_sensitivity(&data).is_empty());
    }

    #[test]
    fn stuck_sensitivity_negative_high_waste() {
        // waste_count=3, iterations=10 → ratio=0.30 >= 0.15
        let data = RunData {
            stuck_count: 2,
            waste_count: 3,
            iterations: 10,
            config_max_retries: 3,
            ..Default::default()
        };
        assert!(detect_stuck_sensitivity(&data).is_empty());
    }

    #[test]
    fn stuck_sensitivity_zero_iterations_uses_max_1() {
        // iterations=0, max(1) → ratio = waste_count / 1
        let data = RunData {
            stuck_count: 1,
            waste_count: 0,
            iterations: 0,
            config_max_retries: 3,
            ..Default::default()
        };
        let findings = detect_stuck_sensitivity(&data);
        assert_eq!(findings.len(), 1);
    }

    // --- detect_stuck_leniency ---

    #[test]
    fn stuck_leniency_positive_produces_finding() {
        // max run of 6, config_max_retries=4 → 6 >= 5 AND 6 > 4
        let data = RunData {
            fingerprint_sequence: vec![1, 2, 3, 3, 3, 3, 3, 3, 4],
            config_max_retries: 4,
            ..Default::default()
        };
        let findings = detect_stuck_leniency(&data);
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.id, "stuck-leniency");
        let (field, cur, new) = make_config_tuning(f);
        assert_eq!(field, "max_retries_before_stuck");
        assert_eq!(cur, "4");
        assert_eq!(new, "3");
    }

    #[test]
    fn stuck_leniency_min_new_value_is_2() {
        // config_max_retries=2 → saturating_sub(1)=1, max(2)=2
        let data = RunData {
            fingerprint_sequence: vec![5, 5, 5, 5, 5, 5],
            config_max_retries: 2,
            ..Default::default()
        };
        let findings = detect_stuck_leniency(&data);
        assert_eq!(findings.len(), 1);
        let (_, _, new) = make_config_tuning(&findings[0]);
        assert_eq!(new, "2"); // stays at 2, not below
    }

    #[test]
    fn stuck_leniency_negative_run_too_short() {
        // max run = 4 < 5
        let data = RunData {
            fingerprint_sequence: vec![1, 2, 2, 2, 2, 3],
            config_max_retries: 2,
            ..Default::default()
        };
        assert!(detect_stuck_leniency(&data).is_empty());
    }

    #[test]
    fn stuck_leniency_negative_run_not_exceeding_max_retries() {
        // max run = 5, config_max_retries = 5 → 5 is NOT > 5
        let data = RunData {
            fingerprint_sequence: vec![7, 7, 7, 7, 7],
            config_max_retries: 5,
            ..Default::default()
        };
        assert!(detect_stuck_leniency(&data).is_empty());
    }

    #[test]
    fn stuck_leniency_negative_empty_sequence() {
        let data = RunData {
            fingerprint_sequence: vec![],
            config_max_retries: 3,
            ..Default::default()
        };
        assert!(detect_stuck_leniency(&data).is_empty());
    }

    // --- detect_checkpoint_overhead ---

    #[test]
    fn checkpoint_overhead_positive_produces_finding() {
        let data = RunData {
            checkpoint_count: 3,
            iterations: 5,
            ..Default::default()
        };
        let findings = detect_checkpoint_overhead(&data);
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.id, "checkpoint-overhead");
        // Severity should be Low
        assert!(matches!(f.severity, Severity::Low));
        let (field, ..) = make_config_tuning(f);
        assert_eq!(field, "checkpoint_interval_iterations");
    }

    #[test]
    fn checkpoint_overhead_negative_too_few_checkpoints() {
        let data = RunData {
            checkpoint_count: 2,
            iterations: 5,
            ..Default::default()
        };
        assert!(detect_checkpoint_overhead(&data).is_empty());
    }

    #[test]
    fn checkpoint_overhead_negative_iterations_at_boundary() {
        // iterations == 15 → condition is iterations < 15, so 15 is excluded
        let data = RunData {
            checkpoint_count: 5,
            iterations: 15,
            ..Default::default()
        };
        assert!(detect_checkpoint_overhead(&data).is_empty());
    }

    #[test]
    fn checkpoint_overhead_negative_large_run() {
        let data = RunData {
            checkpoint_count: 10,
            iterations: 50,
            ..Default::default()
        };
        assert!(detect_checkpoint_overhead(&data).is_empty());
    }

    // --- analyze() integration ---

    #[test]
    fn analyze_multi_detector_run_produces_multiple_findings() {
        // Trigger: silence_mismatch (fast_trigger=2), stuck_sensitivity (stuck=1, waste=0/10=0%),
        // checkpoint_overhead (checkpoints=4, iterations=8)
        let data = RunData {
            fast_trigger_during_output: 2,
            config_silence_timeout: 30,
            stuck_count: 1,
            waste_count: 0,
            iterations: 10,
            config_max_retries: 3,
            checkpoint_count: 4,
            avg_idle_between_iterations_secs: 5.0, // below threshold — silence_waste NOT triggered
            fingerprint_sequence: vec![1, 2, 3],  // max run=1 — stuck_leniency NOT triggered
            ..Default::default()
        };
        let findings = analyze(&data);
        let ids: Vec<&str> = findings.iter().map(|f| f.id.as_str()).collect();
        assert!(ids.contains(&"silence-mismatch"), "expected silence-mismatch in {:?}", ids);
        assert!(ids.contains(&"stuck-sensitivity"), "expected stuck-sensitivity in {:?}", ids);
        assert!(ids.contains(&"checkpoint-overhead"), "expected checkpoint-overhead in {:?}", ids);
        assert!(!ids.contains(&"silence-waste"), "silence-waste should NOT fire");
        assert!(!ids.contains(&"stuck-leniency"), "stuck-leniency should NOT fire");
    }

    #[test]
    fn analyze_clean_run_produces_no_findings() {
        let data = RunData {
            fast_trigger_during_output: 0,
            config_silence_timeout: 30,
            avg_idle_between_iterations_secs: 5.0,
            iterations: 10,
            stuck_count: 0,
            waste_count: 0,
            config_max_retries: 3,
            fingerprint_sequence: vec![1, 2, 3, 4, 5],
            checkpoint_count: 1,
            ..Default::default()
        };
        assert!(analyze(&data).is_empty());
    }

    #[test]
    fn analyze_all_five_detectors_fire() {
        // silence_mismatch: fast_trigger=2
        // silence_waste: avg_idle=70 > 30*2=60, iterations=6 >= 5
        // stuck_sensitivity: stuck=1, waste=0, iterations=6 → ratio=0 < 0.15
        // stuck_leniency: fingerprint run=6 > max_retries=4
        // checkpoint_overhead: checkpoints=4, iterations=6 < 15
        let data = RunData {
            fast_trigger_during_output: 2,
            config_silence_timeout: 30,
            avg_idle_between_iterations_secs: 70.0,
            iterations: 6,
            stuck_count: 1,
            waste_count: 0,
            config_max_retries: 4,
            fingerprint_sequence: vec![9, 9, 9, 9, 9, 9],
            checkpoint_count: 4,
            ..Default::default()
        };
        let findings = analyze(&data);
        let ids: Vec<&str> = findings.iter().map(|f| f.id.as_str()).collect();
        assert!(ids.contains(&"silence-mismatch"), "missing silence-mismatch");
        assert!(ids.contains(&"silence-waste"), "missing silence-waste");
        assert!(ids.contains(&"stuck-sensitivity"), "missing stuck-sensitivity");
        assert!(ids.contains(&"stuck-leniency"), "missing stuck-leniency");
        assert!(ids.contains(&"checkpoint-overhead"), "missing checkpoint-overhead");
    }
}
