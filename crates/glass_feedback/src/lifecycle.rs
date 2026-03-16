//! LifecycleManager — stateless functions that advance Rule status through
//! the Proposed → Provisional → Confirmed / Rejected / Stale / Archived
//! lifecycle.

use std::collections::HashMap;

use crate::types::{Finding, FindingAction, Rule, RuleStatus, RunMetrics};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of Provisional rules created in a single `apply_findings` call.
const MAX_PROVISIONAL: usize = 3;
/// Conservative cap used when the previous run ended in bulk rejection.
const MAX_PROVISIONAL_CONSERVATIVE: usize = 1;
/// Number of stale_runs before a Confirmed rule transitions to Stale.
const STALE_THRESHOLD: u32 = 10;
/// Number of stale_runs (total, from Confirmed) before a Stale rule is archived.
const ARCHIVE_THRESHOLD: u32 = 15;
/// Initial cooldown applied when a rule is rejected.
const REJECT_COOLDOWN: u32 = 5;
/// Minimum number of confirmed_run epochs before drift demotion is applied.
const DRIFT_MIN_CONFIRMED_RUNS: u32 = 3;

// ---------------------------------------------------------------------------
// apply_findings
// ---------------------------------------------------------------------------

/// Convert `BehavioralRule` findings into new [`Rule`] structs.
///
/// - Findings are first created as `Proposed`.
/// - Up to `MAX_PROVISIONAL` (or 1 if `bulk_rejection_last_run`) are
///   immediately promoted to `Provisional`.
/// - Findings whose action already exists in `rules` are skipped.
/// - Findings that match a `Rejected` rule (by action + action_params) still
///   in cooldown are skipped.
///
/// The new rules are appended to `rules` **and** returned.
pub fn apply_findings(
    rules: &mut Vec<Rule>,
    findings: &[Finding],
    run_id: &str,
    bulk_rejection_last_run: bool,
) -> Vec<Rule> {
    let cap = if bulk_rejection_last_run {
        MAX_PROVISIONAL_CONSERVATIVE
    } else {
        MAX_PROVISIONAL
    };

    // Build lookup sets for quick duplicate / cooldown checks.
    let existing_actions: std::collections::HashSet<String> =
        rules.iter().map(|r| r.action.clone()).collect();

    // Collect rejected rules still in cooldown for conflict checking.
    let rejected_in_cooldown: Vec<&Rule> = rules
        .iter()
        .filter(|r| r.status == RuleStatus::Rejected && r.cooldown_remaining > 0)
        .collect();

    // Count how many Provisional rules already exist (cap is applied across
    // the full rules vec, not just those created this call).
    let existing_provisional = rules
        .iter()
        .filter(|r| r.status == RuleStatus::Provisional)
        .count();

    let mut provisional_added = 0usize;
    let mut new_rules: Vec<Rule> = Vec::new();

    for finding in findings {
        // Only handle BehavioralRule findings.
        let (action, params) = match &finding.action {
            FindingAction::BehavioralRule { action, params } => (action.clone(), params.clone()),
            _ => continue,
        };

        // Skip if a rule with this action already exists (any status).
        if existing_actions.contains(&action) {
            continue;
        }

        // Skip if there is a rejected rule with matching action + params still
        // in cooldown.
        let in_cooldown = rejected_in_cooldown.iter().any(|r| {
            r.action == action && params_equal(&r.action_params, &params)
        });
        if in_cooldown {
            continue;
        }

        // Determine starting status.
        let status = if existing_provisional + provisional_added < cap {
            provisional_added += 1;
            RuleStatus::Provisional
        } else {
            RuleStatus::Proposed
        };

        let rule = Rule {
            id: finding.id.clone(),
            trigger: "behavioral".to_string(),
            trigger_params: HashMap::new(),
            action: action.clone(),
            action_params: params,
            status,
            severity: finding.severity.clone(),
            scope: finding.scope.clone(),
            tags: vec![],
            added_run: run_id.to_string(),
            added_metric: String::new(),
            confirmed_run: String::new(),
            rejected_run: String::new(),
            rejected_reason: String::new(),
            last_triggered_run: String::new(),
            trigger_count: 0,
            cooldown_remaining: 0,
            stale_runs: 0,
        };

        new_rules.push(rule);
    }

    rules.extend(new_rules.clone());
    new_rules
}

