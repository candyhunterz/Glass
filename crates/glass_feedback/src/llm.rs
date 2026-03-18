//! LLM analyzer — prompt construction and response parsing.
//!
//! This module is pure data transformation: no network calls, no async.
//! The caller (main.rs) is responsible for passing the prompt to an LLM and
//! feeding the raw text response back to `parse_llm_response`.

use crate::types::{Finding, FindingAction, FindingCategory, Rule, RunData, Scope, Severity};

// ---------------------------------------------------------------------------
// Prompt builder
// ---------------------------------------------------------------------------

/// Build the analysis prompt to send to an LLM.
///
/// * Truncates `iterations_tsv` to the **last 50 lines**.
/// * Truncates `prd_content` to the **first 500 words**.
/// * Lists `rule_based_findings` as bullet points so the LLM avoids
///   repeating what the rule engine already detected.
pub fn build_analysis_prompt(data: &RunData, rule_based_findings: &[Finding]) -> String {
    let duration_mins = data.duration_secs / 60;

    // Last 50 lines of the iterations TSV.
    let iter_lines: Vec<&str> = data.iterations_tsv.lines().collect();
    let iter_snippet = if iter_lines.len() > 50 {
        iter_lines[iter_lines.len() - 50..].join("\n")
    } else {
        iter_lines.join("\n")
    };

    // First 500 words of the PRD.
    let prd_snippet = match &data.prd_content {
        Some(prd) => {
            let words: Vec<&str> = prd.split_whitespace().collect();
            if words.len() > 500 {
                words[..500].join(" ")
            } else {
                prd.clone()
            }
        }
        None => "(none)".to_string(),
    };

    // Git diff summary.
    let git_diff = data
        .git_diff_stat
        .as_deref()
        .unwrap_or("(none)")
        .to_string();

    // Rule-based findings as bullet points.
    let existing_findings_text = if rule_based_findings.is_empty() {
        "(none)".to_string()
    } else {
        rule_based_findings
            .iter()
            .map(|f| {
                let desc = match &f.action {
                    FindingAction::PromptHint { text } => text.clone(),
                    FindingAction::ConfigTuning {
                        field,
                        current_value,
                        new_value,
                    } => format!("Tune {field}: {current_value} -> {new_value}"),
                    FindingAction::BehavioralRule { action, .. } => action.clone(),
                };
                format!("- {desc}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "[FEEDBACK_ANALYSIS]
Analyze this orchestrator run and identify qualitative issues that
wouldn't be caught by quantitative rules.

RUN METRICS:
iterations: {iterations}, reverts: {reverts}, stuck: {stuck}, duration: {duration}min

ITERATION LOG (last 50 lines):
{iter_snippet}

PRD SUMMARY:
{prd_snippet}

GIT DIFF SUMMARY:
{git_diff}

RULE-BASED FINDINGS ALREADY DETECTED:
{existing_findings_text}

Respond in this exact format:
FINDING: <description>
SCOPE: project|global
SEVERITY: high|medium|low
---
(repeat for each finding, max 5)",
        iterations = data.iterations,
        reverts = data.revert_count,
        stuck = data.stuck_count,
        duration = duration_mins,
        iter_snippet = iter_snippet,
        prd_snippet = prd_snippet,
        git_diff = git_diff,
        existing_findings_text = existing_findings_text,
    )
}

// ---------------------------------------------------------------------------
// Response parser
// ---------------------------------------------------------------------------

/// Parse an LLM response in the structured block format into `Finding`s.
///
/// Rules:
/// * Blocks are separated by `---`.
/// * Each block must have a `FINDING:` line; `SCOPE:` and `SEVERITY:` are
///   optional and default to `Project` / `Medium` when missing or unrecognised.
/// * Malformed blocks (no `FINDING:` line) are silently skipped.
/// * At most 5 findings are returned.
pub fn parse_llm_response(response: &str) -> Vec<Finding> {
    let mut findings = Vec::new();

    for block in response.split("---") {
        if findings.len() >= 5 {
            break;
        }

        let block = block.trim();
        if block.is_empty() {
            continue;
        }

        let mut description: Option<String> = None;
        let mut scope = Scope::Project;
        let mut severity = Severity::Medium;

        for line in block.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("FINDING:") {
                description = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("SCOPE:") {
                scope = match rest.trim().to_lowercase().as_str() {
                    "global" => Scope::Global,
                    _ => Scope::Project,
                };
            } else if let Some(rest) = line.strip_prefix("SEVERITY:") {
                severity = match rest.trim().to_lowercase().as_str() {
                    "high" => Severity::High,
                    "low" => Severity::Low,
                    _ => Severity::Medium,
                };
            }
        }

        let text = match description {
            Some(t) if !t.is_empty() => t,
            _ => continue, // malformed — no FINDING line
        };

        findings.push(Finding {
            id: format!("llm-{}", findings.len() + 1),
            category: FindingCategory::PromptHint,
            severity,
            action: FindingAction::PromptHint { text: text.clone() },
            evidence: text,
            scope,
        });
    }

    findings
}

// ---------------------------------------------------------------------------
// Deduplication
// ---------------------------------------------------------------------------

/// Remove findings whose text already appears (as a substring) in any
/// existing hint rule's `action_params["text"]`, then truncate to the
/// remaining capacity (`max_hints - existing_hints.len()`).
pub fn dedup_findings(
    new: Vec<Finding>,
    existing_hints: &[Rule],
    max_hints: usize,
) -> Vec<Finding> {
    let capacity = max_hints.saturating_sub(existing_hints.len());

    new.into_iter()
        .filter(|f| {
            let text = match &f.action {
                FindingAction::PromptHint { text } => text.as_str(),
                _ => return true, // non-hint findings are not deduplicated here
            };
            // Keep the finding only if its text does NOT appear in any existing hint.
            !existing_hints.iter().any(|rule| {
                rule.action_params
                    .get("text")
                    .map(|t| t.contains(text))
                    .unwrap_or(false)
            })
        })
        .take(capacity)
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::types::{AblationResult, Rule, RuleStatus};

    // Helper: build a minimal RunData with the given iteration count / revert count.
    fn make_run_data(iterations: u32, revert_count: u32) -> RunData {
        RunData {
            iterations,
            revert_count,
            ..RunData::default()
        }
    }

    // Helper: build a Rule with a given text in action_params.
    fn hint_rule(text: &str) -> Rule {
        let mut params = HashMap::new();
        params.insert("text".to_string(), text.to_string());
        Rule {
            id: "r1".to_string(),
            trigger: "manual".to_string(),
            trigger_params: HashMap::new(),
            action: "prompt_hint".to_string(),
            action_params: params,
            status: RuleStatus::Confirmed,
            severity: Severity::Low,
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
            last_ablation_run: String::new(),
            ablation_result: AblationResult::Untested,
        }
    }

    // -----------------------------------------------------------------------
    // build_analysis_prompt
    // -----------------------------------------------------------------------

    #[test]
    fn build_prompt_includes_metrics() {
        let data = make_run_data(42, 7);
        let prompt = build_analysis_prompt(&data, &[]);
        assert!(
            prompt.contains("iterations: 42"),
            "prompt missing iteration count"
        );
        assert!(prompt.contains("reverts: 7"), "prompt missing revert count");
    }

    #[test]
    fn build_prompt_truncates_iterations() {
        let lines: Vec<String> = (1..=100).map(|i| format!("line{i}")).collect();
        let data = RunData {
            iterations_tsv: lines.join("\n"),
            ..RunData::default()
        };
        let prompt = build_analysis_prompt(&data, &[]);
        // The first line of the 100 should NOT appear in the snippet.
        assert!(
            !prompt.contains("line1\n"),
            "first line of 100 should be truncated away"
        );
        // The last line should appear.
        assert!(
            prompt.contains("line100"),
            "last line should be present in truncated snippet"
        );
    }

    #[test]
    fn build_prompt_truncates_prd() {
        // Build a PRD with 1000 words.
        let prd: String = (1..=1000)
            .map(|i| format!("word{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let data = RunData {
            prd_content: Some(prd),
            ..RunData::default()
        };
        let prompt = build_analysis_prompt(&data, &[]);
        // word501 is the 501st word — it should NOT appear in the PRD snippet.
        assert!(
            !prompt.contains("word501"),
            "PRD should be truncated to 500 words"
        );
        // word500 should appear.
        assert!(
            prompt.contains("word500"),
            "word500 should appear in truncated PRD"
        );
    }

    #[test]
    fn build_prompt_includes_existing_findings() {
        let data = make_run_data(5, 1);
        let finding = Finding {
            id: "f1".to_string(),
            category: FindingCategory::PromptHint,
            severity: Severity::Medium,
            action: FindingAction::PromptHint {
                text: "Avoid large commits".to_string(),
            },
            evidence: "Avoid large commits".to_string(),
            scope: Scope::Project,
        };
        let prompt = build_analysis_prompt(&data, &[finding]);
        assert!(
            prompt.contains("Avoid large commits"),
            "existing finding text should appear in prompt"
        );
    }

    // -----------------------------------------------------------------------
    // parse_llm_response
    // -----------------------------------------------------------------------

    #[test]
    fn parse_valid_response() {
        let response = "FINDING: Prefer atomic commits\nSCOPE: project\nSEVERITY: high\n---";
        let findings = parse_llm_response(response);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].severity, Severity::High));
        assert!(matches!(findings[0].scope, Scope::Project));
        if let FindingAction::PromptHint { text } = &findings[0].action {
            assert_eq!(text, "Prefer atomic commits");
        } else {
            panic!("expected PromptHint action");
        }
    }

    #[test]
    fn parse_partial_response() {
        // Missing SCOPE — should default to Project.
        let response = "FINDING: Check edge cases\nSEVERITY: low\n---";
        let findings = parse_llm_response(response);
        assert_eq!(findings.len(), 1);
        assert!(matches!(findings[0].scope, Scope::Project));
        assert!(matches!(findings[0].severity, Severity::Low));
    }

    #[test]
    fn parse_empty_response() {
        let findings = parse_llm_response("");
        assert!(findings.is_empty());
    }

    #[test]
    fn parse_malformed_blocks_skipped() {
        let response = concat!(
            "this block has no FINDING line\n---\n",
            "FINDING: Valid finding\nSCOPE: global\nSEVERITY: medium\n---"
        );
        let findings = parse_llm_response(response);
        // Only the valid block should produce a finding.
        assert_eq!(findings.len(), 1);
        if let FindingAction::PromptHint { text } = &findings[0].action {
            assert_eq!(text, "Valid finding");
        } else {
            panic!("expected PromptHint");
        }
    }

    #[test]
    fn parse_max_5_findings() {
        // Build a response with 8 blocks.
        let block = "FINDING: Finding number X\nSCOPE: project\nSEVERITY: low\n---\n";
        let response = block.repeat(8);
        let findings = parse_llm_response(&response);
        assert_eq!(findings.len(), 5, "should cap at 5 findings");
    }

    // -----------------------------------------------------------------------
    // dedup_findings
    // -----------------------------------------------------------------------

    #[test]
    fn dedup_removes_existing() {
        let new = vec![Finding {
            id: "llm-1".to_string(),
            category: FindingCategory::PromptHint,
            severity: Severity::Medium,
            action: FindingAction::PromptHint {
                text: "Prefer atomic commits".to_string(),
            },
            evidence: "Prefer atomic commits".to_string(),
            scope: Scope::Project,
        }];
        // existing_hints already contains this text.
        let existing = vec![hint_rule("Prefer atomic commits")];
        let deduped = dedup_findings(new, &existing, 10);
        assert!(
            deduped.is_empty(),
            "duplicate finding should be removed by dedup"
        );
    }

    #[test]
    fn dedup_respects_capacity() {
        // 8 existing hints, max 10 => capacity of 2.
        let existing: Vec<Rule> = (0..8)
            .map(|i| hint_rule(&format!("existing hint {i}")))
            .collect();

        let new: Vec<Finding> = (0..5)
            .map(|i| Finding {
                id: format!("llm-{i}"),
                category: FindingCategory::PromptHint,
                severity: Severity::Low,
                action: FindingAction::PromptHint {
                    text: format!("new finding {i}"),
                },
                evidence: format!("new finding {i}"),
                scope: Scope::Project,
            })
            .collect();

        let deduped = dedup_findings(new, &existing, 10);
        assert_eq!(deduped.len(), 2, "only 2 slots remain (10 - 8 = 2)");
    }
}
