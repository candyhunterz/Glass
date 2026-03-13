//! Aggregate query logic for the GlassContext MCP tool.
//!
//! Provides a high-level activity summary (command counts, failure rate,
//! time range, active directories) from the Glass history database.

use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;

/// A single SOI (Structured Output Intelligence) summary entry.
#[derive(Debug, Serialize)]
pub struct SoiSummaryEntry {
    /// The command's ID in the history database.
    pub command_id: i64,
    /// The command string.
    pub command: String,
    /// The output type detected by the SOI classifier.
    pub output_type: String,
    /// The overall severity of the output.
    pub severity: String,
    /// One-line human-readable summary of the output.
    pub one_line: String,
}

/// High-level summary of terminal activity over a time window.
#[derive(Debug, Serialize)]
pub struct ContextSummary {
    /// Total number of commands in the time window.
    pub command_count: i64,
    /// Number of commands with non-zero exit code.
    pub failure_count: i64,
    /// Earliest started_at epoch in the window (None if no commands).
    pub earliest_timestamp: Option<i64>,
    /// Latest finished_at epoch in the window (None if no commands).
    pub latest_timestamp: Option<i64>,
    /// Up to 10 most recently used distinct working directories.
    pub recent_directories: Vec<String>,
    /// Number of distinct commands that have pipeline stages.
    pub pipeline_count: i64,
    /// Average number of stages per pipeline command.
    pub avg_pipeline_stages: f64,
    /// Fraction of pipeline commands with non-zero exit code.
    pub pipeline_failure_rate: f64,
    /// Up to 10 most recent SOI summaries from command_output_records.
    pub soi_summaries: Vec<SoiSummaryEntry>,
}

