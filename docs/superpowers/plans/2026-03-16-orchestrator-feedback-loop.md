# Orchestrator Feedback Loop Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a self-improving feedback loop for the Glass orchestrator that analyzes runs, applies guarded config/rule changes, and auto-rolls back regressions.

**Architecture:** New `glass_feedback` crate with four subsystems — rule-based analyzer (15 detectors), rule engine (load/match/inject), regression guard (snapshot/compare/rollback), and lifecycle manager (promote/reject/stale/archive). Integrates via four entry points in `main.rs` and `orchestrator.rs`. LLM analyzer is opt-in.

**Tech Stack:** Rust 2021, serde + toml/toml_edit for TOML I/O, existing `glass_core::config::update_config_field()` for config writes, existing `OrchestratorEventBuffer` for event data.

**Spec:** `docs/superpowers/specs/2026-03-16-orchestrator-feedback-loop-design.md`

**Key dependencies:** `glass_feedback` depends on `glass_core` (for `update_config_field`). It does NOT depend on `glass_terminal` or `glass_renderer`. The main binary depends on `glass_feedback` for integration.

**RunData field sourcing:** Several `RunData` fields require new tracking in `OrchestratorState` (added in Task 13). Specifically: `fast_trigger_during_output`, `avg_idle_between_iterations_secs`, `silence_interruptions`, `waste_count` (iterations with no git diff), `commit_count`, and `reverted_files`. Fields extractable from existing data: `revert_count`/`keep_count` from `MetricBaseline`, `stuck_count` from iterations.tsv, `agent_responses` from `OrchestratorEventBuffer` `AgentText` events, `verify_pass_fail_sequence` from `VerifyResult` events, `fingerprint_sequence` from `OrchestratorState.recent_fingerprints` (hashed to u64).

---

## Chunk 1: Crate Scaffolding, Types & Data I/O

### Task 1: Create `glass_feedback` crate skeleton

**Files:**
- Create: `crates/glass_feedback/Cargo.toml`
- Create: `crates/glass_feedback/src/lib.rs`
- Modify: `Cargo.toml` (root workspace)

- [ ] **Step 1: Create crate directory**

```bash
mkdir -p crates/glass_feedback/src
```

- [ ] **Step 2: Write Cargo.toml**

```toml
[package]
name = "glass_feedback"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { workspace = true }
toml = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
glass_core = { path = "../glass_core" }
```

- [ ] **Step 3: Write lib.rs with module declarations and public types**

```rust
//! glass_feedback — Self-improving orchestrator feedback loop.
//!
//! Analyzes orchestrator runs, produces findings across three tiers
//! (config tuning, behavioral rules, prompt hints), applies changes
//! through a guarded lifecycle, and auto-rolls back regressions.

pub mod types;
pub mod io;
pub mod analyzer;
pub mod rules;
pub mod regression;
pub mod lifecycle;

pub use types::*;
```

- [ ] **Step 4: Add glass_feedback to root Cargo.toml dependencies**

In root `Cargo.toml`, add to `[dependencies]` section:
```toml
glass_feedback = { path = "crates/glass_feedback" }
```

- [ ] **Step 5: Verify it compiles**

```bash
cargo build -p glass_feedback
```
Expected: compiles with empty modules warning

- [ ] **Step 6: Commit**

```bash
git add crates/glass_feedback/ Cargo.toml
git commit -m "feat(feedback): scaffold glass_feedback crate"
```

---

### Task 2: Define core types

**Files:**
- Create: `crates/glass_feedback/src/types.rs`

- [ ] **Step 1: Write test for type construction and serialization**

At the bottom of `types.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finding_action_config_tuning() {
        let action = FindingAction::ConfigTuning {
            field: "silence_timeout_secs".to_string(),
            current_value: "30".to_string(),
            new_value: "45".to_string(),
        };
        assert!(matches!(action, FindingAction::ConfigTuning { .. }));
    }

    #[test]
    fn finding_action_behavioral_rule() {
        let mut params = std::collections::HashMap::new();
        params.insert("file".to_string(), "src/main.rs".to_string());
        let action = FindingAction::BehavioralRule {
            action: "isolate_commits".to_string(),
            params,
        };
        assert!(matches!(action, FindingAction::BehavioralRule { .. }));
    }

    #[test]
    fn finding_action_prompt_hint() {
        let action = FindingAction::PromptHint {
            text: "Run migrations before tests".to_string(),
        };
        assert!(matches!(action, FindingAction::PromptHint { .. }));
    }

    #[test]
    fn run_metrics_rates() {
        let m = RunMetrics {
            run_id: "2026-03-16T14:30:00".to_string(),
            project_root: "/tmp/project".to_string(),
            iterations: 20,
            duration_secs: 600,
            revert_rate: 0.15,
            stuck_rate: 0.05,
            waste_rate: 0.10,
            checkpoint_rate: 0.10,
            completion: "complete".to_string(),
            prd_items_completed: 5,
            prd_items_total: 5,
            kickoff_duration_secs: 120,
        };
        assert!(m.revert_rate < 0.2);
    }

    #[test]
    fn rule_status_transitions() {
        assert_eq!(RuleStatus::Proposed.can_transition_to(&RuleStatus::Provisional), true);
        assert_eq!(RuleStatus::Provisional.can_transition_to(&RuleStatus::Confirmed), true);
        assert_eq!(RuleStatus::Provisional.can_transition_to(&RuleStatus::Rejected), true);
        assert_eq!(RuleStatus::Confirmed.can_transition_to(&RuleStatus::Stale), true);
        assert_eq!(RuleStatus::Stale.can_transition_to(&RuleStatus::Confirmed), true);
        assert_eq!(RuleStatus::Rejected.can_transition_to(&RuleStatus::Proposed), true);
        // Invalid transitions
        assert_eq!(RuleStatus::Proposed.can_transition_to(&RuleStatus::Confirmed), false);
        assert_eq!(RuleStatus::Confirmed.can_transition_to(&RuleStatus::Proposed), false);
    }

    #[test]
    fn rule_action_text_injection() {
        let action = RuleAction::TextInjection("Commit src/main.rs separately".to_string());
        assert!(matches!(action, RuleAction::TextInjection(_)));
    }

    #[test]
    fn rule_action_rust_level() {
        let action = RuleAction::ExtendSilence { extra_secs: 30 };
        assert!(matches!(action, RuleAction::ExtendSilence { .. }));
    }

    #[test]
    fn run_data_default() {
        let data = RunData::default();
        assert_eq!(data.iterations, 0);
        assert_eq!(data.completion_reason, "");
    }

    #[test]
    fn feedback_config_defaults() {
        let config = FeedbackConfig::default();
        assert!(!config.feedback_llm);
        assert_eq!(config.max_prompt_hints, 10);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p glass_feedback
```
Expected: FAIL — types not defined yet

