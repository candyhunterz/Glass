use std::collections::HashMap;

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
    findings.extend(detect_checkpoint_frequency(data));
    findings.extend(detect_hot_files(data));
    findings.extend(detect_uncommitted_drift(data));
    findings.extend(detect_instruction_overload(data));
    findings.extend(detect_flaky_verification(data));
    findings.extend(detect_ordering_failure(data));
    findings.extend(detect_scope_creep(data));
    findings.extend(detect_oscillation(data));
    findings.extend(detect_revert_rate(data));
    findings.extend(detect_waste_rate(data));
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
    let waste_ratio = data.waste_count as f64 / data.iterations.max(1) as f64;
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
/// < 15 iterations), suggest increasing checkpoint interval by 50%.
fn detect_checkpoint_overhead(data: &RunData) -> Vec<Finding> {
    if data.checkpoint_count < 3 || data.iterations >= 15 {
        return vec![];
    }

    let current = data.config_checkpoint_interval;
    let new_value = ((current as f64) * 1.5).round() as u64;

    vec![Finding {
        id: "checkpoint-overhead".to_string(),
        category: FindingCategory::ConfigTuning,
        severity: Severity::Low,
        action: FindingAction::ConfigTuning {
            field: "checkpoint_interval".to_string(),
            current_value: current.to_string(),
            new_value: new_value.to_string(),
        },
        evidence: format!(
            "{} checkpoints in only {} iterations; increasing checkpoint interval from {} to {} (+50%).",
            data.checkpoint_count, data.iterations, current, new_value
        ),
        scope: Scope::Project,
    }]
}

/// If iterations >= 20 and checkpoint_count == 0, suggest decreasing checkpoint interval by 25%.
fn detect_checkpoint_frequency(data: &RunData) -> Vec<Finding> {
    if data.iterations < 20 || data.checkpoint_count != 0 {
        return vec![];
    }

    let current = data.config_checkpoint_interval;
    let new_value = ((current as f64) * 0.75).round() as u64;

    vec![Finding {
        id: "checkpoint-frequency".to_string(),
        category: FindingCategory::ConfigTuning,
        severity: Severity::Medium,
        action: FindingAction::ConfigTuning {
            field: "checkpoint_interval".to_string(),
            current_value: current.to_string(),
            new_value: new_value.to_string(),
        },
        evidence: format!(
            "{} iterations completed with 0 checkpoints; decreasing checkpoint interval from {} to {} (-25%).",
            data.iterations, current, new_value
        ),
        scope: Scope::Project,
    }]
}

/// For each file in reverted_files appearing >= 3 times, produce a BehavioralRule
/// finding with action = "isolate_commits".
fn detect_hot_files(data: &RunData) -> Vec<Finding> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for f in &data.reverted_files {
        *counts.entry(f.as_str()).or_insert(0) += 1;
    }

    let mut findings = Vec::new();
    let mut hot: Vec<&&str> = counts
        .iter()
        .filter(|(_, &c)| c >= 3)
        .map(|(f, _)| f)
        .collect();
    hot.sort(); // deterministic ordering

    for file in hot {
        let mut params = HashMap::new();
        params.insert("file".to_string(), (*file).to_string());
        findings.push(Finding {
            id: format!("hot-file:{}", file),
            category: FindingCategory::BehavioralRule,
            severity: Severity::Medium,
            action: FindingAction::BehavioralRule {
                action: "isolate_commits".to_string(),
                params,
            },
            evidence: format!(
                "File '{}' was reverted {} time(s); isolating its commits may reduce churn.",
                file, counts[*file]
            ),
            scope: Scope::Project,
        });
    }
    findings
}