// ---------------------------------------------------------------------------
// promote_provisional
// ---------------------------------------------------------------------------

/// Set all `Provisional` rules to `Confirmed` and record `confirmed_run`.
pub fn promote_provisional(rules: &mut [Rule], run_id: &str) {
    for rule in rules.iter_mut() {
        if rule.status == RuleStatus::Provisional
            && rule.status.can_transition_to(&RuleStatus::Confirmed)
        {
            rule.status = RuleStatus::Confirmed;
            rule.confirmed_run = run_id.to_string();
        }
    }
}

// ---------------------------------------------------------------------------
// reject_provisional
// ---------------------------------------------------------------------------

/// Set all `Provisional` rules to `Rejected`, recording the run, reason, and
/// a cooldown of [`REJECT_COOLDOWN`] runs.
pub fn reject_provisional(rules: &mut [Rule], run_id: &str, reason: &str) {
    for rule in rules.iter_mut() {
        if rule.status == RuleStatus::Provisional
            && rule.status.can_transition_to(&RuleStatus::Rejected)
        {
            rule.status = RuleStatus::Rejected;
            rule.rejected_run = run_id.to_string();
            rule.rejected_reason = reason.to_string();
            rule.cooldown_remaining = REJECT_COOLDOWN;
        }
    }
}

// ---------------------------------------------------------------------------
// update_staleness
// ---------------------------------------------------------------------------

/// Advance staleness counters and archive rules that have been stale too long.
///
/// - `Confirmed` rules with `trigger_count == 0` or no recent trigger have
///   their `stale_runs` incremented.  At [`STALE_THRESHOLD`] they become
///   `Stale`.
/// - `Stale` rules have `stale_runs` incremented further.  At
///   [`ARCHIVE_THRESHOLD`] they are removed from `rules` and pushed to
///   `archived`.
/// - A `Stale` rule that has been re-triggered (`trigger_count > 0`) is
///   promoted back to `Confirmed` and its `stale_runs` is reset.
///
/// `current_run_count` is reserved for future use (e.g. recency window) but
/// is unused at this abstraction level for now.
pub fn update_staleness(rules: &mut Vec<Rule>, archived: &mut Vec<Rule>, _current_run_count: u32) {
    let mut to_archive: Vec<usize> = Vec::new();

    for (idx, rule) in rules.iter_mut().enumerate() {
        match rule.status {
            RuleStatus::Confirmed => {
                // A rule is "inactive" if it has never fired.
                if rule.trigger_count == 0 {
                    rule.stale_runs += 1;
                    if rule.stale_runs >= STALE_THRESHOLD
                        && rule.status.can_transition_to(&RuleStatus::Stale)
                    {
                        rule.status = RuleStatus::Stale;
                    }
                }
                // If trigger_count > 0 the rule is active — leave it alone.
            }
            RuleStatus::Stale => {
                // Re-triggered while stale → promote back to Confirmed.
                if rule.trigger_count > 0 {
                    if rule.status.can_transition_to(&RuleStatus::Confirmed) {
                        rule.status = RuleStatus::Confirmed;
                        rule.stale_runs = 0;
                    }
                } else {
                    rule.stale_runs += 1;
                    if rule.stale_runs >= ARCHIVE_THRESHOLD {
                        to_archive.push(idx);
                    }
                }
            }
            _ => {}
        }
    }

    // Remove archived rules in reverse order to keep indices valid.
    for idx in to_archive.into_iter().rev() {
        let r = rules.remove(idx);
        archived.push(r);
    }
}