- [ ] **Step 3: Write all type definitions**

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Severity of a finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    High,
    Medium,
    Low,
}

/// Scope of a finding or rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Project,
    Global,
}

/// Category of a finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingCategory {
    ConfigTuning,
    BehavioralRule,
    PromptHint,
}

/// Typed action per tier.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum FindingAction {
    ConfigTuning {
        field: String,
        current_value: String,
        new_value: String,
    },
    BehavioralRule {
        action: String,
        params: HashMap<String, String>,
    },
    PromptHint {
        text: String,
    },
}

/// A finding produced by the analyzer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub category: FindingCategory,
    pub severity: Severity,
    pub action: FindingAction,
    pub evidence: String,
    pub scope: Scope,
}

/// Rule lifecycle status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleStatus {
    Proposed,
    Provisional,
    Confirmed,
    Rejected,
    Pinned,
    Stale,
}

impl RuleStatus {
    /// Check if a transition from self to target is valid.
    pub fn can_transition_to(&self, target: &RuleStatus) -> bool {
        matches!(
            (self, target),
            (RuleStatus::Proposed, RuleStatus::Provisional)
                | (RuleStatus::Provisional, RuleStatus::Confirmed)
                | (RuleStatus::Provisional, RuleStatus::Rejected)
                | (RuleStatus::Confirmed, RuleStatus::Stale)
                | (RuleStatus::Confirmed, RuleStatus::Provisional) // env drift demotion
                | (RuleStatus::Stale, RuleStatus::Confirmed) // re-triggered
                | (RuleStatus::Rejected, RuleStatus::Proposed) // cooldown re-eligible
        )
    }
}

/// A behavioral rule with lifecycle state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub trigger: String,
    pub trigger_params: HashMap<String, String>,
    pub action: String,
    pub action_params: HashMap<String, String>,
    pub status: RuleStatus,
    pub severity: Severity,
    pub scope: Scope,
    pub tags: Vec<String>,
    pub added_run: String,
    pub added_metric: String,
    #[serde(default)]
    pub confirmed_run: String,
    #[serde(default)]
    pub rejected_run: String,
    #[serde(default)]
    pub rejected_reason: String,
    #[serde(default)]
    pub last_triggered_run: String,
    #[serde(default)]
    pub trigger_count: u32,
    #[serde(default)]
    pub cooldown_remaining: u32,
    #[serde(default)]
    pub stale_runs: u32,
}

/// Rules file with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RulesFile {
    #[serde(default)]
    pub meta: RulesMeta,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

/// Metadata for a rules file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RulesMeta {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
}

/// Actions returned by the rule engine at runtime.
#[derive(Debug, Clone)]
pub enum RuleAction {
    /// Text to append to the [FEEDBACK_RULES] section of context.
    TextInjection(String),
    /// Extend silence threshold by N seconds for this iteration.
    ExtendSilence { extra_secs: u64 },
    /// Run verification twice before reverting.
    RunVerifyTwice,
    /// Lower stuck threshold for this run.
    EarlyStuck { threshold: u32 },
}

/// Metrics captured for each orchestrator run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMetrics {
    pub run_id: String,
    pub project_root: String,
    pub iterations: u32,
    pub duration_secs: u64,
    pub revert_rate: f64,
    pub stuck_rate: f64,
    pub waste_rate: f64,
    pub checkpoint_rate: f64,
    pub completion: String,
    pub prd_items_completed: u32,
    pub prd_items_total: u32,
    pub kickoff_duration_secs: u64,
}

/// Run metrics history file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunMetricsFile {
    #[serde(default)]
    pub runs: Vec<RunMetrics>,
}

/// Config snapshot for regression guard rollback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSnapshot {
    pub run_id: String,
    pub config_values: HashMap<String, String>,
    pub provisional_rules: Vec<String>,
}

/// Tuning history file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TuningHistoryFile {
    #[serde(default)]
    pub snapshots: Vec<ConfigSnapshot>,
}

/// Current run state passed to rule engine for runtime matching.
#[derive(Debug, Clone, Default)]
pub struct RunState {
    pub iteration: u32,
    pub uncommitted_iterations: u32,
    pub revert_rate: f64,
    pub stuck_rate: f64,
    pub waste_rate: f64,
    pub recent_reverted_files: Vec<String>,
    pub verify_alternations: u32,
}

/// Configuration for the feedback loop (subset of orchestrator config).
#[derive(Debug, Clone)]
pub struct FeedbackConfig {
    pub project_root: String,
    pub feedback_llm: bool,
    pub max_prompt_hints: usize,
}

impl Default for FeedbackConfig {
    fn default() -> Self {
        Self {
            project_root: String::new(),
            feedback_llm: false,
            max_prompt_hints: 10,
        }
    }
}

