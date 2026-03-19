//! LLM-based quality verification for general (non-code) projects.
//!
//! At checkpoint boundaries, evaluates deliverable content against PRD
//! requirements by sending both to an ephemeral claude session.

/// Quality verdict from the LLM evaluator.
#[derive(Debug, Clone, PartialEq)]
pub struct QualityVerdict {
    pub score: u32,
    pub completeness: f64,
    pub gaps: Vec<String>,
    pub regressed: bool,
}

/// Strip markdown code fences from LLM response text.
fn strip_markdown_fences(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim().to_string();
        }
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim().to_string();
        }
    }
    trimmed.to_string()
}

/// Parse a quality verdict from LLM JSON response.
/// Strips markdown fences, handles missing fields with defaults,
/// and clamps score to 1-10 range.
pub fn parse_quality_verdict(json_text: &str) -> Result<QualityVerdict, String> {
    let cleaned = strip_markdown_fences(json_text);
    let val: serde_json::Value =
        serde_json::from_str(&cleaned).map_err(|e| format!("invalid JSON: {e}"))?;

    let score = val
        .get("score")
        .and_then(|v| v.as_u64())
        .unwrap_or(5)
        .clamp(1, 10) as u32;
    let completeness = val
        .get("completeness")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let gaps = val
        .get("gaps")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let regressed = val
        .get("regressed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    Ok(QualityVerdict {
        score,
        completeness,
        gaps,
        regressed,
    })
}

/// Build the system prompt for quality verification.
pub fn quality_system_prompt() -> String {
    r#"You are a quality verifier for a document-based project. Evaluate the deliverable against the requirements. Respond with JSON only, no explanation outside the JSON block.

JSON schema:
{
  "score": <1-10 quality score>,
  "completeness": <0.0-1.0 fraction of requirements addressed>,
  "gaps": [<string descriptions of unmet requirements>],
  "regressed": <true if quality decreased from previous score>
}"#
    .to_string()
}

/// Build the user message for quality verification.
pub fn quality_user_message(
    deliverable_content: &str,
    prd_requirements: &str,
    previous_score: Option<u32>,
) -> String {
    let prev = previous_score
        .map(|s| format!("Previous quality score: {s}/10\n\n"))
        .unwrap_or_default();
    format!("{prev}## Requirements\n\n{prd_requirements}\n\n## Deliverable Content\n\n{deliverable_content}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_verdict() {
        let json =
            r#"{"score": 7, "completeness": 0.6, "gaps": ["missing budget"], "regressed": false}"#;
        let v = parse_quality_verdict(json).unwrap();
        assert_eq!(v.score, 7);
        assert!((v.completeness - 0.6).abs() < 1e-6);
        assert_eq!(v.gaps, vec!["missing budget"]);
        assert!(!v.regressed);
    }

    #[test]
    fn parse_missing_fields_uses_defaults() {
        let json = r#"{}"#;
        let v = parse_quality_verdict(json).unwrap();
        assert_eq!(v.score, 5);
        assert_eq!(v.completeness, 0.0);
        assert!(v.gaps.is_empty());
        assert!(!v.regressed);
    }

    #[test]
    fn parse_invalid_json_returns_error() {
        assert!(parse_quality_verdict("not json").is_err());
    }

    #[test]
    fn parse_regressed_true() {
        let json = r#"{"score": 3, "regressed": true, "gaps": ["lost content"]}"#;
        let v = parse_quality_verdict(json).unwrap();
        assert!(v.regressed);
        assert_eq!(v.score, 3);
    }

    #[test]
    fn parse_markdown_fenced_json() {
        let json = "```json\n{\"score\": 8, \"completeness\": 0.9, \"gaps\": [], \"regressed\": false}\n```";
        let v = parse_quality_verdict(json).unwrap();
        assert_eq!(v.score, 8);
    }

    #[test]
    fn parse_score_clamped_to_range() {
        let json = r#"{"score": 100}"#;
        let v = parse_quality_verdict(json).unwrap();
        assert_eq!(v.score, 10);
    }

    #[test]
    fn parse_score_clamped_minimum() {
        let json = r#"{"score": 0}"#;
        let v = parse_quality_verdict(json).unwrap();
        assert_eq!(v.score, 1);
    }

    #[test]
    fn system_prompt_contains_schema() {
        let prompt = quality_system_prompt();
        assert!(prompt.contains("score"));
        assert!(prompt.contains("completeness"));
        assert!(prompt.contains("gaps"));
        assert!(prompt.contains("regressed"));
    }

    #[test]
    fn user_message_includes_previous_score() {
        let msg = quality_user_message("content", "requirements", Some(7));
        assert!(msg.contains("Previous quality score: 7/10"));
    }

    #[test]
    fn user_message_no_previous_score() {
        let msg = quality_user_message("content", "requirements", None);
        assert!(!msg.contains("Previous quality score"));
    }
}
