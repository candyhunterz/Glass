//! SOI (Structured Output Intelligence) storage layer for glass_history.
//!
//! Stores `ParsedOutput` from `glass_soi` into the history database:
//! - `command_output_records`: one row per command with a summary
//! - `output_records`: one row per structured record in the output
//!
//! Functions here operate on a raw `&Connection` and are called via
//! delegation from `HistoryDb`.

use anyhow::Result;
use rusqlite::{params, Connection};

use glass_soi::{OutputRecord, ParsedOutput, Severity};

/// A row from the `command_output_records` table.
#[derive(Debug, Clone)]
pub struct CommandOutputSummaryRow {
    pub id: i64,
    pub command_id: i64,
    pub output_type: String,
    pub severity: String,
    pub one_line: String,
    pub token_estimate: i64,
    pub raw_line_count: i64,
    pub raw_byte_count: i64,
}

/// A row from the `output_records` table.
#[derive(Debug, Clone)]
pub struct OutputRecordRow {
    pub id: i64,
    pub command_id: i64,
    pub record_type: String,
    pub severity: Option<String>,
    pub file_path: Option<String>,
    pub data: String,
}

/// Insert a `ParsedOutput` for a command atomically.
///
/// Inserts one row into `command_output_records` (the summary) and one row
/// per record into `output_records`. Uses an unchecked transaction so it
/// integrates cleanly with the caller's connection lifecycle.
pub fn insert_parsed_output(
    conn: &Connection,
    command_id: i64,
    parsed: &ParsedOutput,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;

    // Insert the summary row
    let output_type = format!("{:?}", parsed.output_type);
    let severity = severity_to_str(&parsed.summary.severity);
    tx.execute(
        "INSERT INTO command_output_records
             (command_id, output_type, severity, one_line, token_estimate, raw_line_count, raw_byte_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            command_id,
            output_type,
            severity,
            parsed.summary.one_line,
            parsed.summary.token_estimate as i64,
            parsed.raw_line_count as i64,
            parsed.raw_byte_count as i64,
        ],
    )?;

    // Insert each record row
    for record in &parsed.records {
        let (record_type, rec_severity, file_path) = extract_record_meta(record);
        let data = serde_json::to_string(record)?;
        tx.execute(
            "INSERT INTO output_records (command_id, record_type, severity, file_path, data)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            params![command_id, record_type, rec_severity, file_path, data],
        )?;
    }

    tx.commit()?;
    Ok(())
}

/// Retrieve the output summary for a command, if any.
pub fn get_output_summary(
    conn: &Connection,
    command_id: i64,
) -> Result<Option<CommandOutputSummaryRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, command_id, output_type, severity, one_line, token_estimate,
                raw_line_count, raw_byte_count
         FROM command_output_records WHERE command_id = ?1",
    )?;
    let mut rows = stmt.query_map(params![command_id], |row| {
        Ok(CommandOutputSummaryRow {
            id: row.get(0)?,
            command_id: row.get(1)?,
            output_type: row.get(2)?,
            severity: row.get(3)?,
            one_line: row.get(4)?,
            token_estimate: row.get(5)?,
            raw_line_count: row.get(6)?,
            raw_byte_count: row.get(7)?,
        })
    })?;
    match rows.next() {
        Some(Ok(row)) => Ok(Some(row)),
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

/// Retrieve output records for a command with optional filters.
///
/// All `Some` filter arguments narrow the result set with an AND clause.
/// Results are ordered by `id` ascending and capped at `limit`.
pub fn get_output_records(
    conn: &Connection,
    command_id: i64,
    severity: Option<&str>,
    file_path: Option<&str>,
    record_type: Option<&str>,
    limit: usize,
) -> Result<Vec<OutputRecordRow>> {
    // Build a dynamic WHERE clause
    let mut conditions = vec!["command_id = ?1".to_string()];
    let mut next_param = 2usize;

    if severity.is_some() {
        conditions.push(format!("severity = ?{}", next_param));
        next_param += 1;
    }
    if file_path.is_some() {
        conditions.push(format!("file_path = ?{}", next_param));
        next_param += 1;
    }
    if record_type.is_some() {
        conditions.push(format!("record_type = ?{}", next_param));
        let _ = next_param; // exhausted; suppress unused_assignments
    }

    let sql = format!(
        "SELECT id, command_id, record_type, severity, file_path, data
         FROM output_records
         WHERE {}
         ORDER BY id ASC
         LIMIT {}",
        conditions.join(" AND "),
        limit
    );

    let mut stmt = conn.prepare(&sql)?;

    // Build params list: command_id is always first, then optional filters in order
    let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(command_id)];
    if let Some(s) = severity {
        param_values.push(Box::new(s.to_string()));
    }
    if let Some(f) = file_path {
        param_values.push(Box::new(f.to_string()));
    }
    if let Some(r) = record_type {
        param_values.push(Box::new(r.to_string()));
    }

    let params_refs: Vec<&dyn rusqlite::ToSql> = param_values.iter().map(|v| v.as_ref()).collect();

    let rows = stmt.query_map(params_refs.as_slice(), |row| {
        Ok(OutputRecordRow {
            id: row.get(0)?,
            command_id: row.get(1)?,
            record_type: row.get(2)?,
            severity: row.get(3)?,
            file_path: row.get(4)?,
            data: row.get(5)?,
        })
    })?;

    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

/// Extract record_type, severity string, and file_path from an `OutputRecord`.
///
/// Severity is returned as a stable string literal ("Error", "Warning", "Info",
/// "Success"), not via `Debug` format, to guard against future rename churn.
fn extract_record_meta(record: &OutputRecord) -> (&'static str, Option<&str>, Option<&str>) {
    match record {
        OutputRecord::CompilerError { file, severity, .. } => (
            "CompilerError",
            Some(severity_to_str(severity)),
            Some(file.as_str()),
        ),
        OutputRecord::TestResult { .. } => ("TestResult", None, None),
        OutputRecord::TestSummary { .. } => ("TestSummary", None, None),
        OutputRecord::PackageEvent { .. } => ("PackageEvent", None, None),
        OutputRecord::GitEvent { .. } => ("GitEvent", None, None),
        OutputRecord::DockerEvent { .. } => ("DockerEvent", None, None),
        OutputRecord::GenericDiagnostic { file, severity, .. } => (
            "GenericDiagnostic",
            Some(severity_to_str(severity)),
            file.as_deref(),
        ),
        OutputRecord::FreeformChunk { .. } => ("FreeformChunk", None, None),
    }
}

/// Convert a `Severity` to a stable string literal.
fn severity_to_str(severity: &Severity) -> &'static str {
    match severity {
        Severity::Error => "Error",
        Severity::Warning => "Warning",
        Severity::Info => "Info",
        Severity::Success => "Success",
    }
}