/// Input data for the analyzer, built from OrchestratorState + event buffer.
#[derive(Debug, Clone, Default)]
pub struct RunData {
    pub project_root: String,
    pub iterations: u32,
    pub duration_secs: u64,
    pub kickoff_duration_secs: u64,
    pub iterations_tsv: String,
    pub revert_count: u32,
    pub keep_count: u32,
    pub stuck_count: u32,
    pub checkpoint_count: u32,
    pub waste_count: u32,
    pub commit_count: u32,
    pub completion_reason: String,
    pub prd_content: Option<String>,
    pub git_log: Option<String>,
    pub git_diff_stat: Option<String>,
    pub reverted_files: Vec<String>,
    pub verify_pass_fail_sequence: Vec<bool>,
    pub agent_responses: Vec<String>,
    pub silence_interruptions: u32,
    pub fast_trigger_during_output: u32,
    pub avg_idle_between_iterations_secs: f64,
    pub fingerprint_sequence: Vec<u64>,
    pub config_silence_timeout: u64,
    pub config_max_retries: u32,
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p glass_feedback
```
Expected: all tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/glass_feedback/src/types.rs
git commit -m "feat(feedback): define core types — findings, rules, metrics, run data"
```

---

### Task 3: TOML I/O for rules and metrics files

**Files:**
- Create: `crates/glass_feedback/src/io.rs`

- [ ] **Step 1: Write tests for TOML round-tripping and error recovery**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn load_rules_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.toml");
        std::fs::write(&path, "").unwrap();
        let file = load_rules_file(&path);
        assert!(file.rules.is_empty());
    }

    #[test]
    fn load_rules_missing_file() {
        let path = std::path::Path::new("/nonexistent/rules.toml");
        let file = load_rules_file(path);
        assert!(file.rules.is_empty());
    }

    #[test]
    fn save_and_load_rules_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.toml");
        let mut file = RulesFile { meta: RulesMeta::default(), rules: vec![] };
        file.rules.push(Rule {
            id: "test-rule".to_string(),
            trigger: "revert_rate > 0.3".to_string(),
            trigger_params: Default::default(),
            action: "smaller_instructions".to_string(),
            action_params: Default::default(),
            status: RuleStatus::Provisional,
            severity: Severity::High,
            scope: Scope::Project,
            tags: vec!["rust".to_string()],
            added_run: "2026-03-16".to_string(),
            added_metric: "revert rate 0.35".to_string(),
            confirmed_run: String::new(),
            rejected_run: String::new(),
            rejected_reason: String::new(),
            last_triggered_run: String::new(),
            trigger_count: 0,
            cooldown_remaining: 0,
            stale_runs: 0,
        });
        save_rules_file(&path, &file).unwrap();
        let loaded = load_rules_file(&path);
        assert_eq!(loaded.rules.len(), 1);
        assert_eq!(loaded.rules[0].id, "test-rule");
        assert_eq!(loaded.rules[0].status, RuleStatus::Provisional);
    }

    #[test]
    fn load_corrupted_toml_recovers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.toml");
        std::fs::write(&path, "this is [[[ not valid toml").unwrap();
        let file = load_rules_file(&path);
        assert!(file.rules.is_empty());
        // Verify backup was created
        assert!(dir.path().join("rules.toml.bak").exists());
    }

    #[test]
    fn save_and_load_metrics_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run-metrics.toml");
        let mut file = RunMetricsFile::default();
        file.runs.push(RunMetrics {
            run_id: "run-1".to_string(),
            project_root: "/tmp".to_string(),
            iterations: 10,
            duration_secs: 300,
            revert_rate: 0.1,
            stuck_rate: 0.0,
            waste_rate: 0.05,
            checkpoint_rate: 0.1,
            completion: "complete".to_string(),
            prd_items_completed: 3,
            prd_items_total: 3,
            kickoff_duration_secs: 60,
        });
        save_metrics_file(&path, &file).unwrap();
        let loaded = load_metrics_file(&path);
        assert_eq!(loaded.runs.len(), 1);
        assert_eq!(loaded.runs[0].run_id, "run-1");
    }

    #[test]
    fn metrics_pruned_to_20() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run-metrics.toml");
        let mut file = RunMetricsFile::default();
        for i in 0..25 {
            file.runs.push(RunMetrics {
                run_id: format!("run-{i}"),
                project_root: "/tmp".to_string(),
                iterations: 10,
                duration_secs: 300,
                revert_rate: 0.0,
                stuck_rate: 0.0,
                waste_rate: 0.0,
                checkpoint_rate: 0.0,
                completion: "complete".to_string(),
                prd_items_completed: 0,
                prd_items_total: 0,
                kickoff_duration_secs: 0,
            });
        }
        save_metrics_file(&path, &file).unwrap();
        let loaded = load_metrics_file(&path);
        assert_eq!(loaded.runs.len(), 20);
        // Oldest pruned, newest kept
        assert_eq!(loaded.runs[0].run_id, "run-5");
        assert_eq!(loaded.runs[19].run_id, "run-24");
    }

    #[test]
    fn save_and_load_tuning_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tuning-history.toml");
        let mut file = TuningHistoryFile::default();
        let mut config = std::collections::HashMap::new();
        config.insert("silence_timeout_secs".to_string(), "30".to_string());
        file.snapshots.push(ConfigSnapshot {
            run_id: "run-1".to_string(),
            config_values: config,
            provisional_rules: vec!["rule-001".to_string()],
        });
        save_tuning_history(&path, &file).unwrap();
        let loaded = load_tuning_history(&path);
        assert_eq!(loaded.snapshots.len(), 1);
        assert_eq!(loaded.snapshots[0].config_values["silence_timeout_secs"], "30");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p glass_feedback
```
Expected: FAIL — io functions not defined

- [ ] **Step 3: Add tempfile dev-dependency**

In `crates/glass_feedback/Cargo.toml`:
```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 4: Write I/O implementation**

```rust
use crate::types::*;
use std::path::Path;

const MAX_METRIC_RUNS: usize = 20;

/// Load a rules file, returning empty on missing/corrupted.
pub fn load_rules_file(path: &Path) -> RulesFile {
    load_toml_or_default(path)
}

/// Save a rules file.
pub fn save_rules_file(path: &Path, file: &RulesFile) -> anyhow::Result<()> {
    save_toml(path, file)
}

/// Load run metrics, returning empty on missing/corrupted.
pub fn load_metrics_file(path: &Path) -> RunMetricsFile {
    load_toml_or_default(path)
}

/// Save run metrics, pruning to last MAX_METRIC_RUNS entries.
pub fn save_metrics_file(path: &Path, file: &RunMetricsFile) -> anyhow::Result<()> {
    let mut pruned = file.clone();
    if pruned.runs.len() > MAX_METRIC_RUNS {
        pruned.runs = pruned.runs.split_off(pruned.runs.len() - MAX_METRIC_RUNS);
    }
    save_toml(path, &pruned)
}

/// Load tuning history.
pub fn load_tuning_history(path: &Path) -> TuningHistoryFile {
    load_toml_or_default(path)
}

/// Save tuning history.
pub fn save_tuning_history(path: &Path, file: &TuningHistoryFile) -> anyhow::Result<()> {
    save_toml(path, file)
}

/// Load archived rules.
pub fn load_archived_rules(path: &Path) -> RulesFile {
    load_toml_or_default(path)
}

/// Save archived rules.
pub fn save_archived_rules(path: &Path, file: &RulesFile) -> anyhow::Result<()> {
    save_toml(path, file)
}

/// Generic TOML loader with corruption recovery.
fn load_toml_or_default<T: serde::de::DeserializeOwned + Default>(path: &Path) -> T {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return T::default(),
    };
    if content.trim().is_empty() {
        return T::default();
    }
    match toml::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("Feedback: corrupted TOML at {}: {e}", path.display());
            // Back up corrupted file
            let bak = path.with_extension("toml.bak");
            let _ = std::fs::copy(path, &bak);
            T::default()
        }
    }
}

/// Generic TOML saver. Creates parent directories if needed.
fn save_toml<T: serde::Serialize>(path: &Path, data: &T) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(data)?;
    std::fs::write(path, content)?;
    Ok(())
}
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cargo test -p glass_feedback
```
Expected: all tests PASS

