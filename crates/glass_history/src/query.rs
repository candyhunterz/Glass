//! Filtered query module for combining FTS5 search with SQL WHERE clauses.
//!
//! Provides `QueryFilter` for building dynamic queries that combine text search,
//! exit code filtering, time range filtering, CWD prefix matching, and result limiting.

use anyhow::{bail, Result};
use chrono::Utc;
use rusqlite::Connection;

use crate::db::CommandRecord;

/// Filter criteria for querying command history.
///
/// All fields are optional -- omitted filters are ignored.
/// When multiple filters are set, results must match ALL of them (AND logic).
#[derive(Debug, Clone)]
pub struct QueryFilter {
    /// FTS5 text search on command text.
    pub text: Option<String>,
    /// Filter by exact exit code.
    pub exit_code: Option<i32>,
    /// Only records with started_at >= this epoch.
    pub after: Option<i64>,
    /// Only records with started_at <= this epoch.
    pub before: Option<i64>,
    /// CWD prefix match (e.g. "/home" matches "/home/user/project").
    pub cwd: Option<String>,
    /// Maximum number of results to return.
    pub limit: usize,
}

impl Default for QueryFilter {
    fn default() -> Self {
        Self {
            text: None,
            exit_code: None,
            after: None,
            before: None,
            cwd: None,
            limit: 25,
        }
    }
}

impl QueryFilter {
    /// Create a new QueryFilter with default values (no filters, limit 25).
    pub fn new() -> Self {
        Self::default()
    }
}

/// Parse a time string into a Unix epoch timestamp.
///
/// Supports:
/// - Relative durations: "30m" (minutes), "1h" (hours), "2d" (days)
/// - ISO 8601 dates: "2024-01-15"
/// - ISO 8601 datetimes: "2024-01-15T14:30:00"
pub fn parse_time(input: &str) -> Result<i64> {
    let input = input.trim();

    // Try relative time: number + suffix (m/h/d)
    if input.len() >= 2 {
        let (num_part, suffix) = input.split_at(input.len() - 1);
        if let Ok(n) = num_part.parse::<i64>() {
            let duration = match suffix {
                "m" => chrono::Duration::minutes(n),
                "h" => chrono::Duration::hours(n),
                "d" => chrono::Duration::days(n),
                _ => {
                    // Not a relative time, fall through to date parsing
                    return parse_date_time(input);
                }
            };
            let target = Utc::now() - duration;
            return Ok(target.timestamp());
        }
    }

    parse_date_time(input)
}

fn parse_date_time(input: &str) -> Result<i64> {
    // Try ISO 8601 datetime: "2024-01-15T14:30:00"
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(input, "%Y-%m-%dT%H:%M:%S") {
        let utc = dt.and_utc();
        return Ok(utc.timestamp());
    }

    // Try ISO 8601 date: "2024-01-15"
    if let Ok(date) = chrono::NaiveDate::parse_from_str(input, "%Y-%m-%d") {
        let dt = date
            .and_hms_opt(0, 0, 0)
            .expect("midnight should always be valid");
        let utc = dt.and_utc();
        return Ok(utc.timestamp());
    }

    bail!(
        "Cannot parse time '{}'. Expected relative (e.g. 30m, 1h, 2d) or ISO date (e.g. 2024-01-15)",
        input
    )
}

