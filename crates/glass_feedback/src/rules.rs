//! Rule engine — loads rules from multiple sources, deduplicates by priority,
//! evaluates preconditions against live `RunState`, and returns `RuleAction`s.

use std::path::Path;

use crate::io::load_rules_file;
use crate::types::{Rule, RuleAction, RuleStatus, RunState};

// ---------------------------------------------------------------------------
// RuleEngine
// ---------------------------------------------------------------------------

/// Holds the merged, deduplicated, active rule set for the current run.
pub struct RuleEngine {
    pub rules: Vec<Rule>,
}

impl RuleEngine {
    /// Load rules from up to three sources (project, global, defaults) and
    /// merge them with project-level priority:
    ///
    /// * `project_path` — highest priority (usually `<project>/.glass/rules.toml`)
    /// * `global_path`  — medium priority (usually `~/.glass/rules.toml`)
    /// * `defaults_path` — lowest priority (optional built-in defaults)
    ///
    /// Rules with duplicate `action` values are deduplicated so that only the
    /// highest-priority source's rule is kept.  Rules whose status is
    /// `Rejected`, `Stale`, or `Proposed` are excluded entirely.
    pub fn load(project_path: &Path, global_path: &Path, defaults_path: Option<&Path>) -> Self {
        // Load all three sources (missing files return empty RulesFile).
        let project_rules = load_rules_file(project_path).rules;
        let global_rules = load_rules_file(global_path).rules;
        let default_rules = defaults_path
            .map(|p| load_rules_file(p).rules)
            .unwrap_or_default();

        // Merge: project first (highest priority), then global, then defaults.
        // We track which `action` strings have already been claimed so that
        // lower-priority duplicates are dropped.
        let mut seen_actions: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut merged: Vec<Rule> = Vec::new();

        for rule in project_rules
            .into_iter()
            .chain(global_rules)
            .chain(default_rules)
        {
            if !is_active(&rule.status) {
                continue;
            }
            if seen_actions.insert(rule.action.clone()) {
                merged.push(rule);
            }
        }

        Self { rules: merged }
    }

    /// Evaluate each active rule against `state` and return the list of
    /// `RuleAction`s whose preconditions are satisfied.
    ///
    /// `ablation_target` — when `Some(id)`, that rule is silently skipped so
    /// its absence can be measured without removing it from the rule set.
    pub fn check_rules(
        &mut self,
        state: &RunState,
        ablation_target: Option<&str>,
    ) -> Vec<RuleAction> {
        let mut actions = Vec::new();

        for rule in &mut self.rules {
            if !is_active(&rule.status) {
                continue;
            }

            // Skip the ablation target — it exists but doesn't fire this run.
            if let Some(target) = ablation_target {
                if rule.id == target {
                    continue;
                }
            }

            match rule.action.as_str() {
                "force_commit" if state.iterations_since_last_commit >= 5 => {
                    rule.trigger_count += 1;
                    actions.push(RuleAction::ForceCommit);
                }
                "isolate_commits" => {
                    let file_param = rule
                        .action_params
                        .get("file")
                        .map(String::as_str)
                        .unwrap_or("");
                    if !file_param.is_empty()
                        && state.recent_reverted_files.iter().any(|f| f == file_param)
                    {
                        rule.trigger_count += 1;
                        actions.push(RuleAction::IsolateCommit {
                            file: file_param.to_string(),
                        });
                    }
                }
                "smaller_instructions" => {
                    rule.trigger_count += 1;
                    actions.push(RuleAction::SplitInstructions);
                }
                "extend_silence" => {
                    rule.trigger_count += 1;
                    actions.push(RuleAction::ExtendSilence { extra_secs: 30 });
                }
                "run_verify_twice" if state.verify_alternations >= 2 => {
                    rule.trigger_count += 1;
                    actions.push(RuleAction::RunVerifyTwice);
                }
                "early_stuck" => {
                    rule.trigger_count += 1;
                    actions.push(RuleAction::EarlyStuck { threshold: 2 });
                }
                "restrict_scope" => {
                    rule.trigger_count += 1;
                    actions.push(RuleAction::RevertOutOfScope { files: vec![] });
                }
                "build_dependency_first" => {
                    let message = rule
                        .action_params
                        .get("message")
                        .cloned()
                        .unwrap_or_else(|| rule.trigger.clone());
                    rule.trigger_count += 1;
                    actions.push(RuleAction::BlockUntilResolved { message });
                }
                "verify_progress" if state.waste_rate > 0.15 => {
                    rule.trigger_count += 1;
                    actions.push(RuleAction::TextInjection(
                        "Verify progress before continuing".to_string(),
                    ));
                }
                // Unknown action strings are silently skipped.
                _ => {}
            }
        }

        actions
    }

