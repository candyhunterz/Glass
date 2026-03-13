//! SQLite-backed persistence for pending agent worktrees.
//!
//! Stores rows in `~/.glass/agents.db` (the same file used by `CoordinationDb`).
//! The `pending_worktrees` table is created by migration version 2.
//!
//! The "register-before-create" pattern: a row is inserted BEFORE the git
//! worktree is created on disk. If the process crashes between the INSERT and
//! the filesystem creation, `prune_orphans` detects the stale row on next startup.

use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::{params, Connection, TransactionBehavior};

use crate::types::PendingWorktree;

/// SQLite database handle for pending-worktree tracking.
pub struct WorktreeDb {
    conn: Connection,
}

impl WorktreeDb {
    /// Open (or create) the database at `path`.
    ///
    /// Enables WAL mode, sets busy timeout, and runs migrations.
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
    /// This is the same physical file used by `CoordinationDb`.
    pub fn open_default() -> Result<Self> {
        let db_path = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not resolve home directory"))?
            .join(".glass")
            .join("agents.db");
        Self::open(&db_path)
    }

    /// Insert a row for a worktree that is about to be created.
    ///
    /// Must be called BEFORE the git worktree or directory is created on disk
    /// (the crash-recovery invariant).
    pub fn insert_pending_worktree(
        &mut self,
        id: &str,
        worktree_path: &Path,
        project_root: &Path,
        proposal_id: &str,
    ) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO pending_worktrees (id, worktree_path, project_root, proposal_id)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                id,
                worktree_path.to_string_lossy().as_ref(),
                project_root.to_string_lossy().as_ref(),
                proposal_id,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Return all pending worktree rows.
    pub fn list_pending_worktrees(&self) -> Result<Vec<PendingWorktree>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, worktree_path, project_root, proposal_id, created_at
             FROM pending_worktrees",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(PendingWorktree {
                id: row.get(0)?,
                worktree_path: PathBuf::from(row.get::<_, String>(1)?),
                project_root: PathBuf::from(row.get::<_, String>(2)?),
                proposal_id: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Remove a pending worktree row by id.
    ///
    /// Called after `apply` or `dismiss` completes, or after orphan pruning.
    pub fn delete_pending_worktree(&mut self, id: &str) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM pending_worktrees WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }
}

/// Run migrations. Creates `pending_worktrees` at schema version 2,
/// and `agent_sessions` at schema version 3.
///
/// Version 1 is owned by `CoordinationDb` (agents, file_locks, messages tables).
/// Version 2 adds `pending_worktrees`.
/// Version 3 adds `agent_sessions` (same DDL as session_db.rs -- IF NOT EXISTS guards
/// prevent conflicts when both migrate() functions run on the same physical file).
fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if version < 1 {
        // Version 1 is CoordinationDb territory; if we're here with version 0
        // it means we're running standalone (e.g., in tests). Set to 1 to
        // allow our version-2 guard to run.
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

    fn open_test_db() -> (WorktreeDb, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test_agents.db");
        let db = WorktreeDb::open(&db_path).unwrap();
        (db, dir)
    }

    #[test]
    fn test_open_creates_table() {
        let (db, _dir) = open_test_db();
        // If the table was created, we can list (empty) without error
        let rows = db.list_pending_worktrees().unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn test_insert_and_list() {
        let (mut db, dir) = open_test_db();
        let worktree_path = dir.path().join("wt-uuid-123");
        let project_root = dir.path().join("project");

        db.insert_pending_worktree("uuid-123", &worktree_path, &project_root, "proposal-1")
            .unwrap();

        let rows = db.list_pending_worktrees().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "uuid-123");
        assert_eq!(rows[0].worktree_path, worktree_path);
        assert_eq!(rows[0].project_root, project_root);
        assert_eq!(rows[0].proposal_id, "proposal-1");
    }

    #[test]
    fn test_delete_removes_row() {
        let (mut db, dir) = open_test_db();
        let worktree_path = dir.path().join("wt-uuid-456");
        let project_root = dir.path().join("project");

        db.insert_pending_worktree("uuid-456", &worktree_path, &project_root, "proposal-2")
            .unwrap();
        db.delete_pending_worktree("uuid-456").unwrap();

        let rows = db.list_pending_worktrees().unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn test_list_empty_on_fresh_db() {
        let (db, _dir) = open_test_db();
        let rows = db.list_pending_worktrees().unwrap();
        assert!(rows.is_empty(), "Fresh DB should have no rows");
    }

    #[test]
    fn test_pending_row_survives_restart() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("agents.db");
        let worktree_path = dir.path().join("wt-persist");
        let project_root = dir.path().join("project");

        // First "process": open DB, insert row, drop connection
        {
            let mut db = WorktreeDb::open(&db_path).unwrap();
            db.insert_pending_worktree(
                "uuid-persist",
                &worktree_path,
                &project_root,
                "proposal-persist",
            )
            .unwrap();
        }

        // Second "process": reopen DB, verify row is still there
        {
            let db = WorktreeDb::open(&db_path).unwrap();
            let rows = db.list_pending_worktrees().unwrap();
            assert_eq!(rows.len(), 1, "Row should survive connection close+reopen");
            assert_eq!(rows[0].id, "uuid-persist");
        }
    }

    #[test]
    fn test_migration_runs_to_version_3() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("agents.db");
        let db = WorktreeDb::open(&db_path).unwrap();
        let version: i64 = db
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 3, "Migration should set user_version to 3");
    }
}
