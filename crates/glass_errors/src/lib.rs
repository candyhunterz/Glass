//! Structured error extraction from compiler output.
//!
//! Provides [`extract_errors`] to parse raw command output into structured error records
//! with file, line, column, message, severity, and optional error code.

mod detect;
mod generic;
mod rust_human;
mod rust_json;

use serde::Serialize;

/// A single structured error extracted from command output.
#[derive(Debug, Clone, Serialize)]
pub struct StructuredError {
    pub file: String,
    pub line: u32,
    pub column: Option<u32>,
    pub severity: Severity,
    pub message: String,
    /// Optional error code (e.g., "E0308" for Rust).
    pub code: Option<String>,
}

/// Severity level of an extracted error.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
}

/// Internal parser kind for dispatch.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ParserKind {
    RustJson,
    RustHuman,
    Generic,
}

/// Extract structured errors from raw command output.
///
/// Automatically detects the output format based on `command_hint` (e.g., "cargo build")
/// and output content sniffing, then dispatches to the appropriate parser.
pub fn extract_errors(output: &str, command_hint: Option<&str>) -> Vec<StructuredError> {
    let kind = detect::detect_parser(output, command_hint);
    match kind {
        ParserKind::RustJson => rust_json::parse_rust_json(output),
        ParserKind::RustHuman => rust_human::parse_rust_human(output),
        ParserKind::Generic => generic::parse_generic(output),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_errors_generic_standard() {
        let output = "src/main.c:10:5: error: undeclared";
        let errors = extract_errors(output, None);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file, "src/main.c");
        assert_eq!(errors[0].line, 10);
        assert_eq!(errors[0].column, Some(5));
        assert_eq!(errors[0].severity, Severity::Error);
        assert_eq!(errors[0].message, "undeclared");
    }

    #[test]
    fn extract_errors_windows_path() {
        let output = r"C:\Users\foo\main.rs:10:5: warning: unused";
        let errors = extract_errors(output, None);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file, r"C:\Users\foo\main.rs");
        assert_eq!(errors[0].line, 10);
        assert_eq!(errors[0].column, Some(5));
        assert_eq!(errors[0].severity, Severity::Warning);
        assert_eq!(errors[0].message, "unused");
    }

    #[test]
    fn extract_errors_no_column() {
        let output = "src/lib.rs:42: warning: deprecated function";
        let errors = extract_errors(output, None);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file, "src/lib.rs");
        assert_eq!(errors[0].line, 42);
        assert_eq!(errors[0].column, None);
        assert_eq!(errors[0].severity, Severity::Warning);
    }

    #[test]
    fn extract_errors_no_severity() {
        let output = "main.c:10:5: some error message here";
        let errors = extract_errors(output, None);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file, "main.c");
        assert_eq!(errors[0].line, 10);
        assert_eq!(errors[0].column, Some(5));
        assert_eq!(errors[0].severity, Severity::Error);
    }

    #[test]
    fn end_to_end_rust_json_with_hint() {
        let output = r#"{"reason":"compiler-message","package_id":"test","manifest_path":"Cargo.toml","message":{"message":"mismatched types","code":{"code":"E0308","explanation":null},"level":"error","spans":[{"file_name":"src/main.rs","byte_start":100,"byte_end":110,"line_start":10,"line_end":10,"column_start":5,"column_end":15,"is_primary":true,"text":[],"label":null}],"children":[],"rendered":"error[E0308]"}}"#;
        let errors = extract_errors(output, Some("cargo build"));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file, "src/main.rs");
        assert_eq!(errors[0].line, 10);
        assert_eq!(errors[0].severity, Severity::Error);
        assert_eq!(errors[0].code, Some("E0308".to_string()));
    }

    #[test]
    fn end_to_end_rust_human_content_sniff() {
        let output = "error[E0308]: mismatched types\n --> src/main.rs:10:5\n  |\n10 |     let x: u32 = \"hello\";\n  |                   ^^^^^^^";
        let errors = extract_errors(output, None);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].file, "src/main.rs");
        assert_eq!(errors[0].severity, Severity::Error);
        assert_eq!(errors[0].code, Some("E0308".to_string()));
    }
}
