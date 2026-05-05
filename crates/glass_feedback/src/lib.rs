//! glass_feedback — Self-improving orchestrator feedback loop.
//!
//! Analyzes orchestrator runs, produces findings across three tiers
//! (config tuning, behavioral rules, prompt hints), applies changes
//! through a guarded lifecycle, and auto-rolls back regressions.

pub mod ablation;
pub mod analyzer;
pub mod attribution;
pub mod coverage;
pub mod defaults;
pub mod io;
pub mod lifecycle;
pub mod llm;
pub mod quality;
pub mod regression;
pub mod rules;
pub mod types;

pub use types::*;

use std::collections::HashMap;
use std::path::PathBuf;

use io::{
    load_archived_rules, load_attribution_file, load_metrics_file, load_rules_file,
    save_archived_rules, save_attribution_file, save_metrics_file, save_rules_file,
};

// ---------------------------------------------------------------------------
// Public state/result types
// ---------------------------------------------------------------------------

/// State handle returned by `on_run_start`, passed to `on_run_end`.
pub struct FeedbackState {
    /// Canonical project root path used to scope feedback data.
    pub project_root: String,
    /// Path to the project-local rules file.
    pub rules_path: PathBuf,
    /// Path to the global (cross-project) rules file.
    pub global_rules_path: PathBuf,
    /// Path to the metrics history file.
    pub metrics_path: PathBuf,
    /// Path to the per-run history log.
    pub history_path: PathBuf,
    /// Path to the archived rules directory.
    pub archived_path: PathBuf,
    /// Snapshot of the agent config at run start (used for diff detection).
    pub snapshot: ConfigSnapshot,
    /// Rule engine loaded with current project + global rules.
    pub engine: rules::RuleEngine,
    /// Whether LLM-based qualitative analysis is enabled.
    pub feedback_llm: bool,
    /// Maximum number of prompt hints to inject per session.
    pub max_prompt_hints: usize,
    /// Rule ID currently targeted for ablation testing, if any.
    pub ablation_target: Option<String>,
    /// Run ID of the most recent ablation sweep.
    pub last_sweep_run: String,
    /// Per-rule attribution scores from the last analysis.
    pub attribution_scores: Vec<types::AttributionScore>,
    /// Path to the attribution scores file.
    pub attribution_path: std::path::PathBuf,
    /// Whether ablation testing is enabled.
    pub ablation_enabled: bool,
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
    /// Tier 4 script generation prompt — None unless existing tiers
    /// produced no findings but the run had high waste or stuck rates.
    /// The caller should send this to an ephemeral agent that returns
    /// a Rhai script to install via the scripting layer.
    pub script_prompt: Option<String>,
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

    let attribution_path = project_dir.join("rule-attribution.toml");

    let mut state = FeedbackState {
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
        ablation_target: None,
        last_sweep_run: String::new(),
        attribution_scores: vec![],
        attribution_path,
        ablation_enabled: config.ablation_enabled,
    };

    // Load attribution data.
    state.attribution_scores = load_attribution_file(&state.attribution_path).scores;

    // Check ablation conditions: only when enabled and no provisionals.
    if state.ablation_enabled {
        let has_provisionals = state
            .engine
            .rules
            .iter()
            .any(|r| r.status == types::RuleStatus::Provisional);
        if !has_provisionals {
            state.ablation_target = ablation::select_target(
                &state.engine.rules,
                &state.attribution_scores,
                &state.last_sweep_run,
            );
        }
    }

    state
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
    let mut attribution_scores = state.attribution_scores;
    if let Some(ref base) = baseline {
        let deltas = types::MetricDeltas {
            revert_rate: current_metrics.revert_rate - base.revert_rate,
            stuck_rate: current_metrics.stuck_rate - base.stuck_rate,
            waste_rate: current_metrics.waste_rate - base.waste_rate,
        };
        let all_rule_ids: Vec<String> = state.engine.rules.iter().map(|r| r.id.clone()).collect();
        attribution::update(
            &mut attribution_scores,
            &rule_firings,
            &all_rule_ids,
            &deltas,
            &state.snapshot.run_id,
        );
    }

    // --- Step 4: regression comparison ---
    let regression = regression::compare(&current_metrics, baseline.as_ref());