/// Build an aggregate activity summary from the commands table.
///
/// If `after` is Some, only commands with `started_at >= after` are included.
/// Uses parameterized queries for safety.
pub fn build_context_summary(conn: &Connection, after: Option<i64>) -> Result<ContextSummary> {
    // Build aggregate query with optional time filter
    let (agg_sql, dir_sql, params) = if let Some(after_epoch) = after {
        (
            "SELECT COUNT(*), \
                    COALESCE(SUM(CASE WHEN exit_code != 0 THEN 1 ELSE 0 END), 0), \
                    MIN(started_at), \
                    MAX(finished_at) \
             FROM commands WHERE started_at >= ?1",
            "SELECT cwd, MAX(started_at) as last_used FROM commands WHERE started_at >= ?1 \
             GROUP BY cwd ORDER BY last_used DESC LIMIT 10",
            vec![rusqlite::types::Value::Integer(after_epoch)],
        )
    } else {
        (
            "SELECT COUNT(*), \
                    COALESCE(SUM(CASE WHEN exit_code != 0 THEN 1 ELSE 0 END), 0), \
                    MIN(started_at), \
                    MAX(finished_at) \
             FROM commands",
            "SELECT cwd, MAX(started_at) as last_used FROM commands \
             GROUP BY cwd ORDER BY last_used DESC LIMIT 10",
            vec![],
        )
    };

    // Run aggregate query
    let mut stmt = conn.prepare(agg_sql)?;
    let (command_count, failure_count, earliest_timestamp, latest_timestamp) =
        stmt.query_row(rusqlite::params_from_iter(params.iter()), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<i64>>(3)?,
            ))
        })?;

    // Run distinct directories query
    let mut dir_stmt = conn.prepare(dir_sql)?;
    let dir_rows = dir_stmt.query_map(rusqlite::params_from_iter(params.iter()), |row| {
        row.get::<_, String>(0)
    })?;
    let mut recent_directories = Vec::new();
    for dir in dir_rows {
        recent_directories.push(dir?);
    }

    // Pipeline stats: count and avg stages
    let (pipe_count_sql, pipe_fail_sql) = if after.is_some() {
        (
            "SELECT COUNT(DISTINCT ps.command_id), \
                    CAST(COUNT(*) AS REAL) / NULLIF(COUNT(DISTINCT ps.command_id), 0) \
             FROM pipe_stages ps \
             JOIN commands c ON c.id = ps.command_id \
             WHERE c.started_at >= ?1",
            "SELECT COUNT(DISTINCT ps.command_id) \
             FROM pipe_stages ps \
             JOIN commands c ON c.id = ps.command_id \
             WHERE c.exit_code != 0 AND c.started_at >= ?1",
        )
    } else {
        (
            "SELECT COUNT(DISTINCT ps.command_id), \
                    CAST(COUNT(*) AS REAL) / NULLIF(COUNT(DISTINCT ps.command_id), 0) \
             FROM pipe_stages ps \
             JOIN commands c ON c.id = ps.command_id",
            "SELECT COUNT(DISTINCT ps.command_id) \
             FROM pipe_stages ps \
             JOIN commands c ON c.id = ps.command_id \
             WHERE c.exit_code != 0",
        )
    };

    let mut pipe_stmt = conn.prepare(pipe_count_sql)?;
    let (pipeline_count, avg_pipeline_stages) = pipe_stmt
        .query_row(rusqlite::params_from_iter(params.iter()), |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Option<f64>>(1)?))
        })?;
    let avg_pipeline_stages = avg_pipeline_stages.unwrap_or(0.0);

    let mut pipe_fail_stmt = conn.prepare(pipe_fail_sql)?;
    let failed_pipeline_count: i64 =
        pipe_fail_stmt.query_row(rusqlite::params_from_iter(params.iter()), |row| row.get(0))?;
    let pipeline_failure_rate = if pipeline_count > 0 {
        failed_pipeline_count as f64 / pipeline_count as f64
    } else {
        0.0
    };

    // SOI summaries: JOIN command_output_records with commands
    let soi_sql = if after.is_some() {
        "SELECT c.id, c.command, cor.output_type, cor.severity, cor.one_line \
         FROM commands c \
         JOIN command_output_records cor ON cor.command_id = c.id \
         WHERE c.started_at >= ?1 \
         ORDER BY c.started_at DESC LIMIT 10"
    } else {
        "SELECT c.id, c.command, cor.output_type, cor.severity, cor.one_line \
         FROM commands c \
         JOIN command_output_records cor ON cor.command_id = c.id \
         ORDER BY c.started_at DESC LIMIT 10"
    };
    let mut soi_stmt = conn.prepare(soi_sql)?;
    let soi_rows = soi_stmt.query_map(rusqlite::params_from_iter(params.iter()), |row| {
        Ok(SoiSummaryEntry {
            command_id: row.get(0)?,
            command: row.get(1)?,
            output_type: row.get(2)?,
            severity: row.get(3)?,
            one_line: row.get(4)?,
        })
    })?;
    let mut soi_summaries = Vec::new();
    for entry in soi_rows {
        soi_summaries.push(entry?);
    }

    Ok(ContextSummary {
        command_count,
        failure_count,
        earliest_timestamp,
        latest_timestamp,
        recent_directories,
        pipeline_count,
        avg_pipeline_stages,
        pipeline_failure_rate,
        soi_summaries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use glass_history::db::{CommandRecord, HistoryDb};
    use tempfile::TempDir;

    fn test_db() -> (HistoryDb, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test_context.db");
        let db = HistoryDb::open(&db_path).unwrap();
        (db, dir)
    }

    fn insert(db: &HistoryDb, cmd: &str, cwd: &str, exit_code: Option<i32>, started_at: i64) {
        let record = CommandRecord {
            id: None,
            command: cmd.to_string(),
            cwd: cwd.to_string(),
            exit_code,
            started_at,
            finished_at: started_at + 5,
            duration_ms: 5000,
            output: None,
        };
        db.insert_command(&record).unwrap();
    }

    #[test]
    fn test_empty_db_returns_zeros() {
        let (db, _dir) = test_db();
        let summary = build_context_summary(db.conn(), None).unwrap();
        assert_eq!(summary.command_count, 0);
        assert_eq!(summary.failure_count, 0);
        assert_eq!(summary.earliest_timestamp, None);
        assert_eq!(summary.latest_timestamp, None);
        assert!(summary.recent_directories.is_empty());
        assert!(summary.soi_summaries.is_empty());
    }

    #[test]
    fn test_mixed_commands_correct_counts() {
        let (db, _dir) = test_db();
        insert(&db, "cargo build", "/home/user", Some(0), 1700000000);
        insert(&db, "bad cmd", "/home/user", Some(1), 1700000010);
        insert(&db, "another fail", "/home/user", Some(127), 1700000020);
        insert(&db, "ls", "/home/user", Some(0), 1700000030);

        let summary = build_context_summary(db.conn(), None).unwrap();
        assert_eq!(summary.command_count, 4);
        assert_eq!(summary.failure_count, 2);
        assert_eq!(summary.earliest_timestamp, Some(1700000000));
        // finished_at = started_at + 5, so latest = 1700000035
        assert_eq!(summary.latest_timestamp, Some(1700000035));
    }

    #[test]
    fn test_after_filter_excludes_old_commands() {
        let (db, _dir) = test_db();
        insert(&db, "old cmd", "/tmp", Some(0), 1700000000);
        insert(&db, "old fail", "/tmp", Some(1), 1700000010);
        insert(&db, "new cmd", "/home", Some(0), 1700000100);
        insert(&db, "new fail", "/home", Some(1), 1700000200);

        // Filter: only commands after epoch 1700000050
        let summary = build_context_summary(db.conn(), Some(1700000050)).unwrap();
        assert_eq!(summary.command_count, 2);
        assert_eq!(summary.failure_count, 1);
        assert_eq!(summary.earliest_timestamp, Some(1700000100));
    }

    #[test]
    fn test_recent_directories_distinct_max_10() {
        let (db, _dir) = test_db();
        // Insert commands in 12 different directories
        for i in 0..12 {
            insert(
                &db,
                &format!("cmd{}", i),
                &format!("/dir/{}", i),
                Some(0),
                1700000000 + i * 10,
            );
        }

        let summary = build_context_summary(db.conn(), None).unwrap();
        assert_eq!(summary.recent_directories.len(), 10);
        // Most recent directories should come first
        assert_eq!(summary.recent_directories[0], "/dir/11");
        assert_eq!(summary.recent_directories[1], "/dir/10");
    }

    #[test]
    fn test_pipeline_stats_empty_db() {
        let (db, _dir) = test_db();
        let summary = build_context_summary(db.conn(), None).unwrap();
        assert_eq!(summary.pipeline_count, 0);
        assert_eq!(summary.avg_pipeline_stages, 0.0);
        assert_eq!(summary.pipeline_failure_rate, 0.0);
    }

    #[test]
    fn test_pipeline_stats_with_data() {
        use glass_history::PipeStageRow;
        let (db, _dir) = test_db();
        // Command 1: success, 3 pipe stages
        let id1 = {
            insert(&db, "cat | grep | wc", "/tmp", Some(0), 1700000000);
            1 // first inserted gets id 1
        };
        db.insert_pipe_stages(
            id1,
            &[
                PipeStageRow {
                    stage_index: 0,
                    command: "cat".into(),
                    output: Some("a".into()),
                    total_bytes: 1,
                    is_binary: false,
                    is_sampled: false,
                },
                PipeStageRow {
                    stage_index: 1,
                    command: "grep".into(),
                    output: Some("b".into()),
                    total_bytes: 1,
                    is_binary: false,
                    is_sampled: false,
                },
                PipeStageRow {
                    stage_index: 2,
                    command: "wc".into(),
                    output: Some("1".into()),
                    total_bytes: 1,
                    is_binary: false,
                    is_sampled: false,
                },
            ],
        )
        .unwrap();

        // Command 2: failure, 2 pipe stages
        let id2 = {
            insert(&db, "echo | sort", "/tmp", Some(1), 1700000010);
            2
        };
        db.insert_pipe_stages(
            id2,
            &[
                PipeStageRow {
                    stage_index: 0,
                    command: "echo".into(),
                    output: Some("x".into()),
                    total_bytes: 1,
                    is_binary: false,
                    is_sampled: false,
                },
                PipeStageRow {
                    stage_index: 1,
                    command: "sort".into(),
                    output: Some("x".into()),
                    total_bytes: 1,
                    is_binary: false,
                    is_sampled: false,
                },
            ],
        )
        .unwrap();

        let summary = build_context_summary(db.conn(), None).unwrap();
        assert_eq!(summary.pipeline_count, 2);
        assert!((summary.avg_pipeline_stages - 2.5).abs() < 0.001);
        assert!((summary.pipeline_failure_rate - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_pipeline_stats_with_time_filter() {
        use glass_history::PipeStageRow;
        let (db, _dir) = test_db();
        // Old command with pipes (before filter)
        let id1 = {
            insert(&db, "cat | grep", "/tmp", Some(0), 1700000000);
            1
        };
        db.insert_pipe_stages(
            id1,
            &[
                PipeStageRow {
                    stage_index: 0,
                    command: "cat".into(),
                    output: None,
                    total_bytes: 0,
                    is_binary: false,
                    is_sampled: false,
                },
                PipeStageRow {
                    stage_index: 1,
                    command: "grep".into(),
                    output: None,
                    total_bytes: 0,
                    is_binary: false,
                    is_sampled: false,
                },
            ],
        )
        .unwrap();

        // New command with pipes (after filter)
        let id2 = {
            insert(&db, "echo | wc", "/tmp", Some(0), 1700000100);
            2
        };
        db.insert_pipe_stages(
            id2,
            &[
                PipeStageRow {
                    stage_index: 0,
                    command: "echo".into(),
                    output: None,
                    total_bytes: 0,
                    is_binary: false,
                    is_sampled: false,
                },
                PipeStageRow {
                    stage_index: 1,
                    command: "wc".into(),
                    output: None,
                    total_bytes: 0,
                    is_binary: false,
                    is_sampled: false,
                },
                PipeStageRow {
                    stage_index: 2,
                    command: "head".into(),
                    output: None,
                    total_bytes: 0,
                    is_binary: false,
                    is_sampled: false,
                },
            ],
        )
        .unwrap();

        // Filter: only after epoch 1700000050
        let summary = build_context_summary(db.conn(), Some(1700000050)).unwrap();
        assert_eq!(summary.pipeline_count, 1);
        assert!((summary.avg_pipeline_stages - 3.0).abs() < 0.001);
        assert_eq!(summary.pipeline_failure_rate, 0.0);
    }

    #[test]
    fn test_pipeline_stats_division_by_zero() {
        let (db, _dir) = test_db();
        // Insert commands but no pipe stages
        insert(&db, "ls", "/tmp", Some(0), 1700000000);
        insert(&db, "pwd", "/tmp", Some(0), 1700000010);

        let summary = build_context_summary(db.conn(), None).unwrap();
        assert_eq!(summary.pipeline_count, 0);
        assert_eq!(summary.avg_pipeline_stages, 0.0);
        assert_eq!(summary.pipeline_failure_rate, 0.0);
        // Ensure no NaN or Infinity
        assert!(summary.avg_pipeline_stages.is_finite());
        assert!(summary.pipeline_failure_rate.is_finite());
    }

    #[test]
    fn test_context_soi_summaries_empty_db() {
        let (db, _dir) = test_db();
        // No commands, no SOI data
        let summary = build_context_summary(db.conn(), None).unwrap();
        assert!(summary.soi_summaries.is_empty());
    }

    #[test]
    fn test_context_soi_summaries_no_soi_data() {
        let (db, _dir) = test_db();
        // Commands present but no SOI records
        insert(&db, "ls", "/tmp", Some(0), 1700000000);
        insert(&db, "pwd", "/tmp", Some(0), 1700000010);
        let summary = build_context_summary(db.conn(), None).unwrap();
        assert!(summary.soi_summaries.is_empty());
    }

    #[test]
    fn test_context_soi_summaries_populated() {
        use glass_soi::{OutputSummary, OutputType, ParsedOutput, Severity};

        let (db, _dir) = test_db();
        // Insert a command and SOI data for it
        insert(&db, "cargo build", "/proj", Some(0), 1700000000);
        let cmd_id = 1i64;

        let parsed = ParsedOutput {
            output_type: OutputType::RustCompiler,
            summary: OutputSummary {
                one_line: "Build succeeded".to_string(),
                token_estimate: 10,
                severity: Severity::Info,
            },
            records: vec![],
            raw_line_count: 5,
            raw_byte_count: 100,
        };
        db.insert_parsed_output(cmd_id, &parsed).unwrap();

        let summary = build_context_summary(db.conn(), None).unwrap();
        assert_eq!(summary.soi_summaries.len(), 1);
        let entry = &summary.soi_summaries[0];
        assert_eq!(entry.command_id, cmd_id);
        assert_eq!(entry.command, "cargo build");
        assert_eq!(entry.output_type, "RustCompiler");
        assert_eq!(entry.severity, "Info");
        assert_eq!(entry.one_line, "Build succeeded");
    }

    #[test]
    fn test_context_soi_summaries_includes_success_and_failure() {
        use glass_soi::{OutputSummary, OutputType, ParsedOutput, Severity};

        let (db, _dir) = test_db();
        // Success command
        insert(&db, "cargo build", "/proj", Some(0), 1700000000);
        let cmd1_id = 1i64;
        // Failure command
        insert(&db, "cargo test", "/proj", Some(1), 1700000010);
        let cmd2_id = 2i64;

        let success_parsed = ParsedOutput {
            output_type: OutputType::RustCompiler,
            summary: OutputSummary {
                one_line: "Build succeeded".to_string(),
                token_estimate: 10,
                severity: Severity::Info,
            },
            records: vec![],
            raw_line_count: 3,
            raw_byte_count: 50,
        };
        let failure_parsed = ParsedOutput {
            output_type: OutputType::RustTest,
            summary: OutputSummary {
                one_line: "1 test failed".to_string(),
                token_estimate: 20,
                severity: Severity::Error,
            },
            records: vec![],
            raw_line_count: 10,
            raw_byte_count: 200,
        };
        db.insert_parsed_output(cmd1_id, &success_parsed).unwrap();
        db.insert_parsed_output(cmd2_id, &failure_parsed).unwrap();

        let summary = build_context_summary(db.conn(), None).unwrap();
        assert_eq!(summary.soi_summaries.len(), 2);
        // Most recent first (cmd2 has higher started_at)
        assert_eq!(summary.soi_summaries[0].command_id, cmd2_id);
        assert_eq!(summary.soi_summaries[0].severity, "Error");
        assert_eq!(summary.soi_summaries[1].command_id, cmd1_id);
        assert_eq!(summary.soi_summaries[1].severity, "Info");
    }

    #[test]
    fn test_context_soi_summaries_after_filter() {
        use glass_soi::{OutputSummary, OutputType, ParsedOutput, Severity};

        let (db, _dir) = test_db();
        // Old command with SOI data
        insert(&db, "old cmd", "/tmp", Some(0), 1700000000);
        let old_id = 1i64;
        // New command with SOI data
        insert(&db, "new cmd", "/tmp", Some(0), 1700000100);
        let new_id = 2i64;

        let parsed = ParsedOutput {
            output_type: OutputType::FreeformText,
            summary: OutputSummary {
                one_line: "some output".to_string(),
                token_estimate: 5,
                severity: Severity::Info,
            },
            records: vec![],
            raw_line_count: 2,
            raw_byte_count: 30,
        };
        db.insert_parsed_output(old_id, &parsed).unwrap();
        db.insert_parsed_output(new_id, &parsed).unwrap();

        // Filter: only after epoch 1700000050 (excludes old command)
        let summary = build_context_summary(db.conn(), Some(1700000050)).unwrap();
        assert_eq!(summary.soi_summaries.len(), 1);
        assert_eq!(summary.soi_summaries[0].command_id, new_id);
    }
}
