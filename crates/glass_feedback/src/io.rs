//! TOML I/O helpers for glass_feedback persistence files.
//!
//! All load functions return a default value on missing or corrupted files.
//! Corrupted files are backed up with a `.bak` extension before returning the
//! default, so no data is silently discarded.

use std::fs;
use std::path::Path;

use anyhow::Context as _;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::types::{AttributionFile, RulesFile, RunMetricsFile, TuningHistoryFile};

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Load a TOML file, returning `T::default()` on any error.
///
/// If the file exists but cannot be parsed, a warning is logged and the file
/// is renamed to `<path>.bak` so that the corruption is preserved for manual
/// inspection.
fn load_toml_or_default<T: DeserializeOwned + Default>(path: &Path) -> T {
    match fs::read_to_string(path) {
        Err(_) => {
            // File missing or unreadable — silently return default.
            T::default()
        }
        Ok(text) => match toml::from_str::<T>(&text) {
            Ok(value) => value,
            Err(err) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %err,
                    "glass_feedback: failed to parse TOML — returning default and backing up file"
                );
                // Rename to .bak so the user can inspect the corruption.
                let bak = path.with_extension("bak");
                if let Err(rename_err) = fs::rename(path, &bak) {
                    tracing::warn!(
                        error = %rename_err,
                        "glass_feedback: could not rename corrupted file to .bak"
                    );
                }
                T::default()
            }
        },
    }
}

/// Serialize `data` as TOML and write it to `path`, creating parent directories
/// as needed.
fn save_toml<T: Serialize>(path: &Path, data: &T) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create_dir_all for {}", parent.display()))?;
    }
    let text = toml::to_string_pretty(data)
        .with_context(|| format!("serialize TOML for {}", path.display()))?;
    fs::write(path, text).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Rules file
// ---------------------------------------------------------------------------

/// Load the rules file at `path`.  Returns an empty [`RulesFile`] if the file
/// is missing or corrupted.
pub fn load_rules_file(path: &Path) -> RulesFile {
    load_toml_or_default(path)
}

/// Persist a [`RulesFile`] to `path`.
pub fn save_rules_file(path: &Path, file: &RulesFile) -> anyhow::Result<()> {
    save_toml(path, file)
}

// ---------------------------------------------------------------------------
// Metrics file
// ---------------------------------------------------------------------------

/// Load the run-metrics file at `path`.  Returns an empty [`RunMetricsFile`]
/// if the file is missing or corrupted.
pub fn load_metrics_file(path: &Path) -> RunMetricsFile {
    load_toml_or_default(path)
}

/// Persist a [`RunMetricsFile`] to `path`, keeping only the last 20 entries.
pub fn save_metrics_file(path: &Path, file: &RunMetricsFile) -> anyhow::Result<()> {
    const MAX_ENTRIES: usize = 20;
    let mut pruned = file.clone();
    if pruned.runs.len() > MAX_ENTRIES {
        let drain_count = pruned.runs.len() - MAX_ENTRIES;
        pruned.runs.drain(..drain_count);
    }
    save_toml(path, &pruned)
}

// ---------------------------------------------------------------------------
// Tuning history file
// ---------------------------------------------------------------------------

/// Load the tuning-history file at `path`.  Returns an empty
/// [`TuningHistoryFile`] if the file is missing or corrupted.
pub fn load_tuning_history(path: &Path) -> TuningHistoryFile {
    load_toml_or_default(path)
}

/// Persist a [`TuningHistoryFile`] to `path`.
pub fn save_tuning_history(path: &Path, file: &TuningHistoryFile) -> anyhow::Result<()> {
    save_toml(path, file)
}

// ---------------------------------------------------------------------------
// Archived rules file
// ---------------------------------------------------------------------------

/// Load the archived-rules file at `path`.  Returns an empty [`RulesFile`] if
/// the file is missing or corrupted.
pub fn load_archived_rules(path: &Path) -> RulesFile {
    load_toml_or_default(path)
}