    // --- Step 5: promote / reject provisionals ---
    let mut project_rules_file = load_rules_file(&state.rules_path);
    // Capture whether any rules existed BEFORE this run's findings are applied.
    // Used by Step 9c to determine if lower tiers have been tried in prior runs.
    let had_rules_before_run = !project_rules_file.rules.is_empty();

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

    // Prune attribution scores for archived rules
    let active_ids: Vec<String> = project_rules_file
        .rules
        .iter()
        .map(|r| r.id.clone())
        .collect();
    attribution::prune(&mut attribution_scores, &active_ids);

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

    // --- Step 3d: evaluate ablation ---
    if let Some(ref target_id) = state.ablation_target.clone() {
        let recent: Vec<_> = metrics_file
            .runs
            .iter()
            .rev()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let ablation_result = ablation::evaluate(&recent, &current_metrics);

        if let Some(rule) = project_rules_file
            .rules
            .iter_mut()
            .find(|r| r.id == *target_id)
        {
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
    }

    // --- Step 8b: evaluate pending ConfigTuning change ---
    let tuning_history_path = state.history_path.clone();
    let mut tuning_history = io::load_tuning_history(&tuning_history_path);
    let mut pending_revert: Option<(String, String, String)> = None;
    let mut suppress_config_tuning = false;

    if let Some(pending) = tuning_history.pending.take() {
        match &regression {
            Some(regression::RegressionResult::Regressed { .. }) => {
                pending_revert = Some((
                    pending.field.clone(),
                    pending.new_value.clone(),
                    pending.old_value.clone(),
                ));
                tuning_history.cooldowns.push(types::ConfigCooldown {
                    field: pending.field,
                    remaining: 5,
                });
                suppress_config_tuning = true;
                tracing::info!("ConfigTuning: reverted pending change (regression detected)");
            }
            _ => {
                tracing::info!("ConfigTuning: confirmed pending change (no regression)");
            }
        }
    }

    // Decrement cooldowns
    tuning_history.cooldowns.retain_mut(|c| {
        c.remaining = c.remaining.saturating_sub(1);
        c.remaining > 0
    });

    // --- Step 9: extract ConfigTuning findings (max 1 per run) ---
    let cooled_fields: Vec<String> = tuning_history
        .cooldowns
        .iter()
        .map(|c| c.field.clone())
        .collect();
    let config_changes: Vec<(String, String, String)> = if suppress_config_tuning {
        vec![]
    } else {
        findings
            .iter()
            .filter_map(|f| {
                if let FindingAction::ConfigTuning {
                    field,
                    current_value,
                    new_value,
                } = &f.action
                {
                    if cooled_fields.contains(field) {
                        None
                    } else {
                        tuning_history.pending = Some(types::PendingConfigChange {
                            field: field.clone(),
                            old_value: current_value.clone(),
                            new_value: new_value.clone(),
                            finding_id: f.id.clone(),
                            run_id: state.snapshot.run_id.clone(),
                        });
                        Some((field.clone(), current_value.clone(), new_value.clone()))
                    }
                } else {
                    None
                }
            })
            .take(1)
            .collect()
    };

    let mut all_config_changes = config_changes;
    if let Some(revert) = pending_revert {
        all_config_changes.push(revert);
    }

    // --- Step 9b: build LLM analysis prompt if enabled ---
    let llm_prompt = if state.feedback_llm {
        Some(llm::build_analysis_prompt(&data, &findings))
    } else {
        None
    };

    // --- Step 9c: Tier 4 script generation prompt ---
    // Escalation: fire when lower tiers have been tried but problems persist.
    // TODO: Read script_generation from FeedbackConfig/GlassConfig when
    // it becomes available on FeedbackState. For now, default to enabled.
    let script_generation = true;
    let has_tried_lower_tiers = had_rules_before_run;
    let high_waste_or_stuck =
        data.stuck_count > data.iterations / 3 || data.waste_count > data.iterations / 3;
    let script_prompt = if script_generation && high_waste_or_stuck && has_tried_lower_tiers {
        Some(build_script_prompt(&data))
    } else {
        None
    };

    // --- Step 9d: persist tuning history ---
    let _ = io::save_tuning_history(&tuning_history_path, &tuning_history);

    // --- Step 10: persist ---
    let mut current_metrics = current_metrics;
    current_metrics.rule_firings = rule_firings;
    metrics_file.runs.push(current_metrics);
    if let Err(e) = save_metrics_file(&state.metrics_path, &metrics_file) {
        tracing::warn!("Failed to save metrics file {:?}: {e}", state.metrics_path);
    }
    if let Err(e) = save_rules_file(&state.rules_path, &project_rules_file) {
        tracing::warn!("Failed to save rules file {:?}: {e}", state.rules_path);
    }
    if let Err(e) = save_archived_rules(&state.archived_path, &archived_file) {
        tracing::warn!(
            "Failed to save archived rules {:?}: {e}",
            state.archived_path
        );
    }

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

        if let Err(e) = save_rules_file(&state.global_rules_path, &global_file) {
            tracing::warn!(
                "Failed to save global rules {:?}: {e}",
                state.global_rules_path
            );
        }
    }