/// If iterations >= 5 and commit_count == 0, or iterations/commits ratio > 5,
/// produce a BehavioralRule finding with action = "force_commit".
fn detect_uncommitted_drift(data: &RunData) -> Vec<Finding> {
    let no_commits = data.iterations >= 5 && data.commit_count == 0;
    let high_ratio =
        data.commit_count > 0 && data.iterations as f64 / data.commit_count as f64 > 5.0;

    if !no_commits && !high_ratio {
        return vec![];
    }

    let evidence = if no_commits {
        format!(
            "{} iterations completed with 0 commits; agent is drifting without committing.",
            data.iterations
        )
    } else {
        format!(
            "{} iterations with only {} commit(s) (ratio {:.1}); committing more frequently is recommended.",
            data.iterations,
            data.commit_count,
            data.iterations as f64 / data.commit_count as f64
        )
    };

    vec![Finding {
        id: "uncommitted-drift".to_string(),
        category: FindingCategory::BehavioralRule,
        severity: Severity::Medium,
        action: FindingAction::BehavioralRule {
            action: "force_commit".to_string(),
            params: HashMap::new(),
        },
        evidence,
        scope: Scope::Global,
    }]
}

/// Count responses with 4+ numbered list items. If count >= 2 AND waste_rate > 0.1,
/// produce a BehavioralRule finding with action = "smaller_instructions".
fn detect_instruction_overload(data: &RunData) -> Vec<Finding> {
    let waste_rate = data.waste_count as f64 / data.iterations.max(1) as f64;
    if waste_rate <= 0.1 {
        return vec![];
    }

    let overloaded_count = data
        .agent_responses
        .iter()
        .filter(|resp| {
            // Check that the response has at least items 1., 2., 3., 4. as line starts
            let has_1 = resp.lines().any(|l| l.trim_start().starts_with("1."));
            let has_2 = resp.lines().any(|l| l.trim_start().starts_with("2."));
            let has_3 = resp.lines().any(|l| l.trim_start().starts_with("3."));
            let has_4 = resp.lines().any(|l| l.trim_start().starts_with("4."));
            has_1 && has_2 && has_3 && has_4
        })
        .count();

    if overloaded_count < 2 {
        return vec![];
    }

    vec![Finding {
        id: "instruction-overload".to_string(),
        category: FindingCategory::BehavioralRule,
        severity: Severity::Medium,
        action: FindingAction::BehavioralRule {
            action: "smaller_instructions".to_string(),
            params: HashMap::new(),
        },
        evidence: format!(
            "{} agent response(s) contained 4+ numbered list items with waste rate {:.0}% (> 10%); instructions may be too large.",
            overloaded_count,
            waste_rate * 100.0
        ),
        scope: Scope::Global,
    }]
}

/// Look for alternations in verify_pass_fail_sequence. If alternations >= 3,
/// produce a BehavioralRule finding with action = "run_verify_twice".
fn detect_flaky_verification(data: &RunData) -> Vec<Finding> {
    let seq = &data.verify_pass_fail_sequence;
    if seq.len() < 2 {
        return vec![];
    }

    let alternations = seq.windows(2).filter(|w| w[0] != w[1]).count();
    if alternations < 3 {
        return vec![];
    }

    vec![Finding {
        id: "flaky-verification".to_string(),
        category: FindingCategory::BehavioralRule,
        severity: Severity::High,
        action: FindingAction::BehavioralRule {
            action: "run_verify_twice".to_string(),
            params: HashMap::new(),
        },
        evidence: format!(
            "Verification sequence has {} alternation(s) (pass↔fail flipping); running verification twice may improve reliability.",
            alternations
        ),
        scope: Scope::Project,
    }]
}