/// Execute a filtered query against the command history database.
///
/// When `filter.text` is set, uses FTS5 MATCH for full-text search.
/// Other filters are applied as SQL WHERE clauses.
/// Results are ordered by started_at DESC and limited by `filter.limit`.
pub fn filtered_query(conn: &Connection, filter: &QueryFilter) -> Result<Vec<CommandRecord>> {
    let mut sql = String::new();
    let mut params: Vec<rusqlite::types::Value> = Vec::new();
    let mut conditions: Vec<String> = Vec::new();

    // Use FTS5 only when text filter is non-empty after trimming;
    // an empty quoted string is not valid FTS5 syntax.
    let use_fts = filter.text.as_ref().is_some_and(|t| !t.trim().is_empty());

    if use_fts {
        let text = filter.text.as_ref().unwrap();
        sql.push_str(
            "SELECT c.id, c.command, c.cwd, c.exit_code, c.started_at, \
             c.finished_at, c.duration_ms, c.output \
             FROM commands_fts f \
             JOIN commands c ON c.id = f.rowid",
        );
        // Escape FTS5 special characters by wrapping in double quotes
        let escaped = format!("\"{}\"", text.trim().replace('"', "\"\""));
        conditions.push("commands_fts MATCH ?".to_string());
        params.push(rusqlite::types::Value::Text(escaped));
    } else {
        sql.push_str(
            "SELECT c.id, c.command, c.cwd, c.exit_code, c.started_at, \
             c.finished_at, c.duration_ms, c.output \
             FROM commands c",
        );
    }

    if let Some(code) = filter.exit_code {
        conditions.push("c.exit_code = ?".to_string());
        params.push(rusqlite::types::Value::Integer(code as i64));
    }

    if let Some(after) = filter.after {
        conditions.push("c.started_at >= ?".to_string());
        params.push(rusqlite::types::Value::Integer(after));
    }

    if let Some(before) = filter.before {
        conditions.push("c.started_at <= ?".to_string());
        params.push(rusqlite::types::Value::Integer(before));
    }

    if let Some(ref cwd) = filter.cwd {
        let escaped_cwd = cwd.replace('%', "\\%").replace('_', "\\_");
        conditions.push("c.cwd LIKE ? ESCAPE '\\'".to_string());
        params.push(rusqlite::types::Value::Text(format!("{}%", escaped_cwd)));
    }

    if !conditions.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&conditions.join(" AND "));
    }

    sql.push_str(" ORDER BY c.started_at DESC LIMIT ?");
    params.push(rusqlite::types::Value::Integer(filter.limit as i64));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), |row| {
        Ok(CommandRecord {
            id: Some(row.get(0)?),
            command: row.get(1)?,
            cwd: row.get(2)?,
            exit_code: row.get(3)?,
            started_at: row.get(4)?,
            finished_at: row.get(5)?,
            duration_ms: row.get(6)?,
            output: row.get(7)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::HistoryDb;
    use tempfile::TempDir;

    fn test_db() -> (HistoryDb, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test_query.db");
        let db = HistoryDb::open(&db_path).unwrap();
        (db, dir)
    }

    fn insert_record(
        db: &HistoryDb,
        command: &str,
        cwd: &str,
        exit_code: Option<i32>,
        started_at: i64,
    ) -> i64 {
        let record = CommandRecord {
            id: None,
            command: command.to_string(),
            cwd: cwd.to_string(),
            exit_code,
            started_at,
            finished_at: started_at + 5,
            duration_ms: 5000,
            output: None,
        };
        db.insert_command(&record).unwrap()
    }

    #[test]
    fn test_no_filters_returns_all() {
        let (db, _dir) = test_db();
        insert_record(&db, "cargo build", "/home/user", Some(0), 1700000000);
        insert_record(&db, "cargo test", "/home/user", Some(0), 1700000010);
        insert_record(&db, "git status", "/home/user", Some(0), 1700000020);

        let filter = QueryFilter::new();
        let results = filtered_query(db.conn(), &filter).unwrap();
        assert_eq!(results.len(), 3);
        // Ordered by started_at DESC
        assert_eq!(results[0].command, "git status");
        assert_eq!(results[1].command, "cargo test");
        assert_eq!(results[2].command, "cargo build");
    }

    #[test]
    fn test_text_filter_uses_fts5() {
        let (db, _dir) = test_db();
        insert_record(&db, "cargo build", "/home/user", Some(0), 1700000000);
        insert_record(&db, "cargo test", "/home/user", Some(0), 1700000010);
        insert_record(&db, "git status", "/home/user", Some(0), 1700000020);

        let filter = QueryFilter {
            text: Some("cargo".to_string()),
            ..QueryFilter::new()
        };
        let results = filtered_query(db.conn(), &filter).unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.command.contains("cargo")));
    }

    #[test]
    fn test_exit_code_filter() {
        let (db, _dir) = test_db();
        insert_record(&db, "cargo build", "/home/user", Some(0), 1700000000);
        insert_record(&db, "bad command", "/home/user", Some(1), 1700000010);
        insert_record(&db, "another fail", "/home/user", Some(1), 1700000020);

        let filter = QueryFilter {
            exit_code: Some(1),
            ..QueryFilter::new()
        };
        let results = filtered_query(db.conn(), &filter).unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.exit_code == Some(1)));
    }

    #[test]
    fn test_after_filter() {
        let (db, _dir) = test_db();
        insert_record(&db, "old cmd", "/home/user", Some(0), 1700000000);
        insert_record(&db, "new cmd", "/home/user", Some(0), 1700000100);
        insert_record(&db, "newest cmd", "/home/user", Some(0), 1700000200);

        let filter = QueryFilter {
            after: Some(1700000050),
            ..QueryFilter::new()
        };
        let results = filtered_query(db.conn(), &filter).unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.started_at >= 1700000050));
    }

    #[test]
    fn test_before_filter() {
        let (db, _dir) = test_db();
        insert_record(&db, "old cmd", "/home/user", Some(0), 1700000000);
        insert_record(&db, "new cmd", "/home/user", Some(0), 1700000100);
        insert_record(&db, "newest cmd", "/home/user", Some(0), 1700000200);

        let filter = QueryFilter {
            before: Some(1700000050),
            ..QueryFilter::new()
        };
        let results = filtered_query(db.conn(), &filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].command, "old cmd");
    }

    #[test]
    fn test_cwd_prefix_matching() {
        let (db, _dir) = test_db();
        insert_record(&db, "cmd1", "/home/user/project", Some(0), 1700000000);
        insert_record(&db, "cmd2", "/home/user/other", Some(0), 1700000010);
        insert_record(&db, "cmd3", "/tmp/stuff", Some(0), 1700000020);

        let filter = QueryFilter {
            cwd: Some("/home".to_string()),
            ..QueryFilter::new()
        };
        let results = filtered_query(db.conn(), &filter).unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.cwd.starts_with("/home")));
    }

    #[test]
    fn test_limit() {
        let (db, _dir) = test_db();
        for i in 0..5 {
            insert_record(
                &db,
                &format!("cmd{}", i),
                "/home/user",
                Some(0),
                1700000000 + i * 10,
            );
        }

        let filter = QueryFilter {
            limit: 2,
            ..QueryFilter::new()
        };
        let results = filtered_query(db.conn(), &filter).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_combined_filters() {
        let (db, _dir) = test_db();
        insert_record(
            &db,
            "cargo build",
            "/home/user/project",
            Some(0),
            1700000000,
        );
        insert_record(&db, "cargo test", "/home/user/project", Some(1), 1700000010);
        insert_record(&db, "cargo build", "/tmp/other", Some(0), 1700000020);
        insert_record(&db, "git push", "/home/user/project", Some(0), 1700000030);

        let filter = QueryFilter {
            text: Some("cargo".to_string()),
            exit_code: Some(0),
            cwd: Some("/home".to_string()),
            ..QueryFilter::new()
        };
        let results = filtered_query(db.conn(), &filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].command, "cargo build");
        assert_eq!(results[0].cwd, "/home/user/project");
    }

    #[test]
    fn test_parse_time_minutes() {
        let before = Utc::now().timestamp();
        let result = parse_time("30m").unwrap();
        let expected_approx = before - 30 * 60;
        // Allow 2 seconds tolerance
        assert!((result - expected_approx).abs() < 2);
    }

    #[test]
    fn test_parse_time_hours() {
        let before = Utc::now().timestamp();
        let result = parse_time("1h").unwrap();
        let expected_approx = before - 3600;
        assert!((result - expected_approx).abs() < 2);
    }

    #[test]
    fn test_parse_time_days() {
        let before = Utc::now().timestamp();
        let result = parse_time("2d").unwrap();
        let expected_approx = before - 2 * 86400;
        assert!((result - expected_approx).abs() < 2);
    }

    #[test]
    fn test_parse_time_iso_date() {
        let result = parse_time("2024-01-15").unwrap();
        // 2024-01-15 00:00:00 UTC
        let expected = chrono::NaiveDate::from_ymd_opt(2024, 1, 15)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp();
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_time_invalid() {
        assert!(parse_time("invalid").is_err());
        assert!(parse_time("").is_err());
        assert!(parse_time("abc123").is_err());
    }

    #[test]
    fn test_fts5_special_characters_escaped() {
        let (db, _dir) = test_db();
        insert_record(
            &db,
            "echo \"hello world\"",
            "/home/user",
            Some(0),
            1700000000,
        );
        insert_record(&db, "normal command", "/home/user", Some(0), 1700000010);

        // Search with quotes -- should not crash
        let filter = QueryFilter {
            text: Some("echo".to_string()),
            ..QueryFilter::new()
        };
        let results = filtered_query(db.conn(), &filter).unwrap();
        assert_eq!(results.len(), 1);

        // Search with special FTS5 chars
        let filter = QueryFilter {
            text: Some("hello\"world".to_string()),
            ..QueryFilter::new()
        };
        // Should not crash, may or may not return results
        let _results = filtered_query(db.conn(), &filter);
    }

    #[test]
    fn test_empty_text_filter_returns_all() {
        let (db, _dir) = test_db();
        insert_record(&db, "cargo build", "/home/user", Some(0), 1700000000);
        insert_record(&db, "git status", "/home/user", Some(0), 1700000010);

        // Empty text filter should fall back to non-FTS query (return all)
        let filter = QueryFilter {
            text: Some("".to_string()),
            ..QueryFilter::new()
        };
        let results = filtered_query(db.conn(), &filter).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_whitespace_text_filter_returns_all() {
        let (db, _dir) = test_db();
        insert_record(&db, "cargo build", "/home/user", Some(0), 1700000000);
        insert_record(&db, "git status", "/home/user", Some(0), 1700000010);

        // Whitespace-only text filter should fall back to non-FTS query
        let filter = QueryFilter {
            text: Some("   ".to_string()),
            ..QueryFilter::new()
        };
        let results = filtered_query(db.conn(), &filter).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_cwd_wildcard_characters_escaped() {
        let (db, _dir) = test_db();
        insert_record(&db, "cmd1", "/home/user", Some(0), 1700000000);
        insert_record(&db, "cmd2", "/tmp/test", Some(0), 1700000010);

        // % and _ in the cwd filter are escaped so they match literal characters,
        // not LIKE wildcards. A bare "%" should match nothing (no cwd starts with literal %).
        let filter = QueryFilter {
            cwd: Some("%".to_string()),
            ..QueryFilter::new()
        };
        let results = filtered_query(db.conn(), &filter).unwrap();
        assert_eq!(
            results.len(),
            0,
            "% in cwd filter should be escaped, not match all"
        );

        // Underscore should also be treated literally.
        let filter = QueryFilter {
            cwd: Some("_".to_string()),
            ..QueryFilter::new()
        };
        let results = filtered_query(db.conn(), &filter).unwrap();
        assert_eq!(
            results.len(),
            0,
            "_ in cwd filter should be escaped, not match any single char"
        );
    }
}
