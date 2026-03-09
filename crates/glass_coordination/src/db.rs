//! Coordination database -- agent registry, file locking, and messaging.
//!
//! All write operations use `BEGIN IMMEDIATE` transactions to prevent
//! `SQLITE_BUSY` errors when multiple agents access the database concurrently.

use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;

use crate::types::AgentInfo;

/// SQLite-backed coordination database.
///
/// Manages agent registration, file locking, and inter-agent messaging.
/// Each instance owns a single `Connection`. For thread safety, open a
/// new `CoordinationDb` per thread (SQLite WAL mode supports concurrent readers).
pub struct CoordinationDb {
    conn: Connection,
}

impl CoordinationDb {
    /// Open (or create) a coordination database at the given path.
    ///
    /// Sets WAL mode and creates the schema if needed.
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
        Self::create_schema(&conn)?;
        Self::migrate(&conn)?;
        Ok(Self { conn })
    }

    /// Open the default coordination database at `~/.glass/agents.db`.
    pub fn open_default() -> Result<Self> {
        Self::open(&crate::resolve_db_path())
    }

    fn create_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS agents (
                id             TEXT PRIMARY KEY,
                name           TEXT NOT NULL,
                agent_type     TEXT NOT NULL,
                project        TEXT NOT NULL,
                cwd            TEXT NOT NULL,
                pid            INTEGER,
                status         TEXT NOT NULL DEFAULT 'active',
                task           TEXT,
                registered_at  INTEGER NOT NULL,
                last_heartbeat INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_agents_project ON agents(project);
            CREATE INDEX IF NOT EXISTS idx_agents_heartbeat ON agents(last_heartbeat);

            CREATE TABLE IF NOT EXISTS file_locks (
                path      TEXT NOT NULL,
                agent_id  TEXT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
                reason    TEXT,
                locked_at INTEGER NOT NULL,
                PRIMARY KEY (path)
            );
            CREATE INDEX IF NOT EXISTS idx_file_locks_agent ON file_locks(agent_id);

            CREATE TABLE IF NOT EXISTS messages (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                from_agent TEXT REFERENCES agents(id) ON DELETE SET NULL,
                to_agent   TEXT REFERENCES agents(id) ON DELETE CASCADE,
                msg_type   TEXT NOT NULL,
                content    TEXT NOT NULL,
                read       INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_messages_to_agent ON messages(to_agent);
            CREATE INDEX IF NOT EXISTS idx_messages_read ON messages(read);",
        )?;
        Ok(())
    }

    /// Get a reference to the underlying database connection.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    fn migrate(conn: &Connection) -> Result<()> {
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

        if version < 1 {
            conn.pragma_update(None, "user_version", 1)?;
        }

        Ok(())
    }

    /// Register a new agent and return its UUID.
    ///
    /// The `project` path is canonicalized for consistent cross-platform matching.
    pub fn register(
        &mut self,
        _name: &str,
        _agent_type: &str,
        _project: &str,
        _cwd: &str,
        _pid: Option<u32>,
    ) -> Result<String> {
        todo!("register not yet implemented")
    }

    /// Deregister an agent, releasing all its locks (via CASCADE).
    ///
    /// Returns `true` if the agent existed and was removed.
    pub fn deregister(&mut self, _agent_id: &str) -> Result<bool> {
        todo!("deregister not yet implemented")
    }

    /// Update an agent's heartbeat timestamp.
    ///
    /// Returns `true` if the agent existed and was updated.
    pub fn heartbeat(&mut self, _agent_id: &str) -> Result<bool> {
        todo!("heartbeat not yet implemented")
    }

    /// Update an agent's status and optional task description.
    ///
    /// Also implicitly refreshes the heartbeat.
    /// Returns `true` if the agent existed and was updated.
    pub fn update_status(
        &mut self,
        _agent_id: &str,
        _status: &str,
        _task: Option<&str>,
    ) -> Result<bool> {
        todo!("update_status not yet implemented")
    }

    /// List all agents registered for a given project.
    pub fn list_agents(&mut self, _project: &str) -> Result<Vec<AgentInfo>> {
        todo!("list_agents not yet implemented")
    }

    /// Prune stale agents (heartbeat timeout or dead PID).
    ///
    /// Agents whose `last_heartbeat` is older than `timeout_secs` are pruned
    /// regardless of PID status. Agents with dead PIDs are pruned even if their
    /// heartbeat is recent. CASCADE removes associated locks.
    ///
    /// Returns the list of pruned agent IDs.
    pub fn prune_stale(&mut self, _timeout_secs: i64) -> Result<Vec<String>> {
        todo!("prune_stale not yet implemented")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use tempfile::TempDir;

    fn test_db() -> (CoordinationDb, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test-agents.db");
        let db = CoordinationDb::open(&db_path).unwrap();
        (db, dir)
    }

    #[test]
    fn test_register() {
        let (mut db, _dir) = test_db();
        let id = db
            .register("agent-1", "claude-code", ".", "/tmp", Some(1234))
            .unwrap();

        // UUID v4 format: 8-4-4-4-12 hex chars with hyphens = 36 chars
        assert_eq!(id.len(), 36);
        assert_eq!(id.chars().filter(|c| *c == '-').count(), 4);

        // Agent should appear in list
        let agents = db.list_agents(".").unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].id, id);
        assert_eq!(agents[0].name, "agent-1");
        assert_eq!(agents[0].agent_type, "claude-code");
        assert_eq!(agents[0].pid, Some(1234));
        assert_eq!(agents[0].status, "active");
    }

    #[test]
    fn test_register_canonicalizes_project() {
        let (mut db, dir) = test_db();
        // Use the temp dir itself as the project path (it exists, so canonicalize works)
        let project_path = dir.path().to_str().unwrap();
        let id = db
            .register("agent-1", "claude-code", project_path, project_path, None)
            .unwrap();

        // The stored project should be the canonical form
        let canonical = crate::canonicalize_path(dir.path()).unwrap();
        let agents = db.list_agents(&canonical).unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].id, id);
    }

    #[test]
    fn test_deregister() {
        let (mut db, _dir) = test_db();
        let id = db
            .register("agent-1", "claude-code", ".", "/tmp", None)
            .unwrap();

        let removed = db.deregister(&id).unwrap();
        assert!(removed);

        let agents = db.list_agents(".").unwrap();
        assert!(agents.is_empty());

        // Deregister non-existent agent should return false
        let removed_again = db.deregister(&id).unwrap();
        assert!(!removed_again);
    }

    #[test]
    fn test_deregister_cascades_locks() {
        let (mut db, _dir) = test_db();
        let id = db
            .register("agent-1", "claude-code", ".", "/tmp", None)
            .unwrap();

        // Manually insert a file lock
        db.conn()
            .execute(
                "INSERT INTO file_locks (path, agent_id, reason, locked_at) VALUES (?1, ?2, ?3, unixepoch())",
                params!["/some/file.rs", &id, "editing"],
            )
            .unwrap();

        // Verify lock exists
        let lock_count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM file_locks", [], |row| row.get(0))
            .unwrap();
        assert_eq!(lock_count, 1);

        // Deregister should cascade-delete the lock
        db.deregister(&id).unwrap();

        let lock_count: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM file_locks", [], |row| row.get(0))
            .unwrap();
        assert_eq!(lock_count, 0);
    }

    #[test]
    fn test_deregister_preserves_messages() {
        let (mut db, _dir) = test_db();
        let id_a = db
            .register("agent-a", "claude-code", ".", "/tmp", None)
            .unwrap();
        let id_b = db
            .register("agent-b", "cursor", ".", "/tmp", None)
            .unwrap();

        // Agent A sends message to Agent B
        db.conn()
            .execute(
                "INSERT INTO messages (from_agent, to_agent, msg_type, content, created_at) VALUES (?1, ?2, ?3, ?4, unixepoch())",
                params![&id_a, &id_b, "chat", "hello from A"],
            )
            .unwrap();

        // Deregister agent A (sender)
        db.deregister(&id_a).unwrap();

        // Message should still exist with from_agent = NULL (SET NULL on delete)
        let (from_agent, content): (Option<String>, String) = db
            .conn()
            .query_row(
                "SELECT from_agent, content FROM messages WHERE to_agent = ?1",
                params![&id_b],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(from_agent.is_none(), "from_agent should be NULL after sender deregistered");
        assert_eq!(content, "hello from A");
    }

    #[test]
    fn test_heartbeat() {
        let (mut db, _dir) = test_db();
        let id = db
            .register("agent-1", "claude-code", ".", "/tmp", None)
            .unwrap();

        // Set heartbeat to old time via direct SQL
        db.conn()
            .execute(
                "UPDATE agents SET last_heartbeat = unixepoch() - 3600 WHERE id = ?1",
                params![&id],
            )
            .unwrap();

        let old_hb: i64 = db
            .conn()
            .query_row(
                "SELECT last_heartbeat FROM agents WHERE id = ?1",
                params![&id],
                |row| row.get(0),
            )
            .unwrap();

        // Heartbeat should update to recent time
        let updated = db.heartbeat(&id).unwrap();
        assert!(updated);

        let new_hb: i64 = db
            .conn()
            .query_row(
                "SELECT last_heartbeat FROM agents WHERE id = ?1",
                params![&id],
                |row| row.get(0),
            )
            .unwrap();

        assert!(new_hb > old_hb, "Heartbeat should be more recent: {new_hb} > {old_hb}");

        // Heartbeat for non-existent agent should return false
        let updated = db.heartbeat("nonexistent").unwrap();
        assert!(!updated);
    }

    #[test]
    fn test_prune_stale_by_timeout() {
        let (mut db, _dir) = test_db();
        let id = db
            .register("stale-agent", "claude-code", ".", "/tmp", None)
            .unwrap();

        // Set heartbeat to more than 10 minutes ago
        db.conn()
            .execute(
                "UPDATE agents SET last_heartbeat = unixepoch() - 700 WHERE id = ?1",
                params![&id],
            )
            .unwrap();

        let pruned = db.prune_stale(600).unwrap(); // 600 seconds = 10 minutes
        assert_eq!(pruned, vec![id.clone()]);

        // Agent should be gone
        let agents = db.list_agents(".").unwrap();
        assert!(agents.is_empty());
    }

    #[test]
    fn test_prune_stale_by_dead_pid() {
        let (mut db, _dir) = test_db();
        // Register with a PID that almost certainly doesn't exist
        let id = db
            .register("dead-pid-agent", "claude-code", ".", "/tmp", Some(999999))
            .unwrap();

        // Heartbeat is fresh, but PID is dead
        let pruned = db.prune_stale(600).unwrap();
        assert_eq!(pruned, vec![id.clone()]);

        let agents = db.list_agents(".").unwrap();
        assert!(agents.is_empty());
    }

    #[test]
    fn test_prune_stale_skips_active() {
        let (mut db, _dir) = test_db();
        // Register with our actual PID (alive) and fresh heartbeat
        let pid = std::process::id();
        let id = db
            .register("active-agent", "claude-code", ".", "/tmp", Some(pid))
            .unwrap();

        let pruned = db.prune_stale(600).unwrap();
        assert!(pruned.is_empty(), "Active agent should not be pruned");

        let agents = db.list_agents(".").unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].id, id);
    }

    #[test]
    fn test_list_agents_by_project() {
        let (mut db, _dir) = test_db();
        db.register("agent-a", "claude-code", "project-alpha", "/tmp/a", None)
            .unwrap();
        db.register("agent-b", "cursor", "project-beta", "/tmp/b", None)
            .unwrap();
        db.register("agent-c", "claude-code", "project-alpha", "/tmp/c", None)
            .unwrap();

        let alpha_agents = db.list_agents("project-alpha").unwrap();
        assert_eq!(alpha_agents.len(), 2);

        let beta_agents = db.list_agents("project-beta").unwrap();
        assert_eq!(beta_agents.len(), 1);
        assert_eq!(beta_agents[0].name, "agent-b");

        let empty = db.list_agents("project-gamma").unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn test_update_status() {
        let (mut db, _dir) = test_db();
        let id = db
            .register("agent-1", "claude-code", ".", "/tmp", None)
            .unwrap();

        let updated = db
            .update_status(&id, "editing", Some("refactoring db.rs"))
            .unwrap();
        assert!(updated);

        let agents = db.list_agents(".").unwrap();
        assert_eq!(agents[0].status, "editing");
        assert_eq!(agents[0].task.as_deref(), Some("refactoring db.rs"));

        // Update status with no task
        db.update_status(&id, "idle", None).unwrap();
        let agents = db.list_agents(".").unwrap();
        assert_eq!(agents[0].status, "idle");
        assert!(agents[0].task.is_none());

        // Non-existent agent
        let updated = db.update_status("nonexistent", "idle", None).unwrap();
        assert!(!updated);
    }
}
