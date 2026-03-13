//! Core types for Structured Output Intelligence (SOI).
//!
//! Defines the type taxonomy, record variants, and summary structures
//! that all SOI parsers produce.

use serde::Serialize;

/// The kind of output a command produced.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum OutputType {
    // Build / compile
    /// cargo build, rustc
    RustCompiler,
    /// tsc
    TypeScript,
    /// go build
    GoBuild,
    /// gcc, g++, clang
    CppCompiler,
    /// file:line:col pattern (generic)
    GenericCompiler,

    // Test runners
    /// cargo test
    RustTest,
    /// jest, npx jest
    Jest,
    /// pytest, python -m pytest
    Pytest,
    /// go test
    GoTest,
    /// TAP protocol output
    GenericTAP,

    // Package managers
    /// npm install, npm run
    Npm,
    /// cargo add, cargo update
    Cargo,
    /// pip install
    Pip,

    // DevOps / Infrastructure
    /// docker build, docker compose
    Docker,
    /// kubectl get, apply, describe
    Kubectl,
    /// terraform plan, apply
    Terraform,

    // Version control
    /// git status, diff, log, merge
    Git,

    // Structured data
    /// NDJSON output
    JsonLines,
    /// Single JSON blob
    JsonObject,
    /// CSV/TSV output
    Csv,

    // Fallback
    /// Unrecognized — store raw with basic stats
    FreeformText,
}

/// Severity level for SOI records and summaries.
///
/// Note: intentionally different from `glass_errors::Severity` which uses
/// Note/Help rather than Info/Success.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Success,
}

/// Status of a single test case.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum TestStatus {
    Passed,
    Failed,
    Skipped,
    Ignored,
}

/// A one-line compressed summary of a parsed command output.
#[derive(Debug, Clone, Serialize)]
pub struct OutputSummary {
    /// Human/agent readable one-liner: e.g. "3 failed, 247 passed, 2 warnings"
    pub one_line: String,
    /// Approximate tokens for this summary
    pub token_estimate: usize,
    /// Highest severity found in the output
    pub severity: Severity,
}

/// A single structured record extracted from command output.
#[derive(Debug, Clone, Serialize)]
pub enum OutputRecord {
    /// A compiler diagnostic (error, warning, note, help).
    CompilerError {
        file: String,
        line: u32,
        column: Option<u32>,
        severity: Severity,
        /// Error code (e.g. "E0308", "TS2345")
        code: Option<String>,
        message: String,
        /// Surrounding source code shown by the compiler
        context_lines: Option<String>,
    },
    /// A single test case result.
    TestResult {
        /// Fully qualified test name (e.g. "module::test_name")
        name: String,
        status: TestStatus,
        duration_ms: Option<u64>,
        failure_message: Option<String>,
        /// file:line of failure
        failure_location: Option<String>,
    },
    /// Aggregate test run summary.
    TestSummary {
        passed: u32,
        failed: u32,
        skipped: u32,
        ignored: u32,
        total_duration_ms: Option<u64>,
    },
    /// A package manager action (add, remove, update, audit).
    PackageEvent {
        /// "added", "removed", "updated", "audited"
        action: String,
        package: String,
        version: Option<String>,
        /// E.g. "3 vulnerabilities found"
        detail: Option<String>,
    },
    /// A git operation event.
    GitEvent {
        /// "merge", "pull", "push", "conflict"
        action: String,
        detail: String,
        files_changed: Option<u32>,
        insertions: Option<u32>,
        deletions: Option<u32>,
    },
    /// A Docker operation event.
    DockerEvent {
        /// "build", "pull", "push", "up", "error"
        action: String,
        image: Option<String>,
        detail: String,
    },
    /// A generic file:line diagnostic (non-compiler-specific).
    GenericDiagnostic {
        file: Option<String>,
        line: Option<u32>,
        severity: Severity,
        message: String,
    },
    /// Raw text chunk for unrecognized output.
    FreeformChunk {
        /// Compressed or sampled raw text
        text: String,
        line_count: usize,
    },
}

/// The complete result of running the SOI pipeline on one command's output.
#[derive(Debug, Clone, Serialize)]
pub struct ParsedOutput {
    pub output_type: OutputType,
    pub summary: OutputSummary,
    pub records: Vec<OutputRecord>,
    pub raw_line_count: usize,
    pub raw_byte_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_type_equality() {
        assert_eq!(OutputType::RustCompiler, OutputType::RustCompiler);
        assert_ne!(OutputType::RustCompiler, OutputType::RustTest);
    }

    #[test]
    fn severity_equality() {
        assert_eq!(Severity::Error, Severity::Error);
        assert_ne!(Severity::Error, Severity::Warning);
    }

    #[test]
    fn test_status_equality() {
        assert_eq!(TestStatus::Passed, TestStatus::Passed);
        assert_ne!(TestStatus::Passed, TestStatus::Failed);
    }

    #[test]
    fn parsed_output_has_all_fields() {
        let po = ParsedOutput {
            output_type: OutputType::FreeformText,
            summary: OutputSummary {
                one_line: "no structured output".to_string(),
                token_estimate: 5,
                severity: Severity::Info,
            },
            records: vec![OutputRecord::FreeformChunk {
                text: "hello world".to_string(),
                line_count: 1,
            }],
            raw_line_count: 1,
            raw_byte_count: 11,
        };
        assert_eq!(po.records.len(), 1);
        assert_eq!(po.output_type, OutputType::FreeformText);
    }
}