/// Search iterations_tsv for dependency/not found/undefined/import errors followed
/// within 3 lines by a revert. If found, produce a BehavioralRule with action =
/// "build_dependency_first".
fn detect_ordering_failure(data: &RunData) -> Vec<Finding> {
    let lines: Vec<&str> = data.iterations_tsv.lines().collect();
    let mut found = false;

    for (i, line) in lines.iter().enumerate() {
        let lower = line.to_lowercase();
        let is_dep_error = lower.contains("dependency")
            || lower.contains("not found")
            || lower.contains("undefined")
            || lower.contains("import");

        if is_dep_error {
            // Check next 3 lines for a revert
            let end = (i + 4).min(lines.len());
            for subsequent in &lines[i + 1..end] {
                if subsequent.to_lowercase().contains("revert") {
                    found = true;
                    break;
                }
            }
        }
        if found {
            break;
        }
    }

    if !found {
        return vec![];
    }

    vec![Finding {
        id: "ordering-failure".to_string(),
        category: FindingCategory::BehavioralRule,
        severity: Severity::Medium,
        action: FindingAction::BehavioralRule {
            action: "build_dependency_first".to_string(),
            params: HashMap::new(),
        },
        evidence:
            "iterations_tsv contains a dependency/import error followed by a revert within 3 lines; build dependencies first.".to_string(),
        scope: Scope::Project,
    }]
}

/// Parse prd_content for deliverable file paths under a `## Deliverables` section.
/// If git_diff_stat contains files NOT in the deliverables list (and deliverables is
/// non-empty), produce a BehavioralRule with action = "restrict_scope".
fn detect_scope_creep(data: &RunData) -> Vec<Finding> {
    let prd = match &data.prd_content {
        Some(p) => p,
        None => return vec![],
    };
    let diff_stat = match &data.git_diff_stat {
        Some(d) => d,
        None => return vec![],
    };

    // Extract deliverables: find `## Deliverables` section, then list items
    let deliverables = parse_deliverables(prd);
    if deliverables.is_empty() {
        return vec![];
    }

    // Extract touched file paths from git_diff_stat (each line starts with stats then filename)
    let touched_files: Vec<&str> = diff_stat
        .lines()
        .filter_map(|line| {
            // git diff --stat lines look like:  " src/foo.rs | 10 ++"
            // split on '|' and take the left side, trim whitespace
            let path = line.split('|').next()?.trim();
            if path.is_empty() || (!path.contains('.') && !path.contains('/')) {
                None
            } else {
                Some(path)
            }
        })
        .collect();

    let out_of_scope: Vec<&str> = touched_files
        .iter()
        .copied()
        .filter(|tf| {
            !deliverables
                .iter()
                .any(|d| tf.contains(d.as_str()) || d.contains(tf))
        })
        .collect();

    if out_of_scope.is_empty() {
        return vec![];
    }

    vec![Finding {
        id: "scope-creep".to_string(),
        category: FindingCategory::BehavioralRule,
        severity: Severity::Medium,
        action: FindingAction::BehavioralRule {
            action: "restrict_scope".to_string(),
            params: HashMap::new(),
        },
        evidence: format!(
            "{} file(s) in git diff are not listed in PRD deliverables (e.g. '{}'); scope may have drifted.",
            out_of_scope.len(),
            out_of_scope.first().copied().unwrap_or("")
        ),
        scope: Scope::Project,
    }]
}

/// Group fingerprint_sequence into consecutive runs. If any window of 4+ consecutive
/// values comes from a set of only 2 unique values, produce a BehavioralRule with
/// action = "early_stuck".
fn detect_oscillation(data: &RunData) -> Vec<Finding> {
    let seq = &data.fingerprint_sequence;
    if seq.len() < 4 {
        return vec![];
    }

    // For each start position, collect the longest contiguous prefix that uses at most 2
    // unique values and check if its length is >= 4.
    let mut found = false;
    'outer: for start in 0..seq.len() {
        let mut unique = std::collections::HashSet::new();
        let mut span = 0usize;
        for &val in &seq[start..] {
            unique.insert(val);
            if unique.len() > 2 {
                break;
            }
            span += 1;
        }
        if span >= 4 {
            found = true;
            break 'outer;
        }
    }

    if !found {
        return vec![];
    }

    vec![Finding {
        id: "oscillation".to_string(),
        category: FindingCategory::BehavioralRule,
        severity: Severity::Medium,
        action: FindingAction::BehavioralRule {
            action: "early_stuck".to_string(),
            params: HashMap::new(),
        },
        evidence: "Fingerprint sequence contains 4+ consecutive values oscillating between only 2 states; agent may be stuck in a loop.".to_string(),
        scope: Scope::Global,
    }]
}