// ---------------------------------------------------------------------------
// check_drift
// ---------------------------------------------------------------------------

/// Demote `Confirmed` rules to `Provisional` when a worsening trend is
/// detected in the last 3 metrics entries.
///
/// "Worsening" means each successive metric is strictly worse (higher) than
/// the previous on at least one of `revert_rate`, `stuck_rate`, or
/// `waste_rate`.
///
/// Only rules that have been `Confirmed` for at least
/// [`DRIFT_MIN_CONFIRMED_RUNS`] epochs (approximated by `trigger_count >= 3`,
/// since we don't store a per-run confirmation epoch counter) are demoted.
pub fn check_drift(rules: &mut [Rule], recent_metrics: &[RunMetrics]) {
    if recent_metrics.len() < 3 {
        return;
    }

    // Look at only the last 3 entries.
    let window = &recent_metrics[recent_metrics.len() - 3..];
    let worsening = is_worsening_trend(window);

    if !worsening {
        return;
    }

    for rule in rules.iter_mut() {
        if rule.status == RuleStatus::Confirmed
            && rule.trigger_count >= DRIFT_MIN_CONFIRMED_RUNS
            && rule.status.can_transition_to(&RuleStatus::Provisional)
        {
            rule.status = RuleStatus::Provisional;
        }
    }
}

// ---------------------------------------------------------------------------
// process_cooldowns
// ---------------------------------------------------------------------------

