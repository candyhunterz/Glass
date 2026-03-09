//! Coordination database -- agent registry, file locking, and messaging.
//!
//! All write operations use `BEGIN IMMEDIATE` transactions to prevent
//! `SQLITE_BUSY` errors when multiple agents access the database concurrently.

use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;

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
}
