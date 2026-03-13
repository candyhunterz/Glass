//! SOI compression engine: transforms stored output records into
//! token-budgeted summaries at four granularity levels.
//!
//! Downstream consumers (Phase 52 display, Phase 53 MCP tools, Phase 55
//! activity stream) call `compress()` to get a `CompressedOutput` at their
//! desired `TokenBudget` rather than receiving a raw record dump.

use serde::Serialize;

use crate::soi::{CommandOutputSummaryRow, OutputRecordRow};

// ────────────────────────────────────────────────────────────────────────────
// Public types
// ────────────────────────────────────────────────────────────────────────────

/// Granularity levels for compressed output, each with a token limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TokenBudget {
    /// Absolute minimum: at most ~10 tokens.
    OneLine,
    /// Short summary: at most ~100 tokens.
    Summary,
    /// Detailed view: at most ~500 tokens (errors before warnings).
    Detailed,
    /// No limit — returns every record without truncation.
    Full,
}

impl TokenBudget {
    /// Return the approximate token ceiling for this budget level.
    pub fn token_limit(self) -> usize {
        match self {
            TokenBudget::OneLine => 10,
            TokenBudget::Summary => 100,
            TokenBudget::Detailed => 500,
            TokenBudget::Full => usize::MAX,
        }
    }
}

/// A compressed representation of command output.
#[derive(Debug, Clone, Serialize)]
pub struct CompressedOutput {
    /// The budget level used to produce this output.
    pub budget: TokenBudget,
    /// The compressed text ready for display or LLM consumption.
    pub text: String,
    /// DB row IDs of the `output_records` rows that were included in `text`.
    /// Empty for `OneLine` and `Full` budgets.
    pub record_ids: Vec<i64>,
    /// Approximate token count of `text`.
    pub token_count: usize,
    /// True if some records were excluded due to the token ceiling.
    pub truncated: bool,
}

// ────────────────────────────────────────────────────────────────────────────
// Public entry point
// ────────────────────────────────────────────────────────────────────────────

