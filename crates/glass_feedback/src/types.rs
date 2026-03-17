use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Project,
    Global,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingCategory {
    ConfigTuning,
    BehavioralRule,
    PromptHint,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    /// Returns true if transitioning from `self` to `target` is valid.
    pub fn can_transition_to(&self, target: &RuleStatus) -> bool {
        matches!(
            (self, target),
            (RuleStatus::Proposed, RuleStatus::Provisional)
                | (RuleStatus::Provisional, RuleStatus::Confirmed)
                | (RuleStatus::Provisional, RuleStatus::Rejected)
                | (RuleStatus::Confirmed, RuleStatus::Stale)
                | (RuleStatus::Confirmed, RuleStatus::Provisional)
                | (RuleStatus::Stale, RuleStatus::Confirmed)
                | (RuleStatus::Rejected, RuleStatus::Proposed)
        )
    }
}

// ---------------------------------------------------------------------------
// FindingAction — tagged serde enum
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub category: FindingCategory,
    pub severity: Severity,
    pub action: FindingAction,
    pub evidence: String,
    pub scope: Scope,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RulesMeta {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RulesFile {
    #[serde(default)]
    pub meta: RulesMeta,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunMetricsFile {
    #[serde(default)]
    pub runs: Vec<RunMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSnapshot {
    pub run_id: String,
    pub config_values: HashMap<String, String>,
    pub provisional_rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TuningHistoryFile {
    #[serde(default)]
    pub snapshots: Vec<ConfigSnapshot>,
}

#[derive(Debug, Clone, Default)]
pub struct RunState {
    pub iteration: u32,
    pub iterations_since_last_commit: u32,
    pub revert_rate: f64,
    pub stuck_rate: f64,
    pub waste_rate: f64,
    pub recent_reverted_files: Vec<String>,
    pub verify_alternations: u32,
}

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

// ---------------------------------------------------------------------------
// RuleAction — runtime only, no serde
// ---------------------------------------------------------------------------

/// Actions returned by the rule engine at runtime.
#[derive(Debug, Clone)]
pub enum RuleAction {
    /// Rust-level: run git commit -am to checkpoint.
    ForceCommit,
    /// Rust-level: git add + commit a specific hot file in isolation.
    IsolateCommit { file: String },
    /// Rust-level: signal that instruction splitting is active.
    SplitInstructions,
    /// Rust-level: signal that scope guard is active (files computed by caller).
    RevertOutOfScope { files: Vec<String> },
    /// Rust-level: block forward progress until dependency resolved.
    BlockUntilResolved { message: String },
    /// Rust-level: extend silence threshold by N seconds.
    ExtendSilence { extra_secs: u64 },
    /// Rust-level: run verification twice before reverting.
    RunVerifyTwice,
    /// Rust-level: lower stuck detection threshold.
    EarlyStuck { threshold: u32 },
    /// Text injection (kept only for verify_progress).
    TextInjection(String),
}

// ---------------------------------------------------------------------------
// RunData — large accumulator struct, derive Default
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finding_action_config_tuning_construction() {
        let action = FindingAction::ConfigTuning {
            field: "silence_timeout".to_string(),
            current_value: "30".to_string(),
            new_value: "45".to_string(),
        };
        if let FindingAction::ConfigTuning {
            field,
            current_value,
            new_value,
        } = action
        {
            assert_eq!(field, "silence_timeout");
            assert_eq!(current_value, "30");
            assert_eq!(new_value, "45");
        } else {
            panic!("expected ConfigTuning variant");
        }
    }

    #[test]
    fn finding_action_behavioral_rule_construction() {
        let mut params = HashMap::new();
        params.insert("key".to_string(), "value".to_string());
        let action = FindingAction::BehavioralRule {
            action: "extend_silence".to_string(),
            params: params.clone(),
        };
        if let FindingAction::BehavioralRule { action, params: p } = action {
            assert_eq!(action, "extend_silence");
            assert_eq!(p.get("key").map(String::as_str), Some("value"));
        } else {
            panic!("expected BehavioralRule variant");
        }
    }

    #[test]
    fn finding_action_prompt_hint_construction() {
        let action = FindingAction::PromptHint {
            text: "Prefer small commits".to_string(),
        };
        if let FindingAction::PromptHint { text } = action {
            assert_eq!(text, "Prefer small commits");
        } else {
            panic!("expected PromptHint variant");
        }
    }

    #[test]
    fn run_metrics_rate_assertions() {
        let metrics = RunMetrics {
            run_id: "run-001".to_string(),
            project_root: "/tmp/project".to_string(),
            iterations: 20,
            duration_secs: 3600,
            revert_rate: 0.15,
            stuck_rate: 0.05,
            waste_rate: 0.10,
            checkpoint_rate: 0.25,
            completion: "success".to_string(),
            prd_items_completed: 8,
            prd_items_total: 10,
            kickoff_duration_secs: 120,
        };
        assert!((metrics.revert_rate - 0.15).abs() < f64::EPSILON);
        assert!((metrics.stuck_rate - 0.05).abs() < f64::EPSILON);
        assert!((metrics.waste_rate - 0.10).abs() < f64::EPSILON);
        assert!((metrics.checkpoint_rate - 0.25).abs() < f64::EPSILON);
        assert_eq!(metrics.prd_items_completed, 8);
        assert_eq!(metrics.prd_items_total, 10);
    }

    #[test]
    fn rule_status_valid_transitions() {
        assert!(RuleStatus::Proposed.can_transition_to(&RuleStatus::Provisional));
        assert!(RuleStatus::Provisional.can_transition_to(&RuleStatus::Confirmed));
        assert!(RuleStatus::Provisional.can_transition_to(&RuleStatus::Rejected));
        assert!(RuleStatus::Confirmed.can_transition_to(&RuleStatus::Stale));
        assert!(RuleStatus::Confirmed.can_transition_to(&RuleStatus::Provisional));
        assert!(RuleStatus::Stale.can_transition_to(&RuleStatus::Confirmed));
        assert!(RuleStatus::Rejected.can_transition_to(&RuleStatus::Proposed));
    }

    #[test]
    fn rule_status_invalid_transitions() {
        assert!(!RuleStatus::Proposed.can_transition_to(&RuleStatus::Confirmed));
        assert!(!RuleStatus::Proposed.can_transition_to(&RuleStatus::Rejected));
        assert!(!RuleStatus::Proposed.can_transition_to(&RuleStatus::Stale));
        assert!(!RuleStatus::Confirmed.can_transition_to(&RuleStatus::Rejected));
        assert!(!RuleStatus::Stale.can_transition_to(&RuleStatus::Proposed));
        assert!(!RuleStatus::Rejected.can_transition_to(&RuleStatus::Confirmed));
        assert!(!RuleStatus::Pinned.can_transition_to(&RuleStatus::Confirmed));
    }

    #[test]
    fn rule_action_text_injection_matches() {
        let action = RuleAction::TextInjection("hint text".to_string());
        if let RuleAction::TextInjection(text) = action {
            assert_eq!(text, "hint text");
        } else {
            panic!("expected TextInjection variant");
        }
    }

    #[test]
    fn rule_action_extend_silence_matches() {
        let action = RuleAction::ExtendSilence { extra_secs: 30 };
        if let RuleAction::ExtendSilence { extra_secs } = action {
            assert_eq!(extra_secs, 30);
        } else {
            panic!("expected ExtendSilence variant");
        }
    }

    #[test]
    fn rule_action_force_commit_matches() {
        let action = RuleAction::ForceCommit;
        assert!(matches!(action, RuleAction::ForceCommit));
    }

    #[test]
    fn rule_action_isolate_commit_matches() {
        let action = RuleAction::IsolateCommit {
            file: "src/lib.rs".to_string(),
        };
        if let RuleAction::IsolateCommit { file } = action {
            assert_eq!(file, "src/lib.rs");
        } else {
            panic!("expected IsolateCommit variant");
        }
    }

    #[test]
    fn rule_action_split_instructions_matches() {
        let action = RuleAction::SplitInstructions;
        assert!(matches!(action, RuleAction::SplitInstructions));
    }

    #[test]
    fn rule_action_revert_out_of_scope_matches() {
        let action = RuleAction::RevertOutOfScope {
            files: vec!["src/foo.rs".to_string(), "src/bar.rs".to_string()],
        };
        if let RuleAction::RevertOutOfScope { files } = action {
            assert_eq!(files.len(), 2);
            assert_eq!(files[0], "src/foo.rs");
        } else {
            panic!("expected RevertOutOfScope variant");
        }
    }

    #[test]
    fn rule_action_block_until_resolved_matches() {
        let action = RuleAction::BlockUntilResolved {
            message: "Resolve build error first".to_string(),
        };
        if let RuleAction::BlockUntilResolved { message } = action {
            assert_eq!(message, "Resolve build error first");
        } else {
            panic!("expected BlockUntilResolved variant");
        }
    }

    #[test]
    fn run_state_iterations_since_last_commit_field() {
        let state = RunState {
            iterations_since_last_commit: 7,
            ..Default::default()
        };
        assert_eq!(state.iterations_since_last_commit, 7);
    }

    #[test]
    fn run_data_default_has_zeroed_fields() {
        let data = RunData::default();
        assert_eq!(data.project_root, "");
        assert_eq!(data.iterations, 0);
        assert_eq!(data.duration_secs, 0);
        assert_eq!(data.kickoff_duration_secs, 0);
        assert_eq!(data.iterations_tsv, "");
        assert_eq!(data.revert_count, 0);
        assert_eq!(data.keep_count, 0);
        assert_eq!(data.stuck_count, 0);
        assert_eq!(data.checkpoint_count, 0);
        assert_eq!(data.waste_count, 0);
        assert_eq!(data.commit_count, 0);
        assert_eq!(data.completion_reason, "");
        assert!(data.prd_content.is_none());
        assert!(data.git_log.is_none());
        assert!(data.git_diff_stat.is_none());
        assert!(data.reverted_files.is_empty());
        assert!(data.verify_pass_fail_sequence.is_empty());
        assert!(data.agent_responses.is_empty());
        assert_eq!(data.silence_interruptions, 0);
        assert_eq!(data.fast_trigger_during_output, 0);
        assert!((data.avg_idle_between_iterations_secs - 0.0).abs() < f64::EPSILON);
        assert!(data.fingerprint_sequence.is_empty());
        assert_eq!(data.config_silence_timeout, 0);
        assert_eq!(data.config_max_retries, 0);
    }

    #[test]
    fn feedback_config_default_has_correct_values() {
        let cfg = FeedbackConfig::default();
        assert_eq!(cfg.project_root, "");
        assert!(!cfg.feedback_llm);
        assert_eq!(cfg.max_prompt_hints, 10);
    }
}
