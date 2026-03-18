//! glass_feedback — Self-improving orchestrator feedback loop.
//!
//! Analyzes orchestrator runs, produces findings across three tiers
//! (config tuning, behavioral rules, prompt hints), applies changes
//! through a guarded lifecycle, and auto-rolls back regressions.

pub mod analyzer;
pub mod defaults;
pub mod io;
pub mod lifecycle;
pub mod llm;
pub mod quality;
pub mod regression;
pub mod rules;
pub mod coverage;
pub mod types;

#[allow(unused_imports)]
pub use types::*;

use std::collections::HashMap;
use std::path::PathBuf;

use io::{
    load_archived_rules, load_metrics_file, load_rules_file, save_archived_rules,
    save_metrics_file, save_rules_file,
};

// ---------------------------------------------------------------------------
// Public state/result types
// ---------------------------------------------------------------------------

/// State handle returned by `on_run_start`, passed to `on_run_end`.
pub struct FeedbackState {
    pub project_root: String,
    pub rules_path: PathBuf,
    pub global_rules_path: PathBuf,
    pub metrics_path: PathBuf,
    pub history_path: PathBuf,
    pub archived_path: PathBuf,
    pub snapshot: ConfigSnapshot,
    pub engine: rules::RuleEngine,
    pub feedback_llm: bool,
    pub max_prompt_hints: usize,
}

/// Result of `on_run_end`.
pub struct FeedbackResult {
    pub findings: Vec<Finding>,
    pub regression: Option<regression::RegressionResult>,
    pub rules_promoted: Vec<String>,
    pub rules_rejected: Vec<String>,
    pub config_changes: Vec<(String, String, String)>, // (field, old, new)
    /// LLM analysis prompt — None if feedback_llm is disabled.
    /// The caller should send this to an ephemeral agent and pass
    /// the response to `apply_llm_findings`.
    pub llm_prompt: Option<String>,
}

// ---------------------------------------------------------------------------
// on_run_start
// ---------------------------------------------------------------------------

/// Initialise the feedback system for an upcoming orchestrator run.
///
/// 1. Computes canonical file paths under `<project_root>/.glass/` and
///    `~/.glass/`.
/// 2. Detects whether the previous run was incomplete (crashed).
/// 3. Loads the merged rule engine from project + global rule files.
/// 4. Takes a config snapshot and persists it to the tuning-history file.
/// 5. Returns a [`FeedbackState`] for use with [`on_run_end`].
pub fn on_run_start(project_root: &str, config: &FeedbackConfig) -> FeedbackState {
    let project_dir = PathBuf::from(project_root).join(".glass");

    let rules_path = project_dir.join("rules.toml");
    let metrics_path = project_dir.join("run-metrics.toml");
    let history_path = project_dir.join("tuning-history.toml");
    let archived_path = project_dir.join("archived-rules.toml");

    let global_rules_path = home_dir().join(".glass").join("global-rules.toml");

    // Detect incomplete previous run (log but do not block startup).
    let _incomplete = regression::check_incomplete_run(&history_path, &metrics_path);

    // Load and merge rule engine (project > global, no defaults path here).
    let mut engine = rules::RuleEngine::load(&rules_path, &global_rules_path, None);

    // Reset trigger_count per-run so staleness detection accurately
    // reflects whether the rule fired during THIS run.
    for rule in &mut engine.rules {
        rule.trigger_count = 0;
    }

    // Build config values snapshot — use values from the RunData fields that
    // are available at start time.  For now we snapshot what we know from the
    // FeedbackConfig; more fields can be added as the config struct grows.
    let mut config_values: HashMap<String, String> = HashMap::new();
    config_values.insert("feedback_llm".to_string(), config.feedback_llm.to_string());
    config_values.insert(
        "max_prompt_hints".to_string(),
        config.max_prompt_hints.to_string(),
    );
    if let Some(v) = config.silence_timeout_secs {
        config_values.insert("silence_timeout_secs".to_string(), v.to_string());
    }
    if let Some(v) = config.max_retries_before_stuck {
        config_values.insert("max_retries_before_stuck".to_string(), v.to_string());
    }

    // IDs of currently-provisional rules become part of the snapshot.
    let project_rules = load_rules_file(&rules_path);
    let provisional_rules: Vec<String> = project_rules
        .rules
        .iter()
        .filter(|r| r.status == RuleStatus::Provisional)
        .map(|r| r.id.clone())
        .collect();

    let run_id = generate_run_id();
    let snapshot =
        regression::take_snapshot(config_values, provisional_rules, &run_id, &history_path);

    FeedbackState {
        project_root: project_root.to_string(),
        rules_path,
        global_rules_path,
        metrics_path,
        history_path,
        archived_path,
        snapshot,
        engine,
        feedback_llm: config.feedback_llm,
        max_prompt_hints: config.max_prompt_hints,
    }
}

