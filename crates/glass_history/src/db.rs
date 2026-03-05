use std::path::Path;

use anyhow::Result;
use rusqlite::{params, Connection};

/// Current schema version. Bump when adding migrations.
const SCHEMA_VERSION: i64 = 1;

/// A command execution record with all metadata.
#[derive(Debug, Clone)]
pub struct CommandRecord {
    /// Database row id. None before insert, Some after.
    pub id: Option<i64>,
    /// The command text that was executed.
    pub command: String,
    /// Working directory where the command was run.
    pub cwd: String,
    /// Process exit code, if available.
    pub exit_code: Option<i32>,
    /// When the command started (Unix epoch seconds).
    pub started_at: i64,
    /// When the command finished (Unix epoch seconds).
    pub finished_at: i64,
    /// Duration in milliseconds.
    pub duration_ms: i64,
    /// Captured command output (ANSI-stripped, possibly truncated).
    pub output: Option<String>,
}

/// SQLite-backed command history database.
pub struct HistoryDb {
    conn: Connection,
}

impl HistoryDb {
    /// Open (or create) a history database at the given path.
    /// Sets WAL mode and creates the schema if needed.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )?;
        Self::create_schema(&conn)?;
        Self::migrate(&conn)?;
        Ok(Self { conn })
    }

    fn create_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS commands (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                command     TEXT NOT NULL,
                cwd         TEXT NOT NULL,
                exit_code   INTEGER,
                started_at  INTEGER NOT NULL,
                finished_at INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                output      TEXT,
                created_at  INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE INDEX IF NOT EXISTS idx_commands_started_at ON commands(started_at);
            CREATE INDEX IF NOT EXISTS idx_commands_cwd ON commands(cwd);

            CREATE VIRTUAL TABLE IF NOT EXISTS commands_fts USING fts5(
                command,
                tokenize='unicode61'
            );",
        )?;
        Ok(())
    }

    /// Apply schema migrations based on PRAGMA user_version.
    fn migrate(conn: &Connection) -> Result<()> {
        let version: i64 =
            conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

        if version < 1 {
            // Phase 6: add output column to existing databases.
            // For fresh databases, the column already exists from create_schema,
            // so we check if it's missing before altering.
            let has_output: bool = conn
                .prepare("SELECT output FROM commands LIMIT 0")
                .is_ok();

            if !has_output {
                conn.execute_batch("ALTER TABLE commands ADD COLUMN output TEXT;")?;
            }

            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }

        Ok(())
    }

    /// Insert a command record into the database. Returns the row id.
    pub fn insert_command(&self, record: &CommandRecord) -> Result<i64> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO commands (command, cwd, exit_code, started_at, finished_at, duration_ms, output)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                record.command,
                record.cwd,
                record.exit_code,
                record.started_at,
                record.finished_at,
                record.duration_ms,
                record.output,
            ],
        )?;
        let rowid = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO commands_fts (rowid, command) VALUES (?1, ?2)",
            params![rowid, record.command],
        )?;
        tx.commit()?;
        Ok(rowid)
    }

    /// Retrieve a command record by id.
    pub fn get_command(&self, id: i64) -> Result<Option<CommandRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, command, cwd, exit_code, started_at, finished_at, duration_ms, output
             FROM commands WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
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
        match rows.next() {
            Some(Ok(record)) => Ok(Some(record)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// Delete a command record by id (from both commands and FTS tables).
    /// Returns true if a record was deleted.
    pub fn delete_command(&self, id: i64) -> Result<bool> {
        let tx = self.conn.unchecked_transaction()?;
        // Delete from FTS first (standard FTS5 -- just DELETE by rowid)
        tx.execute(
            "DELETE FROM commands_fts WHERE rowid = ?1",
            params![id],
        )?;
        let deleted = tx.execute("DELETE FROM commands WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(deleted > 0)
    }

    /// Return the total number of command records.
    pub fn command_count(&self) -> Result<u64> {
        let count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM commands", [], |row| row.get(0))?;
        Ok(count as u64)
    }

    /// Update the output field on an existing command record.
    pub fn update_output(&self, id: i64, output: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE commands SET output = ?1 WHERE id = ?2",
            params![output, id],
        )?;
        Ok(())
    }

    /// Get a reference to the underlying connection (for search/retention modules).
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Search command history using FTS5 full-text search.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<crate::search::SearchResult>> {
        crate::search::search(&self.conn, query, limit)
    }

    /// Prune old records by age and size limits.
    pub fn prune(&self, max_age_days: u32, max_size_bytes: u64) -> Result<u64> {
        crate::retention::prune(&self.conn, max_age_days, max_size_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_db() -> (HistoryDb, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let db = HistoryDb::open(&db_path).unwrap();
        (db, dir)
    }

    fn sample_record(command: &str) -> CommandRecord {
        CommandRecord {
            id: None,
            command: command.to_string(),
            cwd: "/home/user".to_string(),
            exit_code: Some(0),
            started_at: 1700000000,
            finished_at: 1700000005,
            duration_ms: 5000,
            output: None,
        }
    }

    #[test]
    fn test_insert_and_retrieve() {
        let (db, _dir) = test_db();
        let record = CommandRecord {
            id: None,
            command: "cargo build".to_string(),
            cwd: "/home/user/project".to_string(),
            exit_code: Some(0),
            started_at: 1700000000,
            finished_at: 1700000010,
            duration_ms: 10000,
            output: None,
        };
        let id = db.insert_command(&record).unwrap();
        assert!(id > 0);

        let retrieved = db.get_command(id).unwrap().expect("record should exist");
        assert_eq!(retrieved.id, Some(id));
        assert_eq!(retrieved.command, "cargo build");
        assert_eq!(retrieved.cwd, "/home/user/project");
        assert_eq!(retrieved.exit_code, Some(0));
        assert_eq!(retrieved.started_at, 1700000000);
        assert_eq!(retrieved.finished_at, 1700000010);
        assert_eq!(retrieved.duration_ms, 10000);
    }

    #[test]
    fn test_insert_populates_fts() {
        let (db, _dir) = test_db();
        let record = sample_record("cargo build");
        db.insert_command(&record).unwrap();

        let results = db.search("cargo", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].command, "cargo build");
    }

    #[test]
    fn test_search_bm25_ranking() {
        let (db, _dir) = test_db();
        // Insert commands with varying relevance to "cargo"
        db.insert_command(&sample_record("cargo build")).unwrap();
        db.insert_command(&sample_record("cargo test --release"))
            .unwrap();
        db.insert_command(&sample_record("git commit -m fix"))
            .unwrap();

        let results = db.search("cargo", 10).unwrap();
        assert_eq!(results.len(), 2);
        // Both cargo commands returned, git command excluded
        assert!(results.iter().all(|r| r.command.contains("cargo")));
    }

    #[test]
    fn test_search_no_results() {
        let (db, _dir) = test_db();
        db.insert_command(&sample_record("cargo build")).unwrap();

        let results = db.search("nonexistent_xyz", 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_prefix() {
        let (db, _dir) = test_db();
        db.insert_command(&sample_record("cargo build")).unwrap();

        let results = db.search("car*", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].command, "cargo build");
    }

    #[test]
    fn test_full_lifecycle_integration() {
        let (db, _dir) = test_db();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // 1-2. Insert 5 diverse command records
        let commands = vec![
            ("cargo build --release", "/home/user/project", Some(0), now),
            ("git push origin main", "/home/user/project", Some(0), now),
            ("git log --oneline", "/home/user/project", Some(0), now),
            ("npm install", "/home/user/webapp", Some(0), now),
            ("docker compose up", "/home/user/infra", Some(1), now),
        ];
        for (cmd, cwd, exit_code, ts) in &commands {
            db.insert_command(&CommandRecord {
                id: None,
                command: cmd.to_string(),
                cwd: cwd.to_string(),
                exit_code: *exit_code,
                started_at: *ts,
                finished_at: ts + 5,
                duration_ms: 5000,
                output: None,
            })
            .unwrap();
        }

        // 3. Verify count
        assert_eq!(db.command_count().unwrap(), 5);

        // 4. Search for "docker", verify correct result
        let results = db.search("docker", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].command, "docker compose up");
        assert_eq!(results[0].cwd, "/home/user/infra");
        assert_eq!(results[0].exit_code, Some(1));

        // 5. Prefix query for "git*"
        let results = db.search("git*", 10).unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.command.starts_with("git")));

        // 6. Insert 3 old records (older than 1 day)
        let old_time = now - 2 * 86400;
        for i in 0..3 {
            db.insert_command(&CommandRecord {
                id: None,
                command: format!("old_integration_cmd_{}", i),
                cwd: "/tmp".to_string(),
                exit_code: Some(0),
                started_at: old_time,
                finished_at: old_time + 1,
                duration_ms: 1000,
                output: None,
            })
            .unwrap();
        }
        assert_eq!(db.command_count().unwrap(), 8);

        // 7. Prune with max_age = 1 day
        let deleted = db.prune(1, u64::MAX).unwrap();
        assert_eq!(deleted, 3);

        // 8. Verify old records are gone
        assert_eq!(db.command_count().unwrap(), 5);

        // 9. FTS search for pruned commands returns empty
        let results = db.search("old_integration_cmd_0", 10).unwrap();
        assert!(results.is_empty());

        // 10. Recent commands still searchable
        let results = db.search("cargo", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].command, "cargo build --release");
    }

    #[test]
    fn test_delete_command() {
        let (db, _dir) = test_db();
        let id = db.insert_command(&sample_record("delete me")).unwrap();

        assert!(db.get_command(id).unwrap().is_some());
        assert!(db.delete_command(id).unwrap());
        assert!(db.get_command(id).unwrap().is_none());

        // FTS should also be cleaned up
        let results = db.search("delete", 10).unwrap();
        assert!(results.is_empty());

        // Deleting non-existent returns false
        assert!(!db.delete_command(9999).unwrap());
    }

    #[test]
    fn test_migration_v0_to_v1() {
        // Create a v0 database manually (no output column, user_version=0)
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("v0.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "PRAGMA journal_mode = WAL;
                 CREATE TABLE commands (
                     id          INTEGER PRIMARY KEY AUTOINCREMENT,
                     command     TEXT NOT NULL,
                     cwd         TEXT NOT NULL,
                     exit_code   INTEGER,
                     started_at  INTEGER NOT NULL,
                     finished_at INTEGER NOT NULL,
                     duration_ms INTEGER NOT NULL,
                     created_at  INTEGER NOT NULL DEFAULT (unixepoch())
                 );
                 CREATE INDEX idx_commands_started_at ON commands(started_at);
                 CREATE INDEX idx_commands_cwd ON commands(cwd);
                 CREATE VIRTUAL TABLE commands_fts USING fts5(
                     command, tokenize='unicode61'
                 );
                 PRAGMA user_version = 0;",
            )
            .unwrap();
        }

        // Open via HistoryDb::open -- should trigger migration
        let db = HistoryDb::open(&db_path).unwrap();

        // Verify output column exists by inserting with output
        let mut record = sample_record("migrated cmd");
        record.output = Some("output after migration".to_string());
        let id = db.insert_command(&record).unwrap();
        let retrieved = db.get_command(id).unwrap().unwrap();
        assert_eq!(retrieved.output, Some("output after migration".to_string()));

        // Verify user_version is now 1
        let version: i64 = db
            .conn()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn test_existing_records_survive_migration() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("v0_with_data.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "PRAGMA journal_mode = WAL;
                 CREATE TABLE commands (
                     id          INTEGER PRIMARY KEY AUTOINCREMENT,
                     command     TEXT NOT NULL,
                     cwd         TEXT NOT NULL,
                     exit_code   INTEGER,
                     started_at  INTEGER NOT NULL,
                     finished_at INTEGER NOT NULL,
                     duration_ms INTEGER NOT NULL,
                     created_at  INTEGER NOT NULL DEFAULT (unixepoch())
                 );
                 CREATE VIRTUAL TABLE commands_fts USING fts5(
                     command, tokenize='unicode61'
                 );
                 PRAGMA user_version = 0;",
            )
            .unwrap();
            // Insert a record into v0 schema
            conn.execute(
                "INSERT INTO commands (command, cwd, exit_code, started_at, finished_at, duration_ms)
                 VALUES ('old cmd', '/tmp', 0, 1700000000, 1700000005, 5000)",
                [],
            )
            .unwrap();
        }

        // Open via HistoryDb -- migrates v0 -> v1
        let db = HistoryDb::open(&db_path).unwrap();

        // Old record should be accessible with output=None
        let record = db.get_command(1).unwrap().unwrap();
        assert_eq!(record.command, "old cmd");
        assert_eq!(record.output, None);
    }

    #[test]
    fn test_insert_with_output() {
        let (db, _dir) = test_db();
        let mut record = sample_record("echo hello");
        record.output = Some("hello\n".to_string());
        let id = db.insert_command(&record).unwrap();

        let retrieved = db.get_command(id).unwrap().unwrap();
        assert_eq!(retrieved.output, Some("hello\n".to_string()));
    }

    #[test]
    fn test_insert_without_output() {
        let (db, _dir) = test_db();
        let record = sample_record("ls");
        let id = db.insert_command(&record).unwrap();

        let retrieved = db.get_command(id).unwrap().unwrap();
        assert_eq!(retrieved.output, None);
    }

    #[test]
    fn test_update_output() {
        let dir = TempDir::new().unwrap();
        let db = HistoryDb::open(&dir.path().join("test.db")).unwrap();
        let record = CommandRecord {
            id: None,
            command: "echo hello".to_string(),
            cwd: "/tmp".to_string(),
            exit_code: Some(0),
            started_at: 1000,
            finished_at: 1001,
            duration_ms: 1000,
            output: None,
        };
        let id = db.insert_command(&record).unwrap();
        db.update_output(id, "hello\n").unwrap();
        let fetched = db.get_command(id).unwrap().unwrap();
        assert_eq!(fetched.output, Some("hello\n".to_string()));
    }

    #[test]
    fn test_fresh_db_has_output_column_and_version() {
        let (db, _dir) = test_db();
        let version: i64 = db
            .conn()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }
}
