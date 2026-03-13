//! SQLite-backed persistence for agent session handoff records.
//!
//! Stores rows in `~/.glass/agents.db` (the same file used by `CoordinationDb`
//! and `WorktreeDb`). The `agent_sessions` table is created by migration version 3.
//!
//! When an agent subprocess emits a `GLASS_HANDOFF` marker at session end,
//! `insert_session` persists the parsed data. On the next session start,
//! `load_prior_handoff` retrieves the most recent record for the project root
//! so the agent can resume from where it left off.

use std::path::Path;

use anyhow::Result;
use rusqlite::{params, Connection, TransactionBehavior};

use crate::types::{AgentSessionRecord, HandoffData};

/// SQLite database handle for agent session handoff tracking.
pub struct AgentSessionDb {
    conn: Connection,
}

impl AgentSessionDb {
    /// Open (or create) the database at `path`.
    ///
    /// Enables WAL mode, sets busy timeout, and runs migrations up to version 3.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;
             PRAGMA foreign_keys = ON;",
        )?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    /// Open the default database at `~/.glass/agents.db`.
    ///
    /// This is the same physical file used by `CoordinationDb` and `WorktreeDb`.
    pub fn open_default() -> Result<Self> {
        let db_path = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not resolve home directory"))?
            .join(".glass")
            .join("agents.db");
        Self::open(&db_path)
    }

    /// Persist a handoff record for the given session.
    ///
    /// Uses an immediate transaction to avoid write contention with `WorktreeDb`.
    pub fn insert_session(&mut self, record: &AgentSessionRecord) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO agent_sessions
                 (id, project_root, session_id, previous_session_id,
                  work_completed, work_remaining, key_decisions,
                  raw_handoff, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                record.id,
                record.project_root,
                record.session_id,
                record.previous_session_id,
                record.handoff.work_completed,
                record.handoff.work_remaining,
                record.handoff.key_decisions,
                record.raw_handoff,
                record.created_at,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Return the most recent handoff record for the given project root, if any.
    ///
    /// Returns `None` when the table has no rows for this project.
    pub fn load_prior_handoff(&self, project_root: &str) -> Result<Option<AgentSessionRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_root, session_id, previous_session_id,
                    work_completed, work_remaining, key_decisions,
                    raw_handoff, created_at
             FROM agent_sessions
             WHERE project_root = ?1
             ORDER BY created_at DESC
             LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![project_root], |row| {
            let work_completed: String = row.get(4)?;
            let work_remaining: String = row.get(5)?;
            let key_decisions: String = row.get(6)?;
            let previous_session_id: Option<String> = row.get(3)?;
            Ok(AgentSessionRecord {
                id: row.get(0)?,
                project_root: row.get(1)?,
                session_id: row.get(2)?,
                previous_session_id: previous_session_id.clone(),
                handoff: HandoffData {
                    work_completed,
                    work_remaining,
                    key_decisions,
                    previous_session_id,
                },
                raw_handoff: row.get(7)?,
                created_at: row.get(8)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }
}

/// Run all migrations. Creates tables idempotently up to version 3.
///
/// Version 1 is owned by `CoordinationDb`. If this db is opened standalone
/// (e.g. in tests with version 0), we bump to 1 so our v2/v3 guards can run.
/// Version 2 adds `pending_worktrees` (same DDL as WorktreeDb — IF NOT EXISTS
/// guards prevent conflicts when both migrate() functions run on the same file).
/// Version 3 adds `agent_sessions`.
fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if version < 1 {
        // Standalone / test mode: bump past CoordinationDb's territory.
        conn.pragma_update(None, "user_version", 1i64)?;
    }

    if version < 2 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS pending_worktrees (
                id             TEXT PRIMARY KEY,
                worktree_path  TEXT NOT NULL,
                project_root   TEXT NOT NULL,
                proposal_id    TEXT NOT NULL,
                created_at     INTEGER NOT NULL DEFAULT (unixepoch())
            );",
        )?;
        conn.pragma_update(None, "user_version", 2i64)?;
    }

    if version < 3 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS agent_sessions (
                id                  TEXT PRIMARY KEY,
                project_root        TEXT NOT NULL,
                session_id          TEXT NOT NULL,
                previous_session_id TEXT,
                work_completed      TEXT NOT NULL,
                work_remaining      TEXT NOT NULL,
                key_decisions       TEXT NOT NULL,
                raw_handoff         TEXT NOT NULL,
                created_at          INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE INDEX IF NOT EXISTS idx_agent_sessions_project
                ON agent_sessions(project_root, created_at DESC);",
        )?;
        conn.pragma_update(None, "user_version", 3i64)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn open_test_db() -> (AgentSessionDb, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test_agents.db");
        let db = AgentSessionDb::open(&db_path).unwrap();
        (db, dir)
    }

    fn make_record(
        id: &str,
        project_root: &str,
        session_id: &str,
        previous_session_id: Option<&str>,
        created_at: i64,
    ) -> AgentSessionRecord {
        let prev = previous_session_id.map(|s| s.to_string());
        AgentSessionRecord {
            id: id.to_string(),
            project_root: project_root.to_string(),
            session_id: session_id.to_string(),
            previous_session_id: prev.clone(),
            handoff: HandoffData {
                work_completed: "Implemented feature X".to_string(),
                work_remaining: "Write tests for Y".to_string(),
                key_decisions: "Used approach Z".to_string(),
                previous_session_id: prev,
            },
            raw_handoff: r#"{"work_completed":"Implemented feature X","work_remaining":"Write tests for Y","key_decisions":"Used approach Z"}"#.to_string(),
            created_at,
        }
    }

    #[test]
    fn handoff_data_deserializes_with_all_fields() {
        let json = r#"{
            "work_completed": "Refactored module A",
            "work_remaining": "Add integration tests",
            "key_decisions": "Used trait objects for extensibility",
            "previous_session_id": "sess-abc-123"
        }"#;
        let data: HandoffData = serde_json::from_str(json).unwrap();
        assert_eq!(data.work_completed, "Refactored module A");
        assert_eq!(data.work_remaining, "Add integration tests");
        assert_eq!(data.key_decisions, "Used trait objects for extensibility");
        assert_eq!(data.previous_session_id, Some("sess-abc-123".to_string()));
    }

    #[test]
    fn handoff_data_deserializes_without_previous_session_id() {
        let json = r#"{
            "work_completed": "Initial setup",
            "work_remaining": "Everything else",
            "key_decisions": "Chose SQLite"
        }"#;
        let data: HandoffData = serde_json::from_str(json).unwrap();
        assert_eq!(data.previous_session_id, None);
    }

    #[test]
    fn insert_session_and_load_prior_handoff_roundtrip() {
        let (mut db, _dir) = open_test_db();
        let record = make_record("id-1", "/project/root", "sess-1", None, 1000);
        db.insert_session(&record).unwrap();

        let loaded = db
            .load_prior_handoff("/project/root")
            .unwrap()
            .expect("should return a record");
        assert_eq!(loaded.id, "id-1");
        assert_eq!(loaded.session_id, "sess-1");
        assert_eq!(loaded.project_root, "/project/root");
        assert_eq!(loaded.previous_session_id, None);
        assert_eq!(loaded.handoff.work_completed, "Implemented feature X");
        assert_eq!(loaded.handoff.work_remaining, "Write tests for Y");
        assert_eq!(loaded.handoff.key_decisions, "Used approach Z");
    }

    #[test]
    fn load_prior_handoff_returns_none_on_empty_table() {
        let (db, _dir) = open_test_db();
        let result = db.load_prior_handoff("/some/project").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn load_prior_handoff_returns_most_recent_by_created_at() {
        let (mut db, _dir) = open_test_db();
        let root = "/my/project";

        // Insert older record first
        let old = make_record("id-old", root, "sess-old", None, 500);
        db.insert_session(&old).unwrap();

        // Insert newer record
        let new = make_record("id-new", root, "sess-new", Some("sess-old"), 1500);
        db.insert_session(&new).unwrap();

        // Insert middle record
        let mid = make_record("id-mid", root, "sess-mid", Some("sess-old"), 1000);
        db.insert_session(&mid).unwrap();

        let loaded = db
            .load_prior_handoff(root)
            .unwrap()
            .expect("should return a record");
        assert_eq!(
            loaded.id, "id-new",
            "should return the record with highest created_at"
        );
        assert_eq!(loaded.previous_session_id, Some("sess-old".to_string()));
    }

    #[test]
    fn session_record_survives_connection_close_and_reopen() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("agents.db");
        let record = make_record("id-persist", "/project", "sess-persist", None, 2000);

        // First "process": open, insert, drop
        {
            let mut db = AgentSessionDb::open(&db_path).unwrap();
            db.insert_session(&record).unwrap();
        }

        // Second "process": reopen, verify
        {
            let db = AgentSessionDb::open(&db_path).unwrap();
            let loaded = db
                .load_prior_handoff("/project")
                .unwrap()
                .expect("record must survive restart");
            assert_eq!(loaded.id, "id-persist");
            assert_eq!(loaded.session_id, "sess-persist");
        }
    }

    #[test]
    fn migration_sets_user_version_to_3() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("agents.db");
        let db = AgentSessionDb::open(&db_path).unwrap();
        let version: i64 = db
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 3, "Migration must set user_version to 3");
    }

    #[test]
    fn three_records_form_traversable_linked_list() {
        let (mut db, _dir) = open_test_db();
        let root = "/chain/project";

        let r1 = make_record("id-1", root, "sess-1", None, 100);
        let r2 = make_record("id-2", root, "sess-2", Some("sess-1"), 200);
        let r3 = make_record("id-3", root, "sess-3", Some("sess-2"), 300);

        db.insert_session(&r1).unwrap();
        db.insert_session(&r2).unwrap();
        db.insert_session(&r3).unwrap();

        // The most recent is sess-3
        let top = db.load_prior_handoff(root).unwrap().unwrap();
        assert_eq!(top.session_id, "sess-3");
        assert_eq!(top.previous_session_id, Some("sess-2".to_string()));

        // Walk back: look up sess-2
        let prev_id = top.previous_session_id.unwrap();
        // We can query by session_id directly to verify the chain
        let mut stmt = db
            .conn
            .prepare(
                "SELECT id, session_id, previous_session_id FROM agent_sessions \
                 WHERE session_id = ?1 AND project_root = ?2",
            )
            .unwrap();
        let (_, sid2, prev2): (String, String, Option<String>) = stmt
            .query_row(params![prev_id, root], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap();
        assert_eq!(sid2, "sess-2");
        assert_eq!(prev2, Some("sess-1".to_string()));
    }

    /// Verify that pending_worktrees table still works after session_db migrates
    /// to version 3 (regression guard for worktree_db).
    #[test]
    fn pending_worktrees_table_unaffected_by_v3_migration() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("agents.db");

        // Open via AgentSessionDb (which migrates to v3)
        let db = AgentSessionDb::open(&db_path).unwrap();

        // pending_worktrees table should exist and be queryable
        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM pending_worktrees",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "pending_worktrees should exist and be empty");
    }
}
