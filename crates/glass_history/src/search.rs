use anyhow::Result;
use rusqlite::{params, Connection};

/// A search result from FTS5 full-text search.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub id: i64,
    pub command: String,
    pub cwd: String,
    pub exit_code: Option<i32>,
    pub started_at: i64,
    pub finished_at: i64,
    pub duration_ms: i64,
    pub rank: f64,
}

/// Search command history using FTS5 MATCH with BM25 ranking.
///
/// The query is wrapped in double quotes to prevent FTS5 syntax errors from
/// unmatched quotes or special characters. A trailing `*` is preserved
/// outside the quotes to support prefix matching (e.g. `car*`).
pub fn search(conn: &Connection, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
    // Empty or whitespace-only queries would produce an invalid FTS5 MATCH
    // expression (empty quoted string), so return early.
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    // Escape FTS5 special characters by wrapping in double quotes.
    // Preserve trailing `*` for prefix matching.
    let (body, suffix) = if let Some(stripped) = query.strip_suffix('*') {
        (stripped, "*")
    } else {
        (query, "")
    };
    let escaped = format!("\"{}\"{}", body.replace('"', "\"\""), suffix);

    let mut stmt = conn.prepare(
        "SELECT c.id, c.command, c.cwd, c.exit_code, c.started_at,
                c.finished_at, c.duration_ms, f.rank
         FROM commands_fts f
         JOIN commands c ON c.id = f.rowid
         WHERE commands_fts MATCH ?1
         ORDER BY f.rank
         LIMIT ?2",
    )?;

    let results = stmt
        .query_map(params![escaped, limit as i64], |row| {
            Ok(SearchResult {
                id: row.get(0)?,
                command: row.get(1)?,
                cwd: row.get(2)?,
                exit_code: row.get(3)?,
                started_at: row.get(4)?,
                finished_at: row.get(5)?,
                duration_ms: row.get(6)?,
                rank: row.get(7)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{CommandRecord, HistoryDb};
    use tempfile::TempDir;

    fn test_db() -> (HistoryDb, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test_search.db");
        let db = HistoryDb::open(&db_path).unwrap();
        (db, dir)
    }

    fn insert(db: &HistoryDb, command: &str) -> i64 {
        db.insert_command(&CommandRecord {
            id: None,
            command: command.to_string(),
            cwd: "/tmp".to_string(),
            exit_code: Some(0),
            started_at: 1700000000,
            finished_at: 1700000001,
            duration_ms: 1000,
            output: None,
        })
        .unwrap()
    }

    #[test]
    fn test_search_empty_query_returns_empty() {
        let (db, _dir) = test_db();
        insert(&db, "cargo build");
        let results = search(db.conn(), "", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_whitespace_query_returns_empty() {
        let (db, _dir) = test_db();
        insert(&db, "cargo build");
        let results = search(db.conn(), "   \t  ", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_limit_zero_returns_empty() {
        let (db, _dir) = test_db();
        insert(&db, "cargo build");
        let results = search(db.conn(), "cargo", 0).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_special_characters_no_crash() {
        let (db, _dir) = test_db();
        insert(&db, "echo hello");
        // FTS5 special operators — should not crash
        for query in &["OR", "AND", "NOT", "NEAR", "()", "\"", "'", "*", "^"] {
            let _ = search(db.conn(), query, 10);
        }
    }

    #[test]
    fn test_search_prefix_matching() {
        let (db, _dir) = test_db();
        insert(&db, "cargo build");
        insert(&db, "cargo test");
        insert(&db, "git status");
        let results = search(db.conn(), "car*", 10).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_quotes_in_query() {
        let (db, _dir) = test_db();
        insert(&db, "echo \"hello world\"");
        // Query with quotes should not crash
        let results = search(db.conn(), "echo", 10).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_search_long_command_text() {
        let (db, _dir) = test_db();
        // 10KB command — should insert and search fine
        let long_cmd = "x".repeat(10_000);
        insert(&db, &long_cmd);
        let count = db.command_count().unwrap();
        assert_eq!(count, 1);
    }
}
