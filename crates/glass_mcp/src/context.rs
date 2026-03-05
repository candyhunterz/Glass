//! Aggregate query logic for the GlassContext MCP tool.
//!
//! Provides a high-level activity summary (command counts, failure rate,
//! time range, active directories) from the Glass history database.

use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;

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
}

/// Build an aggregate activity summary from the commands table.
///
/// If `after` is Some, only commands with `started_at >= after` are included.
/// Uses parameterized queries for safety.
pub fn build_context_summary(
    conn: &Connection,
    after: Option<i64>,
) -> Result<ContextSummary> {
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
    let (command_count, failure_count, earliest_timestamp, latest_timestamp) = stmt
        .query_row(rusqlite::params_from_iter(params.iter()), |row| {
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

    Ok(ContextSummary {
        command_count,
        failure_count,
        earliest_timestamp,
        latest_timestamp,
        recent_directories,
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
}