// ---------------------------------------------------------------------------
// on_run_end
// ---------------------------------------------------------------------------

/// Process the completed orchestrator run and update all persistent state.
///
/// Steps:
/// 1. Run `analyzer::analyze` to get rule-based findings.
/// 2. Compute [`RunMetrics`] from the supplied [`RunData`].
/// 3. Load previous metrics and get the baseline (last entry).
/// 4. Run [`regression::compare`].
/// 5. Promote or reject provisional rules based on the regression result.
/// 6. Apply new findings via [`lifecycle::apply_findings`].
/// 7. Update staleness counters and process cooldowns.
/// 8. Check for drift.
/// 9. Extract Tier 1 (ConfigTuning) findings → `config_changes` (max 1).
/// 10. Persist updated rules and metrics.
pub fn on_run_end(state: FeedbackState, data: RunData) -> FeedbackResult {
    // --- Step 1: analyze ---
    let findings = analyzer::analyze(&data);

    // --- Step 2: compute current metrics ---
    let current_metrics = metrics_from_run_data(&state.snapshot.run_id, &data);

    // --- Step 3: load previous metrics ---
    let mut metrics_file = load_metrics_file(&state.metrics_path);
    let baseline: Option<RunMetrics> = metrics_file.runs.last().cloned();

    // --- Step 4: regression comparison ---
    let regression = regression::compare(&current_metrics, baseline.as_ref());

    // --- Step 5: promote / reject provisionals ---
    let mut project_rules_file = load_rules_file(&state.rules_path);

    let mut rules_promoted: Vec<String> = Vec::new();
    let mut rules_rejected: Vec<String> = Vec::new();

    let bulk_rejection = match &regression {
        Some(regression::RegressionResult::Regressed { .. }) => {
            // Collect IDs before mutating.
            rules_rejected = project_rules_file
                .rules
                .iter()
                .filter(|r| r.status == RuleStatus::Provisional)
                .map(|r| r.id.clone())
                .collect();
            lifecycle::reject_provisional(
                &mut project_rules_file.rules,
                &state.snapshot.run_id,
                "regression detected",
            );
            true
        }
        _ => {
            // Improved or Neutral — promote.
            rules_promoted = project_rules_file
                .rules
                .iter()
                .filter(|r| r.status == RuleStatus::Provisional)
                .map(|r| r.id.clone())
                .collect();
            lifecycle::promote_provisional(&mut project_rules_file.rules, &state.snapshot.run_id);
            false
        }
    };

    // --- Step 6: apply new findings ---
    lifecycle::apply_findings(
        &mut project_rules_file.rules,
        &findings,
        &state.snapshot.run_id,
        bulk_rejection,
    );

    // --- Step 7: staleness + cooldowns ---
    let mut archived_file = load_archived_rules(&state.archived_path);
    let run_count = metrics_file.runs.len() as u32;
    lifecycle::update_staleness(
        &mut project_rules_file.rules,
        &mut archived_file.rules,
        run_count,
    );
    lifecycle::process_cooldowns(&mut project_rules_file.rules);

    // --- Step 8: drift check ---
    let recent_metrics: Vec<RunMetrics> = metrics_file
        .runs
        .iter()
        .rev()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    lifecycle::check_drift(&mut project_rules_file.rules, &recent_metrics);

    // --- Step 9: extract ConfigTuning findings (max 1 per run) ---
    let config_changes: Vec<(String, String, String)> = findings
        .iter()
        .filter_map(|f| {
            if let FindingAction::ConfigTuning {
                field,
                current_value,
                new_value,
            } = &f.action
            {
                Some((field.clone(), current_value.clone(), new_value.clone()))
            } else {
                None
            }
        })
        .take(1)
        .collect();

    // --- Step 9b: build LLM analysis prompt if enabled ---
    let llm_prompt = if state.feedback_llm {
        Some(llm::build_analysis_prompt(&data, &findings))
    } else {
        None
    };

    // --- Step 10: persist ---
    metrics_file.runs.push(current_metrics);
    let _ = save_metrics_file(&state.metrics_path, &metrics_file);
    let _ = save_rules_file(&state.rules_path, &project_rules_file);
    let _ = save_archived_rules(&state.archived_path, &archived_file);

    // --- Step 10b: sync global-scoped rules to ~/.glass/global-rules.toml ---
    // Rules with scope=Global are useful across projects. Merge them into the
    // global rules file so other projects benefit from learnings.
    let global_rules_from_project: Vec<_> = project_rules_file
        .rules
        .iter()
        .filter(|r| matches!(r.scope, types::Scope::Global))
        .filter(|r| {
            matches!(
                r.status,
                types::RuleStatus::Provisional | types::RuleStatus::Confirmed
            )
        })
        .cloned()
        .collect();
    if !global_rules_from_project.is_empty() {
        let mut global_file = load_rules_file(&state.global_rules_path);
        for rule in &global_rules_from_project {
            // Upsert: replace existing rule with same ID, or append
            if let Some(existing) = global_file.rules.iter_mut().find(|r| r.id == rule.id) {
                *existing = rule.clone();
            } else {
                global_file.rules.push(rule.clone());
            }
        }
        // Remove global rules that were rejected or archived in this project
        let rejected_ids: Vec<String> = project_rules_file
            .rules
            .iter()
            .filter(|r| matches!(r.scope, types::Scope::Global))
            .filter(|r| {
                matches!(
                    r.status,
                    types::RuleStatus::Rejected | types::RuleStatus::Stale
                )
            })
            .map(|r| r.id.clone())
            .collect();
        global_file.rules.retain(|r| !rejected_ids.contains(&r.id));

        let _ = save_rules_file(&state.global_rules_path, &global_file);
    }

    FeedbackResult {
        findings,
        regression,
        rules_promoted,
        rules_rejected,
        config_changes,
        llm_prompt,
    }
}