    // --- Step 10c: persist attribution ---
    let attribution_file = types::AttributionFile {
        scores: attribution_scores,
    };
    if let Err(e) = save_attribution_file(&state.attribution_path, &attribution_file) {
        tracing::warn!(
            "Failed to save attribution file {:?}: {e}",
            state.attribution_path
        );
    }

    FeedbackResult {
        findings,
        regression,
        rules_promoted,
        rules_rejected,
        config_changes: all_config_changes,
        llm_prompt,
        script_prompt,
    }
}

// ---------------------------------------------------------------------------
// check_rules / prompt_hints
// ---------------------------------------------------------------------------

/// Evaluate active rules against the live run state and return any triggered
/// [`RuleAction`]s.  Delegates to the [`rules::RuleEngine`] stored in `state`.
///
/// The ablation target rule (if any) is silently skipped this run.
pub fn check_rules(state: &mut FeedbackState, run_state: &RunState) -> Vec<RuleAction> {
    state
        .engine
        .check_rules(run_state, state.ablation_target.as_deref())
}

/// Return the text of all `prompt_hint` rules that are `Confirmed` or
/// `Provisional`.  Delegates to the [`rules::RuleEngine`] stored in `state`.
pub fn prompt_hints(state: &mut FeedbackState) -> Vec<String> {
    state.engine.prompt_hints_mut()
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
    if let Err(e) = save_rules_file(&rules_path, &rules_file) {
        tracing::warn!("Failed to save rules file {:?}: {e}", rules_path);
    }

    tracing::info!(
        "Feedback LLM: applied {} prompt hint(s) to {}",
        deduped.len(),
        rules_path.display()
    );
}

// ---------------------------------------------------------------------------
// Tier 4: Script generation prompt + response parser
// ---------------------------------------------------------------------------

/// Parsed result of a Tier 4 ephemeral-agent response.
///
/// The LLM has three valid outcomes:
/// 1. It writes a Rhai script (`Script`) to install via the scripting layer.
/// 2. It decides a TOML rule already covers the issue (`TomlSufficient`) —
///    treated as a successful, no-action response.
/// 3. The response can't be interpreted (`Unparseable`) — counts toward the
///    consecutive-failure budget that suppresses Tier 4.
#[derive(Debug, PartialEq, Eq)]
pub enum ScriptResponse {
    Script {
        name: String,
        hooks: String,
        source: String,
    },
    TomlSufficient,
    Unparseable,
}

