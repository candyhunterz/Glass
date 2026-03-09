//! Coordination database -- agent registry, file locking, and messaging.
//!
//! All write operations use `BEGIN IMMEDIATE` transactions to prevent
//! `SQLITE_BUSY` errors when multiple agents access the database concurrently.

use std::path::Path;

use anyhow::Result;
use rusqlite::{params, Connection, TransactionBehavior};

use crate::types::{AgentInfo, FileLock, LockConflict, LockResult};

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
    /// If canonicalization fails (e.g., path doesn't exist), the raw project string is used.
    pub fn register(
        &mut self,
        name: &str,
        agent_type: &str,
        project: &str,
        cwd: &str,
        pid: Option<u32>,
    ) -> Result<String> {
        let canonical_project =
            crate::canonicalize_path(Path::new(project)).unwrap_or_else(|_| project.to_string());
        let id = uuid::Uuid::new_v4().to_string();

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO agents (id, name, agent_type, project, cwd, pid, status, registered_at, last_heartbeat)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', unixepoch(), unixepoch())",
            params![&id, name, agent_type, &canonical_project, cwd, pid.map(|p| p as i64)],
        )?;
        tx.commit()?;

        Ok(id)
    }

    /// Deregister an agent, releasing all its locks (via CASCADE).
    ///
    /// Returns `true` if the agent existed and was removed.
    pub fn deregister(&mut self, agent_id: &str) -> Result<bool> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let rows = tx.execute("DELETE FROM agents WHERE id = ?1", params![agent_id])?;
        tx.commit()?;
        Ok(rows > 0)
    }

    /// Update an agent's heartbeat timestamp.
    ///
    /// Returns `true` if the agent existed and was updated.
    pub fn heartbeat(&mut self, agent_id: &str) -> Result<bool> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let rows = tx.execute(
            "UPDATE agents SET last_heartbeat = unixepoch() WHERE id = ?1",
            params![agent_id],
        )?;
        tx.commit()?;
        Ok(rows > 0)
    }

    /// Update an agent's status and optional task description.
    ///
    /// Also implicitly refreshes the heartbeat.
    /// Returns `true` if the agent existed and was updated.
    pub fn update_status(
        &mut self,
        agent_id: &str,
        status: &str,
        task: Option<&str>,
    ) -> Result<bool> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let rows = tx.execute(
            "UPDATE agents SET status = ?1, task = ?2, last_heartbeat = unixepoch() WHERE id = ?3",
            params![status, task, agent_id],
        )?;
        tx.commit()?;
        Ok(rows > 0)
    }

    /// List all agents registered for a given project.
    ///
    /// The `project` path is canonicalized before matching to ensure consistency
    /// with the canonicalization done during `register`.
    pub fn list_agents(&mut self, project: &str) -> Result<Vec<AgentInfo>> {
        let canonical_project =
            crate::canonicalize_path(Path::new(project)).unwrap_or_else(|_| project.to_string());
        let mut stmt = self.conn.prepare(
            "SELECT id, name, agent_type, project, cwd, pid, status, task, registered_at, last_heartbeat
             FROM agents WHERE project = ?1",
        )?;
        let agents = stmt
            .query_map(params![&canonical_project], |row| {
                let pid_val: Option<i64> = row.get(5)?;
                Ok(AgentInfo {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    agent_type: row.get(2)?,
                    project: row.get(3)?,
                    cwd: row.get(4)?,
                    pid: pid_val.map(|p| p as u32),
                    status: row.get(6)?,
                    task: row.get(7)?,
                    registered_at: row.get(8)?,
                    last_heartbeat: row.get(9)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(agents)
    }

    /// Prune stale agents (heartbeat timeout or dead PID).
    ///
    /// Agents whose `last_heartbeat` is older than `timeout_secs` are pruned
    /// regardless of PID status. Agents with dead PIDs are pruned even if their
    /// heartbeat is recent. CASCADE removes associated locks.
    ///
    /// Returns the list of pruned agent IDs.
    pub fn prune_stale(&mut self, timeout_secs: i64) -> Result<Vec<String>> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        // Collect all agents to check
        let mut stmt = tx.prepare("SELECT id, name, pid, last_heartbeat FROM agents")?;
        let agents: Vec<(String, String, Option<i64>, i64)> = stmt
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);

        let now: i64 = tx.query_row("SELECT unixepoch()", [], |row| row.get(0))?;
        let cutoff = now - timeout_secs;

        let mut pruned = Vec::new();

        for (id, name, pid, last_heartbeat) in &agents {
            let stale_by_timeout = *last_heartbeat < cutoff;
            let stale_by_pid = pid
                .map(|p| !crate::pid::is_pid_alive(p as u32))
                .unwrap_or(false);

            if stale_by_timeout || stale_by_pid {
                let reason = if stale_by_timeout && stale_by_pid {
                    "heartbeat timeout and dead PID"
                } else if stale_by_timeout {
                    "heartbeat timeout"
                } else {
                    "dead PID"
                };
                tracing::info!(
                    agent_id = %id,
                    agent_name = %name,
                    reason = %reason,
                    "Pruning stale agent"
                );
                tx.execute("DELETE FROM agents WHERE id = ?1", params![id])?;
                pruned.push(id.clone());
            }
        }

        tx.commit()?;
        Ok(pruned)
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
        let id_b = db.register("agent-b", "cursor", ".", "/tmp", None).unwrap();

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
        assert!(
            from_agent.is_none(),
            "from_agent should be NULL after sender deregistered"
        );
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

        assert!(
            new_hb > old_hb,
            "Heartbeat should be more recent: {new_hb} > {old_hb}"
        );

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

    // ---- File locking tests ----

    #[test]
    fn test_lock_files_single() {
        let (mut db, dir) = test_db();
        let id = db
            .register("agent-1", "claude-code", ".", "/tmp", None)
            .unwrap();

        // Create a real file so canonicalization works
        let file_path = dir.path().join("foo.rs");
        std::fs::write(&file_path, "").unwrap();

        let result = db
            .lock_files(&id, &[file_path.clone()], Some("editing"))
            .unwrap();

        match result {
            LockResult::Acquired(paths) => {
                assert_eq!(paths.len(), 1);
                // The returned path should be the canonical form
                let canonical = crate::canonicalize_path(&file_path).unwrap();
                assert_eq!(paths[0], canonical);
            }
            LockResult::Conflict(_) => panic!("Expected Acquired, got Conflict"),
        }
    }

    #[test]
    fn test_lock_files_multiple() {
        let (mut db, dir) = test_db();
        let id = db
            .register("agent-1", "claude-code", ".", "/tmp", None)
            .unwrap();

        let f1 = dir.path().join("a.rs");
        let f2 = dir.path().join("b.rs");
        let f3 = dir.path().join("c.rs");
        std::fs::write(&f1, "").unwrap();
        std::fs::write(&f2, "").unwrap();
        std::fs::write(&f3, "").unwrap();

        let result = db
            .lock_files(&id, &[f1, f2, f3], Some("refactoring"))
            .unwrap();

        match result {
            LockResult::Acquired(paths) => {
                assert_eq!(paths.len(), 3);
            }
            LockResult::Conflict(_) => panic!("Expected Acquired, got Conflict"),
        }
    }

    #[test]
    fn test_lock_files_conflict() {
        let (mut db, dir) = test_db();
        let id_a = db
            .register("agent-a", "claude-code", ".", "/tmp", None)
            .unwrap();
        let _id_b = db
            .register("agent-b", "cursor", ".", "/tmp", None)
            .unwrap();

        let file_path = dir.path().join("shared.rs");
        std::fs::write(&file_path, "").unwrap();

        // Agent A locks the file
        let result = db
            .lock_files(&id_a, &[file_path.clone()], Some("editing shared.rs"))
            .unwrap();
        assert!(matches!(result, LockResult::Acquired(_)));

        // Agent B tries to lock the same file
        let result = db
            .lock_files(&_id_b, &[file_path], Some("also want shared.rs"))
            .unwrap();

        match result {
            LockResult::Conflict(conflicts) => {
                assert_eq!(conflicts.len(), 1);
                assert_eq!(conflicts[0].held_by_agent_id, id_a);
                assert_eq!(conflicts[0].held_by_agent_name, "agent-a");
                assert_eq!(
                    conflicts[0].reason.as_deref(),
                    Some("editing shared.rs")
                );
            }
            LockResult::Acquired(_) => panic!("Expected Conflict, got Acquired"),
        }
    }

    #[test]
    fn test_lock_files_partial_conflict() {
        let (mut db, dir) = test_db();
        let id_a = db
            .register("agent-a", "claude-code", ".", "/tmp", None)
            .unwrap();
        let id_b = db
            .register("agent-b", "cursor", ".", "/tmp", None)
            .unwrap();

        let file_x = dir.path().join("x.rs");
        let file_y = dir.path().join("y.rs");
        std::fs::write(&file_x, "").unwrap();
        std::fs::write(&file_y, "").unwrap();

        // Agent A locks file X
        let result = db
            .lock_files(&id_a, &[file_x.clone()], Some("editing x"))
            .unwrap();
        assert!(matches!(result, LockResult::Acquired(_)));

        // Agent B tries to lock [X, Y] -- should fail entirely (all-or-nothing)
        let result = db
            .lock_files(&id_b, &[file_x, file_y.clone()], Some("want both"))
            .unwrap();
        assert!(
            matches!(result, LockResult::Conflict(_)),
            "Should be Conflict for partial overlap"
        );

        // Y should NOT be locked either (all-or-nothing)
        let locks = db.list_locks(None).unwrap();
        let y_canonical = crate::canonicalize_path(&file_y).unwrap();
        assert!(
            !locks.iter().any(|l| l.path == y_canonical),
            "File Y should not be locked (all-or-nothing semantics)"
        );
    }

    #[test]
    fn test_lock_files_same_agent_relock() {
        let (mut db, dir) = test_db();
        let id = db
            .register("agent-1", "claude-code", ".", "/tmp", None)
            .unwrap();

        let file_path = dir.path().join("relock.rs");
        std::fs::write(&file_path, "").unwrap();

        // Lock the file
        let result = db
            .lock_files(&id, &[file_path.clone()], Some("first lock"))
            .unwrap();
        assert!(matches!(result, LockResult::Acquired(_)));

        // Lock the same file again (same agent) -- should succeed with INSERT OR REPLACE
        let result = db
            .lock_files(&id, &[file_path], Some("updated reason"))
            .unwrap();
        assert!(matches!(result, LockResult::Acquired(_)));

        // Should still only be one lock
        let locks = db.list_locks(None).unwrap();
        assert_eq!(locks.len(), 1);
        assert_eq!(locks[0].reason.as_deref(), Some("updated reason"));
    }

    #[test]
    fn test_lock_files_canonicalization() {
        let (mut db, dir) = test_db();
        let id_a = db
            .register("agent-a", "claude-code", ".", "/tmp", None)
            .unwrap();
        let id_b = db
            .register("agent-b", "cursor", ".", "/tmp", None)
            .unwrap();

        // Create a real file
        let subdir = dir.path().join("sub");
        std::fs::create_dir(&subdir).unwrap();
        let file_path = subdir.join("target.rs");
        std::fs::write(&file_path, "").unwrap();

        // Agent A locks via the absolute path
        let result = db
            .lock_files(&id_a, &[file_path.clone()], Some("via absolute"))
            .unwrap();
        assert!(matches!(result, LockResult::Acquired(_)));

        // Agent B tries to lock via a path with ".." component
        let relative_path = subdir.join("..").join("sub").join("target.rs");
        let result = db
            .lock_files(&id_b, &[relative_path], Some("via relative"))
            .unwrap();

        assert!(
            matches!(result, LockResult::Conflict(_)),
            "Same file via different path representation should conflict"
        );
    }

    #[test]
    fn test_unlock_file() {
        let (mut db, dir) = test_db();
        let id = db
            .register("agent-1", "claude-code", ".", "/tmp", None)
            .unwrap();

        let file_path = dir.path().join("unlock_me.rs");
        std::fs::write(&file_path, "").unwrap();

        db.lock_files(&id, &[file_path.clone()], Some("temp lock"))
            .unwrap();

        let unlocked = db.unlock_file(&id, &file_path).unwrap();
        assert!(unlocked);

        // Lock should be gone
        let locks = db.list_locks(None).unwrap();
        assert!(locks.is_empty());
    }

    #[test]
    fn test_unlock_all() {
        let (mut db, dir) = test_db();
        let id = db
            .register("agent-1", "claude-code", ".", "/tmp", None)
            .unwrap();

        let f1 = dir.path().join("u1.rs");
        let f2 = dir.path().join("u2.rs");
        let f3 = dir.path().join("u3.rs");
        std::fs::write(&f1, "").unwrap();
        std::fs::write(&f2, "").unwrap();
        std::fs::write(&f3, "").unwrap();

        db.lock_files(&id, &[f1, f2, f3], Some("batch lock"))
            .unwrap();

        let count = db.unlock_all(&id).unwrap();
        assert_eq!(count, 3);

        let locks = db.list_locks(None).unwrap();
        assert!(locks.is_empty());
    }

    #[test]
    fn test_unlock_file_not_owned() {
        let (mut db, dir) = test_db();
        let id_a = db
            .register("agent-a", "claude-code", ".", "/tmp", None)
            .unwrap();
        let id_b = db
            .register("agent-b", "cursor", ".", "/tmp", None)
            .unwrap();

        let file_path = dir.path().join("owned.rs");
        std::fs::write(&file_path, "").unwrap();

        // Agent A locks the file
        db.lock_files(&id_a, &[file_path.clone()], Some("mine"))
            .unwrap();

        // Agent B tries to unlock it -- should return false
        let unlocked = db.unlock_file(&id_b, &file_path).unwrap();
        assert!(!unlocked, "Agent B should not be able to unlock Agent A's file");

        // Lock should still be there
        let locks = db.list_locks(None).unwrap();
        assert_eq!(locks.len(), 1);
    }

    #[test]
    fn test_list_locks_by_project() {
        let (mut db, dir) = test_db();

        // Use the temp dir as both projects (need real paths for canonicalization)
        let proj_a = dir.path().join("proj_a");
        let proj_b = dir.path().join("proj_b");
        std::fs::create_dir(&proj_a).unwrap();
        std::fs::create_dir(&proj_b).unwrap();

        let proj_a_str = proj_a.to_str().unwrap();
        let proj_b_str = proj_b.to_str().unwrap();

        let id_a = db
            .register("agent-a", "claude-code", proj_a_str, proj_a_str, None)
            .unwrap();
        let id_b = db
            .register("agent-b", "cursor", proj_b_str, proj_b_str, None)
            .unwrap();

        let file_a = dir.path().join("file_a.rs");
        let file_b = dir.path().join("file_b.rs");
        std::fs::write(&file_a, "").unwrap();
        std::fs::write(&file_b, "").unwrap();

        db.lock_files(&id_a, &[file_a], Some("project A work"))
            .unwrap();
        db.lock_files(&id_b, &[file_b], Some("project B work"))
            .unwrap();

        // list_locks with project A should only show agent A's locks
        let locks_a = db.list_locks(Some(proj_a_str)).unwrap();
        assert_eq!(locks_a.len(), 1);
        assert_eq!(locks_a[0].agent_name, "agent-a");

        // list_locks with project B should only show agent B's locks
        let locks_b = db.list_locks(Some(proj_b_str)).unwrap();
        assert_eq!(locks_b.len(), 1);
        assert_eq!(locks_b[0].agent_name, "agent-b");
    }

    #[test]
    fn test_list_locks_all() {
        let (mut db, dir) = test_db();

        let proj_a = dir.path().join("proj_x");
        let proj_b = dir.path().join("proj_y");
        std::fs::create_dir(&proj_a).unwrap();
        std::fs::create_dir(&proj_b).unwrap();

        let proj_a_str = proj_a.to_str().unwrap();
        let proj_b_str = proj_b.to_str().unwrap();

        let id_a = db
            .register("agent-a", "claude-code", proj_a_str, proj_a_str, None)
            .unwrap();
        let id_b = db
            .register("agent-b", "cursor", proj_b_str, proj_b_str, None)
            .unwrap();

        let file_a = dir.path().join("all_a.rs");
        let file_b = dir.path().join("all_b.rs");
        std::fs::write(&file_a, "").unwrap();
        std::fs::write(&file_b, "").unwrap();

        db.lock_files(&id_a, &[file_a], None).unwrap();
        db.lock_files(&id_b, &[file_b], None).unwrap();

        // list_locks with None should show all locks regardless of project
        let all_locks = db.list_locks(None).unwrap();
        assert_eq!(all_locks.len(), 2);
    }
}