#[cfg(test)]
mod tests {
    use crate::db::HistoryDb;
    use glass_soi::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity, TestStatus};
    use tempfile::TempDir;

    fn make_db() -> (HistoryDb, TempDir) {
        let dir = TempDir::new().unwrap();
        let db = HistoryDb::open(&dir.path().join("test.db")).unwrap();
        (db, dir)
    }

    fn sample_command(db: &HistoryDb) -> i64 {
        db.insert_command(&crate::db::CommandRecord {
            id: None,
            command: "cargo build".to_string(),
            cwd: "/home/user/project".to_string(),
            exit_code: Some(1),
            started_at: 1700000000,
            finished_at: 1700000005,
            duration_ms: 5000,
            output: None,
        })
        .unwrap()
    }

    fn make_parsed_output_with_compiler_errors() -> ParsedOutput {
        ParsedOutput {
            output_type: OutputType::RustCompiler,
            summary: OutputSummary {
                one_line: "3 errors".to_string(),
                token_estimate: 42,
                severity: Severity::Error,
            },
            records: vec![
                OutputRecord::CompilerError {
                    file: "src/main.rs".to_string(),
                    line: 10,
                    column: Some(5),
                    severity: Severity::Error,
                    code: Some("E0308".to_string()),
                    message: "mismatched types".to_string(),
                    context_lines: None,
                },
                OutputRecord::CompilerError {
                    file: "src/lib.rs".to_string(),
                    line: 20,
                    column: None,
                    severity: Severity::Warning,
                    code: None,
                    message: "unused import".to_string(),
                    context_lines: None,
                },
                OutputRecord::CompilerError {
                    file: "src/main.rs".to_string(),
                    line: 30,
                    column: Some(1),
                    severity: Severity::Error,
                    code: Some("E0425".to_string()),
                    message: "cannot find value".to_string(),
                    context_lines: Some("   |".to_string()),
                },
            ],
            raw_line_count: 50,
            raw_byte_count: 1024,
        }
    }

    #[test]
    fn test_insert_and_query_parsed_output() {
        let (db, _dir) = make_db();
        let cmd_id = sample_command(&db);
        let parsed = make_parsed_output_with_compiler_errors();

        db.insert_parsed_output(cmd_id, &parsed).unwrap();

        let records = db
            .get_output_records(cmd_id, None, None, None, 100)
            .unwrap();
        assert_eq!(records.len(), 3, "should have 3 records");
        assert_eq!(records[0].record_type, "CompilerError");
        assert_eq!(records[1].record_type, "CompilerError");
        assert_eq!(records[2].record_type, "CompilerError");
    }

    #[test]
    fn test_query_by_severity() {
        let (db, _dir) = make_db();
        let cmd_id = sample_command(&db);
        let parsed = make_parsed_output_with_compiler_errors();

        db.insert_parsed_output(cmd_id, &parsed).unwrap();

        let errors = db
            .get_output_records(cmd_id, Some("Error"), None, None, 100)
            .unwrap();
        assert_eq!(errors.len(), 2, "should have 2 Error records");
        for r in &errors {
            assert_eq!(r.severity.as_deref(), Some("Error"));
        }

        let warnings = db
            .get_output_records(cmd_id, Some("Warning"), None, None, 100)
            .unwrap();
        assert_eq!(warnings.len(), 1, "should have 1 Warning record");
    }

    #[test]
    fn test_query_by_file_path() {
        let (db, _dir) = make_db();
        let cmd_id = sample_command(&db);
        let parsed = make_parsed_output_with_compiler_errors();

        db.insert_parsed_output(cmd_id, &parsed).unwrap();

        let main_rs = db
            .get_output_records(cmd_id, None, Some("src/main.rs"), None, 100)
            .unwrap();
        assert_eq!(main_rs.len(), 2, "src/main.rs should have 2 records");

        let lib_rs = db
            .get_output_records(cmd_id, None, Some("src/lib.rs"), None, 100)
            .unwrap();
        assert_eq!(lib_rs.len(), 1, "src/lib.rs should have 1 record");
    }

    #[test]
    fn test_query_by_record_type() {
        let (db, _dir) = make_db();
        let cmd_id = sample_command(&db);

        // Mixed record types
        let parsed = ParsedOutput {
            output_type: OutputType::RustTest,
            summary: OutputSummary {
                one_line: "2 passed, 1 failed".to_string(),
                token_estimate: 10,
                severity: Severity::Error,
            },
            records: vec![
                OutputRecord::CompilerError {
                    file: "src/main.rs".to_string(),
                    line: 1,
                    column: None,
                    severity: Severity::Error,
                    code: None,
                    message: "compile error".to_string(),
                    context_lines: None,
                },
                OutputRecord::TestResult {
                    name: "test_foo".to_string(),
                    status: TestStatus::Passed,
                    duration_ms: Some(10),
                    failure_message: None,
                    failure_location: None,
                },
                OutputRecord::TestResult {
                    name: "test_bar".to_string(),
                    status: TestStatus::Failed,
                    duration_ms: Some(5),
                    failure_message: Some("assertion failed".to_string()),
                    failure_location: Some("src/lib.rs:42".to_string()),
                },
            ],
            raw_line_count: 20,
            raw_byte_count: 500,
        };

        db.insert_parsed_output(cmd_id, &parsed).unwrap();

        let test_results = db
            .get_output_records(cmd_id, None, None, Some("TestResult"), 100)
            .unwrap();
        assert_eq!(test_results.len(), 2, "should have 2 TestResult records");
        for r in &test_results {
            assert_eq!(r.record_type, "TestResult");
        }

        let compiler_errors = db
            .get_output_records(cmd_id, None, None, Some("CompilerError"), 100)
            .unwrap();
        assert_eq!(compiler_errors.len(), 1);
    }

    #[test]
    fn test_get_output_summary() {
        let (db, _dir) = make_db();
        let cmd_id = sample_command(&db);
        let parsed = make_parsed_output_with_compiler_errors();

        db.insert_parsed_output(cmd_id, &parsed).unwrap();

        let summary = db.get_output_summary(cmd_id).unwrap();
        assert!(summary.is_some(), "summary should exist");
        let s = summary.unwrap();
        assert_eq!(s.command_id, cmd_id);
        assert_eq!(s.output_type, "RustCompiler");
        assert_eq!(s.severity, "Error");
        assert_eq!(s.one_line, "3 errors");
        assert_eq!(s.token_estimate, 42);
        assert_eq!(s.raw_line_count, 50);
        assert_eq!(s.raw_byte_count, 1024);
    }

    #[test]
    fn test_insert_empty_records() {
        let (db, _dir) = make_db();
        let cmd_id = sample_command(&db);

        let parsed = ParsedOutput {
            output_type: OutputType::FreeformText,
            summary: OutputSummary {
                one_line: "no output".to_string(),
                token_estimate: 0,
                severity: Severity::Info,
            },
            records: vec![],
            raw_line_count: 0,
            raw_byte_count: 0,
        };

        db.insert_parsed_output(cmd_id, &parsed).unwrap();

        // Summary row should exist
        let summary = db.get_output_summary(cmd_id).unwrap();
        assert!(summary.is_some());

        // But no output_records rows
        let records = db
            .get_output_records(cmd_id, None, None, None, 100)
            .unwrap();
        assert!(records.is_empty(), "no records expected for empty input");
    }
}
