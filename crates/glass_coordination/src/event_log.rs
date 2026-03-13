//! Coordination event log: types and DB operations for the activity stream.
//!
//! These are human-facing UI events about agent behavior, distinct from
//! the SOI `ActivityEvent` in `glass_core::activity_stream`.

use rusqlite::{params, Connection};

/// A single coordination event for the activity stream UI.
#[derive(Debug, Clone)]
pub struct CoordinationEvent {
    pub id: i64,
    pub timestamp: i64,
    pub project: String,
    pub category: String,
    pub agent_id: Option<String>,
    pub agent_name: Option<String>,
    pub event_type: String,
    pub summary: String,
    pub detail: Option<String>,
    pub pinned: bool,
}

/// Insert a coordination event into the database.
///
/// Called as a side-effect within existing `CoordinationDb` method transactions.
/// The `conn` parameter is the active transaction connection.
#[allow(clippy::too_many_arguments)]
pub fn insert_event(
    conn: &Connection,
    project: &str,
    category: &str,
    agent_id: Option<&str>,
    agent_name: Option<&str>,
    event_type: &str,
    summary: &str,
    detail: Option<&str>,
    pinned: bool,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO coordination_events
         (timestamp, project, category, agent_id, agent_name, event_type, summary, detail, pinned)
         VALUES (unixepoch(), ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            project,
            category,
            agent_id,
            agent_name,
            event_type,
            summary,
            detail,
            pinned as i32,
        ],
    )?;
    Ok(())
}

/// Query recent coordination events for a project.
///
/// Returns the most recent `limit` events, ordered newest-first.
pub fn recent_events(
    conn: &Connection,
    project: &str,
    limit: usize,
) -> rusqlite::Result<Vec<CoordinationEvent>> {
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, project, category, agent_id, agent_name,
                event_type, summary, detail, pinned
         FROM coordination_events
         WHERE project = ?1
         ORDER BY timestamp DESC, id DESC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![project, limit as i64], |row| {
        Ok(CoordinationEvent {
            id: row.get(0)?,
            timestamp: row.get(1)?,
            project: row.get(2)?,
            category: row.get(3)?,
            agent_id: row.get(4)?,
            agent_name: row.get(5)?,
            event_type: row.get(6)?,
            summary: row.get(7)?,
            detail: row.get(8)?,
            pinned: row.get::<_, i32>(9)? != 0,
        })
    })?;
    rows.collect()
}

/// Prune old non-pinned events, keeping at most `max_count` per project.
/// Pinned events older than 24 hours are also pruned.
pub fn prune_events(conn: &Connection, project: &str, max_count: usize) -> rusqlite::Result<()> {
    // Delete non-pinned events beyond the retention limit
    conn.execute(
        "DELETE FROM coordination_events
         WHERE project = ?1 AND pinned = 0 AND id NOT IN (
             SELECT id FROM coordination_events
             WHERE project = ?1 AND pinned = 0
             ORDER BY timestamp DESC, id DESC
             LIMIT ?2
         )",
        params![project, max_count as i64],
    )?;
    // Delete pinned events older than 24 hours
    conn.execute(
        "DELETE FROM coordination_events
         WHERE project = ?1 AND pinned = 1
         AND timestamp < unixepoch() - 86400",
        params![project],
    )?;
    Ok(())
}