    /// Returns true if any loaded rule with the given `action_name` is in an
    /// active status (`Confirmed`, `Provisional`, or `Pinned`).
    pub fn is_rule_active(&self, action_name: &str) -> bool {
        self.rules.iter().any(|r| {
            r.action == action_name
                && matches!(
                    r.status,
                    RuleStatus::Confirmed | RuleStatus::Provisional | RuleStatus::Pinned
                )
        })
    }

    /// Return the text of all `prompt_hint` rules that are `Confirmed` or
    /// `Provisional`.  These are Tier 3 hints for system prompt injection.
    pub fn prompt_hints(&self) -> Vec<String> {
        self.rules
            .iter()
            .filter(|r| {
                r.action == "prompt_hint"
                    && matches!(r.status, RuleStatus::Confirmed | RuleStatus::Provisional)
            })
            .filter_map(|r| r.action_params.get("text").cloned())
            .collect()
    }

    /// Return hint texts, cap at 5 most recent, and increment trigger_count.
    pub fn prompt_hints_mut(&mut self) -> Vec<String> {
        let mut hints: Vec<&mut Rule> = self
            .rules
            .iter_mut()
            .filter(|r| r.action == "prompt_hint")
            .filter(|r| matches!(r.status, RuleStatus::Confirmed | RuleStatus::Provisional))
            .collect();
        // Sort by added_run descending (most recent first)
        hints.sort_by(|a, b| b.added_run.cmp(&a.added_run));
        hints.truncate(5);
        hints
            .iter_mut()
            .map(|r| {
                r.trigger_count += 1;
                r.action_params.get("text").cloned().unwrap_or_default()
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn is_active(status: &RuleStatus) -> bool {
    matches!(
        status,
        RuleStatus::Confirmed | RuleStatus::Provisional | RuleStatus::Pinned
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tempfile::TempDir;

    use super::*;
    use crate::io::save_rules_file;
    use crate::types::{AblationResult, RulesFile, Scope, Severity};

    // -----------------------------------------------------------------------
    // Helper: build a minimal Rule with a specific action and status.
    // -----------------------------------------------------------------------

    fn make_test_rule(id: &str, action: &str, status: RuleStatus) -> Rule {
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

    fn save_rules(dir: &TempDir, name: &str, rules: Vec<Rule>) -> std::path::PathBuf {
        let path = dir.path().join(name);
        let file = RulesFile {
            meta: Default::default(),
            rules,
        };
        save_rules_file(&path, &file).unwrap();
        path
    }

    // -----------------------------------------------------------------------
    // 1. load_rules_from_multiple_sources
    // -----------------------------------------------------------------------

    #[test]
    fn load_rules_from_multiple_sources() {
        let dir = TempDir::new().unwrap();

        let project_path = save_rules(
            &dir,
            "project.toml",
            vec![make_test_rule("p1", "force_commit", RuleStatus::Confirmed)],
        );
        let global_path = save_rules(
            &dir,
            "global.toml",
            vec![make_test_rule(
                "g1",
                "extend_silence",
                RuleStatus::Confirmed,
            )],
        );

        let engine = RuleEngine::load(&project_path, &global_path, None);

        assert_eq!(engine.rules.len(), 2);
        let actions: Vec<&str> = engine.rules.iter().map(|r| r.action.as_str()).collect();
        assert!(actions.contains(&"force_commit"));
        assert!(actions.contains(&"extend_silence"));
    }

    // -----------------------------------------------------------------------
    // 2. project_rule_overrides_global
    // -----------------------------------------------------------------------

    #[test]
    fn project_rule_overrides_global() {
        let dir = TempDir::new().unwrap();

        // Both have the same action "force_commit"; project should win.
        let mut project_rule = make_test_rule("p1", "force_commit", RuleStatus::Confirmed);
        project_rule.added_metric = "project_metric".to_string();

        let mut global_rule = make_test_rule("g1", "force_commit", RuleStatus::Confirmed);
        global_rule.added_metric = "global_metric".to_string();

        let project_path = save_rules(&dir, "project.toml", vec![project_rule]);
        let global_path = save_rules(&dir, "global.toml", vec![global_rule]);

        let engine = RuleEngine::load(&project_path, &global_path, None);

        // Only one rule for the "force_commit" action.
        assert_eq!(engine.rules.len(), 1);
        assert_eq!(engine.rules[0].id, "p1");
        assert_eq!(engine.rules[0].added_metric, "project_metric");
    }

    // -----------------------------------------------------------------------
    // 3. check_rules_returns_text_actions — force_commit fires at >= 5
    // -----------------------------------------------------------------------

    #[test]
    fn check_rules_returns_text_actions() {
        let mut engine = RuleEngine {
            rules: vec![make_test_rule("r1", "force_commit", RuleStatus::Confirmed)],
        };

        let state = RunState {
            iterations_since_last_commit: 5,
            ..Default::default()
        };

        let result = engine.check_rules(&state, None);
        assert_eq!(result.len(), 1);
        assert!(
            matches!(result[0], RuleAction::ForceCommit),
            "expected ForceCommit variant"
        );
    }

    // -----------------------------------------------------------------------
    // 4. check_rules_returns_rust_level_actions — extend_silence
    // -----------------------------------------------------------------------

    #[test]
    fn check_rules_returns_rust_level_actions() {
        let mut engine = RuleEngine {
            rules: vec![make_test_rule(
                "r1",
                "extend_silence",
                RuleStatus::Confirmed,
            )],
        };

        let state = RunState::default();
        let result = engine.check_rules(&state, None);

        assert_eq!(result.len(), 1);
        if let RuleAction::ExtendSilence { extra_secs } = result[0] {
            assert_eq!(extra_secs, 30);
        } else {
            panic!("expected ExtendSilence");
        }
    }

    // -----------------------------------------------------------------------
    // 5. only_confirmed_provisional_pinned_rules_fire
    // -----------------------------------------------------------------------

    #[test]
    fn only_confirmed_provisional_pinned_rules_fire() {
        let mut engine = RuleEngine {
            rules: vec![
                make_test_rule("r1", "extend_silence", RuleStatus::Confirmed),
                make_test_rule("r2", "smaller_instructions", RuleStatus::Provisional),
                make_test_rule("r3", "restrict_scope", RuleStatus::Pinned),
                make_test_rule("r4", "force_commit", RuleStatus::Rejected),
                make_test_rule("r5", "early_stuck", RuleStatus::Stale),
                make_test_rule("r6", "verify_progress", RuleStatus::Proposed),
            ],
        };

        let state = RunState {
            iterations_since_last_commit: 10,
            waste_rate: 0.5,
            ..Default::default()
        };

        let result = engine.check_rules(&state, None);

        // Only rules r1 (extend_silence), r2 (smaller_instructions), r3 (restrict_scope)
        // should fire. Rejected, Stale, and Proposed are skipped.
        assert_eq!(result.len(), 3);
    }

    // -----------------------------------------------------------------------
    // 6. check_rules_force_commit_no_trigger — iterations_since_last_commit < 5
    // -----------------------------------------------------------------------

    #[test]
    fn check_rules_force_commit_no_trigger() {
        let mut engine = RuleEngine {
            rules: vec![make_test_rule("r1", "force_commit", RuleStatus::Confirmed)],
        };

        let state = RunState {
            iterations_since_last_commit: 4,
            ..Default::default()
        };

        let result = engine.check_rules(&state, None);
        assert!(result.is_empty());
    }

    // -----------------------------------------------------------------------
    // 7. check_rules_verify_progress_fires — waste_rate > 0.15
    // -----------------------------------------------------------------------

    #[test]
    fn check_rules_verify_progress_fires() {
        let mut engine = RuleEngine {
            rules: vec![make_test_rule(
                "r1",
                "verify_progress",
                RuleStatus::Confirmed,
            )],
        };

        let below = RunState {
            waste_rate: 0.10,
            ..Default::default()
        };
        assert!(engine.check_rules(&below, None).is_empty());

        let above = RunState {
            waste_rate: 0.20,
            ..Default::default()
        };
        let result = engine.check_rules(&above, None);
        assert_eq!(result.len(), 1);
        if let RuleAction::TextInjection(text) = &result[0] {
            assert!(text.contains("Verify progress"));
        } else {
            panic!("expected TextInjection");
        }
    }

    // -----------------------------------------------------------------------
    // 8. prompt_hints_returns_confirmed_hints
    // -----------------------------------------------------------------------

    #[test]
    fn prompt_hints_returns_confirmed_hints() {
        let mut hint_rule = make_test_rule("h1", "prompt_hint", RuleStatus::Confirmed);
        hint_rule
            .action_params
            .insert("text".to_string(), "Keep responses concise".to_string());

        let mut prov_hint = make_test_rule("h2", "prompt_hint", RuleStatus::Provisional);
        prov_hint
            .action_params
            .insert("text".to_string(), "Prefer small commits".to_string());

        let mut rejected_hint = make_test_rule("h3", "prompt_hint", RuleStatus::Rejected);
        rejected_hint
            .action_params
            .insert("text".to_string(), "Should not appear".to_string());

        let engine = RuleEngine {
            rules: vec![hint_rule, prov_hint, rejected_hint],
        };

        let hints = engine.prompt_hints();
        assert_eq!(hints.len(), 2);
        assert!(hints.contains(&"Keep responses concise".to_string()));
        assert!(hints.contains(&"Prefer small commits".to_string()));
        assert!(!hints.contains(&"Should not appear".to_string()));
    }

    // -----------------------------------------------------------------------
    // Extra: isolate_commits fires only when file is in reverted list
    // -----------------------------------------------------------------------

    #[test]
    fn check_rules_isolate_commits_fires_on_match() {
        let mut rule = make_test_rule("r1", "isolate_commits", RuleStatus::Confirmed);
        rule.action_params
            .insert("file".to_string(), "src/lib.rs".to_string());

        let mut engine = RuleEngine { rules: vec![rule] };

        // File not in reverted list — should not fire.
        let state_no_match = RunState {
            recent_reverted_files: vec!["src/main.rs".to_string()],
            ..Default::default()
        };
        assert!(engine.check_rules(&state_no_match, None).is_empty());

        // File present — should fire.
        let state_match = RunState {
            recent_reverted_files: vec!["src/lib.rs".to_string()],
            ..Default::default()
        };
        let result = engine.check_rules(&state_match, None);
        assert_eq!(result.len(), 1);
        if let RuleAction::IsolateCommit { file } = &result[0] {
            assert_eq!(file, "src/lib.rs");
        } else {
            panic!("expected IsolateCommit");
        }
    }

    // -----------------------------------------------------------------------
    // Extra: run_verify_twice fires only when alternations >= 2
    // -----------------------------------------------------------------------

    #[test]
    fn check_rules_run_verify_twice_threshold() {
        let mut engine = RuleEngine {
            rules: vec![make_test_rule(
                "r1",
                "run_verify_twice",
                RuleStatus::Confirmed,
            )],
        };

        let below = RunState {
            verify_alternations: 1,
            ..Default::default()
        };
        assert!(engine.check_rules(&below, None).is_empty());

        let at = RunState {
            verify_alternations: 2,
            ..Default::default()
        };
        let result = engine.check_rules(&at, None);
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0], RuleAction::RunVerifyTwice));
    }

    // -----------------------------------------------------------------------
    // Extra: defaults_path is used as lowest priority source
    // -----------------------------------------------------------------------

    #[test]
    fn load_uses_defaults_path() {
        let dir = TempDir::new().unwrap();

        // Empty project and global files.
        let project_path = save_rules(&dir, "project.toml", vec![]);
        let global_path = save_rules(&dir, "global.toml", vec![]);
        let defaults_path = save_rules(
            &dir,
            "defaults.toml",
            vec![make_test_rule("d1", "early_stuck", RuleStatus::Pinned)],
        );

        let engine = RuleEngine::load(&project_path, &global_path, Some(&defaults_path));

        assert_eq!(engine.rules.len(), 1);
        assert_eq!(engine.rules[0].id, "d1");
    }

    // -----------------------------------------------------------------------
    // New typed-variant tests
    // -----------------------------------------------------------------------

    #[test]
    fn check_rules_force_commit_returns_variant() {
        let mut engine = RuleEngine {
            rules: vec![make_test_rule("r1", "force_commit", RuleStatus::Confirmed)],
        };

        let state = RunState {
            iterations_since_last_commit: 6,
            ..Default::default()
        };

        let result = engine.check_rules(&state, None);
        assert_eq!(result.len(), 1);
        assert!(
            matches!(result[0], RuleAction::ForceCommit),
            "expected ForceCommit variant"
        );
    }

    #[test]
    fn check_rules_isolate_commit_returns_variant() {
        let mut rule = make_test_rule("r1", "isolate_commits", RuleStatus::Confirmed);
        rule.action_params
            .insert("file".to_string(), "src/hot.rs".to_string());

        let mut engine = RuleEngine { rules: vec![rule] };

        let state = RunState {
            recent_reverted_files: vec!["src/hot.rs".to_string()],
            ..Default::default()
        };

        let result = engine.check_rules(&state, None);
        assert_eq!(result.len(), 1);
        if let RuleAction::IsolateCommit { file } = &result[0] {
            assert_eq!(file, "src/hot.rs");
        } else {
            panic!("expected IsolateCommit variant");
        }
    }

    #[test]
    fn check_rules_smaller_instructions_returns_split() {
        let mut engine = RuleEngine {
            rules: vec![make_test_rule(
                "r1",
                "smaller_instructions",
                RuleStatus::Confirmed,
            )],
        };

        let state = RunState::default();
        let result = engine.check_rules(&state, None);
        assert_eq!(result.len(), 1);
        assert!(
            matches!(result[0], RuleAction::SplitInstructions),
            "expected SplitInstructions variant"
        );
    }

    #[test]
    fn check_rules_restrict_scope_returns_revert() {
        let mut engine = RuleEngine {
            rules: vec![make_test_rule(
                "r1",
                "restrict_scope",
                RuleStatus::Confirmed,
            )],
        };

        let state = RunState::default();
        let result = engine.check_rules(&state, None);
        assert_eq!(result.len(), 1);
        if let RuleAction::RevertOutOfScope { files } = &result[0] {
            // Signal variant — files vec is empty (caller computes actual files).
            assert!(files.is_empty());
        } else {
            panic!("expected RevertOutOfScope variant");
        }
    }

    #[test]
    fn check_rules_build_dependency_returns_block() {
        let mut rule = make_test_rule("r1", "build_dependency_first", RuleStatus::Confirmed);
        rule.action_params.insert(
            "message".to_string(),
            "Fix the linker error first".to_string(),
        );

        let mut engine = RuleEngine { rules: vec![rule] };

        let state = RunState::default();
        let result = engine.check_rules(&state, None);
        assert_eq!(result.len(), 1);
        if let RuleAction::BlockUntilResolved { message } = &result[0] {
            assert_eq!(message, "Fix the linker error first");
        } else {
            panic!("expected BlockUntilResolved variant");
        }
    }

    #[test]
    fn check_rules_verify_progress_still_text() {
        let mut engine = RuleEngine {
            rules: vec![make_test_rule(
                "r1",
                "verify_progress",
                RuleStatus::Confirmed,
            )],
        };

        let state = RunState {
            waste_rate: 0.20,
            ..Default::default()
        };

        let result = engine.check_rules(&state, None);
        assert_eq!(result.len(), 1);
        if let RuleAction::TextInjection(text) = &result[0] {
            assert!(text.contains("Verify progress"));
        } else {
            panic!("expected TextInjection for verify_progress");
        }
    }

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

    #[test]
    fn is_rule_active_true() {
        let engine = RuleEngine {
            rules: vec![make_test_rule("r1", "force_commit", RuleStatus::Confirmed)],
        };
        assert!(engine.is_rule_active("force_commit"));
    }

    #[test]
    fn is_rule_active_false_rejected() {
        let engine = RuleEngine {
            rules: vec![make_test_rule("r1", "force_commit", RuleStatus::Rejected)],
        };
        assert!(!engine.is_rule_active("force_commit"));
    }

    // -----------------------------------------------------------------------
    // prompt_hints_mut tests
    // -----------------------------------------------------------------------

    #[test]
    fn prompt_hints_mut_caps_at_5() {
        let mut rules = Vec::new();
        for i in 0..8 {
            let mut r = make_test_rule(&format!("hint-{i}"), "prompt_hint", RuleStatus::Confirmed);
            r.action_params
                .insert("text".to_string(), format!("Hint {i}"));
            r.added_run = format!("run-{i:03}");
            rules.push(r);
        }
        let mut engine = RuleEngine { rules };
        let hints = engine.prompt_hints_mut();
        assert_eq!(hints.len(), 5, "should cap at 5 hints");
    }

    #[test]
    fn prompt_hints_mut_increments_trigger_count() {
        let mut r = make_test_rule("hint-1", "prompt_hint", RuleStatus::Confirmed);
        r.action_params
            .insert("text".to_string(), "Keep PRs small".to_string());
        r.trigger_count = 0;
        let mut engine = RuleEngine { rules: vec![r] };
        let _ = engine.prompt_hints_mut();
        assert_eq!(engine.rules[0].trigger_count, 1);
    }
}