/// Compress a set of output records into a token-budgeted summary.
///
/// # Arguments
/// * `records`  – The `output_records` rows for a single command.
/// * `summary`  – The `command_output_records` summary row for that command.
/// * `budget`   – Desired detail level.
pub fn compress(
    records: &[OutputRecordRow],
    summary: &CommandOutputSummaryRow,
    budget: TokenBudget,
) -> CompressedOutput {
    match budget {
        TokenBudget::OneLine => compress_one_line(records, summary),
        TokenBudget::Full => compress_full(records),
        TokenBudget::Summary | TokenBudget::Detailed => {
            compress_greedy(records, summary, budget)
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Budget-specific implementations
// ────────────────────────────────────────────────────────────────────────────

fn compress_one_line(
    records: &[OutputRecordRow],
    summary: &CommandOutputSummaryRow,
) -> CompressedOutput {
    let error_count = records
        .iter()
        .filter(|r| r.severity.as_deref() == Some("Error"))
        .count();

    let text = if error_count > 0 {
        // Find the first Error record (smallest id)
        let first_error = records
            .iter()
            .filter(|r| r.severity.as_deref() == Some("Error"))
            .min_by_key(|r| r.id);

        match first_error.and_then(|r| r.file_path.as_deref()) {
            Some(fp) => format!(
                "{} error{} in {}",
                error_count,
                if error_count == 1 { "" } else { "s" },
                fp
            ),
            None => format!(
                "{} error{}",
                error_count,
                if error_count == 1 { "" } else { "s" }
            ),
        }
    } else {
        summary.one_line.clone()
    };

    let token_count = estimate_tokens(&text);
    CompressedOutput {
        budget: TokenBudget::OneLine,
        text,
        record_ids: Vec::new(),
        token_count,
        truncated: false,
    }
}

fn compress_full(records: &[OutputRecordRow]) -> CompressedOutput {
    let mut lines = Vec::with_capacity(records.len());
    let mut record_ids = Vec::with_capacity(records.len());

    for row in records {
        lines.push(format_record(row));
        record_ids.push(row.id);
    }

    let text = lines.join("\n");
    let token_count = estimate_tokens(&text);

    CompressedOutput {
        budget: TokenBudget::Full,
        text,
        record_ids,
        token_count,
        truncated: false,
    }
}

fn compress_greedy(
    records: &[OutputRecordRow],
    summary: &CommandOutputSummaryRow,
    budget: TokenBudget,
) -> CompressedOutput {
    if records.is_empty() {
        let text = summary.one_line.clone();
        let token_count = estimate_tokens(&text);
        return CompressedOutput {
            budget,
            text,
            record_ids: Vec::new(),
            token_count,
            truncated: false,
        };
    }

    let limit = budget.token_limit();

    // Sort by severity rank ASC, then by id ASC (stable sort preserves order
    // among equal-rank records).
    let mut sorted: Vec<&OutputRecordRow> = records.iter().collect();
    sorted.sort_by_key(|r| (severity_rank(r.severity.as_deref()), r.id));

    let mut lines: Vec<String> = Vec::new();
    let mut record_ids: Vec<i64> = Vec::new();
    let mut token_count = 0usize;
    let mut excluded = 0usize;

    for row in &sorted {
        let line = format_record(row);
        let line_tokens = estimate_tokens(&line);
        if token_count + line_tokens > limit {
            excluded += 1;
        } else {
            token_count += line_tokens;
            record_ids.push(row.id);
            lines.push(line);
        }
    }

    let text = if lines.is_empty() {
        summary.one_line.clone()
    } else {
        lines.join("\n")
    };

    CompressedOutput {
        budget,
        text,
        record_ids,
        token_count,
        truncated: excluded > 0,
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Helper functions
// ────────────────────────────────────────────────────────────────────────────

/// Map severity string to a numeric rank for priority ordering (lower = higher priority).
pub fn severity_rank(severity: Option<&str>) -> u8 {
    match severity {
        Some("Error") => 0,
        Some("Warning") => 1,
        Some("Info") => 2,
        Some("Success") => 3,
        _ => 4,
    }
}

/// Produce a human-readable one-liner from an `OutputRecordRow`.
///
/// Parses `row.data` as JSON and extracts a message field according to
/// `record_type`. Falls back gracefully if JSON parsing fails.
pub fn format_record(row: &OutputRecordRow) -> String {
    let file_prefix = row
        .file_path
        .as_deref()
        .map(|f| format!("{}: ", f))
        .unwrap_or_default();

    let severity_prefix = row
        .severity
        .as_deref()
        .map(|s| format!("[{}] ", s))
        .unwrap_or_default();

    let message = extract_message(&row.record_type, &row.data);

    format!(
        "{}{}{}: {}",
        severity_prefix, file_prefix, row.record_type, message
    )
}

fn extract_message(record_type: &str, data: &str) -> String {
    let v: serde_json::Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) => return format!("{} (details unavailable)", record_type),
    };

    let msg = match record_type {
        "CompilerError" | "GenericDiagnostic" => v
            .get("message")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string()),
        "TestResult" => v
            .get("name")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string()),
        "PackageEvent" => v
            .get("package")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string()),
        "FreeformChunk" => v.get("text").and_then(|m| m.as_str()).map(|s| {
            let truncated = &s[..s.len().min(80)];
            truncated.to_string()
        }),
        _ => v
            .get("message")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string()),
    };

    msg.unwrap_or_else(|| format!("{} (details unavailable)", record_type))
}

