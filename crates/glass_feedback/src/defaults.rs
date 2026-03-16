//! Default rules shipped with glass_feedback.
//!
//! These six battle-tested rules seed new projects and the global defaults file
//! when no prior rules exist.  All defaults start as `Provisional` with `Global`
//! scope so users can promote, reject, or override them per-project.

use std::collections::HashMap;
use std::path::Path;

use crate::io::{load_rules_file, save_rules_file};
use crate::types::{Rule, RulesFile, RuleStatus, Scope, Severity};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// The semantic version of the built-in default rule set.
/// Bump this whenever the set of default rule IDs changes.
pub const DEFAULT_RULES_VERSION: &str = "1.0.0";

// ---------------------------------------------------------------------------
// default_rules
// ---------------------------------------------------------------------------

/// Return the six built-in default rules.
///
/// All rules are `Provisional` + `Global` with empty param maps and tags.
pub fn default_rules() -> Vec<Rule> {
    vec![
        Rule {
            id: "default-uncommitted-drift".to_string(),
            trigger: "uncommitted_iterations >= 5".to_string(),
            trigger_params: HashMap::new(),
            action: "force_commit".to_string(),
            action_params: HashMap::new(),
            status: RuleStatus::Provisional,
            severity: Severity::Medium,
            scope: Scope::Global,
            tags: vec![],
            added_run: String::new(),
            added_metric: String::new(),
            confirmed_run: String::new(),
            rejected_run: String::new(),
            rejected_reason: String::new(),
            last_triggered_run: String::new(),
            trigger_count: 0,
            cooldown_remaining: 0,
            stale_runs: 0,
        },
        Rule {
            id: "default-hot-file".to_string(),
            trigger: "same_file_reverted >= 3".to_string(),
            trigger_params: HashMap::new(),
            action: "isolate_commits".to_string(),
            action_params: HashMap::new(),
            status: RuleStatus::Provisional,
            severity: Severity::High,
            scope: Scope::Global,
            tags: vec![],
            added_run: String::new(),
            added_metric: String::new(),
            confirmed_run: String::new(),
            rejected_run: String::new(),
            rejected_reason: String::new(),
            last_triggered_run: String::new(),
            trigger_count: 0,
            cooldown_remaining: 0,
            stale_runs: 0,
        },
        Rule {
            id: "default-instruction-overload".to_string(),
            trigger: "instruction_count >= 4 && partial_completion".to_string(),
            trigger_params: HashMap::new(),
            action: "smaller_instructions".to_string(),
            action_params: HashMap::new(),
            status: RuleStatus::Provisional,
            severity: Severity::Medium,
            scope: Scope::Global,
            tags: vec![],
            added_run: String::new(),
            added_metric: String::new(),
            confirmed_run: String::new(),
            rejected_run: String::new(),
            rejected_reason: String::new(),
            last_triggered_run: String::new(),
            trigger_count: 0,
            cooldown_remaining: 0,
            stale_runs: 0,
        },
        Rule {
            id: "default-flaky-verify".to_string(),
            trigger: "verify_alternates_pass_fail >= 2".to_string(),
            trigger_params: HashMap::new(),
            action: "run_verify_twice".to_string(),
            action_params: HashMap::new(),
            status: RuleStatus::Provisional,
            severity: Severity::High,
            scope: Scope::Global,
            tags: vec![],
            added_run: String::new(),
            added_metric: String::new(),
            confirmed_run: String::new(),
            rejected_run: String::new(),
            rejected_reason: String::new(),
            last_triggered_run: String::new(),
            trigger_count: 0,
            cooldown_remaining: 0,
            stale_runs: 0,
        },
        Rule {
            id: "default-revert-rate".to_string(),
            trigger: "revert_rate > 0.3".to_string(),
            trigger_params: HashMap::new(),
            action: "smaller_instructions".to_string(),
            action_params: HashMap::new(),
            status: RuleStatus::Provisional,
            severity: Severity::High,
            scope: Scope::Global,
            tags: vec![],
            added_run: String::new(),
            added_metric: String::new(),
            confirmed_run: String::new(),
            rejected_run: String::new(),
            rejected_reason: String::new(),
            last_triggered_run: String::new(),
            trigger_count: 0,
            cooldown_remaining: 0,
            stale_runs: 0,
        },
        Rule {
            id: "default-waste-rate".to_string(),
            trigger: "waste_rate > 0.15".to_string(),
            trigger_params: HashMap::new(),
            action: "verify_progress".to_string(),
            action_params: HashMap::new(),
            status: RuleStatus::Provisional,
            severity: Severity::Medium,
            scope: Scope::Global,
            tags: vec![],
            added_run: String::new(),
            added_metric: String::new(),
            confirmed_run: String::new(),
            rejected_run: String::new(),
            rejected_reason: String::new(),
            last_triggered_run: String::new(),
            trigger_count: 0,
            cooldown_remaining: 0,
            stale_runs: 0,
        },
    ]
}

