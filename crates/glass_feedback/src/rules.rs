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
    pub fn check_rules(&self, state: &RunState) -> Vec<RuleAction> {
        let mut actions = Vec::new();

        for rule in &self.rules {
            if !is_active(&rule.status) {
                continue;
            }

            match rule.action.as_str() {
                "force_commit" => {
                    if state.uncommitted_iterations >= 5 {
                        actions.push(RuleAction::TextInjection(
                            "Commit your changes now — too many uncommitted iterations.".to_string(),
                        ));
                    }
                }
                "isolate_commits" => {
                    let file_param = rule.action_params.get("file").map(String::as_str).unwrap_or("");
                    if !file_param.is_empty()
                        && state.recent_reverted_files.iter().any(|f| f == file_param)
                    {
                        actions.push(RuleAction::TextInjection(format!(
                            "Isolate commits for file: {file_param}"
                        )));
                    }
                }
                "smaller_instructions" => {
                    actions.push(RuleAction::TextInjection(
                        "Give ONE instruction per response".to_string(),
                    ));
                }
                "extend_silence" => {
                    actions.push(RuleAction::ExtendSilence { extra_secs: 30 });
                }
                "run_verify_twice" => {
                    if state.verify_alternations >= 2 {
                        actions.push(RuleAction::RunVerifyTwice);
                    }
                }
                "early_stuck" => {
                    actions.push(RuleAction::EarlyStuck { threshold: 2 });
                }
                "restrict_scope" => {
                    actions.push(RuleAction::TextInjection(
                        "Only modify files related to current PRD item".to_string(),
                    ));
                }
                "build_dependency_first" => {
                    let extra = if rule.action_params.is_empty() {
                        String::new()
                    } else {
                        let parts: Vec<String> = rule
                            .action_params
                            .iter()
                            .map(|(k, v)| format!("{k}={v}"))
                            .collect();
                        format!(": {}", parts.join(", "))
                    };
                    actions.push(RuleAction::TextInjection(format!(
                        "Build dependency first{extra}"
                    )));
                }
                "verify_progress" => {
                    if state.waste_rate > 0.15 {
                        actions.push(RuleAction::TextInjection(
                            "Verify progress before continuing".to_string(),
                        ));
                    }
                }
                // Unknown action strings are silently skipped.
                _ => {}
            }
        }

        actions
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
    use crate::types::{RulesFile, Scope, Severity};

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
            vec![make_test_rule("g1", "extend_silence", RuleStatus::Confirmed)],
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
        let engine = RuleEngine {
            rules: vec![make_test_rule("r1", "force_commit", RuleStatus::Confirmed)],
        };

        let state = RunState {
            uncommitted_iterations: 5,
            ..Default::default()
        };

        let result = engine.check_rules(&state);
        assert_eq!(result.len(), 1);
        if let RuleAction::TextInjection(text) = &result[0] {
            assert!(text.contains("Commit"));
        } else {
            panic!("expected TextInjection");
        }
    }

    // -----------------------------------------------------------------------
    // 4. check_rules_returns_rust_level_actions — extend_silence
    // -----------------------------------------------------------------------

    #[test]
    fn check_rules_returns_rust_level_actions() {
        let engine = RuleEngine {
            rules: vec![make_test_rule("r1", "extend_silence", RuleStatus::Confirmed)],
        };

        let state = RunState::default();
        let result = engine.check_rules(&state);

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
        let engine = RuleEngine {
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
            uncommitted_iterations: 10,
            waste_rate: 0.5,
            ..Default::default()
        };

        let result = engine.check_rules(&state);

        // Only rules r1 (extend_silence), r2 (smaller_instructions), r3 (restrict_scope)
        // should fire. Rejected, Stale, and Proposed are skipped.
        assert_eq!(result.len(), 3);
    }

    // -----------------------------------------------------------------------
    // 6. check_rules_force_commit_no_trigger — uncommitted < 5
    // -----------------------------------------------------------------------

    #[test]
    fn check_rules_force_commit_no_trigger() {
        let engine = RuleEngine {
            rules: vec![make_test_rule("r1", "force_commit", RuleStatus::Confirmed)],
        };

        let state = RunState {
            uncommitted_iterations: 4,
            ..Default::default()
        };

        let result = engine.check_rules(&state);
        assert!(result.is_empty());
    }

    // -----------------------------------------------------------------------
    // 7. check_rules_verify_progress_fires — waste_rate > 0.15
    // -----------------------------------------------------------------------

    #[test]
    fn check_rules_verify_progress_fires() {
        let engine = RuleEngine {
            rules: vec![make_test_rule("r1", "verify_progress", RuleStatus::Confirmed)],
        };

        let below = RunState {
            waste_rate: 0.10,
            ..Default::default()
        };
        assert!(engine.check_rules(&below).is_empty());

        let above = RunState {
            waste_rate: 0.20,
            ..Default::default()
        };
        let result = engine.check_rules(&above);
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

        let engine = RuleEngine { rules: vec![rule] };

        // File not in reverted list — should not fire.
        let state_no_match = RunState {
            recent_reverted_files: vec!["src/main.rs".to_string()],
            ..Default::default()
        };
        assert!(engine.check_rules(&state_no_match).is_empty());

        // File present — should fire.
        let state_match = RunState {
            recent_reverted_files: vec!["src/lib.rs".to_string()],
            ..Default::default()
        };
        let result = engine.check_rules(&state_match);
        assert_eq!(result.len(), 1);
        if let RuleAction::TextInjection(text) = &result[0] {
            assert!(text.contains("src/lib.rs"));
        } else {
            panic!("expected TextInjection");
        }
    }

    // -----------------------------------------------------------------------
    // Extra: run_verify_twice fires only when alternations >= 2
    // -----------------------------------------------------------------------

    #[test]
    fn check_rules_run_verify_twice_threshold() {
        let engine = RuleEngine {
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
        assert!(engine.check_rules(&below).is_empty());

        let at = RunState {
            verify_alternations: 2,
            ..Default::default()
        };
        let result = engine.check_rules(&at);
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
}