/// Parse a Tier 4 ephemeral-agent response.
///
/// Looks for `SCRIPT_NAME:` and `SCRIPT_HOOKS:` headers and a fenced
/// ```` ```rhai ```` source block. If the response begins (anywhere on a
/// line) with `TOML_SUFFICIENT`, that is recognized as a deliberate
/// "no script needed" answer and reported as `TomlSufficient` rather than
/// `Unparseable`, so it does not count against the failure budget.
pub fn parse_script_response(text: &str) -> ScriptResponse {
    if text
        .lines()
        .any(|l| l.trim_start().starts_with("TOML_SUFFICIENT"))
    {
        return ScriptResponse::TomlSufficient;
    }

    let name = match text.lines().find(|l| l.starts_with("SCRIPT_NAME:")) {
        Some(l) => l.trim_start_matches("SCRIPT_NAME:").trim().to_string(),
        None => return ScriptResponse::Unparseable,
    };
    let hooks_raw = match text.lines().find(|l| l.starts_with("SCRIPT_HOOKS:")) {
        Some(l) => l.trim_start_matches("SCRIPT_HOOKS:").trim().to_string(),
        None => return ScriptResponse::Unparseable,
    };
    let hooks = hooks_raw
        .split(',')
        .map(|h| format!("\"{}\"", h.trim()))
        .collect::<Vec<_>>()
        .join(", ");
    let source_start = match text.find("```rhai") {
        Some(i) => i + 7,
        None => return ScriptResponse::Unparseable,
    };
    let source_end = match text[source_start..].find("```") {
        Some(i) => source_start + i,
        None => return ScriptResponse::Unparseable,
    };
    let source = text[source_start..source_end].trim().to_string();
    if name.is_empty() || source.is_empty() {
        return ScriptResponse::Unparseable;
    }
    ScriptResponse::Script {
        name,
        hooks,
        source,
    }
}

/// Build a prompt instructing an LLM to produce a Rhai script that addresses
/// the root cause of high waste/stuck rates when Tier 1-3 findings are
/// insufficient.
///
/// The prompt includes run metrics, available hook points, and the GlassApi
/// action methods so the generated script can be directly loaded by the
/// scripting layer.
fn build_script_prompt(run_data: &RunData) -> String {
    let iter_nonzero = run_data.iterations.max(1);
    let stuck_pct = (run_data.stuck_count as f64 / iter_nonzero as f64) * 100.0;
    let waste_pct = (run_data.waste_count as f64 / iter_nonzero as f64) * 100.0;

    format!(
        "[SCRIPT_GENERATION]
The orchestrator run completed with high waste or stuck rates that
existing TOML rules could not explain or fix. Write a Rhai script ONLY
if a TOML rule cannot express the required behavior.

RUN METRICS:
  iterations: {iterations}
  stuck_count: {stuck} ({stuck_pct:.0}%)
  waste_count: {waste} ({waste_pct:.0}%)
  revert_count: {reverts}
  checkpoint_count: {checkpoints}
  duration_secs: {duration}
  completion: {completion}

AVAILABLE HOOK POINTS (attach script to one or more):
  command_start, command_complete, block_state_change,
  snapshot_before, snapshot_after, history_query, history_insert,
  pipeline_complete, config_reload,
  orchestrator_run_start, orchestrator_run_end,
  orchestrator_iteration, orchestrator_checkpoint, orchestrator_stuck,
  mcp_request, mcp_response,
  tab_create, tab_close, session_start, session_end

GLASS API (available as `glass` variable in scripts):
  Read-only:
    glass.cwd()               -> String
    glass.git_branch()         -> String
    glass.git_dirty_files()    -> Array<String>
    glass.config(key)          -> Dynamic
    glass.active_rules()       -> Array<String>
  Actions:
    glass.commit(message)
    glass.log(level, message)
    glass.notify(message)
    glass.set_config(key, value)
    glass.inject_prompt_hint(text)
    glass.force_snapshot(paths)
    glass.trigger_checkpoint(reason)
    glass.extend_silence(extra_secs)

EVENT DATA (available as `event` variable, fields depend on hook):
  command, exit_code, duration_ms, iteration, stuck_count,
  waste_count, checkpoint_reason, file_paths, query

INSTRUCTIONS:
1. Analyze the run metrics above to identify the likely root cause.
2. If a TOML rule (trigger + action pair) can fix it, respond with a single
   line beginning with TOML_SUFFICIENT followed by a description of the
   rule — do not write a script.
3. Otherwise, write a Rhai script using EXACTLY this format (the headers are
   parsed literally; do not rename them):

SCRIPT_NAME: <kebab-case-slug>
SCRIPT_HOOKS: <hook_name>[, <hook_name>...]
```rhai
// Description of what this script does
if <condition> {{
    glass.log(\"info\", \"<explanation>\");
    glass.<action>(<args>);
}}
```

SCRIPT_NAME must be a short kebab-case identifier (e.g. commit-on-stuck).
SCRIPT_HOOKS is a comma-separated list of hook names from the list above.
Respond with at most ONE script. Keep it under 30 lines.",
        iterations = run_data.iterations,
        stuck = run_data.stuck_count,
        stuck_pct = stuck_pct,
        waste = run_data.waste_count,
        waste_pct = waste_pct,
        reverts = run_data.revert_count,
        checkpoints = run_data.checkpoint_count,
        duration = run_data.duration_secs,
        completion = run_data.completion_reason,
    )
}