// ---------------------------------------------------------------------------
// check_rules / prompt_hints
// ---------------------------------------------------------------------------

/// Evaluate active rules against the live run state and return any triggered
/// [`RuleAction`]s.  Delegates to the [`rules::RuleEngine`] stored in `state`.
pub fn check_rules(state: &mut FeedbackState, run_state: &RunState) -> Vec<RuleAction> {
    state.engine.check_rules(run_state)
}

/// Return the text of all `prompt_hint` rules that are `Confirmed` or
/// `Provisional`.  Delegates to the [`rules::RuleEngine`] stored in `state`.
pub fn prompt_hints(state: &FeedbackState) -> Vec<String> {
    state.engine.prompt_hints()
}

/// Apply LLM-generated findings to the project's rules file.
///
/// Called asynchronously after `on_run_end` when the ephemeral agent
/// returns its analysis. Parses the response, deduplicates against
/// existing prompt_hint rules, and persists to rules.toml.
pub fn apply_llm_findings(project_root: &str, llm_response: &str, max_prompt_hints: usize) {
    let project_dir = PathBuf::from(project_root).join(".glass");
    let rules_path = project_dir.join("rules.toml");

    let raw_findings = llm::parse_llm_response(llm_response);
    if raw_findings.is_empty() {
        return;
    }

    // Load current rules to get existing prompt_hint rules for dedup
    let rules_file = load_rules_file(&rules_path);
    let existing_hints: Vec<_> = rules_file
        .rules
        .iter()
        .filter(|r| r.action == "prompt_hint")
        .cloned()
        .collect();

    let deduped = llm::dedup_findings(raw_findings, &existing_hints, max_prompt_hints);
    if deduped.is_empty() {
        return;
    }

    // Apply as new findings — they enter as Provisional
    let mut rules_file = load_rules_file(&rules_path);
    let run_id = format!(
        "llm-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    );
    lifecycle::apply_findings(&mut rules_file.rules, &deduped, &run_id, false);
    let _ = save_rules_file(&rules_path, &rules_file);

    tracing::info!(
        "Feedback LLM: applied {} prompt hint(s) to {}",
        deduped.len(),
        rules_path.display()
    );
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Compute a [`RunMetrics`] struct from a completed [`RunData`].
fn metrics_from_run_data(run_id: &str, data: &RunData) -> RunMetrics {
    let total = data.revert_count + data.keep_count;
    let revert_rate = if total > 0 {
        data.revert_count as f64 / total as f64
    } else {
        0.0
    };

    let iter_nonzero = data.iterations.max(1);
    let stuck_rate = data.stuck_count as f64 / iter_nonzero as f64;
    let waste_rate = data.waste_count as f64 / iter_nonzero as f64;
    let checkpoint_rate = data.checkpoint_count as f64 / iter_nonzero as f64;

    let prd_total = data
        .prd_content
        .as_deref()
        .map(|p| {
            p.lines()
                .filter(|l| {
                    l.trim_start().starts_with("- [ ]") || l.trim_start().starts_with("- [x]")
                })
                .count() as u32
        })
        .unwrap_or(0);

    RunMetrics {
        run_id: run_id.to_string(),
        project_root: data.project_root.clone(),
        iterations: data.iterations,
        duration_secs: data.duration_secs,
        revert_rate,
        stuck_rate,
        waste_rate,
        checkpoint_rate,
        completion: data.completion_reason.clone(),
        prd_items_completed: data.prd_content.as_deref()
            .map(|p| p.lines()
                .filter(|l| l.trim_start().starts_with("- [x]"))
                .count() as u32)
            .unwrap_or(0),
        prd_items_total: prd_total,
        kickoff_duration_secs: data.kickoff_duration_secs,
    }
}

/// Generate a simple run ID based on the current Unix timestamp.
fn generate_run_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("run-{ts}")
}

/// Return the user's home directory as a [`PathBuf`].
///
/// Falls back to `"."` if the environment variable is not set.
fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tempfile::TempDir;

    use super::*;
    use crate::io::{save_rules_file, save_tuning_history};
    use crate::types::{
        ConfigSnapshot, Rule, RuleStatus, RulesFile, RulesMeta, Scope, Severity, TuningHistoryFile,
    };

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn make_run_data(project_root: &str) -> RunData {
        RunData {
            project_root: project_root.to_string(),
            iterations: 10,
            duration_secs: 600,
            kickoff_duration_secs: 30,
            revert_count: 1,
            keep_count: 9,
            stuck_count: 1,
            checkpoint_count: 2,
            waste_count: 1,
            commit_count: 5,
            completion_reason: "success".to_string(),
            ..RunData::default()
        }
    }

    /// Build a FeedbackState that points all paths at a temp directory.
    fn make_state_in_dir(dir: &TempDir) -> FeedbackState {
        let root = dir.path().to_str().unwrap().to_string();
        let glass_dir = dir.path().join(".glass");
        std::fs::create_dir_all(&glass_dir).unwrap();

        let rules_path = glass_dir.join("rules.toml");
        let metrics_path = glass_dir.join("run-metrics.toml");
        let history_path = glass_dir.join("tuning-history.toml");
        let archived_path = glass_dir.join("archived-rules.toml");
        let global_rules_path = glass_dir.join("global-rules.toml");

        let snapshot = ConfigSnapshot {
            run_id: "run-test".to_string(),
            config_values: HashMap::new(),
            provisional_rules: vec![],
        };

        let engine = rules::RuleEngine { rules: vec![] };

        FeedbackState {
            project_root: root,
            rules_path,
            global_rules_path,
            metrics_path,
            history_path,
            archived_path,
            feedback_llm: false,
            max_prompt_hints: 10,
            snapshot,
            engine,
        }
    }

    fn make_rule(id: &str, action: &str, status: RuleStatus) -> Rule {
        Rule {
            id: id.to_string(),
            trigger: "always".to_string(),
            trigger_params: HashMap::new(),
            action: action.to_string(),
            action_params: HashMap::new(),
            status,
            severity: Severity::Medium,
            scope: Scope::Project,
            tags: vec![],
            added_run: "run-000".to_string(),
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

    // -----------------------------------------------------------------------
    // 1. full_lifecycle_cold_start
    //    First run: no baseline → regression is None, findings may be produced.
    // -----------------------------------------------------------------------

    #[test]
    fn full_lifecycle_cold_start() {
        let dir = TempDir::new().unwrap();
        let state = make_state_in_dir(&dir);
        let data = make_run_data(&state.project_root);

        let result = on_run_end(state, data);

        // Cold start — no baseline → regression must be None.
        assert!(
            result.regression.is_none(),
            "cold start must yield no regression result"
        );

        // No provisional rules existed, so nothing to promote/reject.
        assert!(result.rules_promoted.is_empty());
        assert!(result.rules_rejected.is_empty());

        // Metrics file should now exist with one entry.
        let glass_dir = dir.path().join(".glass");
        let metrics = crate::io::load_metrics_file(&glass_dir.join("run-metrics.toml"));
        assert_eq!(metrics.runs.len(), 1);
    }

    // -----------------------------------------------------------------------
    // 2. full_lifecycle_second_run_no_regression
    //    Two runs: second run improves → provisionals are promoted.
    // -----------------------------------------------------------------------

    #[test]
    fn full_lifecycle_second_run_no_regression() {
        let dir = TempDir::new().unwrap();

        // --- Run 1 ---
        {
            let state = make_state_in_dir(&dir);
            let data = make_run_data(&state.project_root.clone());
            let _ = on_run_end(state, data);
        }

        // Insert a provisional rule into the rules file before run 2.
        let glass_dir = dir.path().join(".glass");
        let rules_path = glass_dir.join("rules.toml");
        let prov_rule = make_rule("prov-001", "extend_silence", RuleStatus::Provisional);
        let rules_file = RulesFile {
            meta: RulesMeta::default(),
            rules: vec![prov_rule],
        };
        save_rules_file(&rules_path, &rules_file).unwrap();

        // Also write a tuning-history entry so check_incomplete_run has something.
        let history = TuningHistoryFile {
            snapshots: vec![ConfigSnapshot {
                run_id: "run-prev".to_string(),
                config_values: HashMap::new(),
                provisional_rules: vec!["prov-001".to_string()],
            }],
        };
        let history_path = glass_dir.join("tuning-history.toml");
        save_tuning_history(&history_path, &history).unwrap();

        // --- Run 2: improved metrics (very low rates) ---
        let mut state2 = make_state_in_dir(&dir);
        // Override snapshot run_id so metrics entry has a distinct ID.
        state2.snapshot.run_id = "run-002".to_string();

        let data2 = RunData {
            project_root: state2.project_root.clone(),
            iterations: 20,
            duration_secs: 900,
            revert_count: 0, // revert_rate = 0 (improved vs run 1)
            keep_count: 20,
            stuck_count: 0,
            checkpoint_count: 1,
            waste_count: 0,
            commit_count: 10,
            completion_reason: "success".to_string(),
            ..RunData::default()
        };

        let result2 = on_run_end(state2, data2);

        // Should have a regression result (Improved or Neutral — not Regressed).
        match &result2.regression {
            Some(regression::RegressionResult::Regressed { .. }) => {
                panic!("second run with lower rates should not be Regressed");
            }
            _ => {} // None (if baseline ended up None), Improved, or Neutral — all fine
        }

        // The provisional rule "prov-001" should have been promoted.
        assert!(
            result2.rules_promoted.contains(&"prov-001".to_string()),
            "provisional rule should have been promoted on improvement; promoted={:?}",
            result2.rules_promoted
        );
        assert!(result2.rules_rejected.is_empty());
    }

    // -----------------------------------------------------------------------
    // 3. check_rules_delegates
    //    check_rules returns actions from the loaded engine.
    // -----------------------------------------------------------------------

    #[test]
    fn check_rules_delegates() {
        let dir = TempDir::new().unwrap();
        let mut state = make_state_in_dir(&dir);

        // Inject a confirmed "force_commit" rule into the engine.
        state.engine = rules::RuleEngine {
            rules: vec![make_rule("r1", "force_commit", RuleStatus::Confirmed)],
        };

        let run_state = RunState {
            iterations_since_last_commit: 6, // > 5 → fires
            ..RunState::default()
        };

        let actions = check_rules(&mut state, &run_state);
        assert!(!actions.is_empty(), "force_commit rule should fire");

        let has_force_commit = actions.iter().any(|a| matches!(a, RuleAction::ForceCommit));
        assert!(has_force_commit, "expected ForceCommit action");
    }

    // -----------------------------------------------------------------------
    // 4. prompt_hints_delegates
    //    prompt_hints returns hints from the loaded engine.
    // -----------------------------------------------------------------------

    #[test]
    fn prompt_hints_delegates() {
        let dir = TempDir::new().unwrap();
        let mut state = make_state_in_dir(&dir);

        // Build a confirmed prompt_hint rule.
        let mut hint_rule = make_rule("h1", "prompt_hint", RuleStatus::Confirmed);
        hint_rule
            .action_params
            .insert("text".to_string(), "Keep PRs small".to_string());

        state.engine = rules::RuleEngine {
            rules: vec![hint_rule],
        };

        let hints = prompt_hints(&state);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0], "Keep PRs small");
    }
}