// ---------------------------------------------------------------------------
// merge_defaults_into_project
// ---------------------------------------------------------------------------

/// Merge `defaults` into `project_rules` if the version hasn't been applied yet.
///
/// Rules that already exist in `project_rules` (matched by `id`, regardless of
/// status) are skipped.  After merging, `project_rules.meta.version` is set to
/// `default_version` so subsequent calls with the same version are no-ops.
pub fn merge_defaults_into_project(
    project_rules: &mut RulesFile,
    defaults: &[Rule],
    default_version: &str,
) {
    // Already at this version — nothing to do.
    if project_rules.meta.version == default_version {
        return;
    }

    for default_rule in defaults {
        let already_present = project_rules
            .rules
            .iter()
            .any(|r| r.id == default_rule.id);

        if !already_present {
            project_rules.rules.push(default_rule.clone());
        }
    }

    project_rules.meta.version = default_version.to_string();
}

// ---------------------------------------------------------------------------
// ensure_global_defaults
// ---------------------------------------------------------------------------

/// Ensure the global defaults file at `global_defaults_path` is up to date.
///
/// * If the file does not exist, create it with all six default rules and
///   version `DEFAULT_RULES_VERSION`.
/// * If the file exists with an older version, add any new default rules that
///   are not already present and bump the version.
/// * If the file is already at `DEFAULT_RULES_VERSION`, this is a no-op.
pub fn ensure_global_defaults(global_defaults_path: &Path) {
    let mut file = load_rules_file(global_defaults_path);

    if file.meta.version == DEFAULT_RULES_VERSION {
        // Already up to date — nothing to write.
        return;
    }

    let defaults = default_rules();
    merge_defaults_into_project(&mut file, &defaults, DEFAULT_RULES_VERSION);

    // Ensure description is set on fresh files.
    if file.meta.description.is_empty() {
        file.meta.description =
            "Glass built-in default rules (auto-generated)".to_string();
    }

    if let Err(err) = save_rules_file(global_defaults_path, &file) {
        tracing::warn!(
            path = %global_defaults_path.display(),
            error = %err,
            "glass_feedback: could not write global defaults file"
        );
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::io::load_rules_file;
    use crate::types::RulesMeta;

    // -----------------------------------------------------------------------
    // 1. default_rules_count
    // -----------------------------------------------------------------------

    #[test]
    fn default_rules_count() {
        assert_eq!(default_rules().len(), 6);
    }

    // -----------------------------------------------------------------------
    // 2. default_rules_all_provisional
    // -----------------------------------------------------------------------

    #[test]
    fn default_rules_all_provisional() {
        for rule in default_rules() {
            assert!(
                matches!(rule.status, RuleStatus::Provisional),
                "rule '{}' is not Provisional",
                rule.id
            );
        }
    }

    // -----------------------------------------------------------------------
    // 3. default_rules_all_global
    // -----------------------------------------------------------------------

    #[test]
    fn default_rules_all_global() {
        for rule in default_rules() {
            assert!(
                matches!(rule.scope, Scope::Global),
                "rule '{}' does not have Global scope",
                rule.id
            );
        }
    }

    // -----------------------------------------------------------------------
    // 4. default_rules_unique_ids
    // -----------------------------------------------------------------------

    #[test]
    fn default_rules_unique_ids() {
        let rules = default_rules();
        let mut seen = std::collections::HashSet::new();
        for rule in &rules {
            assert!(
                seen.insert(rule.id.clone()),
                "duplicate rule id: '{}'",
                rule.id
            );
        }
    }

    // -----------------------------------------------------------------------
    // 5. merge_adds_to_empty_project
    // -----------------------------------------------------------------------

    #[test]
    fn merge_adds_to_empty_project() {
        let mut project = RulesFile::default();
        let defaults = default_rules();

        merge_defaults_into_project(&mut project, &defaults, DEFAULT_RULES_VERSION);

        assert_eq!(project.rules.len(), 6, "all six defaults should be added");
        assert_eq!(project.meta.version, DEFAULT_RULES_VERSION);
    }

    // -----------------------------------------------------------------------
    // 6. merge_skips_existing
    // -----------------------------------------------------------------------

    #[test]
    fn merge_skips_existing() {
        // Pre-populate with one of the default rule IDs (any status).
        let existing = Rule {
            id: "default-hot-file".to_string(),
            trigger: "custom-trigger".to_string(),
            trigger_params: std::collections::HashMap::new(),
            action: "isolate_commits".to_string(),
            action_params: std::collections::HashMap::new(),
            status: RuleStatus::Confirmed,
            severity: Severity::High,
            scope: Scope::Global,
            tags: vec![],
            added_run: String::new(),
            added_metric: String::new(),
            confirmed_run: String::new(),
            rejected_run: String::new(),
            rejected_reason: String::new(),
            last_triggered_run: String::new(),
            trigger_count: 0,
            cooldown_remaining: 0,
            stale_runs: 0,
        };

        let mut project = RulesFile {
            meta: RulesMeta::default(),
            rules: vec![existing],
        };

        let defaults = default_rules();
        merge_defaults_into_project(&mut project, &defaults, DEFAULT_RULES_VERSION);

        // Should have 1 existing + 5 new = 6, not 7.
        assert_eq!(
            project.rules.len(),
            6,
            "existing rule should not be duplicated; len={}",
            project.rules.len()
        );

        // The existing rule's trigger must remain unchanged (not overwritten).
        let hot_file = project
            .rules
            .iter()
            .find(|r| r.id == "default-hot-file")
            .expect("default-hot-file must exist");
        assert_eq!(hot_file.trigger, "custom-trigger");
    }

    // -----------------------------------------------------------------------
    // 7. merge_skips_when_version_matches
    // -----------------------------------------------------------------------

    #[test]
    fn merge_skips_when_version_matches() {
        let mut project = RulesFile {
            meta: RulesMeta {
                version: DEFAULT_RULES_VERSION.to_string(),
                description: String::new(),
            },
            rules: vec![],
        };

        let defaults = default_rules();
        merge_defaults_into_project(&mut project, &defaults, DEFAULT_RULES_VERSION);

        // Version already matches — no rules should be added.
        assert!(
            project.rules.is_empty(),
            "no rules should be added when version already matches"
        );
    }

    // -----------------------------------------------------------------------
    // 8. ensure_global_defaults_creates_file
    // -----------------------------------------------------------------------

    #[test]
    fn ensure_global_defaults_creates_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("global-rules.toml");

        assert!(!path.exists(), "file must not exist before the call");

        ensure_global_defaults(&path);

        assert!(path.exists(), "file must be created by ensure_global_defaults");

        let loaded = load_rules_file(&path);
        assert_eq!(
            loaded.rules.len(),
            6,
            "newly created file must contain all 6 defaults"
        );
        assert_eq!(loaded.meta.version, DEFAULT_RULES_VERSION);
    }

    // -----------------------------------------------------------------------
    // 9. ensure_global_defaults_no_overwrite
    // -----------------------------------------------------------------------

    #[test]
    fn ensure_global_defaults_no_overwrite() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("global-rules.toml");

        // First call — creates the file.
        ensure_global_defaults(&path);

        let after_first = load_rules_file(&path);
        assert_eq!(after_first.rules.len(), 6);

        // Mutate the on-disk file: change one trigger.
        let mut mutated = after_first.clone();
        mutated.rules[0].trigger = "mutated-trigger".to_string();
        crate::io::save_rules_file(&path, &mutated).unwrap();

        // Second call — version already matches, must be a no-op.
        ensure_global_defaults(&path);

        let after_second = load_rules_file(&path);
        assert_eq!(
            after_second.rules[0].trigger, "mutated-trigger",
            "existing file must not be overwritten when version matches"
        );
    }
}