// ---------------------------------------------------------------------------
// Run summary
// ---------------------------------------------------------------------------

/// Summary data for a single orchestrator run's feedback loop activity.
/// Passed to [`build_run_summary`] to generate the markdown report.
pub struct RunSummaryInput<'a> {
    pub run_id: &'a str,
    pub data: &'a RunData,
    pub result: &'a FeedbackResult,
    pub ablation_target: Option<&'a str>,
    pub active_rules: &'a [types::Rule],
    pub attribution_scores: &'a [types::AttributionScore],
}

/// Build a per-run feedback summary in markdown.
///
/// Covers what the feedback loop did across all four tiers:
/// config tuning, behavioral rules, prompt hints, and script generation.
pub fn build_run_summary(input: &RunSummaryInput<'_>) -> String {
    let d = input.data;
    let r = input.result;
    let iter_nz = d.iterations.max(1);

    let mut out = String::with_capacity(2048);

    // Header
    out.push_str(&format!("# Feedback Loop Summary — {}\n\n", input.run_id));

    // Run overview
    let mins = d.duration_secs / 60;
    let secs = d.duration_secs % 60;
    out.push_str("## Run Overview\n\n");
    out.push_str(&format!(
        "| Metric | Value |\n|--------|-------|\n\
         | Iterations | {} |\n\
         | Duration | {}m {}s |\n\
         | Commits | {} |\n\
         | Stuck events | {} ({:.0}%) |\n\
         | Reverts | {} ({:.0}%) |\n\
         | Checkpoints | {} ({:.0}%) |\n\
         | Completion | {} |\n\n",
        d.iterations,
        mins,
        secs,
        d.commit_count,
        d.stuck_count,
        d.stuck_count as f64 / iter_nz as f64 * 100.0,
        d.revert_count,
        d.revert_count as f64 / (d.revert_count + d.keep_count).max(1) as f64 * 100.0,
        d.checkpoint_count,
        d.checkpoint_count as f64 / iter_nz as f64 * 100.0,
        d.completion_reason,
    ));

    // Tier 1: Config Tuning
    out.push_str("## Tier 1: Config Tuning\n\n");
    if r.config_changes.is_empty() {
        out.push_str("No config changes applied this run.\n\n");
    } else {
        out.push_str("| Field | Old | New |\n|-------|-----|-----|\n");
        for (field, old, new) in &r.config_changes {
            out.push_str(&format!("| {} | {} | {} |\n", field, old, new));
        }
        out.push('\n');
    }

    // Tier 2: Behavioral Rules
    out.push_str("## Tier 2: Behavioral Rules\n\n");

    // New findings
    if r.findings.is_empty() {
        out.push_str("No new findings detected.\n\n");
    } else {
        out.push_str(&format!("**{} new finding(s):**\n\n", r.findings.len()));
        for f in &r.findings {
            out.push_str(&format!(
                "- `{}` ({:?}, {:?}) — {}\n",
                f.id, f.severity, f.category, f.evidence
            ));
        }
        out.push('\n');
    }

    // Promotions / rejections
    if !r.rules_promoted.is_empty() {
        out.push_str(&format!(
            "**Promoted** (provisional → confirmed): {}\n\n",
            r.rules_promoted.join(", ")
        ));
    }
    if !r.rules_rejected.is_empty() {
        out.push_str(&format!(
            "**Rejected** (regression detected): {}\n\n",
            r.rules_rejected.join(", ")
        ));
    }

    // Regression
    match &r.regression {
        Some(regression::RegressionResult::Regressed { reasons }) => {
            out.push_str(&format!(
                "**Regression detected:** {}\n\n",
                reasons.join(", ")
            ));
        }
        Some(regression::RegressionResult::Improved) => {
            out.push_str("**Improved** vs previous run.\n\n");
        }
        _ => {
            out.push_str("Metrics: neutral (no regression or improvement).\n\n");
        }
    }

    // Active rules + firings
    if input.active_rules.is_empty() {
        out.push_str("No active rules.\n\n");
    } else {
        out.push_str("**Active rules this run:**\n\n");
        out.push_str("| Rule | Status | Action | Fired |\n|------|--------|--------|-------|\n");
        for rule in input.active_rules {
            out.push_str(&format!(
                "| {} | {:?} | {} | {}x |\n",
                rule.id, rule.status, rule.action, rule.trigger_count
            ));
        }
        out.push('\n');
    }

    // Ablation
    out.push_str("## Ablation Testing\n\n");
    if let Some(target) = input.ablation_target {
        let result_str = input
            .active_rules
            .iter()
            .find(|r| r.id == target)
            .map(|r| format!("{:?}", r.ablation_result))
            .unwrap_or_else(|| "unknown".to_string());
        out.push_str(&format!(
            "Target: `{}` — Result: **{}**\n\n",
            target, result_str
        ));
    } else {
        out.push_str("No ablation target this run.\n\n");
    }

    // Attribution
    out.push_str("## Rule Attribution\n\n");
    if input.attribution_scores.is_empty() {
        out.push_str("No attribution data yet.\n\n");
    } else {
        out.push_str("| Rule | Passenger Score | Runs Fired | Runs Not Fired |\n|------|----------------|------------|----------------|\n");
        for score in input.attribution_scores {
            out.push_str(&format!(
                "| {} | {:.0}% | {} | {} |\n",
                score.rule_id,
                score.passenger_score * 100.0,
                score.runs_fired,
                score.runs_not_fired,
            ));
        }
        out.push('\n');
    }

    // Tier 3: Prompt Hints
    out.push_str("## Tier 3: Prompt Hints\n\n");
    let hint_count = input
        .active_rules
        .iter()
        .filter(|r| r.action == "prompt_hint")
        .count();
    if hint_count == 0 {
        out.push_str("No prompt hints active.\n\n");
    } else {
        out.push_str(&format!(
            "{} prompt hint(s) injected into agent context.\n\n",
            hint_count
        ));
    }

    // Tier 4: Script Generation
    out.push_str("## Tier 4: Script Generation\n\n");
    if r.script_prompt.is_some() {
        out.push_str("Script generation **triggered** — ephemeral agent spawned.\n\n");
    } else {
        out.push_str("Not triggered (findings sufficient or waste/stuck rates low).\n\n");
    }

    // LLM Analysis
    out.push_str("## LLM Analysis\n\n");
    if r.llm_prompt.is_some() {
        out.push_str(
            "LLM analysis **triggered** — ephemeral agent spawned for qualitative review.\n\n",
        );
    } else {
        out.push_str("Not triggered (feedback_llm disabled).\n\n");
    }

    out
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
        prd_items_completed: data
            .prd_content
            .as_deref()
            .map(|p| {
                p.lines()
                    .filter(|l| l.trim_start().starts_with("- [x]"))
                    .count() as u32
            })
            .unwrap_or(0),
        prd_items_total: prd_total,
        kickoff_duration_secs: data.kickoff_duration_secs,
        rule_firings: vec![],
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
        AblationResult, ConfigSnapshot, Rule, RuleStatus, RulesFile, RulesMeta, RunMetrics, Scope,
        Severity, TuningHistoryFile,
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
            ablation_target: None,
            last_sweep_run: String::new(),
            attribution_scores: vec![],
            attribution_path: glass_dir.join("rule-attribution.toml"),
            ablation_enabled: true,
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
            last_ablation_run: String::new(),
            ablation_result: AblationResult::Untested,
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
            ..Default::default()
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

        let hints = prompt_hints(&mut state);
        assert_eq!(hints.len(), 1);
        assert_eq!(hints[0], "Keep PRs small");
    }

    // -----------------------------------------------------------------------
    // 5. ConfigTuning provisional lifecycle tests
    // -----------------------------------------------------------------------

    #[test]
    fn config_tuning_records_pending() {
        let dir = TempDir::new().unwrap();
        let project_root = dir.path().to_str().unwrap();
        let state = make_state_in_dir(&dir);
        let mut data = make_run_data(project_root);
        data.config_silence_timeout = 30;
        data.avg_idle_between_iterations_secs = 100.0;
        data.iterations = 10;
        let result = on_run_end(state, data);
        assert!(!result.config_changes.is_empty());
        let history =
            io::load_tuning_history(&dir.path().join(".glass").join("tuning-history.toml"));
        assert!(history.pending.is_some());
        assert_eq!(history.pending.unwrap().field, "silence_timeout_secs");
    }

    #[test]
    fn config_tuning_reverts_on_regression() {
        let dir = TempDir::new().unwrap();
        let project_root = dir.path().to_str().unwrap();
        let history_path = dir.path().join(".glass").join("tuning-history.toml");
        std::fs::create_dir_all(dir.path().join(".glass")).unwrap();
        let mut history = TuningHistoryFile::default();
        history.pending = Some(types::PendingConfigChange {
            field: "silence_timeout_secs".to_string(),
            old_value: "30".to_string(),
            new_value: "23".to_string(),
            finding_id: "silence-waste".to_string(),
            run_id: "prev-run".to_string(),
        });
        io::save_tuning_history(&history_path, &history).unwrap();

        // Seed a baseline metric entry so regression::compare has a baseline.
        let metrics_path = dir.path().join(".glass").join("run-metrics.toml");
        let baseline = RunMetrics {
            run_id: "baseline".to_string(),
            project_root: project_root.to_string(),
            iterations: 10,
            duration_secs: 600,
            revert_rate: 0.05,
            stuck_rate: 0.05,
            waste_rate: 0.05,
            checkpoint_rate: 0.20,
            completion: "success".to_string(),
            prd_items_completed: 5,
            prd_items_total: 10,
            kickoff_duration_secs: 60,
            rule_firings: vec![],
        };
        io::save_metrics_file(
            &metrics_path,
            &types::RunMetricsFile {
                runs: vec![baseline],
            },
        )
        .unwrap();

        let state = make_state_in_dir(&dir);
        let mut data = make_run_data(project_root);
        data.iterations = 10;
        data.revert_count = 5; // 50% revert rate -> triggers regression
        data.keep_count = 5;
        let result = on_run_end(state, data);

        let revert = result
            .config_changes
            .iter()
            .find(|(f, _, _)| f == "silence_timeout_secs");
        assert!(revert.is_some());
        let (_, _, new_val) = revert.unwrap();
        assert_eq!(new_val, "30"); // reverted to old_value

        let history = io::load_tuning_history(&history_path);
        assert!(history.pending.is_none());
        // Cooldown was pushed at 5 then decremented to 4 in the same run
        assert!(history
            .cooldowns
            .iter()
            .any(|c| c.field == "silence_timeout_secs" && c.remaining == 4));
    }

    #[test]
    fn config_tuning_confirms_on_improvement() {
        let dir = TempDir::new().unwrap();
        let project_root = dir.path().to_str().unwrap();
        let history_path = dir.path().join(".glass").join("tuning-history.toml");
        std::fs::create_dir_all(dir.path().join(".glass")).unwrap();
        let mut history = TuningHistoryFile::default();
        history.pending = Some(types::PendingConfigChange {
            field: "silence_timeout_secs".to_string(),
            old_value: "30".to_string(),
            new_value: "23".to_string(),
            finding_id: "silence-waste".to_string(),
            run_id: "prev-run".to_string(),
        });
        io::save_tuning_history(&history_path, &history).unwrap();

        let state = make_state_in_dir(&dir);
        // Use data that won't trigger any ConfigTuning detectors
        let mut data = make_run_data(project_root);
        data.stuck_count = 0; // avoid stuck_sensitivity ConfigTuning finding
        let _result = on_run_end(state, data);

        let history = io::load_tuning_history(&history_path);
        assert!(history.pending.is_none());
        assert!(history.cooldowns.is_empty());
    }

    #[test]
    fn config_tuning_skips_field_in_cooldown() {
        let dir = TempDir::new().unwrap();
        let project_root = dir.path().to_str().unwrap();
        let history_path = dir.path().join(".glass").join("tuning-history.toml");
        std::fs::create_dir_all(dir.path().join(".glass")).unwrap();
        let mut history = TuningHistoryFile::default();
        history.cooldowns.push(types::ConfigCooldown {
            field: "silence_timeout_secs".to_string(),
            remaining: 3,
        });
        io::save_tuning_history(&history_path, &history).unwrap();

        let state = make_state_in_dir(&dir);
        let mut data = make_run_data(project_root);
        data.config_silence_timeout = 30;
        data.avg_idle_between_iterations_secs = 100.0;
        data.iterations = 10;
        data.stuck_count = 0; // avoid stuck_sensitivity ConfigTuning finding
        let result = on_run_end(state, data);

        // silence_timeout_secs is in cooldown so config change should be empty
        assert!(result.config_changes.is_empty());

        let history = io::load_tuning_history(&history_path);
        // Cooldown was 3, decremented to 2
        assert_eq!(history.cooldowns[0].remaining, 2);
    }

    #[test]
    fn script_generation_fires_with_rules_and_high_waste() {
        let dir = TempDir::new().unwrap();
        let project_root = dir.path().to_str().unwrap();
        let state = make_state_in_dir(&dir);
        // Write a rule to disk so had_rules_before_run is true at Step 5.
        let rules_file = RulesFile {
            meta: RulesMeta::default(),
            rules: vec![make_rule("r1", "force_commit", RuleStatus::Confirmed)],
        };
        save_rules_file(&state.rules_path, &rules_file).unwrap();
        let mut data = make_run_data(project_root);
        data.iterations = 9;
        data.waste_count = 4; // > 9/3 = 3
        let result = on_run_end(state, data);
        assert!(
            result.script_prompt.is_some(),
            "Tier 4 should fire with active rules + high waste"
        );
    }

    #[test]
    fn script_generation_does_not_fire_without_rules() {
        let dir = TempDir::new().unwrap();
        let project_root = dir.path().to_str().unwrap();
        let state = make_state_in_dir(&dir); // no rules
        let mut data = make_run_data(project_root);
        data.iterations = 9;
        data.waste_count = 4;
        let result = on_run_end(state, data);
        assert!(
            result.script_prompt.is_none(),
            "Tier 4 should not fire without any rules"
        );
    }

    // -----------------------------------------------------------------------
    // Tier 4 prompt/parser format contract
    //
    // Regression guard: these tests link the prompt builder to the parser.
    // A bug existed where the prompt instructed the LLM to emit `HOOK:` and
    // `SCRIPT:`, while the parser looked for `SCRIPT_NAME:` and
    // `SCRIPT_HOOKS:` — making every Tier 4 response silently unparseable.
    // -----------------------------------------------------------------------

    #[test]
    fn tier4_prompt_documents_fields_parser_requires() {
        let data = make_run_data("/tmp/x");
        let prompt = build_script_prompt(&data);
        assert!(
            prompt.contains("SCRIPT_NAME:"),
            "prompt must instruct LLM to emit SCRIPT_NAME:, otherwise parser fails"
        );
        assert!(
            prompt.contains("SCRIPT_HOOKS:"),
            "prompt must instruct LLM to emit SCRIPT_HOOKS:, otherwise parser fails"
        );
    }

    #[test]
    fn tier4_compliant_response_parses_to_script() {
        let response = "\
SCRIPT_NAME: commit-on-stuck
SCRIPT_HOOKS: orchestrator_iteration, command_complete
```rhai
glass.log(\"info\", \"committing because stuck\");
glass.commit(\"checkpoint\");
```
";
        match parse_script_response(response) {
            ScriptResponse::Script {
                name,
                hooks,
                source,
            } => {
                assert_eq!(name, "commit-on-stuck");
                assert!(hooks.contains("orchestrator_iteration"), "hooks: {hooks}");
                assert!(hooks.contains("command_complete"), "hooks: {hooks}");
                assert!(source.contains("glass.commit"), "source: {source}");
            }
            other => panic!("expected Script variant, got {other:?}"),
        }
    }

    #[test]
    fn tier4_toml_sufficient_response_recognized() {
        let response =
            "TOML_SUFFICIENT: a force_commit rule keyed on iterations_since_last_commit > 5";
        assert!(
            matches!(
                parse_script_response(response),
                ScriptResponse::TomlSufficient
            ),
            "TOML_SUFFICIENT must be a recognized response, not Unparseable"
        );
    }

    #[test]
    fn tier4_malformed_response_returns_unparseable() {
        let response = "I think you should write a script that does something cool.";
        assert!(matches!(
            parse_script_response(response),
            ScriptResponse::Unparseable
        ));
    }
}
