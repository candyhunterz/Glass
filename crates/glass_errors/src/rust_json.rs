//! Rust JSON diagnostic parser (cargo + raw rustc).
//!
//! Handles two JSON formats:
//! - Cargo wrapper: `{"reason":"compiler-message","message":{...diagnostic...}}`
//! - Raw rustc: `{"$message_type":"diagnostic","message":"...","level":"error",...}`

use serde::Deserialize;

use crate::{Severity, StructuredError};

#[derive(Deserialize)]
struct CargoMessage {
    reason: String,
    message: Option<RustDiagnostic>,
}

#[derive(Deserialize)]
struct RustDiagnostic {
    message: String,
    level: String,
    code: Option<DiagCode>,
    #[serde(default)]
    spans: Vec<DiagSpan>,
}

#[derive(Deserialize)]
struct DiagCode {
    code: String,
}

#[derive(Deserialize)]
struct DiagSpan {
    file_name: String,
    line_start: u32,
    column_start: u32,
    is_primary: bool,
}

/// Map rustc level string to Severity.
fn map_level(level: &str) -> Severity {
    match level {
        "error" | "error: internal compiler error" => Severity::Error,
        "warning" => Severity::Warning,
        "note" => Severity::Note,
        "help" => Severity::Help,
        _ => Severity::Error,
    }
}

/// Collect a diagnostic into structured errors (only if it has a primary span).
fn collect_diagnostic(diag: &RustDiagnostic, errors: &mut Vec<StructuredError>) {
    let severity = map_level(&diag.level);
    if let Some(span) = diag.spans.iter().find(|s| s.is_primary) {
        errors.push(StructuredError {
            file: span.file_name.clone(),
            line: span.line_start,
            column: Some(span.column_start),
            severity,
            message: diag.message.clone(),
            code: diag.code.as_ref().map(|c| c.code.clone()),
        });
    }
}

/// Parse Rust JSON diagnostic output into structured errors.
///
/// Processes line-by-line, skipping non-JSON lines. Tries cargo wrapper format
/// first, then falls back to raw rustc diagnostic format.
pub(crate) fn parse_rust_json(output: &str) -> Vec<StructuredError> {
    let mut errors = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }

        // Try cargo wrapper format first
        if let Ok(cargo_msg) = serde_json::from_str::<CargoMessage>(line) {
            if cargo_msg.reason == "compiler-message" {
                if let Some(ref diag) = cargo_msg.message {
                    collect_diagnostic(diag, &mut errors);
                }
            }
            continue;
        }

        // Try raw rustc diagnostic
        if let Ok(diag) = serde_json::from_str::<RustDiagnostic>(line) {
            collect_diagnostic(&diag, &mut errors);
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Severity;

    #[test]
    fn parse_cargo_wrapper_format() {
        let output = r#"{"reason":"compiler-message","package_id":"glass 2.2.0","manifest_path":"Cargo.toml","message":{"message":"mismatched types","code":{"code":"E0308","explanation":null},"level":"error","spans":[{"file_name":"src/main.rs","byte_start":100,"byte_end":110,"line_start":10,"line_end":10,"column_start":5,"column_end":15,"is_primary":true,"text":[],"label":null}],"children":[],"rendered":"error[E0308]: mismatched types"}}"#;
        let errors = parse_rust_json(output);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file, "src/main.rs");
        assert_eq!(errors[0].line, 10);
        assert_eq!(errors[0].column, Some(5));
        assert_eq!(errors[0].severity, Severity::Error);
        assert_eq!(errors[0].message, "mismatched types");
        assert_eq!(errors[0].code, Some("E0308".to_string()));
    }

    #[test]
    fn parse_raw_rustc_format() {
        let output = r#"{"$message_type":"diagnostic","message":"unused variable: `x`","code":{"code":"E0599","explanation":null},"level":"warning","spans":[{"file_name":"src/lib.rs","byte_start":50,"byte_end":51,"line_start":3,"line_end":3,"column_start":9,"column_end":10,"is_primary":true,"text":[],"label":null}],"children":[],"rendered":"warning: unused"}"#;
        let errors = parse_rust_json(output);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file, "src/lib.rs");
        assert_eq!(errors[0].line, 3);
        assert_eq!(errors[0].column, Some(9));
        assert_eq!(errors[0].severity, Severity::Warning);
        assert_eq!(errors[0].message, "unused variable: `x`");
    }

    #[test]
    fn skip_non_diagnostic_cargo_lines() {
        let output = r#"{"reason":"compiler-artifact","package_id":"serde","target":{"name":"serde"}}
{"reason":"build-finished","success":true}"#;
        let errors = parse_rust_json(output);
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn skip_empty_spans() {
        let output = r#"{"reason":"compiler-message","message":{"message":"aborting due to 2 previous errors","code":null,"level":"error","spans":[],"children":[],"rendered":"error: aborting"}}"#;
        let errors = parse_rust_json(output);
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn extract_primary_span() {
        let output = r#"{"reason":"compiler-message","message":{"message":"type mismatch","code":{"code":"E0308","explanation":null},"level":"error","spans":[{"file_name":"src/other.rs","byte_start":0,"byte_end":10,"line_start":1,"line_end":1,"column_start":1,"column_end":10,"is_primary":false,"text":[],"label":"expected"},{"file_name":"src/main.rs","byte_start":100,"byte_end":110,"line_start":15,"line_end":15,"column_start":12,"column_end":22,"is_primary":true,"text":[],"label":"found"}],"children":[],"rendered":"error[E0308]"}}"#;
        let errors = parse_rust_json(output);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file, "src/main.rs");
        assert_eq!(errors[0].line, 15);
        assert_eq!(errors[0].column, Some(12));
    }

    #[test]
    fn map_severity_levels() {
        assert_eq!(map_level("error"), Severity::Error);
        assert_eq!(map_level("warning"), Severity::Warning);
        assert_eq!(map_level("note"), Severity::Note);
        assert_eq!(map_level("help"), Severity::Help);
        assert_eq!(
            map_level("error: internal compiler error"),
            Severity::Error
        );
    }

    #[test]
    fn skip_non_json_lines() {
        let output = "Compiling glass v2.2.0\nsome random text\n";
        let errors = parse_rust_json(output);
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn no_code_field() {
        let output = r#"{"reason":"compiler-message","message":{"message":"unused import","code":null,"level":"warning","spans":[{"file_name":"src/lib.rs","byte_start":0,"byte_end":10,"line_start":1,"line_end":1,"column_start":5,"column_end":15,"is_primary":true,"text":[],"label":null}],"children":[],"rendered":"warning: unused import"}}"#;
        let errors = parse_rust_json(output);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, None);
    }
}
