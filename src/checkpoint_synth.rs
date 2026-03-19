//! Checkpoint synthesis: gathers session data and produces checkpoint.md
//! via an ephemeral claude session or raw-data fallback.

use crate::orchestrator;

/// Gathered session data for checkpoint synthesis.
#[derive(Debug, Clone)]
pub struct CheckpointData {
    pub soi_errors: Vec<String>,
    pub iterations_tsv: String,
    pub git_log: String,
    pub git_diff_stat: String,
    pub git_diff_names: String,
    pub metric_summary: String,
    pub prd_content: String,
    pub coverage_gaps: String,
    pub completed: String,
    pub next: String,
}

/// Build the system prompt for checkpoint synthesis.
pub fn synthesis_system_prompt() -> String {
    r#"You are a session handoff synthesizer. Given raw data from an autonomous coding/task session, produce a structured handoff document for the next agent session that will continue this work.

Rules:
- Preserve SOI errors exactly as provided (file:line, error codes)
- Identify abandoned approaches from iteration log entries marked "stuck" or "revert"
- Note uncommitted changes and their purpose
- Be concise but complete — the next session has no other context
- Use this structure:

## Completed
## Current Errors
## Abandoned Approaches
## Key Decisions
## Git State
## Next"#.to_string()
}

/// Build the user message from gathered checkpoint data.
pub fn synthesis_user_message(data: &CheckpointData) -> String {
    let mut msg = String::new();
    msg.push_str(&format!(
        "## Agent Summary\nCompleted: {}\nNext: {}\n\n",
        data.completed, data.next
    ));

    if !data.soi_errors.is_empty() {
        msg.push_str("## SOI Errors\n");
        for err in &data.soi_errors {
            msg.push_str(&format!("- {err}\n"));
        }
        msg.push('\n');
    }

    if !data.iterations_tsv.is_empty() {
        msg.push_str("## Iteration Log\n```\n");
        msg.push_str(&data.iterations_tsv);
        msg.push_str("```\n\n");
    }

    if !data.git_log.is_empty() {
        msg.push_str("## Git Log\n```\n");
        msg.push_str(&data.git_log);
        msg.push_str("```\n\n");
    }

    if !data.git_diff_stat.is_empty() {
        msg.push_str("## Uncommitted Changes\n```\n");
        msg.push_str(&data.git_diff_stat);
        msg.push_str("```\n\n");
    }

    if !data.git_diff_names.is_empty() {
        msg.push_str("## Changed Files\n");
        for f in data.git_diff_names.lines() {
            msg.push_str(&format!("- {f}\n"));
        }
        msg.push('\n');
    }

    if !data.metric_summary.is_empty() {
        msg.push_str(&format!("## Metric Guard\n{}\n\n", data.metric_summary));
    }

    if !data.coverage_gaps.is_empty() {
        msg.push_str(&format!("## Coverage Gaps\n{}\n\n", data.coverage_gaps));
    }

    if !data.prd_content.is_empty() {
        msg.push_str("## PRD\n");
        let words: Vec<&str> = data.prd_content.split_whitespace().collect();
        if words.len() > 4000 {
            msg.push_str(&words[..4000].join(" "));
            msg.push_str("\n... (truncated)\n");
        } else {
            msg.push_str(&data.prd_content);
        }
        msg.push('\n');
    }

    msg
}

/// Build a fallback checkpoint.md from raw data (no LLM synthesis).
pub fn build_fallback_checkpoint(data: &CheckpointData) -> String {
    let mut cp = String::new();

    cp.push_str("## Completed\n");
    if data.completed.is_empty() {
        cp.push_str("(no summary available)\n");
    } else {
        cp.push_str(&format!("{}\n", data.completed));
    }
    cp.push('\n');

    cp.push_str("## Current Errors\n");
    if data.soi_errors.is_empty() {
        cp.push_str("None\n");
    } else {
        for err in &data.soi_errors {
            cp.push_str(&format!("- {err}\n"));
        }
    }
    cp.push('\n');

    cp.push_str("## Abandoned Approaches\n");
    let abandoned: Vec<&str> = data
        .iterations_tsv
        .lines()
        .filter(|l| l.contains("stuck") || l.contains("revert"))
        .collect();
    if abandoned.is_empty() {
        cp.push_str("None\n");
    } else {
        for line in abandoned {
            cp.push_str(&format!("- {line}\n"));
        }
    }
    cp.push('\n');

    cp.push_str("## Key Decisions\n(not available in fallback mode)\n\n");

    cp.push_str("## Git State\n");
    if !data.git_log.is_empty() {
        cp.push_str(&format!("Recent commits:\n```\n{}```\n", data.git_log));
    }
    if !data.git_diff_stat.is_empty() {
        cp.push_str(&format!(
            "Uncommitted changes:\n```\n{}```\n",
            data.git_diff_stat
        ));
    }
    cp.push('\n');

    cp.push_str("## Next\n");
    if data.next.is_empty() {
        cp.push_str("(no summary available)\n");
    } else {
        cp.push_str(&format!("{}\n", data.next));
    }

    cp
}

