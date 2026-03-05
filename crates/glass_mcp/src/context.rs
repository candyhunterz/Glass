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
            "SELECT DISTINCT cwd FROM commands WHERE started_at >= ?1 \
             ORDER BY MAX(started_at) DESC LIMIT 10",
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
