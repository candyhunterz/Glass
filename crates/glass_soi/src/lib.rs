//! Glass Structured Output Intelligence (SOI).
//!
//! Classifies and parses command output into structured, token-efficient records
//! that AI agents can query through MCP tools.
//!
//! # Usage
//!
//! ```rust
//! use glass_soi::{classify, parse, OutputType};
//!
//! let output_type = classify("cargo test output here", Some("cargo test"));
//! assert_eq!(output_type, OutputType::RustTest);
//!
//! let parsed = parse("cargo test output here", output_type, Some("cargo test"));
//! println!("{}", parsed.summary.one_line);
//! ```

mod ansi;
mod classifier;
mod types;

// Stub parser modules — implemented in plans 48-02 and 48-03
mod cargo_build;
mod cargo_test;
mod jest;
mod npm;
mod pytest;

pub use ansi::strip_ansi;
pub use classifier::classify;
pub use types::{
    OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity, TestStatus,
};

/// Dispatch parsed output to the appropriate parser based on `output_type`.
///
/// For Phase 48, all parsers are stubs that return a freeform fallback.
/// Plans 48-02 and 48-03 will implement the full parsers.
pub fn parse(output: &str, output_type: OutputType, command_hint: Option<&str>) -> ParsedOutput {
    match output_type {
        OutputType::RustCompiler => cargo_build::parse(output, command_hint),
        OutputType::RustTest => cargo_test::parse(output),
        OutputType::Npm => npm::parse(output),
        OutputType::Pytest => pytest::parse(output),
        OutputType::Jest => jest::parse(output),
        other => freeform_parse(output, Some(other), command_hint),
    }
}

/// Produce a minimal `ParsedOutput` wrapping the entire output as a single
/// `FreeformChunk`. Used as the fallback for unrecognized output types.
pub(crate) fn freeform_parse(
    output: &str,
    output_type: Option<OutputType>,
    _command_hint: Option<&str>,
) -> ParsedOutput {
    let raw_line_count = output.lines().count();
    let raw_byte_count = output.len();
    let one_line = format!("{} lines of unstructured output", raw_line_count);
    let token_estimate = one_line.split_whitespace().count() + 2; // rough heuristic

    ParsedOutput {
        output_type: output_type.unwrap_or(OutputType::FreeformText),
        summary: OutputSummary {
            one_line,
            token_estimate,
            severity: Severity::Info,
        },
        records: vec![OutputRecord::FreeformChunk {
            text: output.to_string(),
            line_count: raw_line_count,
        }],
        raw_line_count,
        raw_byte_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rust_compiler_dispatches() {
        let output = r#"{"reason":"compiler-message"}"#;
        let parsed = parse(output, OutputType::RustCompiler, Some("cargo build"));
        // Stub: returns freeform with RustCompiler type
        assert_eq!(parsed.output_type, OutputType::RustCompiler);
    }

    #[test]
    fn parse_freeform_fallback_for_unrecognized() {
        let output = "some unknown output\nline 2\n";
        let parsed = parse(output, OutputType::FreeformText, None);
        assert_eq!(parsed.output_type, OutputType::FreeformText);
        assert_eq!(parsed.raw_line_count, 2);
        assert!(matches!(parsed.records[0], OutputRecord::FreeformChunk { .. }));
    }

    #[test]
    fn parse_git_fallback() {
        let output = "On branch main\nnothing to commit\n";
        let parsed = parse(output, OutputType::Git, Some("git status"));
        assert_eq!(parsed.output_type, OutputType::Git);
    }

    #[test]
    fn freeform_parse_counts_correctly() {
        let output = "line1\nline2\nline3";
        let parsed = freeform_parse(output, None, None);
        assert_eq!(parsed.raw_line_count, 3);
        assert_eq!(parsed.raw_byte_count, output.len());
        assert_eq!(parsed.output_type, OutputType::FreeformText);
    }
}
