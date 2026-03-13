//! Parser for `cargo build` / `cargo check` / `cargo clippy` output.
//!
//! Delegates to `glass_errors::extract_errors` for error extraction and maps
//! the results to SOI `OutputRecord::CompilerError` variants.

use crate::types::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity};

/// Map `glass_errors::Severity` to SOI `Severity`.
///
/// `Note` and `Help` map to `Info` since SOI uses an outcome-oriented scale.
fn map_severity(s: &glass_errors::Severity) -> Severity {
    match s {
        glass_errors::Severity::Error => Severity::Error,
        glass_errors::Severity::Warning => Severity::Warning,
        glass_errors::Severity::Note | glass_errors::Severity::Help => Severity::Info,
    }
}

/// Parse Rust compiler output (cargo build / cargo check / cargo clippy) into structured records.
///
/// Delegates error extraction to `glass_errors::extract_errors` and maps the results
/// into `OutputRecord::CompilerError` entries.
pub fn parse(output: &str, command_hint: Option<&str>) -> ParsedOutput {
    let errors = glass_errors::extract_errors(output, command_hint);

    let records: Vec<OutputRecord> = errors
        .iter()
        .map(|e| OutputRecord::CompilerError {
            file: e.file.clone(),
            line: e.line,
            column: e.column,
            severity: map_severity(&e.severity),
            code: e.code.clone(),
            message: e.message.clone(),
            context_lines: None,
        })
        .collect();

    let error_count = records
        .iter()
        .filter(|r| {
            matches!(
                r,
                OutputRecord::CompilerError {
                    severity: Severity::Error,
                    ..
                }
            )
        })
        .count();

    let warning_count = records
        .iter()
        .filter(|r| {
            matches!(
                r,
                OutputRecord::CompilerError {
                    severity: Severity::Warning,
                    ..
                }
            )
        })
        .count();

    // Find first error file for the summary line
    let first_error_file: Option<&str> = records.iter().find_map(|r| {
        if let OutputRecord::CompilerError {
            file,
            severity: Severity::Error,
            ..
        } = r
        {
            Some(file.as_str())
        } else {
            None
        }
    });

    let one_line = if error_count > 0 {
        match first_error_file {
            Some(f) => format!(
                "{} error{}, {} warning{} in {}",
                error_count,
                if error_count == 1 { "" } else { "s" },
                warning_count,
                if warning_count == 1 { "" } else { "s" },
                f
            ),
            None => format!(
                "{} error{}, {} warning{}",
                error_count,
                if error_count == 1 { "" } else { "s" },
                warning_count,
                if warning_count == 1 { "" } else { "s" }
            ),
        }
    } else if warning_count > 0 {
        format!(
            "{} warning{}",
            warning_count,
            if warning_count == 1 { "" } else { "s" }
        )
    } else {
        "build succeeded".to_string()
    };

    let severity = if error_count > 0 {
        Severity::Error
    } else if warning_count > 0 {
        Severity::Warning
    } else {
        Severity::Success
    };

    let token_estimate = 5 + records.len() * 10;

    ParsedOutput {
        output_type: OutputType::RustCompiler,
        summary: OutputSummary {
            one_line,
            token_estimate,
            severity,
        },
        records,
        raw_line_count: output.lines().count(),
        raw_byte_count: output.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUST_JSON_ERROR: &str = r#"{"reason":"compiler-message","package_id":"test","manifest_path":"Cargo.toml","message":{"message":"mismatched types","code":{"code":"E0308","explanation":null},"level":"error","spans":[{"file_name":"src/main.rs","byte_start":100,"byte_end":110,"line_start":10,"line_end":10,"column_start":5,"column_end":15,"is_primary":true,"text":[],"label":null}],"children":[],"rendered":"error[E0308]"}}"#;

    const RUST_HUMAN_ERROR: &str =
        "error[E0308]: mismatched types\n --> src/main.rs:10:5\n  |\n10 |     let x: u32 = \"hello\";\n  |                   ^^^^^^^";

    // Plain "warning:" (no code) with cargo hint forces RustHuman parser.
    // detect.rs requires "warning[" for content sniffing so we use command_hint.
    const RUST_HUMAN_WARNING: &str = "warning: unused variable `x`\n --> src/lib.rs:5:9";

    const CLEAN_BUILD: &str = "   Compiling glass_soi v0.1.0\n    Finished dev [unoptimized + debuginfo] target(s) in 1.23s";

    #[test]
    fn json_error_produces_compiler_error_record() {
        let parsed = parse(RUST_JSON_ERROR, Some("cargo build"));
        assert_eq!(parsed.output_type, OutputType::RustCompiler);
        assert_eq!(parsed.records.len(), 1);
        if let OutputRecord::CompilerError {
            file,
            line,
            column,
            severity,
            code,
            message,
            ..
        } = &parsed.records[0]
        {
            assert_eq!(file, "src/main.rs");
            assert_eq!(*line, 10);
            assert_eq!(*column, Some(5));
            assert_eq!(*severity, Severity::Error);
            assert_eq!(code.as_deref(), Some("E0308"));
            assert!(message.contains("mismatched"));
        } else {
            panic!("Expected CompilerError record");
        }
    }

    #[test]
    fn json_error_summary_reflects_counts() {
        let parsed = parse(RUST_JSON_ERROR, Some("cargo build"));
        assert_eq!(parsed.summary.severity, Severity::Error);
        assert!(
            parsed.summary.one_line.contains("1 error"),
            "one_line was: {}",
            parsed.summary.one_line
        );
        assert!(
            parsed.summary.one_line.contains("src/main.rs"),
            "one_line should name file: {}",
            parsed.summary.one_line
        );
    }

    #[test]
    fn human_readable_error_produces_compiler_error_record() {
        let parsed = parse(RUST_HUMAN_ERROR, None);
        assert_eq!(parsed.output_type, OutputType::RustCompiler);
        assert!(!parsed.records.is_empty());
        if let OutputRecord::CompilerError {
            file,
            line,
            severity,
            code,
            ..
        } = &parsed.records[0]
        {
            assert_eq!(file, "src/main.rs");
            assert_eq!(*line, 10);
            assert_eq!(*severity, Severity::Error);
            assert_eq!(code.as_deref(), Some("E0308"));
        } else {
            panic!("Expected CompilerError record");
        }
    }

    #[test]
    fn clean_build_produces_no_records_and_success_severity() {
        let parsed = parse(CLEAN_BUILD, Some("cargo build"));
        assert_eq!(parsed.output_type, OutputType::RustCompiler);
        assert_eq!(parsed.records.len(), 0);
        assert_eq!(parsed.summary.severity, Severity::Success);
        assert_eq!(parsed.summary.one_line, "build succeeded");
    }

    #[test]
    fn warning_only_output_produces_warning_severity() {
        // Use "cargo build" hint so detect.rs uses RustHuman (not Generic) parser
        let parsed = parse(RUST_HUMAN_WARNING, Some("cargo build"));
        assert_eq!(parsed.output_type, OutputType::RustCompiler);
        assert_eq!(parsed.summary.severity, Severity::Warning);
        assert!(
            parsed.summary.one_line.contains("warning"),
            "one_line was: {}",
            parsed.summary.one_line
        );
    }

    #[test]
    fn mixed_errors_and_warnings_counts_both() {
        // Combine error + warning output (with a blank line separator)
        // Use "cargo build" hint so detect.rs uses RustHuman for the warning line
        let combined = format!("{}\n\n{}", RUST_HUMAN_ERROR, RUST_HUMAN_WARNING);
        let parsed = parse(&combined, Some("cargo build"));
        // Should have Error severity (error beats warning)
        assert_eq!(parsed.summary.severity, Severity::Error);
        // Should have both records
        let errors: Vec<_> = parsed
            .records
            .iter()
            .filter(|r| {
                matches!(
                    r,
                    OutputRecord::CompilerError {
                        severity: Severity::Error,
                        ..
                    }
                )
            })
            .collect();
        let warnings: Vec<_> = parsed
            .records
            .iter()
            .filter(|r| {
                matches!(
                    r,
                    OutputRecord::CompilerError {
                        severity: Severity::Warning,
                        ..
                    }
                )
            })
            .collect();
        assert!(!errors.is_empty(), "Should have at least one error record");
        assert!(!warnings.is_empty(), "Should have at least one warning record");
    }

    #[test]
    fn empty_output_produces_success_with_no_records() {
        let parsed = parse("", Some("cargo build"));
        assert_eq!(parsed.records.len(), 0);
        assert_eq!(parsed.summary.severity, Severity::Success);
        assert_eq!(parsed.summary.one_line, "build succeeded");
    }

    #[test]
    fn raw_metrics_populated_correctly() {
        let output = "line1\nline2\nline3";
        let parsed = parse(output, None);
        assert_eq!(parsed.raw_line_count, 3);
        assert_eq!(parsed.raw_byte_count, output.len());
    }

    #[test]
    fn token_estimate_is_reasonable() {
        let parsed = parse(RUST_JSON_ERROR, Some("cargo build"));
        // 5 + 1 record * 10 = 15
        assert_eq!(parsed.summary.token_estimate, 15);
    }

    #[test]
    fn note_severity_maps_to_info() {
        // glass_errors Note -> SOI Info
        let note_sev = glass_errors::Severity::Note;
        assert_eq!(map_severity(&note_sev), Severity::Info);
        let help_sev = glass_errors::Severity::Help;
        assert_eq!(map_severity(&help_sev), Severity::Info);
    }
}