/// Dismiss a pinned event by clearing its pinned flag.
pub fn dismiss_event(conn: &Connection, event_id: i64) -> rusqlite::Result<bool> {
    let rows = conn.execute(
        "UPDATE coordination_events SET pinned = 0 WHERE id = ?1",
        params![event_id],
    )?;
    Ok(rows > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE coordination_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL,
                project TEXT NOT NULL,
                category TEXT NOT NULL,
                agent_id TEXT,
                agent_name TEXT,
                event_type TEXT NOT NULL,
                summary TEXT NOT NULL,
                detail TEXT,
                pinned INTEGER DEFAULT 0
            );
            CREATE INDEX idx_coord_events_project_ts
                ON coordination_events(project, timestamp);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_insert_and_query_event() {
        let conn = setup_db();
        insert_event(
            &conn,
            "/project",
            "agent",
            Some("agent-1"),
            Some("claude-code"),
            "registered",
            "claude-code registered project: Glass",
            None,
            false,
        )
        .unwrap();

        let events = recent_events(&conn, "/project", 10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].category, "agent");
        assert_eq!(events[0].event_type, "registered");
        assert_eq!(events[0].agent_name.as_deref(), Some("claude-code"));
        assert!(!events[0].pinned);
    }

    #[test]
    fn test_pinned_event() {
        let conn = setup_db();
        insert_event(
            &conn,
            "/project",
            "lock",
            Some("agent-1"),
            Some("cursor"),
            "conflict",
            "cursor conflict pty.rs (held by claude-code)",
            None,
            true,
        )
        .unwrap();

        let events = recent_events(&conn, "/project", 10).unwrap();
        assert!(events[0].pinned);
    }

    #[test]
    fn test_recent_events_ordered_newest_first() {
        let conn = setup_db();
        for i in 0..5 {
            conn.execute(
                "INSERT INTO coordination_events
                 (timestamp, project, category, event_type, summary, pinned)
                 VALUES (?1, '/project', 'agent', 'registered', ?2, 0)",
                params![1000 + i, format!("event {}", i)],
            )
            .unwrap();
        }

        let events = recent_events(&conn, "/project", 3).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].summary, "event 4"); // newest
        assert_eq!(events[2].summary, "event 2"); // oldest of the 3
    }

    #[test]
    fn test_prune_keeps_max_count() {
        let conn = setup_db();
        for i in 0..10 {
            conn.execute(
                "INSERT INTO coordination_events
                 (timestamp, project, category, event_type, summary, pinned)
                 VALUES (?1, '/project', 'agent', 'registered', ?2, 0)",
                params![1000 + i, format!("event {}", i)],
            )
            .unwrap();
        }

        prune_events(&conn, "/project", 5).unwrap();
        let events = recent_events(&conn, "/project", 100).unwrap();
        assert_eq!(events.len(), 5);
        // Should keep the 5 newest
        assert_eq!(events[0].summary, "event 9");
        assert_eq!(events[4].summary, "event 5");
    }

    #[test]
    fn test_prune_preserves_pinned() {
        let conn = setup_db();
        // Insert 5 non-pinned + 1 pinned
        for i in 0..5 {
            conn.execute(
                "INSERT INTO coordination_events
                 (timestamp, project, category, event_type, summary, pinned)
                 VALUES (unixepoch(), '/project', 'agent', 'test', ?1, 0)",
                params![format!("normal {}", i)],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO coordination_events
             (timestamp, project, category, event_type, summary, pinned)
             VALUES (unixepoch(), '/project', 'lock', 'conflict', 'pinned event', 1)",
            [],
        )
        .unwrap();

        prune_events(&conn, "/project", 3).unwrap();
        let events = recent_events(&conn, "/project", 100).unwrap();
        // 3 non-pinned kept + 1 pinned = 4
        assert_eq!(events.len(), 4);
        assert!(events
            .iter()
            .any(|e| e.summary == "pinned event" && e.pinned));
    }

    #[test]
    fn test_dismiss_event() {
        let conn = setup_db();
        insert_event(
            &conn, "/project", "lock", None, None, "conflict", "conflict", None, true,
        )
        .unwrap();

        let events = recent_events(&conn, "/project", 10).unwrap();
        assert!(events[0].pinned);

        let dismissed = dismiss_event(&conn, events[0].id).unwrap();
        assert!(dismissed);

        let events = recent_events(&conn, "/project", 10).unwrap();
        assert!(!events[0].pinned);
    }

    #[test]
    fn test_events_scoped_by_project() {
        let conn = setup_db();
        insert_event(
            &conn,
            "/project-a",
            "agent",
            None,
            None,
            "registered",
            "a",
            None,
            false,
        )
        .unwrap();
        insert_event(
            &conn,
            "/project-b",
            "agent",
            None,
            None,
            "registered",
            "b",
            None,
            false,
        )
        .unwrap();

        let a_events = recent_events(&conn, "/project-a", 10).unwrap();
        assert_eq!(a_events.len(), 1);
        assert_eq!(a_events[0].summary, "a");

        let b_events = recent_events(&conn, "/project-b", 10).unwrap();
        assert_eq!(b_events.len(), 1);
        assert_eq!(b_events[0].summary, "b");
    }
}
