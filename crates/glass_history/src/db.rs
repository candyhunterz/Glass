use std::path::{Path, PathBuf};

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

/// Current schema version. Bump when adding migrations.
#[cfg(test)]
const SCHEMA_VERSION: i64 = 3;

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

/// A pipe stage record for database storage.
/// Accepts pre-serialized data (caller converts from FinalizedBuffer).
#[derive(Debug, Clone)]
pub struct PipeStageRow {
    pub stage_index: i64,
    pub command: String,
    pub output: Option<String>,
    pub total_bytes: i64,
    pub is_binary: bool,
    pub is_sampled: bool,
}

/// SQLite-backed command history database.
pub struct HistoryDb {
    conn: Connection,
    path: PathBuf,
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
             PRAGMA busy_timeout = 5000;
             PRAGMA foreign_keys = ON;",
        )?;
        Self::create_schema(&conn)?;
        Self::migrate(&conn)?;
        Ok(Self {
            conn,
            path: path.to_path_buf(),
        })
    }

    /// Return the filesystem path of this database file.
    pub fn path(&self) -> &Path {
        &self.path
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
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

        if version < 1 {
            // Phase 6: add output column to existing databases.
            // For fresh databases, the column already exists from create_schema,
            // so we check if it's missing before altering.
            let has_output: bool = conn.prepare("SELECT output FROM commands LIMIT 0").is_ok();

            if !has_output {
                conn.execute_batch("ALTER TABLE commands ADD COLUMN output TEXT;")?;
            }

            conn.pragma_update(None, "user_version", 1)?;
        }

        if version < 2 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS pipe_stages (
                    id          INTEGER PRIMARY KEY AUTOINCREMENT,
                    command_id  INTEGER NOT NULL REFERENCES commands(id) ON DELETE CASCADE,
                    stage_index INTEGER NOT NULL,
                    command     TEXT NOT NULL,
                    output      TEXT,
                    total_bytes INTEGER NOT NULL,
                    is_binary   INTEGER NOT NULL DEFAULT 0,
                    is_sampled  INTEGER NOT NULL DEFAULT 0
                );
                CREATE INDEX IF NOT EXISTS idx_pipe_stages_command ON pipe_stages(command_id);",
            )?;
            conn.pragma_update(None, "user_version", 2)?;
        }

        if version < 3 {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS command_output_records (
                    id              INTEGER PRIMARY KEY AUTOINCREMENT,
                    command_id      INTEGER NOT NULL REFERENCES commands(id) ON DELETE CASCADE,
                    output_type     TEXT NOT NULL,
                    severity        TEXT NOT NULL,
                    one_line        TEXT NOT NULL,
                    token_estimate  INTEGER NOT NULL,
                    raw_line_count  INTEGER NOT NULL,
                    raw_byte_count  INTEGER NOT NULL,
                    created_at      INTEGER NOT NULL DEFAULT (unixepoch())
                );
                CREATE INDEX IF NOT EXISTS idx_cor_command ON command_output_records(command_id);
                CREATE TABLE IF NOT EXISTS output_records (
                    id              INTEGER PRIMARY KEY AUTOINCREMENT,
                    command_id      INTEGER NOT NULL REFERENCES commands(id) ON DELETE CASCADE,
                    record_type     TEXT NOT NULL,
                    severity        TEXT,
                    file_path       TEXT,
                    data            TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_or_command   ON output_records(command_id);
                CREATE INDEX IF NOT EXISTS idx_or_severity  ON output_records(severity);
                CREATE INDEX IF NOT EXISTS idx_or_file      ON output_records(file_path);
                CREATE INDEX IF NOT EXISTS idx_or_type      ON output_records(record_type);",
            )?;
            conn.pragma_update(None, "user_version", 3)?;
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

    /// Insert pipeline stage records for a command. No-op if stages is empty.
    pub fn insert_pipe_stages(&self, command_id: i64, stages: &[PipeStageRow]) -> Result<()> {
        if stages.is_empty() {
            return Ok(());
        }
        let tx = self.conn.unchecked_transaction()?;
        for stage in stages {
            tx.execute(
                "INSERT INTO pipe_stages (command_id, stage_index, command, output, total_bytes, is_binary, is_sampled)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    command_id,
                    stage.stage_index,
                    stage.command,
                    stage.output,
                    stage.total_bytes,
                    stage.is_binary,
                    stage.is_sampled,
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Retrieve pipeline stage records for a command, ordered by stage_index.
    pub fn get_pipe_stages(&self, command_id: i64) -> Result<Vec<PipeStageRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT stage_index, command, output, total_bytes, is_binary, is_sampled
             FROM pipe_stages WHERE command_id = ?1 ORDER BY stage_index ASC",
        )?;
        let rows = stmt.query_map(params![command_id], |row| {
            Ok(PipeStageRow {
                stage_index: row.get(0)?,
                command: row.get(1)?,
                output: row.get(2)?,
                total_bytes: row.get(3)?,
                is_binary: row.get(4)?,
                is_sampled: row.get(5)?,
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Delete a command record by id (from SOI tables, pipe_stages, FTS, and commands tables).
    /// Returns true if a record was deleted.
    pub fn delete_command(&self, id: i64) -> Result<bool> {
        let tx = self.conn.unchecked_transaction()?;
        // Delete SOI records first (belt and suspenders with CASCADE)
        tx.execute(
            "DELETE FROM output_records WHERE command_id = ?1",
            params![id],
        )?;
        tx.execute(
            "DELETE FROM command_output_records WHERE command_id = ?1",
            params![id],
        )?;
        // Delete pipe_stages (belt and suspenders with CASCADE)
        tx.execute("DELETE FROM pipe_stages WHERE command_id = ?1", params![id])?;
        // Delete from FTS (standard FTS5 -- just DELETE by rowid)
        tx.execute("DELETE FROM commands_fts WHERE rowid = ?1", params![id])?;
        let deleted = tx.execute("DELETE FROM commands WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(deleted > 0)
    }

    /// Return the total number of command records.
    pub fn command_count(&self) -> Result<u64> {
        let count: i64 = self
            .conn
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

    /// Fetch the stored output text for a command by id.
    /// Returns None if no output was recorded (e.g. alt-screen apps like vim/htop)
    /// or if the command id does not exist.
    pub fn get_output_for_command(&self, command_id: i64) -> Result<Option<String>> {
        // Use Option<String> to handle NULL output column values.
        let result: Option<Option<String>> = self
            .conn
            .query_row(
                "SELECT output FROM commands WHERE id = ?1",
                params![command_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(anyhow::Error::from)?;
        // Flatten: None (no row) or Some(None) (NULL output) both become None.
        Ok(result.flatten())
    }

    /// Fetch the command text for a command by id.
    /// Returns None if the command id does not exist.
    pub fn get_command_text(&self, command_id: i64) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT command FROM commands WHERE id = ?1",
                params![command_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(anyhow::Error::from)
    }

    /// Get a reference to the underlying connection (for search/retention modules).
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Search command history using FTS5 full-text search.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<crate::search::SearchResult>> {
        crate::search::search(&self.conn, query, limit)
    }

    /// Execute a filtered query combining FTS5 text search with SQL WHERE clauses.
    pub fn filtered_query(&self, filter: &crate::query::QueryFilter) -> Result<Vec<CommandRecord>> {
        crate::query::filtered_query(&self.conn, filter)
    }

    /// Prune old records by age and size limits.
    pub fn prune(&self, max_age_days: u32, max_size_bytes: u64) -> Result<u64> {
        crate::retention::prune(&self.conn, max_age_days, max_size_bytes)
    }

    /// Insert a `ParsedOutput` for a command into the SOI tables atomically.
    pub fn insert_parsed_output(
        &self,
        command_id: i64,
        parsed: &glass_soi::ParsedOutput,
    ) -> Result<()> {
        crate::soi::insert_parsed_output(&self.conn, command_id, parsed)
    }

    /// Retrieve the output summary row for a command, if any.
    pub fn get_output_summary(
        &self,
        command_id: i64,
    ) -> Result<Option<crate::soi::CommandOutputSummaryRow>> {
        crate::soi::get_output_summary(&self.conn, command_id)
    }

    /// Retrieve output records for a command with optional filters.
    pub fn get_output_records(
        &self,
        command_id: i64,
        severity: Option<&str>,
        file_path: Option<&str>,
        record_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<crate::soi::OutputRecordRow>> {
        crate::soi::get_output_records(
            &self.conn,
            command_id,
            severity,
            file_path,
            record_type,
            limit,
        )
    }

    /// Compress output records for a command at the given token budget level.
    ///
    /// Fetches the summary and records from the DB, then runs the compression
    /// engine. Returns None if the command has no SOI data.
    pub fn compress_output(
        &self,
        command_id: i64,
        budget: crate::compress::TokenBudget,
    ) -> Result<Option<crate::compress::CompressedOutput>> {
        let summary = match crate::soi::get_output_summary(&self.conn, command_id)? {
            Some(s) => s,
            None => return Ok(None),
        };
        let records =
            crate::soi::get_output_records(&self.conn, command_id, None, None, None, 10000)?;
        Ok(Some(crate::compress::compress(&records, &summary, budget)))
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

    #[test]
    fn test_migration_v1_to_v2() {
        // Create a v1 database manually (has output column, no pipe_stages table)
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("v1.db");
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
                     output      TEXT,
                     created_at  INTEGER NOT NULL DEFAULT (unixepoch())
                 );
                 CREATE INDEX idx_commands_started_at ON commands(started_at);
                 CREATE INDEX idx_commands_cwd ON commands(cwd);
                 CREATE VIRTUAL TABLE commands_fts USING fts5(
                     command, tokenize='unicode61'
                 );
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        }

        // Open via HistoryDb::open -- should trigger v1->v2 migration
        let db = HistoryDb::open(&db_path).unwrap();

        // Verify pipe_stages table exists
        let table_exists: bool = db
            .conn()
            .prepare("SELECT 1 FROM pipe_stages LIMIT 0")
            .is_ok();
        assert!(
            table_exists,
            "pipe_stages table should exist after migration"
        );

        // Verify user_version is now at the current schema version
        // (v1->v2 migration also triggers v2->v3 in the same open call)
        let version: i64 = db
            .conn()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn test_migration_v2_to_v3() {
        // Create a v2 database manually (has commands, pipe_stages, user_version=2)
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("v2.db");
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
                     output      TEXT,
                     created_at  INTEGER NOT NULL DEFAULT (unixepoch())
                 );
                 CREATE INDEX idx_commands_started_at ON commands(started_at);
                 CREATE INDEX idx_commands_cwd ON commands(cwd);
                 CREATE VIRTUAL TABLE commands_fts USING fts5(
                     command, tokenize='unicode61'
                 );
                 CREATE TABLE pipe_stages (
                     id          INTEGER PRIMARY KEY AUTOINCREMENT,
                     command_id  INTEGER NOT NULL REFERENCES commands(id) ON DELETE CASCADE,
                     stage_index INTEGER NOT NULL,
                     command     TEXT NOT NULL,
                     output      TEXT,
                     total_bytes INTEGER NOT NULL,
                     is_binary   INTEGER NOT NULL DEFAULT 0,
                     is_sampled  INTEGER NOT NULL DEFAULT 0
                 );
                 PRAGMA user_version = 2;",
            )
            .unwrap();
            // Insert existing v2 data
            conn.execute(
                "INSERT INTO commands (command, cwd, exit_code, started_at, finished_at, duration_ms)
                 VALUES ('git status', '/home', 0, 1700000000, 1700000001, 1000)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO pipe_stages (command_id, stage_index, command, output, total_bytes, is_binary, is_sampled)
                 VALUES (1, 0, 'git status', 'On branch main', 15, 0, 0)",
                [],
            )
            .unwrap();
        }

        // Open via HistoryDb::open -- should trigger v2->v3 migration
        let db = HistoryDb::open(&db_path).unwrap();

        // Verify both new SOI tables exist
        let cor_exists: bool = db
            .conn()
            .prepare("SELECT 1 FROM command_output_records LIMIT 0")
            .is_ok();
        assert!(
            cor_exists,
            "command_output_records table should exist after migration"
        );
        let or_exists: bool = db
            .conn()
            .prepare("SELECT 1 FROM output_records LIMIT 0")
            .is_ok();
        assert!(
            or_exists,
            "output_records table should exist after migration"
        );

        // Verify user_version is now 3
        let version: i64 = db
            .conn()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);

        // Verify old data is intact
        let record = db.get_command(1).unwrap().unwrap();
        assert_eq!(record.command, "git status");

        let stages = db.get_pipe_stages(1).unwrap();
        assert_eq!(stages.len(), 1);
        assert_eq!(stages[0].command, "git status");
    }

    #[test]
    fn test_existing_records_survive_v2_migration() {
        // Create a v1 database with command records
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("v1_with_data.db");
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
                     output      TEXT,
                     created_at  INTEGER NOT NULL DEFAULT (unixepoch())
                 );
                 CREATE VIRTUAL TABLE commands_fts USING fts5(
                     command, tokenize='unicode61'
                 );
                 PRAGMA user_version = 1;",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO commands (command, cwd, exit_code, started_at, finished_at, duration_ms, output)
                 VALUES ('ls -la', '/home', 0, 1700000000, 1700000005, 5000, 'file1\nfile2\n')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO commands_fts (rowid, command) VALUES (1, 'ls -la')",
                [],
            )
            .unwrap();
        }

        // Open via HistoryDb::open -- migrates v1 -> v2
        let db = HistoryDb::open(&db_path).unwrap();

        // Old record should be intact
        let record = db.get_command(1).unwrap().unwrap();
        assert_eq!(record.command, "ls -la");
        assert_eq!(record.output, Some("file1\nfile2\n".to_string()));

        // FTS should still work
        let results = db.search("ls", 10).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_insert_and_get_pipe_stages() {
        let (db, _dir) = test_db();
        let id = db
            .insert_command(&sample_record("cat file | grep foo | wc -l"))
            .unwrap();

        let stages = vec![
            PipeStageRow {
                stage_index: 0,
                command: "cat file".to_string(),
                output: Some("line1\nline2\nfoo bar\n".to_string()),
                total_bytes: 20,
                is_binary: false,
                is_sampled: false,
            },
            PipeStageRow {
                stage_index: 1,
                command: "grep foo".to_string(),
                output: Some("foo bar\n".to_string()),
                total_bytes: 8,
                is_binary: false,
                is_sampled: false,
            },
            PipeStageRow {
                stage_index: 2,
                command: "wc -l".to_string(),
                output: Some("1\n".to_string()),
                total_bytes: 2,
                is_binary: false,
                is_sampled: false,
            },
        ];
        db.insert_pipe_stages(id, &stages).unwrap();

        let retrieved = db.get_pipe_stages(id).unwrap();
        assert_eq!(retrieved.len(), 3);
        assert_eq!(retrieved[0].stage_index, 0);
        assert_eq!(retrieved[0].command, "cat file");
        assert_eq!(
            retrieved[0].output,
            Some("line1\nline2\nfoo bar\n".to_string())
        );
        assert_eq!(retrieved[1].stage_index, 1);
        assert_eq!(retrieved[1].command, "grep foo");
        assert_eq!(retrieved[2].stage_index, 2);
        assert_eq!(retrieved[2].command, "wc -l");
    }

    #[test]
    fn test_no_pipe_stages_for_simple_command() {
        let (db, _dir) = test_db();
        let id = db.insert_command(&sample_record("ls -la")).unwrap();

        // Do NOT insert pipe stages
        let stages = db.get_pipe_stages(id).unwrap();
        assert!(stages.is_empty());
    }

    #[test]
    fn test_pipe_stage_buffer_variants() {
        let (db, _dir) = test_db();
        let id = db.insert_command(&sample_record("pipeline cmd")).unwrap();

        let stages = vec![
            // Complete variant
            PipeStageRow {
                stage_index: 0,
                command: "cat file".to_string(),
                output: Some("line1\nline2\n".to_string()),
                total_bytes: 12,
                is_binary: false,
                is_sampled: false,
            },
            // Sampled variant
            PipeStageRow {
                stage_index: 1,
                command: "sort".to_string(),
                output: Some("head...\n[...500 bytes omitted...]\n...tail".to_string()),
                total_bytes: 1000,
                is_binary: false,
                is_sampled: true,
            },
            // Binary variant
            PipeStageRow {
                stage_index: 2,
                command: "gzip".to_string(),
                output: None,
                total_bytes: 4096,
                is_binary: true,
                is_sampled: false,
            },
        ];
        db.insert_pipe_stages(id, &stages).unwrap();

        let retrieved = db.get_pipe_stages(id).unwrap();
        assert_eq!(retrieved.len(), 3);

        // Complete
        assert_eq!(retrieved[0].output, Some("line1\nline2\n".to_string()));
        assert_eq!(retrieved[0].total_bytes, 12);
        assert!(!retrieved[0].is_binary);
        assert!(!retrieved[0].is_sampled);

        // Sampled
        assert_eq!(
            retrieved[1].output,
            Some("head...\n[...500 bytes omitted...]\n...tail".to_string())
        );
        assert_eq!(retrieved[1].total_bytes, 1000);
        assert!(!retrieved[1].is_binary);
        assert!(retrieved[1].is_sampled);

        // Binary
        assert_eq!(retrieved[2].output, None);
        assert_eq!(retrieved[2].total_bytes, 4096);
        assert!(retrieved[2].is_binary);
        assert!(!retrieved[2].is_sampled);
    }

    #[test]
    fn test_prune_cascades_to_pipe_stages() {
        let (db, _dir) = test_db();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Insert old command with pipe stages (2 days ago)
        let old_time = now - 2 * 86400;
        let old_id = db
            .insert_command(&CommandRecord {
                id: None,
                command: "old pipeline".to_string(),
                cwd: "/tmp".to_string(),
                exit_code: Some(0),
                started_at: old_time,
                finished_at: old_time + 1,
                duration_ms: 1000,
                output: None,
            })
            .unwrap();
        db.insert_pipe_stages(
            old_id,
            &[PipeStageRow {
                stage_index: 0,
                command: "cat".to_string(),
                output: Some("old data".to_string()),
                total_bytes: 8,
                is_binary: false,
                is_sampled: false,
            }],
        )
        .unwrap();

        // Insert recent command with pipe stages
        let recent_id = db
            .insert_command(&CommandRecord {
                id: None,
                command: "recent pipeline".to_string(),
                cwd: "/tmp".to_string(),
                exit_code: Some(0),
                started_at: now,
                finished_at: now + 1,
                duration_ms: 1000,
                output: None,
            })
            .unwrap();
        db.insert_pipe_stages(
            recent_id,
            &[PipeStageRow {
                stage_index: 0,
                command: "grep".to_string(),
                output: Some("recent data".to_string()),
                total_bytes: 11,
                is_binary: false,
                is_sampled: false,
            }],
        )
        .unwrap();

        // Verify both have stages before pruning
        assert_eq!(db.get_pipe_stages(old_id).unwrap().len(), 1);
        assert_eq!(db.get_pipe_stages(recent_id).unwrap().len(), 1);

        // Prune by age (max 1 day)
        let deleted = db.prune(1, u64::MAX).unwrap();
        assert_eq!(deleted, 1);

        // Old command's pipe_stages should be gone
        assert!(db.get_pipe_stages(old_id).unwrap().is_empty());
        // Recent command's pipe_stages should survive
        assert_eq!(db.get_pipe_stages(recent_id).unwrap().len(), 1);
        assert_eq!(db.get_pipe_stages(recent_id).unwrap()[0].command, "grep");
    }

    #[test]
    fn test_size_prune_cascades_to_pipe_stages() {
        let (db, _dir) = test_db();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Insert commands with pipe stages and large output to force size-based pruning
        let mut ids = Vec::new();
        for i in 0..10 {
            let id = db
                .insert_command(&CommandRecord {
                    id: None,
                    command: format!("pipeline cmd {}", i),
                    cwd: "/tmp".to_string(),
                    exit_code: Some(0),
                    started_at: now - (10 - i), // oldest first
                    finished_at: now - (10 - i) + 1,
                    duration_ms: 1000,
                    output: Some("x".repeat(1000)), // generate some size
                })
                .unwrap();
            db.insert_pipe_stages(
                id,
                &[PipeStageRow {
                    stage_index: 0,
                    command: format!("stage of cmd {}", i),
                    output: Some("x".repeat(500)),
                    total_bytes: 500,
                    is_binary: false,
                    is_sampled: false,
                }],
            )
            .unwrap();
            ids.push(id);
        }

        // Prune by size (set limit to 1 byte to force pruning)
        let deleted = db.prune(u32::MAX, 1).unwrap();
        assert!(deleted > 0, "should have deleted some records");

        // The oldest commands should have been pruned along with their pipe_stages
        let oldest_stages = db.get_pipe_stages(ids[0]).unwrap();
        assert!(
            oldest_stages.is_empty(),
            "oldest command's pipe_stages should be deleted"
        );
    }

    #[test]
    fn test_delete_command_cascades_pipe_stages() {
        let (db, _dir) = test_db();
        let id = db
            .insert_command(&sample_record("pipeline to delete"))
            .unwrap();

        db.insert_pipe_stages(
            id,
            &[
                PipeStageRow {
                    stage_index: 0,
                    command: "cat".to_string(),
                    output: Some("data".to_string()),
                    total_bytes: 4,
                    is_binary: false,
                    is_sampled: false,
                },
                PipeStageRow {
                    stage_index: 1,
                    command: "grep".to_string(),
                    output: Some("filtered".to_string()),
                    total_bytes: 8,
                    is_binary: false,
                    is_sampled: false,
                },
            ],
        )
        .unwrap();

        // Verify stages exist
        assert_eq!(db.get_pipe_stages(id).unwrap().len(), 2);

        // Delete command
        assert!(db.delete_command(id).unwrap());

        // Both command and pipe_stages should be gone
        assert!(db.get_command(id).unwrap().is_none());
        assert!(db.get_pipe_stages(id).unwrap().is_empty());
    }

    #[test]
    fn test_get_output_for_command() {
        let dir = TempDir::new().unwrap();
        let db = HistoryDb::open(&dir.path().join("test.db")).unwrap();
        let record = CommandRecord {
            id: None,
            command: "echo hello".to_string(),
            cwd: "/tmp".to_string(),
            exit_code: Some(0),
            started_at: 1000,
            finished_at: 1001,
            duration_ms: 100,
            output: None,
        };
        let cmd_id = db.insert_command(&record).unwrap();
        // Before update_output, should be None
        assert!(db.get_output_for_command(cmd_id).unwrap().is_none());
        db.update_output(cmd_id, "hello\n").unwrap();
        assert_eq!(
            db.get_output_for_command(cmd_id).unwrap(),
            Some("hello\n".to_string())
        );
    }

    #[test]
    fn test_get_command_text() {
        let dir = TempDir::new().unwrap();
        let db = HistoryDb::open(&dir.path().join("test.db")).unwrap();
        let record = CommandRecord {
            id: None,
            command: "cargo build".to_string(),
            cwd: "/tmp".to_string(),
            exit_code: Some(0),
            started_at: 1000,
            finished_at: 1001,
            duration_ms: 100,
            output: None,
        };
        let cmd_id = db.insert_command(&record).unwrap();
        assert_eq!(
            db.get_command_text(cmd_id).unwrap(),
            Some("cargo build".to_string())
        );
    }

    #[test]
    fn test_db_path_accessor() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let db = HistoryDb::open(&db_path).unwrap();
        assert_eq!(db.path(), db_path);
    }

    /// SOIL-04: SOI worker handles None output (alt-screen apps like vim/htop)
    #[test]
    fn soi_worker_no_output() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let db = HistoryDb::open(&db_path).unwrap();

        let record = CommandRecord {
            id: None,
            command: "vim".to_string(),
            cwd: "/tmp".to_string(),
            exit_code: Some(0),
            started_at: 1000,
            finished_at: 6000,
            duration_ms: 5000,
            output: None,
        };
        let cmd_id = db.insert_command(&record).unwrap();

        // No update_output called -- simulates alt-screen app
        let output = db.get_output_for_command(cmd_id).unwrap();
        assert!(
            output.is_none(),
            "Alt-screen commands should have None output"
        );

        // Verify command text is still retrievable
        let cmd_text = db.get_command_text(cmd_id).unwrap();
        assert_eq!(cmd_text, Some("vim".to_string()));
    }

    /// SOIL-04: SOI worker handles binary placeholder output
    #[test]
    fn soi_worker_binary() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let db = HistoryDb::open(&db_path).unwrap();

        let record = CommandRecord {
            id: None,
            command: "cat binary.bin".to_string(),
            cwd: "/tmp".to_string(),
            exit_code: Some(0),
            started_at: 1000,
            finished_at: 1001,
            duration_ms: 100,
            output: None,
        };
        let cmd_id = db.insert_command(&record).unwrap();

        // Store binary placeholder (what process_output produces for binary content)
        db.update_output(cmd_id, "[binary output: 4096 bytes]")
            .unwrap();

        let output = db.get_output_for_command(cmd_id).unwrap();
        assert!(output.is_some());
        let text = output.unwrap();
        assert!(
            text.contains("binary output"),
            "Binary placeholder should be retrievable"
        );

        let cmd_text = db.get_command_text(cmd_id).unwrap();
        assert_eq!(cmd_text, Some("cat binary.bin".to_string()));
    }
}
