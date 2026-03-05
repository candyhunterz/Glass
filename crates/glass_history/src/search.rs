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
pub fn search(conn: &Connection, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
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
        .query_map(params![query, limit as i64], |row| {
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