/// Persist an archived [`RulesFile`] to `path`.
pub fn save_archived_rules(path: &Path, file: &RulesFile) -> anyhow::Result<()> {
    save_toml(path, file)
}

// ---------------------------------------------------------------------------
// Attribution file
// ---------------------------------------------------------------------------

/// Load the rule-attribution file at `path`. Returns an empty
/// [`AttributionFile`] if the file is missing or corrupted.
pub fn load_attribution_file(path: &Path) -> AttributionFile {
    load_toml_or_default(path)
}

/// Persist an [`AttributionFile`] to `path`.
pub fn save_attribution_file(path: &Path, file: &AttributionFile) -> anyhow::Result<()> {
    save_toml(path, file)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use tempfile::TempDir;

    use super::*;
    use crate::types::{
        AblationResult, AttributionFile, AttributionScore, ConfigSnapshot, MetricDeltas, Rule,
        RuleStatus, RulesMeta, RunMetrics, Scope, Severity,
    };

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn make_rule(id: &str) -> Rule {
        Rule {
            id: id.to_string(),
            trigger: "stuck_rate_high".to_string(),
            trigger_params: HashMap::new(),
            action: "extend_silence".to_string(),
            action_params: HashMap::new(),
            status: RuleStatus::Proposed,
            severity: Severity::Medium,
            scope: Scope::Project,
            tags: vec!["test".to_string()],
            added_run: "run-001".to_string(),
            added_metric: "stuck_rate=0.25".to_string(),
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

    fn make_run_metrics(run_id: &str) -> RunMetrics {
        RunMetrics {
            run_id: run_id.to_string(),
            project_root: "/tmp/glass".to_string(),
            iterations: 10,
            duration_secs: 600,
            revert_rate: 0.10,
            stuck_rate: 0.05,
            waste_rate: 0.08,
            checkpoint_rate: 0.20,
            completion: "success".to_string(),
            prd_items_completed: 5,
            prd_items_total: 8,
            kickoff_duration_secs: 60,
            rule_firings: vec![],
        }
    }

    // -----------------------------------------------------------------------
    // Rules tests
    // -----------------------------------------------------------------------

    #[test]
    fn load_rules_empty_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("rules.toml");
        // Write an empty file.
        fs::write(&path, "").unwrap();
        let loaded = load_rules_file(&path);
        assert!(loaded.rules.is_empty());
    }

    #[test]
    fn load_rules_missing_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("rules.toml");
        // File does not exist.
        let loaded = load_rules_file(&path);
        assert!(loaded.rules.is_empty());
    }

    #[test]
    fn save_and_load_rules_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("rules.toml");

        let rule = make_rule("rule-001");
        let file = RulesFile {
            meta: RulesMeta {
                version: "1".to_string(),
                description: "test rules".to_string(),
            },
            rules: vec![rule],
        };

        save_rules_file(&path, &file).unwrap();
        let loaded = load_rules_file(&path);

        assert_eq!(loaded.rules.len(), 1);
        let r = &loaded.rules[0];
        assert_eq!(r.id, "rule-001");
        assert_eq!(r.trigger, "stuck_rate_high");
        assert_eq!(r.action, "extend_silence");
        assert!(matches!(r.status, RuleStatus::Proposed));
        assert!(matches!(r.severity, Severity::Medium));
        assert!(matches!(r.scope, Scope::Project));
        assert_eq!(r.tags, vec!["test"]);
        assert_eq!(r.added_run, "run-001");
    }

    #[test]
    fn load_corrupted_toml_recovers() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("rules.toml");
        // Write invalid TOML.
        fs::write(&path, "[[rules\nthis is not valid toml!!!").unwrap();

        let loaded = load_rules_file(&path);
        assert!(loaded.rules.is_empty(), "should return empty on corruption");

        // Original file must have been renamed to .bak.
        let bak = path.with_extension("bak");
        assert!(
            bak.exists(),
            ".bak file must exist after corruption recovery"
        );
        // The original path should no longer exist.
        assert!(
            !path.exists(),
            "corrupted file should have been moved to .bak"
        );
    }

    // -----------------------------------------------------------------------
    // Metrics tests
    // -----------------------------------------------------------------------

    #[test]
    fn save_and_load_metrics_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("metrics.toml");

        let file = RunMetricsFile {
            runs: vec![make_run_metrics("run-001")],
        };
        save_metrics_file(&path, &file).unwrap();
        let loaded = load_metrics_file(&path);

        assert_eq!(loaded.runs.len(), 1);
        let m = &loaded.runs[0];
        assert_eq!(m.run_id, "run-001");
        assert_eq!(m.iterations, 10);
        assert_eq!(m.duration_secs, 600);
        assert!((m.revert_rate - 0.10).abs() < f64::EPSILON);
    }

    #[test]
    fn metrics_pruned_to_20() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("metrics.toml");

        // Create 25 entries: run-0 through run-24.
        let runs: Vec<RunMetrics> = (0..25)
            .map(|i| make_run_metrics(&format!("run-{i}")))
            .collect();
        let file = RunMetricsFile { runs };

        save_metrics_file(&path, &file).unwrap();
        let loaded = load_metrics_file(&path);

        assert_eq!(loaded.runs.len(), 20, "should prune to last 20");
        // First kept entry should be run-5 (index 5 of 0..25).
        assert_eq!(loaded.runs[0].run_id, "run-5");
        // Last entry should be run-24.
        assert_eq!(loaded.runs[19].run_id, "run-24");
    }

    // -----------------------------------------------------------------------
    // Tuning history tests
    // -----------------------------------------------------------------------

    #[test]
    fn save_and_load_tuning_history() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("tuning.toml");

        let mut config_values = HashMap::new();
        config_values.insert("silence_timeout".to_string(), "45".to_string());
        config_values.insert("max_retries".to_string(), "5".to_string());

        let snapshot = ConfigSnapshot {
            run_id: "run-001".to_string(),
            config_values,
            provisional_rules: vec!["rule-001".to_string()],
        };
        let file = TuningHistoryFile {
            snapshots: vec![snapshot],
            ..Default::default()
        };

        save_tuning_history(&path, &file).unwrap();
        let loaded = load_tuning_history(&path);

        assert_eq!(loaded.snapshots.len(), 1);
        let s = &loaded.snapshots[0];
        assert_eq!(s.run_id, "run-001");
        assert_eq!(
            s.config_values.get("silence_timeout").map(String::as_str),
            Some("45")
        );
        assert_eq!(
            s.config_values.get("max_retries").map(String::as_str),
            Some("5")
        );
        assert_eq!(s.provisional_rules, vec!["rule-001"]);
    }

    // -----------------------------------------------------------------------
    // Attribution tests
    // -----------------------------------------------------------------------

    #[test]
    fn save_and_load_attribution_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("attribution.toml");

        let score = AttributionScore {
            rule_id: "force-commit".to_string(),
            runs_fired: 5,
            runs_not_fired: 3,
            avg_delta_when_fired: MetricDeltas {
                revert_rate: -0.04,
                stuck_rate: -0.01,
                waste_rate: -0.02,
            },
            avg_delta_when_not_fired: MetricDeltas::default(),
            passenger_score: 0.25,
            last_updated_run: "run-100".to_string(),
        };
        let file = AttributionFile {
            scores: vec![score],
        };
        save_attribution_file(&path, &file).unwrap();
        let loaded = load_attribution_file(&path);

        assert_eq!(loaded.scores.len(), 1);
        assert_eq!(loaded.scores[0].rule_id, "force-commit");
        assert_eq!(loaded.scores[0].runs_fired, 5);
        assert!((loaded.scores[0].passenger_score - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn load_attribution_missing_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("attribution.toml");
        let loaded = load_attribution_file(&path);
        assert!(loaded.scores.is_empty());
    }
}