/// Estimate token count as whitespace-separated word count.
pub fn estimate_tokens(text: &str) -> usize {
    text.split_whitespace().count()
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_summary(one_line: &str) -> CommandOutputSummaryRow {
        CommandOutputSummaryRow {
            id: 1,
            command_id: 42,
            output_type: "RustCompiler".to_string(),
            severity: "Error".to_string(),
            one_line: one_line.to_string(),
            token_estimate: 10,
            raw_line_count: 50,
            raw_byte_count: 1024,
        }
    }

    fn make_record(
        id: i64,
        record_type: &str,
        severity: Option<&str>,
        file_path: Option<&str>,
        message: &str,
    ) -> OutputRecordRow {
        let data = serde_json::json!({ "message": message }).to_string();
        OutputRecordRow {
            id,
            command_id: 42,
            record_type: record_type.to_string(),
            severity: severity.map(|s| s.to_string()),
            file_path: file_path.map(|f| f.to_string()),
            data,
        }
    }

    // ── OneLine tests ────────────────────────────────────────────────────────

    #[test]
    fn compress_one_line_failed_build() {
        // Two Error records in src/main.rs and src/lib.rs
        let records = vec![
            make_record(1, "CompilerError", Some("Error"), Some("src/main.rs"), "mismatched types"),
            make_record(2, "CompilerError", Some("Error"), Some("src/lib.rs"), "unused import"),
        ];
        let summary = make_summary("2 errors");

        let out = compress(&records, &summary, TokenBudget::OneLine);
        assert!(
            out.token_count <= 10,
            "OneLine token count {} should be <= 10",
            out.token_count
        );
        // Should mention the error count and first error file
        assert!(
            out.text.contains("2 error"),
            "Expected error count in text, got: {}",
            out.text
        );
        assert!(
            out.text.contains("src/main.rs"),
            "Expected first error file in text, got: {}",
            out.text
        );
    }

    #[test]
    fn compress_one_line_no_errors() {
        // No Error-severity records -- fall back to summary.one_line
        let records = vec![make_record(
            1,
            "CompilerError",
            Some("Warning"),
            Some("src/main.rs"),
            "unused variable",
        )];
        let summary = make_summary("1 warning");

        let out = compress(&records, &summary, TokenBudget::OneLine);
        assert_eq!(out.text, "1 warning", "Expected fallback to summary.one_line");
    }

    // ── Summary budget ───────────────────────────────────────────────────────

    #[test]
    fn compress_summary_budget() {
        // Build records that would exceed 100 tokens if all included
        let mut records = Vec::new();
        for i in 0..30 {
            records.push(make_record(
                i,
                "CompilerError",
                Some("Error"),
                Some("src/main.rs"),
                &format!("error number {} with a longer message to pad tokens", i),
            ));
        }
        let summary = make_summary("30 errors");

        let out = compress(&records, &summary, TokenBudget::Summary);
        assert!(
            out.token_count <= 100,
            "Summary token_count {} should be <= 100",
            out.token_count
        );
    }

    // ── Detailed budget ──────────────────────────────────────────────────────

    #[test]
    fn compress_detailed_budget() {
        let mut records = Vec::new();
        for i in 0..100 {
            records.push(make_record(
                i,
                "CompilerError",
                Some("Error"),
                Some("src/main.rs"),
                &format!("error {}", i),
            ));
        }
        let summary = make_summary("100 errors");

        let out = compress(&records, &summary, TokenBudget::Detailed);
        assert!(
            out.token_count <= 500,
            "Detailed token_count {} should be <= 500",
            out.token_count
        );
    }

    // ── Full budget ──────────────────────────────────────────────────────────

    #[test]
    fn compress_full_budget_no_truncation() {
        let records = vec![
            make_record(1, "CompilerError", Some("Error"), Some("src/a.rs"), "error a"),
            make_record(2, "CompilerError", Some("Warning"), Some("src/b.rs"), "warn b"),
            make_record(3, "CompilerError", Some("Info"), None, "info c"),
        ];
        let summary = make_summary("mixed");

        let out = compress(&records, &summary, TokenBudget::Full);
        assert!(!out.truncated, "Full budget should never truncate");
        assert_eq!(out.record_ids.len(), 3, "Full budget should include all 3 records");
    }

    // ── Ordering / priority ──────────────────────────────────────────────────

    #[test]
    fn compress_errors_before_warnings() {
        // Mix Warning (id=1) and Error (id=2) -- Error should come first in output
        let records = vec![
            make_record(1, "CompilerError", Some("Warning"), Some("src/main.rs"), "unused"),
            make_record(2, "CompilerError", Some("Error"), Some("src/main.rs"), "mismatched types"),
        ];
        let summary = make_summary("1 error, 1 warning");

        // Use Summary budget so greedy path is exercised
        let out = compress(&records, &summary, TokenBudget::Summary);

        // The first entry in record_ids should be the Error record (id=2)
        assert!(
            out.record_ids.first() == Some(&2),
            "Expected Error record (id=2) first, got record_ids: {:?}",
            out.record_ids
        );
    }

    // ── Drill-down record IDs ─────────────────────────────────────────────────

    #[test]
    fn compress_drill_down_record_ids() {
        let records = vec![
            make_record(10, "CompilerError", Some("Error"), Some("src/main.rs"), "err1"),
            make_record(20, "CompilerError", Some("Warning"), Some("src/lib.rs"), "warn1"),
        ];
        let summary = make_summary("1 error, 1 warning");

        let out_summary = compress(&records, &summary, TokenBudget::Summary);
        assert!(
            !out_summary.record_ids.is_empty(),
            "Summary budget should populate record_ids"
        );

        let out_detailed = compress(&records, &summary, TokenBudget::Detailed);
        assert!(
            !out_detailed.record_ids.is_empty(),
            "Detailed budget should populate record_ids"
        );
    }

    // ── Empty records ─────────────────────────────────────────────────────────

    #[test]
    fn compress_empty_records() {
        let records = vec![];
        let summary = make_summary("no output");

        let out = compress(&records, &summary, TokenBudget::Summary);
        assert_eq!(out.text, "no output", "Expected summary.one_line fallback");
        assert!(out.record_ids.is_empty(), "No record_ids for empty input");
    }
}