- [ ] **Step 6: Run clippy**

```bash
cargo clippy -p glass_feedback -- -D warnings
```
Expected: no warnings

- [ ] **Step 7: Commit**

```bash
git add crates/glass_feedback/src/io.rs crates/glass_feedback/Cargo.toml
git commit -m "feat(feedback): TOML I/O with corruption recovery and metrics pruning"
```

---

## Chunk 2: Rule-Based Analyzer (15 Detectors)

### Task 4: Analyzer scaffolding and first 5 detectors

**Files:**
- Create: `crates/glass_feedback/src/analyzer.rs`

- [ ] **Step 1: Write tests for the first 5 detectors**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_silence_mismatch_triggers() {
        let data = RunData {
            fast_trigger_during_output: 3,
            config_silence_timeout: 30,
            iterations: 10,
            ..Default::default()
        };
        let findings = detect_silence_mismatch(&data);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].action, FindingAction::ConfigTuning { .. }));
        if let FindingAction::ConfigTuning { ref new_value, .. } = findings[0].action {
            assert_eq!(new_value, "45"); // 30 * 1.5
        }
    }

    #[test]
    fn detect_silence_mismatch_no_trigger() {
        let data = RunData {
            fast_trigger_during_output: 1, // below threshold of 2
            ..Default::default()
        };
        let findings = detect_silence_mismatch(&data);
        assert!(findings.is_empty());
    }

    #[test]
    fn detect_silence_waste_triggers() {
        let data = RunData {
            avg_idle_between_iterations_secs: 70.0,
            config_silence_timeout: 30,
            iterations: 10,
            ..Default::default()
        };
        let findings = detect_silence_waste(&data);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn detect_stuck_sensitivity_triggers() {
        let data = RunData {
            stuck_count: 2,
            iterations: 20,
            waste_count: 2, // low waste = progress after stuck
            config_max_retries: 3,
            ..Default::default()
        };
        let findings = detect_stuck_sensitivity(&data);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn detect_stuck_leniency_triggers() {
        // 6 consecutive identical fingerprints
        let data = RunData {
            fingerprint_sequence: vec![42, 42, 42, 42, 42, 42],
            config_max_retries: 3,
            iterations: 10,
            ..Default::default()
        };
        let findings = detect_stuck_leniency(&data);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn detect_checkpoint_overhead_triggers() {
        let data = RunData {
            checkpoint_count: 4,
            iterations: 12,
            ..Default::default()
        };
        let findings = detect_checkpoint_overhead(&data);
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn detect_checkpoint_overhead_no_trigger() {
        let data = RunData {
            checkpoint_count: 1,
            iterations: 20,
            ..Default::default()
        };
        let findings = detect_checkpoint_overhead(&data);
        assert!(findings.is_empty());
    }

    #[test]
    fn analyze_runs_all_detectors() {
        let data = RunData {
            iterations: 20,
            revert_count: 8,
            waste_count: 5,
            config_silence_timeout: 30,
            config_max_retries: 3,
            ..Default::default()
        };
        let findings = analyze(&data);
        // Should produce at least revert_rate and waste_rate findings
        assert!(findings.len() >= 2);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p glass_feedback
```
Expected: FAIL

- [ ] **Step 3: Implement analyzer with first 5 detectors**

Write the `analyze()` entry point and detectors 1-5:
- `detect_silence_mismatch` — fast trigger during output >= 2 → increase timeout by 50%
- `detect_silence_waste` — avg idle > 2x threshold → decrease by 25%
- `detect_stuck_sensitivity` — stuck triggers with low waste after → increase max_retries
- `detect_stuck_leniency` — 5+ consecutive identical fingerprints → decrease max_retries
- `detect_checkpoint_overhead` — 3+ checkpoints for <15 iterations → raise interval

Each is a `fn detect_xxx(data: &RunData) -> Vec<Finding>` pure function.

The `analyze()` function calls all detectors and collects findings:

```rust
pub fn analyze(data: &RunData) -> Vec<Finding> {
    let mut findings = Vec::new();
    findings.extend(detect_silence_mismatch(data));
    findings.extend(detect_silence_waste(data));
    findings.extend(detect_stuck_sensitivity(data));
    findings.extend(detect_stuck_leniency(data));
    findings.extend(detect_checkpoint_overhead(data));
    // ... more detectors added in Task 5
    findings
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p glass_feedback
```
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/glass_feedback/src/analyzer.rs
git commit -m "feat(feedback): analyzer with first 5 detectors — silence, stuck, checkpoint"
```

---

### Task 5: Remaining 10 detectors

**Files:**
- Modify: `crates/glass_feedback/src/analyzer.rs`

- [ ] **Step 1: Write tests for detectors 6-15**

Add tests for:
- `detect_checkpoint_frequency` — efficiency drops in later iterations
- `detect_hot_files` — same file in 3+ reverts
- `detect_uncommitted_drift` — 5+ iterations no commit
- `detect_instruction_overload` — 4+ instructions in response, partial completion
- `detect_flaky_verification` — pass/fail alternation
- `detect_ordering_failure` — dependency error + backtrack (uses iterations_tsv content)
- `detect_scope_creep` — files outside PRD deliverables (skips if no deliverables)
- `detect_oscillation` — similar fingerprints across 4+ iterations
- `detect_revert_rate` — revert rate > 0.3
- `detect_waste_rate` — waste rate > 0.15

Each test constructs a `RunData` with the triggering condition and asserts findings are produced, plus a negative test where the condition is not met.

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p glass_feedback
```
Expected: FAIL

- [ ] **Step 3: Implement remaining detectors**

Each detector follows the same pattern as Task 4. Key implementation notes:

- `detect_hot_files`: count file occurrences in `data.reverted_files`, produce a finding per file with count >= 3. The finding includes `action_params = { file: "..." }`.
- `detect_instruction_overload`: count numbered list items (`1.`, `2.`, etc.) in `data.agent_responses`. Heuristic, not exact.
- `detect_flaky_verification`: look for pass→fail→pass or fail→pass→fail patterns in `data.verify_pass_fail_sequence`.
- `detect_oscillation`: group `data.fingerprint_sequence` into runs of similar values (within hamming distance threshold). 4+ similar but non-identical = oscillation.
- `detect_scope_creep`: only runs when `data.prd_content` is Some and `parse_prd_deliverables()` returns files. Compare against `data.git_diff_stat` file list.

Add all new detectors to `analyze()`.

- [ ] **Step 4: Run tests**

```bash
cargo test -p glass_feedback
```
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/glass_feedback/src/analyzer.rs
git commit -m "feat(feedback): remaining 10 detectors — hot files, drift, overload, flaky, oscillation"
```

---

## Chunk 3: Rule Engine & Lifecycle Manager

### Task 6: Rule engine — load, match, inject

**Files:**
- Create: `crates/glass_feedback/src/rules.rs`

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_rules_from_multiple_sources() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project-rules.toml");
        let global = dir.path().join("global-rules.toml");

        // Project rule
        let project_file = RulesFile {
            meta: RulesMeta::default(),
            rules: vec![make_test_rule("proj-1", "isolate_commits", RuleStatus::Confirmed)],
        };
        crate::io::save_rules_file(&project, &project_file).unwrap();

        // Global rule
        let global_file = RulesFile {
            meta: RulesMeta::default(),
            rules: vec![make_test_rule("glob-1", "force_commit", RuleStatus::Confirmed)],
        };
        crate::io::save_rules_file(&global, &global_file).unwrap();

        let engine = RuleEngine::load(&project, &global, None);
        assert_eq!(engine.rules.len(), 2);
    }

    #[test]
    fn project_rule_overrides_global() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project.toml");
        let global = dir.path().join("global.toml");

        // Same action in both — project wins
        let project_file = RulesFile {
            meta: RulesMeta::default(),
            rules: vec![make_test_rule("proj-1", "smaller_instructions", RuleStatus::Confirmed)],
        };
        crate::io::save_rules_file(&project, &project_file).unwrap();

        let global_file = RulesFile {
            meta: RulesMeta::default(),
            rules: vec![make_test_rule("glob-1", "smaller_instructions", RuleStatus::Provisional)],
        };
        crate::io::save_rules_file(&global, &global_file).unwrap();

        let engine = RuleEngine::load(&project, &global, None);
        let matching: Vec<_> = engine.rules.iter()
            .filter(|r| r.action == "smaller_instructions")
            .collect();
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].id, "proj-1"); // project wins
    }

    #[test]
    fn check_rules_returns_text_actions() {
        let engine = RuleEngine {
            rules: vec![make_test_rule("r1", "force_commit", RuleStatus::Confirmed)],
        };
        let state = RunState {
            uncommitted_iterations: 6,
            ..Default::default()
        };
        let actions = engine.check_rules(&state);
        assert!(actions.iter().any(|a| matches!(a, RuleAction::TextInjection(_))));
    }

    #[test]
    fn check_rules_returns_rust_level_actions() {
        let engine = RuleEngine {
            rules: vec![make_test_rule("r1", "extend_silence", RuleStatus::Confirmed)],
        };
        let state = RunState::default();
        let actions = engine.check_rules(&state);
        assert!(actions.iter().any(|a| matches!(a, RuleAction::ExtendSilence { .. })));
    }

    #[test]
    fn only_confirmed_provisional_pinned_rules_fire() {
        let engine = RuleEngine {
            rules: vec![
                make_test_rule("r1", "force_commit", RuleStatus::Rejected),
                make_test_rule("r2", "force_commit", RuleStatus::Stale),
                make_test_rule("r3", "force_commit", RuleStatus::Proposed),
            ],
        };
        let state = RunState { uncommitted_iterations: 10, ..Default::default() };
        let actions = engine.check_rules(&state);
        assert!(actions.is_empty());
    }

    fn make_test_rule(id: &str, action: &str, status: RuleStatus) -> Rule {
        Rule {
            id: id.to_string(),
            trigger: String::new(),
            trigger_params: Default::default(),
            action: action.to_string(),
            action_params: Default::default(),
            status,
            severity: Severity::Medium,
            scope: Scope::Project,
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
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

- [ ] **Step 3: Implement RuleEngine**

```rust
use crate::types::*;
use crate::io;
use std::path::Path;

pub struct RuleEngine {
    pub rules: Vec<Rule>,
}

impl RuleEngine {
    /// Load rules from project, global, and default sources.
    /// Project rules override global for same action.
    pub fn load(
        project_path: &Path,
        global_path: &Path,
        defaults_path: Option<&Path>,
    ) -> Self { ... }

    /// Check which rules should fire given current run state.
    /// Returns actions to inject. Only confirmed/provisional/pinned rules fire.
    pub fn check_rules(&self, state: &RunState) -> Vec<RuleAction> { ... }

    /// Get prompt hints (Tier 3) for system prompt injection.
    pub fn prompt_hints(&self) -> Vec<String> { ... }
}
```

Each action type has a hardcoded match function against `RunState`:
- `force_commit` → fires when `state.uncommitted_iterations >= 5`
- `isolate_commits` → fires when `state.recent_reverted_files` contains the file in `action_params`
- `smaller_instructions` → always fires (reduces instruction count)
- `extend_silence` → fires always, returns `RuleAction::ExtendSilence`
- `run_verify_twice` → fires when `state.verify_alternations >= 2`
- `early_stuck` → fires always, returns `RuleAction::EarlyStuck`
- `restrict_scope` → always fires
- `build_dependency_first` → always fires
- `verify_progress` → fires when `state.waste_rate > 0.15`

- [ ] **Step 4: Run tests**

```bash
cargo test -p glass_feedback
```
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/glass_feedback/src/rules.rs
git commit -m "feat(feedback): rule engine — load, match, inject with priority override"
```

---

### Task 7: Lifecycle manager

**Files:**
- Create: `crates/glass_feedback/src/lifecycle.rs`

- [ ] **Step 1: Write tests for lifecycle transitions**

Tests for:
- `promote_provisional` — marks provisional → confirmed, sets confirmed_run
- `reject_provisional` — marks provisional → rejected, sets rejected_run and reason
- `detect_staleness` — marks rules not triggered in 10 runs as stale
- `archive_stale` — moves stale rules to archived list after 5 more runs
- `unstale_on_trigger` — stale → confirmed when re-triggered
- `demote_on_drift` — confirmed → provisional when associated metric worsening
- `cooldown_decrement` — decrements cooldown_remaining on rejected rules each run
- `re_propose_after_cooldown` — rejected with cooldown=0 → proposed
- `apply_findings` — converts analyzer findings into new proposed rules, respects 3-provisional cap
- `conservative_after_bulk_rejection` — max 1 re-proposal per run after bulk rejection

- [ ] **Step 2: Run tests to verify they fail**

- [ ] **Step 3: Implement lifecycle manager**

```rust
pub struct LifecycleManager;

impl LifecycleManager {
    /// Apply findings from analyzer to create new proposed rules.
    /// Respects the provisional cap (max 3, or max 1 after bulk rejection).
    pub fn apply_findings(
        rules: &mut Vec<Rule>,
        findings: &[Finding],
        run_id: &str,
        bulk_rejection_last_run: bool,
    ) -> Vec<Rule> { ... }

    /// Promote provisional rules that survived a non-regressing run.
    pub fn promote_provisional(rules: &mut [Rule], run_id: &str) { ... }

    /// Reject provisional rules due to regression.
    pub fn reject_provisional(rules: &mut [Rule], run_id: &str, reason: &str) { ... }

    /// Run staleness checks after each run.
    pub fn update_staleness(rules: &mut Vec<Rule>, archived: &mut Vec<Rule>) { ... }

    /// Demote confirmed rules whose metrics are trending worse.
    pub fn check_drift(rules: &mut [Rule], recent_metrics: &[RunMetrics]) { ... }

    /// Decrement cooldowns on rejected rules, re-propose eligible ones.
    pub fn process_cooldowns(rules: &mut [Rule]) { ... }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p glass_feedback
```
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/glass_feedback/src/lifecycle.rs
git commit -m "feat(feedback): lifecycle manager — promote, reject, stale, drift, cooldown"
```

---

## Chunk 4: Regression Guard, LLM Analyzer & Integration

### Task 8: Regression guard

**Files:**
- Create: `crates/glass_feedback/src/regression.rs`

- [ ] **Step 1: Write tests**

Tests for:
- `take_snapshot` — captures config values and provisional rule IDs
- `compare_metrics` — detects regression vs. improvement vs. neutral
- `rollback` — restores config values, marks rules rejected
- `cold_start` — first run with no baseline returns observation-only (no regression possible)
- `incomplete_run_detection` — snapshot with no matching metrics entry = no-op

- [ ] **Step 2: Run tests to verify they fail**

- [ ] **Step 3: Implement regression guard**

```rust
pub struct RegressionGuard;

impl RegressionGuard {
    /// Take a snapshot before the run starts. Persists to tuning-history.toml.
    pub fn take_snapshot(
        config_values: HashMap<String, String>,
        provisional_rules: Vec<String>,
        run_id: &str,
        history_path: &Path,
    ) -> ConfigSnapshot { ... }

    /// Compare this run's metrics against the baseline.
    /// Returns None on cold start (no previous metrics).
    pub fn compare(
        current: &RunMetrics,
        baseline: Option<&RunMetrics>,
    ) -> Option<RegressionResult> { ... }

    /// Apply the regression result — promote or reject.
    pub fn apply(
        result: &RegressionResult,
        rules: &mut [Rule],
        snapshot: &ConfigSnapshot,
        config_path: &Path,
        run_id: &str,
    ) -> anyhow::Result<()> { ... }
}

pub enum RegressionResult {
    Improved,
    Neutral,
    Regressed { reasons: Vec<String> },
}
```

- [ ] **Step 4: Run tests**

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/glass_feedback/src/regression.rs
git commit -m "feat(feedback): regression guard — snapshot, compare, rollback with cold start"
```

---

### Task 9: LLM analyzer

**Files:**
- Create: `crates/glass_feedback/src/llm.rs`

- [ ] **Step 1: Write tests for response parsing**

Tests for:
- `parse_llm_response` — parses the structured FINDING/SCOPE/SEVERITY format
- `parse_llm_response_partial` — handles partial/malformed responses gracefully
- `parse_llm_response_empty` — empty response returns no findings
- `build_analysis_prompt` — constructs the prompt from RunData
- `dedup_against_existing` — deduplicates findings against existing prompt hints

- [ ] **Step 2: Run tests to verify they fail**

- [ ] **Step 3: Implement LLM analyzer**

This module handles prompt construction and response parsing only — it does NOT make the LLM call itself. The actual call is made by `main.rs` using the agent runtime. This keeps `glass_feedback` free of async/network dependencies.

```rust
/// Build the analysis prompt for the LLM.
pub fn build_analysis_prompt(
    data: &RunData,
    rule_based_findings: &[Finding],
) -> String { ... }

/// Parse the LLM response into findings.
pub fn parse_llm_response(response: &str) -> Vec<Finding> { ... }

/// Deduplicate new findings against existing prompt hints.
pub fn dedup_findings(
    new: Vec<Finding>,
    existing_hints: &[Rule],
    max_hints: usize,
) -> Vec<Finding> { ... }
```

- [ ] **Step 4: Run tests**

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/glass_feedback/src/llm.rs
git commit -m "feat(feedback): LLM analyzer — prompt builder and response parser"
```

---

### Task 10: Public API — on_run_start, on_run_end, check_rules, prompt_hints

**Files:**
- Modify: `crates/glass_feedback/src/lib.rs`

- [ ] **Step 1: Write integration tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_lifecycle_cold_start() {
        let dir = tempfile::tempdir().unwrap();
        let project_root = dir.path().to_string_lossy().to_string();

        // First run — cold start, observation only
        let state = on_run_start(&project_root, &default_config());
        let data = RunData { iterations: 10, revert_count: 4, ..Default::default() };
        let result = on_run_end(state, data);
        // Should produce findings but no regression (no baseline)
        assert!(!result.findings.is_empty());
        assert!(result.regression.is_none());
    }

    #[test]
    fn full_lifecycle_second_run_no_regression() {
        let dir = tempfile::tempdir().unwrap();
        let project_root = dir.path().to_string_lossy().to_string();

        // Run 1
        let state = on_run_start(&project_root, &default_config());
        let data = RunData { iterations: 20, revert_count: 6, ..Default::default() };
        let _ = on_run_end(state, data);

        // Run 2 — better metrics
        let state = on_run_start(&project_root, &default_config());
        let data = RunData { iterations: 20, revert_count: 2, ..Default::default() };
        let result = on_run_end(state, data);
        // Provisional rules should be promoted
        assert!(matches!(result.regression, Some(RegressionResult::Improved | RegressionResult::Neutral)));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

- [ ] **Step 3: Implement public API**

```rust
/// State handle returned by on_run_start, passed to on_run_end.
pub struct FeedbackState {
    pub project_root: String,
    pub rules_path: PathBuf,
    pub global_rules_path: PathBuf,
    pub metrics_path: PathBuf,
    pub history_path: PathBuf,
    pub snapshot: ConfigSnapshot,
    pub engine: rules::RuleEngine,
    pub feedback_write_pending: bool,
}

/// Result of on_run_end.
pub struct FeedbackResult {
    pub findings: Vec<Finding>,
    pub regression: Option<regression::RegressionResult>,
    pub rules_promoted: Vec<String>,
    pub rules_rejected: Vec<String>,
    pub config_changes: Vec<(String, String, String)>, // (field, old, new)
}

/// Called when orchestrator activates.
pub fn on_run_start(project_root: &str, config: &FeedbackConfig) -> FeedbackState { ... }

/// Called when orchestrator stops.
pub fn on_run_end(state: FeedbackState, data: RunData) -> FeedbackResult { ... }

/// Check active rules for runtime injection.
pub fn check_rules(state: &FeedbackState, run_state: &RunState) -> Vec<RuleAction> { ... }

/// Get prompt hints for system prompt.
pub fn prompt_hints(state: &FeedbackState) -> Vec<String> { ... }
```

- [ ] **Step 4: Run tests**

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/glass_feedback/src/lib.rs
git commit -m "feat(feedback): public API — on_run_start, on_run_end, check_rules, prompt_hints"
```

---

### Task 11: Default rules

**Files:**
- Create: `crates/glass_feedback/src/defaults.rs`

- [ ] **Step 1: Write test that defaults are valid and complete**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_rules_parse() {
        let rules = default_rules();
        assert_eq!(rules.len(), 6);
        for rule in &rules {
            assert!(rule.id.starts_with("default-"));
            assert_eq!(rule.status, RuleStatus::Provisional);
            assert_eq!(rule.scope, Scope::Global);
        }
    }

    #[test]
    fn default_rules_no_duplicate_ids() {
        let rules = default_rules();
        let ids: Vec<_> = rules.iter().map(|r| &r.id).collect();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(ids.len(), unique.len());
    }
}
```

- [ ] **Step 2: Implement default rules and lifecycle integration**

```rust
/// Return the 6 default rules defined in the spec.
pub fn default_rules() -> Vec<Rule> { ... }

/// Merge defaults into a project's rules file on first run.
/// - If rules file doesn't exist or has no defaults, copy all defaults as Provisional
/// - If rules file has older defaults (by meta.version), add only new ones
/// - If a default was previously Rejected in this project, skip it
pub fn merge_defaults_into_project(
    project_rules: &mut RulesFile,
    defaults: &[Rule],
    default_version: &str,
) { ... }

/// Write defaults to ~/.glass/default-rules.toml if missing or outdated.
pub fn ensure_global_defaults(global_defaults_path: &Path) { ... }
```

The `merge_defaults_into_project` function is called by `on_run_start` when loading rules. It checks `project_rules.meta.version` against the current defaults version. Rejected defaults (matched by `id`) are not re-proposed.

- [ ] **Step 3: Run tests, commit**

```bash
cargo test -p glass_feedback && cargo clippy -p glass_feedback -- -D warnings
git add crates/glass_feedback/src/defaults.rs
git commit -m "feat(feedback): 6 default rules with lifecycle integration and version tracking"
```

---

## Chunk 5: Main.rs Integration & Config

### Task 12: Add config fields

**Files:**
- Modify: `crates/glass_core/src/config.rs`

- [ ] **Step 1: Add feedback fields to OrchestratorSection**

Add to `OrchestratorSection`:
```rust
#[serde(default)]
pub feedback_llm: bool,

#[serde(default = "default_max_prompt_hints")]
pub max_prompt_hints: usize,
```

Add default function:
```rust
fn default_max_prompt_hints() -> usize { 10 }
```

- [ ] **Step 2: Run all tests to verify nothing breaks**

```bash
cargo test --workspace
```
Expected: PASS (serde defaults handle existing configs)

- [ ] **Step 3: Commit**

```bash
git add crates/glass_core/src/config.rs
git commit -m "feat(feedback): add feedback_llm and max_prompt_hints config fields"
```

---

### Task 13: Integrate into main.rs

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add FeedbackState to Processor struct**

Add field:
```rust
feedback_state: Option<glass_feedback::FeedbackState>,
feedback_write_pending: bool,
```

Initialize to `None` / `false` in `Processor::new()`.

- [ ] **Step 2: Call on_run_start in both orchestrator activation paths**

In the Ctrl+Shift+O handler (after `self.orchestrator.active = true`):
```rust
let feedback_config = glass_feedback::FeedbackConfig {
    project_root: current_cwd.clone(),
    feedback_llm: self.config.agent.as_ref()
        .and_then(|a| a.orchestrator.as_ref())
        .map(|o| o.feedback_llm)
        .unwrap_or(false),
    max_prompt_hints: self.config.agent.as_ref()
        .and_then(|a| a.orchestrator.as_ref())
        .map(|o| o.max_prompt_hints)
        .unwrap_or(10),
};
self.feedback_state = Some(glass_feedback::on_run_start(
    &current_cwd,
    &feedback_config,
));
```

Same in the config hot-reload activation path.

- [ ] **Step 3: Call on_run_end on every orchestrator deactivation**

At every point where `self.orchestrator.active = false` is set, before deactivating:
```rust
if let Some(feedback_state) = self.feedback_state.take() {
    let run_data = build_run_data(&self.orchestrator, &self.orchestrator_event_buffer);
    let result = glass_feedback::on_run_end(feedback_state, run_data);
    tracing::info!(
        "Feedback: {} findings, {} promoted, {} rejected",
        result.findings.len(),
        result.rules_promoted.len(),
        result.rules_rejected.len(),
    );
}
```

- [ ] **Step 4: Call check_rules in OrchestratorSilence handler**

Before the context send, after the kickoff guard:
```rust
if let Some(ref feedback_state) = self.feedback_state {
    let run_state = build_run_state(&self.orchestrator);
    let actions = glass_feedback::check_rules(feedback_state, &run_state);
    for action in &actions {
        match action {
            glass_feedback::RuleAction::TextInjection(text) => {
                feedback_instructions.push(text.clone());
            }
            glass_feedback::RuleAction::ExtendSilence { extra_secs } => {
                // Handled by adjusting next silence threshold
            }
            glass_feedback::RuleAction::RunVerifyTwice => {
                // Set flag for verify handler
            }
            glass_feedback::RuleAction::EarlyStuck { threshold } => {
                // Temporarily lower stuck threshold
            }
        }
    }
}
```

Append `feedback_instructions` as `[FEEDBACK_RULES]` section in the context.

- [ ] **Step 5: Inject prompt hints in system prompt builder**

In `build_orchestrator_system_prompt()`, add after checkpoint content:
```rust
if let Some(ref feedback_state) = self.feedback_state {
    let hints = glass_feedback::prompt_hints(feedback_state);
    if !hints.is_empty() {
        // Append as LESSONS FROM PREVIOUS RUNS section
    }
}
```

- [ ] **Step 6: Add feedback_write_pending guard to ConfigReloaded handler**

In the `ConfigReloaded` event handler, skip orchestrator config reload when the flag is set:
```rust
if self.feedback_write_pending {
    self.feedback_write_pending = false;
    // Skip orchestrator config reload — feedback loop just wrote it
} else {
    // Normal reload logic
}
```

- [ ] **Step 7: Add new tracking fields to OrchestratorState**

In `src/orchestrator.rs`, add to `OrchestratorState`:
```rust
pub fast_trigger_during_output: u32,    // incremented in SmartTrigger when output was flowing
pub silence_interruptions: u32,         // incremented when silence fires during active output
pub waste_iterations: u32,              // iterations where git diff --stat is empty
pub commit_count: u32,                  // incremented on each detected git commit
pub reverted_files: Vec<String>,        // files from git diff --stat on revert
pub iteration_timestamps: Vec<std::time::Instant>,  // for avg idle calculation
```

Initialize all to 0/empty in `OrchestratorState::new()`. Reset on orchestrator activation (same places as `kickoff_complete`).

Increment `waste_iterations` in the `OrchestratorSilence` handler when git diff --stat is empty.
Increment `commit_count` in the iteration handler when a new commit is detected.
Populate `reverted_files` in the `VerifyComplete` handler when a revert occurs.
Push to `iteration_timestamps` on each iteration.

- [ ] **Step 8: Write build_run_data helper function**

In `src/main.rs`, add:
```rust
fn build_run_data(
    orch: &orchestrator::OrchestratorState,
    events: &orchestrator_events::OrchestratorEventBuffer,
    activated_at: Option<std::time::Instant>,
) -> glass_feedback::RunData {
    // revert_count, keep_count from MetricBaseline
    // stuck_count: count "stuck" entries in events
    // agent_responses: extract AgentText events
    // verify_pass_fail_sequence: extract VerifyResult events
    // fingerprint_sequence: hash each StateFingerprint fields
    // avg_idle: compute from iteration_timestamps
    // All other fields mapped directly from OrchestratorState
    ...
}
```

- [ ] **Step 9: Write build_run_state helper function**

```rust
fn build_run_state(orch: &orchestrator::OrchestratorState) -> glass_feedback::RunState {
    RunState {
        iteration: orch.iteration,
        uncommitted_iterations: /* count since last commit */,
        revert_rate: /* from MetricBaseline */,
        stuck_rate: /* stuck_count / iteration */,
        waste_rate: /* waste_iterations / iteration */,
        recent_reverted_files: orch.reverted_files.clone(),
        verify_alternations: /* count from verify sequence */,
    }
}
```

- [ ] **Step 10: Apply config changes from FeedbackResult**

After `on_run_end` returns, apply Tier 1 config changes:
```rust
if let Some(ref result) = feedback_result {
    for (field, _old, new_val) in &result.config_changes {
        if let Some(config_path) = dirs::home_dir().map(|h| h.join(".glass/config.toml")) {
            self.feedback_write_pending = true;
            let _ = glass_core::config::update_config_field(
                &config_path,
                Some("agent.orchestrator"),
                field,
                &format!("\"{new_val}\""),
            );
        }
    }
}
```

The `on_run_end` function enforces the "max 1 config change per run" constraint internally — it only includes the highest-severity ConfigTuning finding in `config_changes`.

- [ ] **Step 11: Build and run tests**

```bash
cargo build
cargo test --workspace
cargo clippy --workspace -- -D warnings
```
Expected: all PASS

- [ ] **Step 12: Commit**

```bash
git add src/main.rs src/orchestrator.rs
git commit -m "feat(feedback): integrate feedback loop into orchestrator — start, end, rules, hints"
```

---

### Task 14: Update README and orchestrator docs

**Files:**
- Modify: `README.md`
- Modify: `docs/src/features/orchestrator.md`

- [ ] **Step 1: Add feedback loop section to README**

Add to the Orchestrator Mode features list:
```
- Feedback loop: rule-based analysis after each run, auto-config tuning, behavioral rules, optional LLM analysis, regression guard with auto-rollback
```

- [ ] **Step 2: Add feedback loop section to orchestrator.md**

Add new section documenting:
- What the feedback loop does
- Three tiers of findings
- Rule lifecycle
- Regression guard
- Configuration (`feedback_llm`, `max_prompt_hints`)
- Default rules
- File locations (`.glass/rules.toml`, etc.)

- [ ] **Step 3: Update config section in README and orchestrator.md**

Add the two new config fields to the example TOML blocks.

- [ ] **Step 4: Commit**

```bash
git add README.md docs/src/features/orchestrator.md
git commit -m "docs: add feedback loop documentation to README and orchestrator guide"
```

---

### Task 15: Final integration test

**Files:**
- Create: `crates/glass_feedback/tests/integration.rs`

- [ ] **Step 1: Write end-to-end test**

Simulates a 3-run sequence:
1. Run 1 (cold start) — high revert rate produces findings, no regression comparison
2. Run 2 — provisional rules active, metrics improve → rules promoted
3. Run 3 — inject a regression → rules rejected, config rolled back

Verifies the full lifecycle: propose → provisional → confirmed (run 2) and propose → provisional → rejected (run 3).

- [ ] **Step 2: Run test**

```bash
cargo test -p glass_feedback --test integration
```
Expected: PASS

- [ ] **Step 3: Run full workspace build, test, clippy, fmt**

```bash
cargo fmt --all -- --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
```
Expected: all PASS

- [ ] **Step 4: Commit**

```bash
git add crates/glass_feedback/tests/
git commit -m "test(feedback): end-to-end integration test — 3-run lifecycle sequence"
```