/// Decrement `cooldown_remaining` for each `Rejected` rule.  When it reaches
/// zero, transition the rule back to `Proposed` (re-eligible for promotion).
pub fn process_cooldowns(rules: &mut [Rule]) {
    for rule in rules.iter_mut() {
        if rule.status == RuleStatus::Rejected && rule.cooldown_remaining > 0 {
            rule.cooldown_remaining -= 1;
            if rule.cooldown_remaining == 0
                && rule.status.can_transition_to(&RuleStatus::Proposed)
            {
                rule.status = RuleStatus::Proposed;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn params_equal(a: &HashMap<String, String>, b: &HashMap<String, String>) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().all(|(k, v)| b.get(k) == Some(v))
}

/// Returns true if every consecutive pair of metrics shows a worsening on at
/// least one rate dimension.
fn is_worsening_trend(window: &[RunMetrics]) -> bool {
    debug_assert!(window.len() >= 2);
    window.windows(2).all(|pair| {
        let prev = &pair[0];
        let next = &pair[1];
        next.revert_rate > prev.revert_rate
            || next.stuck_rate > prev.stuck_rate
            || next.waste_rate > prev.waste_rate
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::types::{FindingCategory, Scope, Severity};

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    fn make_rule(id: &str, action: &str, status: RuleStatus) -> Rule {
        Rule {
            id: id.to_string(),
            trigger: "behavioral".to_string(),
            trigger_params: HashMap::new(),
            action: action.to_string(),
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
        }
    }

    fn make_behavioral_finding(id: &str, action: &str) -> Finding {
        Finding {
            id: id.to_string(),
            category: FindingCategory::BehavioralRule,
            severity: Severity::Medium,
            action: FindingAction::BehavioralRule {
                action: action.to_string(),
                params: HashMap::new(),
            },
            evidence: String::new(),
            scope: Scope::Project,
        }
    }

    fn make_metrics(run_id: &str, revert: f64, stuck: f64, waste: f64) -> RunMetrics {
        RunMetrics {
            run_id: run_id.to_string(),
            project_root: "/tmp/project".to_string(),
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
        }
    }

    // -----------------------------------------------------------------------
    // 1. apply_findings_creates_rules
    // -----------------------------------------------------------------------

    #[test]
    fn apply_findings_creates_rules() {
        let mut rules: Vec<Rule> = vec![];
        let findings = vec![
            make_behavioral_finding("f1", "extend_silence"),
            make_behavioral_finding("f2", "force_commit"),
        ];

        let new_rules = apply_findings(&mut rules, &findings, "run-001", false);

        assert_eq!(new_rules.len(), 2);
        assert_eq!(rules.len(), 2);

        // First two become Provisional (within cap of 3).
        assert_eq!(new_rules[0].status, RuleStatus::Provisional);
        assert_eq!(new_rules[1].status, RuleStatus::Provisional);
    }

    // -----------------------------------------------------------------------
    // 2. apply_findings_respects_cap
    // -----------------------------------------------------------------------

    #[test]
    fn apply_findings_respects_cap() {
        let mut rules: Vec<Rule> = vec![];
        let findings = vec![
            make_behavioral_finding("f1", "action_a"),
            make_behavioral_finding("f2", "action_b"),
            make_behavioral_finding("f3", "action_c"),
            make_behavioral_finding("f4", "action_d"),
        ];

        let new_rules = apply_findings(&mut rules, &findings, "run-001", false);

        assert_eq!(new_rules.len(), 4);
        let provisional_count = new_rules
            .iter()
            .filter(|r| r.status == RuleStatus::Provisional)
            .count();
        let proposed_count = new_rules
            .iter()
            .filter(|r| r.status == RuleStatus::Proposed)
            .count();
        assert_eq!(provisional_count, 3);
        assert_eq!(proposed_count, 1);
    }

    // -----------------------------------------------------------------------
    // 3. apply_findings_conservative_after_rejection
    // -----------------------------------------------------------------------

    #[test]
    fn apply_findings_conservative_after_rejection() {
        let mut rules: Vec<Rule> = vec![];
        let findings = vec![
            make_behavioral_finding("f1", "action_a"),
            make_behavioral_finding("f2", "action_b"),
            make_behavioral_finding("f3", "action_c"),
        ];

        let new_rules = apply_findings(&mut rules, &findings, "run-001", true);

        let provisional_count = new_rules
            .iter()
            .filter(|r| r.status == RuleStatus::Provisional)
            .count();
        assert_eq!(provisional_count, 1);
        assert_eq!(new_rules.len(), 3);
    }

    // -----------------------------------------------------------------------
    // 4. apply_findings_skips_duplicates
    // -----------------------------------------------------------------------

    #[test]
    fn apply_findings_skips_duplicates() {
        let mut rules = vec![make_rule("r1", "extend_silence", RuleStatus::Confirmed)];
        let findings = vec![
            make_behavioral_finding("f1", "extend_silence"), // duplicate
            make_behavioral_finding("f2", "force_commit"),   // new
        ];

        let new_rules = apply_findings(&mut rules, &findings, "run-002", false);

        assert_eq!(new_rules.len(), 1);
        assert_eq!(new_rules[0].action, "force_commit");
        assert_eq!(rules.len(), 2);
    }

    // -----------------------------------------------------------------------
    // 5. promote_provisional_sets_confirmed
    // -----------------------------------------------------------------------

    #[test]
    fn promote_provisional_sets_confirmed() {
        let mut rules = vec![
            make_rule("r1", "action_a", RuleStatus::Provisional),
            make_rule("r2", "action_b", RuleStatus::Provisional),
            make_rule("r3", "action_c", RuleStatus::Proposed),
        ];

        promote_provisional(&mut rules, "run-003");

        assert_eq!(rules[0].status, RuleStatus::Confirmed);
        assert_eq!(rules[0].confirmed_run, "run-003");
        assert_eq!(rules[1].status, RuleStatus::Confirmed);
        assert_eq!(rules[1].confirmed_run, "run-003");
        // Proposed rule is unchanged.
        assert_eq!(rules[2].status, RuleStatus::Proposed);
        assert_eq!(rules[2].confirmed_run, "");
    }

    // -----------------------------------------------------------------------
    // 6. reject_provisional_sets_rejected
    // -----------------------------------------------------------------------

    #[test]
    fn reject_provisional_sets_rejected() {
        let mut rules = vec![
            make_rule("r1", "action_a", RuleStatus::Provisional),
            make_rule("r2", "action_b", RuleStatus::Confirmed),
        ];

        reject_provisional(&mut rules, "run-004", "regression detected");

        assert_eq!(rules[0].status, RuleStatus::Rejected);
        assert_eq!(rules[0].rejected_run, "run-004");
        assert_eq!(rules[0].rejected_reason, "regression detected");
        assert_eq!(rules[0].cooldown_remaining, REJECT_COOLDOWN);
        // Confirmed rule is unchanged.
        assert_eq!(rules[1].status, RuleStatus::Confirmed);
    }

    // -----------------------------------------------------------------------
    // 7. staleness_marks_stale_after_10
    // -----------------------------------------------------------------------

    #[test]
    fn staleness_marks_stale_after_10() {
        let mut rule = make_rule("r1", "action_a", RuleStatus::Confirmed);
        rule.trigger_count = 0;
        rule.stale_runs = 9; // one more will cross threshold

        let mut rules = vec![rule];
        let mut archived: Vec<Rule> = vec![];

        update_staleness(&mut rules, &mut archived, 10);

        assert_eq!(rules[0].status, RuleStatus::Stale);
        assert_eq!(rules[0].stale_runs, 10);
        assert!(archived.is_empty());
    }

    // -----------------------------------------------------------------------
    // 8. staleness_archives_after_15
    // -----------------------------------------------------------------------

    #[test]
    fn staleness_archives_after_15() {
        let mut rule = make_rule("r1", "action_a", RuleStatus::Stale);
        rule.trigger_count = 0;
        rule.stale_runs = 14; // one more push past ARCHIVE_THRESHOLD

        let mut rules = vec![rule];
        let mut archived: Vec<Rule> = vec![];

        update_staleness(&mut rules, &mut archived, 20);

        assert!(rules.is_empty(), "stale rule should have been archived");
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].id, "r1");
    }

    // -----------------------------------------------------------------------
    // 9. staleness_reset_on_trigger
    // -----------------------------------------------------------------------

    #[test]
    fn staleness_reset_on_trigger() {
        let mut rule = make_rule("r1", "action_a", RuleStatus::Stale);
        rule.trigger_count = 2; // re-triggered while stale
        rule.stale_runs = 12;

        let mut rules = vec![rule];
        let mut archived: Vec<Rule> = vec![];

        update_staleness(&mut rules, &mut archived, 20);

        assert_eq!(rules[0].status, RuleStatus::Confirmed);
        assert_eq!(rules[0].stale_runs, 0);
        assert!(archived.is_empty());
    }

    // -----------------------------------------------------------------------
    // 10. drift_demotes_confirmed
    // -----------------------------------------------------------------------

    #[test]
    fn drift_demotes_confirmed() {
        let mut rule = make_rule("r1", "action_a", RuleStatus::Confirmed);
        rule.trigger_count = DRIFT_MIN_CONFIRMED_RUNS; // eligible

        let mut rules = vec![rule];

        let metrics = vec![
            make_metrics("run-001", 0.10, 0.05, 0.10),
            make_metrics("run-002", 0.15, 0.08, 0.12),
            make_metrics("run-003", 0.20, 0.12, 0.15),
        ];

        check_drift(&mut rules, &metrics);

        assert_eq!(rules[0].status, RuleStatus::Provisional);
    }

    // -----------------------------------------------------------------------
    // 11. cooldown_decrement
    // -----------------------------------------------------------------------

    #[test]
    fn cooldown_decrement() {
        let mut rule = make_rule("r1", "action_a", RuleStatus::Rejected);
        rule.cooldown_remaining = 3;

        let mut rules = vec![rule];
        process_cooldowns(&mut rules);

        assert_eq!(rules[0].cooldown_remaining, 2);
        assert_eq!(rules[0].status, RuleStatus::Rejected); // not yet 0
    }

    // -----------------------------------------------------------------------
    // 12. cooldown_re_propose
    // -----------------------------------------------------------------------

    #[test]
    fn cooldown_re_propose() {
        let mut rule = make_rule("r1", "action_a", RuleStatus::Rejected);
        rule.cooldown_remaining = 1;

        let mut rules = vec![rule];
        process_cooldowns(&mut rules);

        assert_eq!(rules[0].cooldown_remaining, 0);
        assert_eq!(rules[0].status, RuleStatus::Proposed);
    }

    // -----------------------------------------------------------------------
    // Extra: apply_findings skips rejected rules in cooldown
    // -----------------------------------------------------------------------

    #[test]
    fn apply_findings_skips_rejected_in_cooldown() {
        let mut rejected = make_rule("r1", "extend_silence", RuleStatus::Rejected);
        rejected.cooldown_remaining = 3;

        let mut rules = vec![rejected];
        let findings = vec![make_behavioral_finding("f1", "extend_silence")];

        let new_rules = apply_findings(&mut rules, &findings, "run-002", false);

        assert!(new_rules.is_empty(), "finding matching cooldown-rejected rule should be skipped");
    }

    // -----------------------------------------------------------------------
    // Extra: drift does NOT demote rules confirmed for fewer than 3 runs
    // -----------------------------------------------------------------------

    #[test]
    fn drift_does_not_demote_recently_confirmed() {
        let mut rule = make_rule("r1", "action_a", RuleStatus::Confirmed);
        rule.trigger_count = 2; // below DRIFT_MIN_CONFIRMED_RUNS (3)

        let mut rules = vec![rule];

        let metrics = vec![
            make_metrics("run-001", 0.10, 0.05, 0.10),
            make_metrics("run-002", 0.15, 0.08, 0.12),
            make_metrics("run-003", 0.20, 0.12, 0.15),
        ];

        check_drift(&mut rules, &metrics);

        // Rule should remain Confirmed because it hasn't been confirmed long enough.
        assert_eq!(rules[0].status, RuleStatus::Confirmed);
    }

    // -----------------------------------------------------------------------
    // Extra: drift no-op when fewer than 3 metrics entries
    // -----------------------------------------------------------------------

    #[test]
    fn drift_no_op_with_insufficient_metrics() {
        let mut rule = make_rule("r1", "action_a", RuleStatus::Confirmed);
        rule.trigger_count = 5;

        let mut rules = vec![rule];

        let metrics = vec![
            make_metrics("run-001", 0.10, 0.05, 0.10),
            make_metrics("run-002", 0.15, 0.08, 0.12),
        ];

        check_drift(&mut rules, &metrics);

        assert_eq!(rules[0].status, RuleStatus::Confirmed);
    }

    // -----------------------------------------------------------------------
    // Extra: apply_findings only handles BehavioralRule findings
    // -----------------------------------------------------------------------

    #[test]
    fn apply_findings_ignores_non_behavioral() {
        let mut rules: Vec<Rule> = vec![];
        let findings = vec![
            Finding {
                id: "f1".to_string(),
                category: FindingCategory::ConfigTuning,
                severity: Severity::Medium,
                action: FindingAction::ConfigTuning {
                    field: "silence_timeout".to_string(),
                    current_value: "30".to_string(),
                    new_value: "45".to_string(),
                },
                evidence: String::new(),
                scope: Scope::Project,
            },
            Finding {
                id: "f2".to_string(),
                category: FindingCategory::PromptHint,
                severity: Severity::Low,
                action: FindingAction::PromptHint {
                    text: "Be concise".to_string(),
                },
                evidence: String::new(),
                scope: Scope::Project,
            },
        ];

        let new_rules = apply_findings(&mut rules, &findings, "run-001", false);

        assert!(new_rules.is_empty());
        assert!(rules.is_empty());
    }
}