/// If iterations >= 5 AND revert_count / iterations > 0.3, produce a BehavioralRule
/// with action = "smaller_instructions".
fn detect_revert_rate(data: &RunData) -> Vec<Finding> {
    if data.iterations < 5 {
        return vec![];
    }
    if data.revert_count as f64 / data.iterations as f64 <= 0.3 {
        return vec![];
    }

    vec![Finding {
        id: "revert-rate".to_string(),
        category: FindingCategory::BehavioralRule,
        severity: Severity::High,
        action: FindingAction::BehavioralRule {
            action: "smaller_instructions".to_string(),
            params: HashMap::new(),
        },
        evidence: format!(
            "Revert rate is {:.0}% ({}/{} iterations); instructions may be too large or ambiguous.",
            data.revert_count as f64 / data.iterations as f64 * 100.0,
            data.revert_count,
            data.iterations
        ),
        scope: Scope::Global,
    }]
}

/// If iterations >= 5 AND waste_count / iterations > 0.15, produce a BehavioralRule
/// with action = "verify_progress".
fn detect_waste_rate(data: &RunData) -> Vec<Finding> {
    if data.iterations < 5 {
        return vec![];
    }
    if data.waste_count as f64 / data.iterations as f64 <= 0.15 {
        return vec![];
    }

    vec![Finding {
        id: "waste-rate".to_string(),
        category: FindingCategory::BehavioralRule,
        severity: Severity::Medium,
        action: FindingAction::BehavioralRule {
            action: "verify_progress".to_string(),
            params: HashMap::new(),
        },
        evidence: format!(
            "Waste rate is {:.0}% ({}/{} iterations); adding progress verification steps may reduce wasted work.",
            data.waste_count as f64 / data.iterations as f64 * 100.0,
            data.waste_count,
            data.iterations
        ),
        scope: Scope::Global,
    }]
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse a PRD string for deliverable file paths under a `## Deliverables` section.
/// Extracts the first token from each list item that contains `.` or `/`.
fn parse_deliverables(prd: &str) -> Vec<String> {
    let mut in_deliverables = false;
    let mut deliverables = Vec::new();

    for line in prd.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## Deliverables") {
            in_deliverables = true;
            continue;
        }
        if in_deliverables {
            // Stop at the next heading
            if trimmed.starts_with("## ") {
                break;
            }
            // List items start with - or *
            if trimmed.starts_with('-') || trimmed.starts_with('*') {
                let content = trimmed.trim_start_matches(['-', '*', ' ']);
                // Find first token containing '.' or '/'
                if let Some(token) = content
                    .split_whitespace()
                    .find(|t| t.contains('.') || t.contains('/'))
                {
                    deliverables.push(
                        token
                            .trim_matches(['`', '"', '\'', '(', ')', '[', ']'])
                            .to_string(),
                    );
                }
            }
        }
    }
    deliverables
}

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
        assert_eq!(field, "checkpoint_interval");
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

    // --- detect_checkpoint_frequency ---

    #[test]
    fn detect_checkpoint_frequency_triggers() {
        let data = RunData {
            iterations: 20,
            checkpoint_count: 0,
            ..Default::default()
        };
        let findings = detect_checkpoint_frequency(&data);
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.id, "checkpoint-frequency");
        let (field, cur, new) = make_config_tuning(f);
        assert_eq!(field, "checkpoint_interval");
        // Default config_checkpoint_interval is 0; 0 * 0.75 = 0
        assert_eq!(cur, "0");
        assert_eq!(new, "0");
    }

    #[test]
    fn detect_checkpoint_frequency_no_trigger_too_few_iterations() {
        let data = RunData {
            iterations: 19,
            checkpoint_count: 0,
            ..Default::default()
        };
        assert!(detect_checkpoint_frequency(&data).is_empty());
    }

    #[test]
    fn detect_checkpoint_frequency_no_trigger_has_checkpoints() {
        let data = RunData {
            iterations: 25,
            checkpoint_count: 2,
            ..Default::default()
        };
        assert!(detect_checkpoint_frequency(&data).is_empty());
    }

    // --- detect_hot_files ---

    #[test]
    fn detect_hot_files_triggers() {
        let data = RunData {
            reverted_files: vec![
                "src/main.rs".to_string(),
                "src/main.rs".to_string(),
                "src/main.rs".to_string(),
            ],
            ..Default::default()
        };
        let findings = detect_hot_files(&data);
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.id, "hot-file:src/main.rs");
        if let FindingAction::BehavioralRule { action, params } = &f.action {
            assert_eq!(action, "isolate_commits");
            assert_eq!(params.get("file").map(String::as_str), Some("src/main.rs"));
        } else {
            panic!("expected BehavioralRule action");
        }
        assert!(matches!(f.scope, Scope::Project));
    }

    #[test]
    fn detect_hot_files_no_trigger_below_threshold() {
        let data = RunData {
            reverted_files: vec!["src/main.rs".to_string(), "src/main.rs".to_string()],
            ..Default::default()
        };
        assert!(detect_hot_files(&data).is_empty());
    }

    #[test]
    fn detect_hot_files_multiple_hot_files() {
        let data = RunData {
            reverted_files: vec![
                "a.rs".to_string(),
                "a.rs".to_string(),
                "a.rs".to_string(),
                "b.rs".to_string(),
                "b.rs".to_string(),
                "b.rs".to_string(),
                "b.rs".to_string(),
            ],
            ..Default::default()
        };
        let findings = detect_hot_files(&data);
        assert_eq!(findings.len(), 2);
        let ids: Vec<&str> = findings.iter().map(|f| f.id.as_str()).collect();
        assert!(ids.contains(&"hot-file:a.rs"));
        assert!(ids.contains(&"hot-file:b.rs"));
    }

    // --- detect_uncommitted_drift ---

    #[test]
    fn detect_uncommitted_drift_triggers_no_commits() {
        let data = RunData {
            iterations: 5,
            commit_count: 0,
            ..Default::default()
        };
        let findings = detect_uncommitted_drift(&data);
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.id, "uncommitted-drift");
        if let FindingAction::BehavioralRule { action, .. } = &f.action {
            assert_eq!(action, "force_commit");
        } else {
            panic!("expected BehavioralRule action");
        }
        assert!(matches!(f.scope, Scope::Global));
    }

    #[test]
    fn detect_uncommitted_drift_triggers_high_ratio() {
        // 12 iterations / 2 commits = 6.0 > 5.0
        let data = RunData {
            iterations: 12,
            commit_count: 2,
            ..Default::default()
        };
        let findings = detect_uncommitted_drift(&data);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].id, "uncommitted-drift");
    }

    #[test]
    fn detect_uncommitted_drift_no_trigger_few_iterations() {
        let data = RunData {
            iterations: 4,
            commit_count: 0,
            ..Default::default()
        };
        assert!(detect_uncommitted_drift(&data).is_empty());
    }

    #[test]
    fn detect_uncommitted_drift_no_trigger_good_ratio() {
        // 10 / 3 = 3.33 <= 5.0
        let data = RunData {
            iterations: 10,
            commit_count: 3,
            ..Default::default()
        };
        assert!(detect_uncommitted_drift(&data).is_empty());
    }

    // --- detect_instruction_overload ---

    #[test]
    fn detect_instruction_overload_triggers() {
        let response_with_list =
            "Here are the steps:\n1. Do this\n2. Do that\n3. Also this\n4. Finally that\n"
                .to_string();
        let data = RunData {
            iterations: 10,
            waste_count: 2, // 2/10 = 0.20 > 0.10
            agent_responses: vec![response_with_list.clone(), response_with_list],
            ..Default::default()
        };
        let findings = detect_instruction_overload(&data);
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.id, "instruction-overload");
        if let FindingAction::BehavioralRule { action, .. } = &f.action {
            assert_eq!(action, "smaller_instructions");
        } else {
            panic!("expected BehavioralRule action");
        }
        assert!(matches!(f.scope, Scope::Global));
    }

    #[test]
    fn detect_instruction_overload_no_trigger_low_waste() {
        let response_with_list = "1. Do this\n2. Do that\n3. Also\n4. Finally\n".to_string();
        let data = RunData {
            iterations: 10,
            waste_count: 1, // 1/10 = 0.10 — NOT > 0.10
            agent_responses: vec![response_with_list.clone(), response_with_list],
            ..Default::default()
        };
        assert!(detect_instruction_overload(&data).is_empty());
    }

    #[test]
    fn detect_instruction_overload_no_trigger_few_overloaded() {
        let response_with_list = "1. Do this\n2. Do that\n3. Also\n4. Finally\n".to_string();
        let short_response = "Just do the thing.".to_string();
        let data = RunData {
            iterations: 10,
            waste_count: 3, // high waste
            agent_responses: vec![response_with_list, short_response],
            ..Default::default()
        };
        // Only 1 response has 4+ items — need >= 2
        assert!(detect_instruction_overload(&data).is_empty());
    }

    // --- detect_flaky_verification ---

    #[test]
    fn detect_flaky_verification_triggers() {
        // pass, fail, pass, fail — 3 alternations
        let data = RunData {
            verify_pass_fail_sequence: vec![true, false, true, false],
            ..Default::default()
        };
        let findings = detect_flaky_verification(&data);
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.id, "flaky-verification");
        if let FindingAction::BehavioralRule { action, .. } = &f.action {
            assert_eq!(action, "run_verify_twice");
        } else {
            panic!("expected BehavioralRule action");
        }
        assert!(matches!(f.severity, Severity::High));
        assert!(matches!(f.scope, Scope::Project));
    }

    #[test]
    fn detect_flaky_verification_no_trigger_too_few_alternations() {
        // pass, fail, pass — 2 alternations (< 3)
        let data = RunData {
            verify_pass_fail_sequence: vec![true, false, true],
            ..Default::default()
        };
        assert!(detect_flaky_verification(&data).is_empty());
    }

    #[test]
    fn detect_flaky_verification_no_trigger_stable_sequence() {
        let data = RunData {
            verify_pass_fail_sequence: vec![true, true, true, true, false],
            ..Default::default()
        };
        // Only 1 alternation
        assert!(detect_flaky_verification(&data).is_empty());
    }

    // --- detect_ordering_failure ---

    #[test]
    fn detect_ordering_failure_triggers_dependency_then_revert() {
        let tsv =
            "1\tkeep\tworking\n2\tkeep\tdependency not found\n3\trevert\tsomething\n4\tkeep\tfix\n";
        let data = RunData {
            iterations_tsv: tsv.to_string(),
            ..Default::default()
        };
        let findings = detect_ordering_failure(&data);
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.id, "ordering-failure");
        if let FindingAction::BehavioralRule { action, .. } = &f.action {
            assert_eq!(action, "build_dependency_first");
        } else {
            panic!("expected BehavioralRule action");
        }
        assert!(matches!(f.scope, Scope::Project));
    }

    #[test]
    fn detect_ordering_failure_no_trigger_no_dependency_errors() {
        let tsv = "1\tkeep\tworking\n2\tkeep\tall good\n3\tkeep\tcontinuing\n";
        let data = RunData {
            iterations_tsv: tsv.to_string(),
            ..Default::default()
        };
        assert!(detect_ordering_failure(&data).is_empty());
    }

    #[test]
    fn detect_ordering_failure_no_trigger_revert_too_far() {
        // dependency error at line 1, revert at line 5 — more than 3 lines away
        let tsv = "1\tkeep\tdependency\n2\tkeep\ta\n3\tkeep\tb\n4\tkeep\tc\n5\trevert\tsomething\n";
        let data = RunData {
            iterations_tsv: tsv.to_string(),
            ..Default::default()
        };
        assert!(detect_ordering_failure(&data).is_empty());
    }

    // --- detect_scope_creep ---

    #[test]
    fn detect_scope_creep_triggers() {
        let prd = "# Project\n\n## Deliverables\n- src/main.rs the main entry point\n- src/lib.rs the library\n\n## Notes\nNothing here.\n";
        let diff_stat = " src/main.rs | 10 ++\n src/extra.rs | 5 +\n";
        let data = RunData {
            prd_content: Some(prd.to_string()),
            git_diff_stat: Some(diff_stat.to_string()),
            ..Default::default()
        };
        let findings = detect_scope_creep(&data);
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.id, "scope-creep");
        if let FindingAction::BehavioralRule { action, .. } = &f.action {
            assert_eq!(action, "restrict_scope");
        } else {
            panic!("expected BehavioralRule action");
        }
        assert!(matches!(f.scope, Scope::Project));
    }

    #[test]
    fn detect_scope_creep_no_trigger_all_in_scope() {
        let prd = "## Deliverables\n- src/main.rs the main file\n- src/lib.rs the lib\n";
        let diff_stat = " src/main.rs | 10 ++\n src/lib.rs | 5 +\n";
        let data = RunData {
            prd_content: Some(prd.to_string()),
            git_diff_stat: Some(diff_stat.to_string()),
            ..Default::default()
        };
        assert!(detect_scope_creep(&data).is_empty());
    }

    #[test]
    fn detect_scope_creep_no_trigger_no_deliverables_section() {
        let prd = "# Project\nJust some notes, no deliverables section.\n";
        let diff_stat = " src/any.rs | 10 ++\n";
        let data = RunData {
            prd_content: Some(prd.to_string()),
            git_diff_stat: Some(diff_stat.to_string()),
            ..Default::default()
        };
        assert!(detect_scope_creep(&data).is_empty());
    }

    // --- detect_oscillation ---

    #[test]
    fn detect_oscillation_triggers() {
        // 1,2,1,2 — 4 values, only 2 unique
        let data = RunData {
            fingerprint_sequence: vec![1, 2, 1, 2],
            ..Default::default()
        };
        let findings = detect_oscillation(&data);
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.id, "oscillation");
        if let FindingAction::BehavioralRule { action, .. } = &f.action {
            assert_eq!(action, "early_stuck");
        } else {
            panic!("expected BehavioralRule action");
        }
        assert!(matches!(f.scope, Scope::Global));
    }

    #[test]
    fn detect_oscillation_triggers_with_prefix() {
        // 3,4,5,1,2,1,2,1 — oscillation starts at index 3
        let data = RunData {
            fingerprint_sequence: vec![3, 4, 5, 1, 2, 1, 2],
            ..Default::default()
        };
        let findings = detect_oscillation(&data);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn detect_oscillation_no_trigger_too_short() {
        let data = RunData {
            fingerprint_sequence: vec![1, 2, 1],
            ..Default::default()
        };
        assert!(detect_oscillation(&data).is_empty());
    }

    #[test]
    fn detect_oscillation_no_trigger_three_unique_values() {
        // 1,2,3,1,2,3 — 3 unique values, not oscillation
        let data = RunData {
            fingerprint_sequence: vec![1, 2, 3, 1, 2, 3],
            ..Default::default()
        };
        assert!(detect_oscillation(&data).is_empty());
    }

    // --- detect_revert_rate ---

    #[test]
    fn detect_revert_rate_triggers() {
        // 4/10 = 0.40 > 0.30
        let data = RunData {
            iterations: 10,
            revert_count: 4,
            ..Default::default()
        };
        let findings = detect_revert_rate(&data);
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.id, "revert-rate");
        if let FindingAction::BehavioralRule { action, .. } = &f.action {
            assert_eq!(action, "smaller_instructions");
        } else {
            panic!("expected BehavioralRule action");
        }
        assert!(matches!(f.severity, Severity::High));
        assert!(matches!(f.scope, Scope::Global));
    }

    #[test]
    fn detect_revert_rate_no_trigger_low_rate() {
        // 3/10 = 0.30 — NOT > 0.30
        let data = RunData {
            iterations: 10,
            revert_count: 3,
            ..Default::default()
        };
        assert!(detect_revert_rate(&data).is_empty());
    }

    #[test]
    fn detect_revert_rate_no_trigger_too_few_iterations() {
        let data = RunData {
            iterations: 4,
            revert_count: 4,
            ..Default::default()
        };
        assert!(detect_revert_rate(&data).is_empty());
    }

    // --- detect_waste_rate ---

    #[test]
    fn detect_waste_rate_triggers() {
        // 2/10 = 0.20 > 0.15
        let data = RunData {
            iterations: 10,
            waste_count: 2,
            ..Default::default()
        };
        let findings = detect_waste_rate(&data);
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.id, "waste-rate");
        if let FindingAction::BehavioralRule { action, .. } = &f.action {
            assert_eq!(action, "verify_progress");
        } else {
            panic!("expected BehavioralRule action");
        }
        assert!(matches!(f.severity, Severity::Medium));
        assert!(matches!(f.scope, Scope::Global));
    }

    #[test]
    fn detect_waste_rate_no_trigger_low_waste() {
        // 1/10 = 0.10 — NOT > 0.15
        let data = RunData {
            iterations: 10,
            waste_count: 1,
            ..Default::default()
        };
        assert!(detect_waste_rate(&data).is_empty());
    }

    #[test]
    fn detect_waste_rate_no_trigger_too_few_iterations() {
        let data = RunData {
            iterations: 4,
            waste_count: 4,
            ..Default::default()
        };
        assert!(detect_waste_rate(&data).is_empty());
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
            fingerprint_sequence: vec![1, 2, 3],   // max run=1 — stuck_leniency NOT triggered
            ..Default::default()
        };
        let findings = analyze(&data);
        let ids: Vec<&str> = findings.iter().map(|f| f.id.as_str()).collect();
        assert!(
            ids.contains(&"silence-mismatch"),
            "expected silence-mismatch in {:?}",
            ids
        );
        assert!(
            ids.contains(&"stuck-sensitivity"),
            "expected stuck-sensitivity in {:?}",
            ids
        );
        assert!(
            ids.contains(&"checkpoint-overhead"),
            "expected checkpoint-overhead in {:?}",
            ids
        );
        assert!(
            !ids.contains(&"silence-waste"),
            "silence-waste should NOT fire"
        );
        assert!(
            !ids.contains(&"stuck-leniency"),
            "stuck-leniency should NOT fire"
        );
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
            revert_count: 0,
            commit_count: 2, // 10/2 = 5.0 — NOT > 5.0, so uncommitted-drift does not fire
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
        assert!(
            ids.contains(&"silence-mismatch"),
            "missing silence-mismatch"
        );
        assert!(ids.contains(&"silence-waste"), "missing silence-waste");
        assert!(
            ids.contains(&"stuck-sensitivity"),
            "missing stuck-sensitivity"
        );
        assert!(ids.contains(&"stuck-leniency"), "missing stuck-leniency");
        assert!(
            ids.contains(&"checkpoint-overhead"),
            "missing checkpoint-overhead"
        );
    }
}