/// Build a metric summary string from baseline data.
pub fn build_metric_summary(baseline: Option<&orchestrator::MetricBaseline>) -> String {
    let Some(b) = baseline else {
        return String::new();
    };
    let mut summary = format!("Keeps: {}, Reverts: {}\n", b.keep_count, b.revert_count);
    if let Some(result) = b.last_results.first() {
        if let Some(passed) = result.tests_passed {
            summary.push_str(&format!("Last test run: {passed} passed"));
            if let Some(failed) = result.tests_failed {
                summary.push_str(&format!(", {failed} failed"));
            }
            summary.push('\n');
        }
    }
    summary
}

/// Gather git state for checkpoint synthesis.
/// Returns (git_log, git_diff_stat, git_diff_names).
pub fn gather_git_state(project_root: &str) -> (String, String, String) {
    let git_log = crate::git_cmd()
        .args(["log", "--oneline", "-20"])
        .current_dir(project_root)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        })
        .unwrap_or_default();

    let git_diff_stat = crate::git_cmd()
        .args(["diff", "--stat"])
        .current_dir(project_root)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();

    let git_diff_names = crate::git_cmd()
        .args(["diff", "--name-only"])
        .current_dir(project_root)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();

    (git_log, git_diff_stat, git_diff_names)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_checkpoint_data() -> CheckpointData {
        CheckpointData {
            soi_errors: vec!["src/main.rs:42 Error[E0277]: trait bound not satisfied".to_string()],
            iterations_tsv: "iteration\tstatus\n1\tkeep\n2\tstuck\n3\trevert\n".to_string(),
            git_log: "abc123 feat: add auth\ndef456 fix: login bug\n".to_string(),
            git_diff_stat: " src/auth.rs | 12 +++\n".to_string(),
            git_diff_names: "src/auth.rs\n".to_string(),
            metric_summary: "Keeps: 5, Reverts: 1\n".to_string(),
            prd_content: "Build a login page".to_string(),
            coverage_gaps: "[COVERAGE_GAP] src/db.rs\n".to_string(),
            completed: "auth module".to_string(),
            next: "database layer".to_string(),
        }
    }

    #[test]
    fn fallback_checkpoint_has_all_sections() {
        let data = make_checkpoint_data();
        let cp = build_fallback_checkpoint(&data);
        assert!(cp.contains("## Completed"));
        assert!(cp.contains("auth module"));
        assert!(cp.contains("## Current Errors"));
        assert!(cp.contains("Error[E0277]"));
        assert!(cp.contains("## Abandoned Approaches"));
        assert!(cp.contains("stuck"));
        assert!(cp.contains("revert"));
        assert!(cp.contains("## Key Decisions"));
        assert!(cp.contains("## Git State"));
        assert!(cp.contains("abc123"));
        assert!(cp.contains("## Next"));
        assert!(cp.contains("database layer"));
    }

    #[test]
    fn fallback_checkpoint_empty_data() {
        let data = CheckpointData {
            soi_errors: vec![],
            iterations_tsv: String::new(),
            git_log: String::new(),
            git_diff_stat: String::new(),
            git_diff_names: String::new(),
            metric_summary: String::new(),
            prd_content: String::new(),
            coverage_gaps: String::new(),
            completed: String::new(),
            next: String::new(),
        };
        let cp = build_fallback_checkpoint(&data);
        assert!(cp.contains("## Completed"));
        assert!(cp.contains("(no summary available)"));
        assert!(cp.contains("## Current Errors"));
        assert!(cp.contains("None"));
    }

    #[test]
    fn synthesis_user_message_includes_all_data() {
        let data = make_checkpoint_data();
        let msg = synthesis_user_message(&data);
        assert!(msg.contains("auth module"));
        assert!(msg.contains("database layer"));
        assert!(msg.contains("Error[E0277]"));
        assert!(msg.contains("abc123"));
        assert!(msg.contains("src/auth.rs"));
        assert!(msg.contains("COVERAGE_GAP"));
    }

    #[test]
    fn synthesis_user_message_truncates_long_prd() {
        let long_prd = "word ".repeat(5000);
        let mut data = make_checkpoint_data();
        data.prd_content = long_prd;
        let msg = synthesis_user_message(&data);
        assert!(msg.contains("(truncated)"));
    }

    #[test]
    fn metric_summary_with_baseline() {
        let mut baseline = orchestrator::MetricBaseline::new();
        baseline.keep_count = 8;
        baseline.revert_count = 2;
        baseline.last_results = vec![orchestrator::VerifyResult {
            command_name: "test".to_string(),
            exit_code: 0,
            tests_passed: Some(45),
            tests_failed: Some(1),
            errors: vec![],
        }];
        let summary = build_metric_summary(Some(&baseline));
        assert!(summary.contains("Keeps: 8"));
        assert!(summary.contains("Reverts: 2"));
        assert!(summary.contains("45 passed"));
    }

    #[test]
    fn metric_summary_without_baseline() {
        assert!(build_metric_summary(None).is_empty());
    }

    #[test]
    fn system_prompt_has_structure() {
        let prompt = synthesis_system_prompt();
        assert!(prompt.contains("## Completed"));
        assert!(prompt.contains("## Current Errors"));
        assert!(prompt.contains("## Abandoned Approaches"));
        assert!(prompt.contains("## Next"));
    }
}
