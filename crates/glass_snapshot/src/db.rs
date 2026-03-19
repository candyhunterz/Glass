use std::path::Path;

use anyhow::Result;
use rusqlite::{params, Connection};

use crate::types::{SnapshotFileRecord, SnapshotRecord};

/// Current schema version. Bump when adding migrations.
const SCHEMA_VERSION: i64 = 1;

/// SQLite-backed snapshot metadata database.
pub struct SnapshotDb {
    conn: Connection,
}

impl SnapshotDb {
    /// Open (or create) the snapshot database at the given path.
    /// Sets WAL mode, enables foreign keys, and creates the schema if needed.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Self::open_connection(path)?;
        Self::create_schema(&conn)?;
        Self::migrate(&conn)?;
        Ok(Self { conn })
    }

    /// Open a connection with WAL pragmas, performing corruption recovery if needed.
    fn open_connection(path: &Path) -> Result<Connection> {
        let conn = Connection::open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        let pragma_result = conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;
             PRAGMA foreign_keys = ON;",
        );
        // If pragmas fail, the file is likely corrupt (e.g. "file is not a database").
        let corrupt_reason = if let Err(ref e) = pragma_result {
            Some(format!("pragma failed: {e}"))
        } else {
            // Check database integrity
            match conn.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0)) {
                Ok(ref result) if result == "ok" => None,
                Ok(ref result) => Some(format!("integrity_check returned: {result}")),
                Err(ref e) => Some(format!("integrity_check error: {e}")),
            }
        };
        if let Some(reason) = corrupt_reason {
            tracing::warn!(
                "Database corruption detected at {} ({reason})",
                path.display()
            );
            drop(conn);
            let backup = path.with_extension("db.corrupt");
            tracing::warn!("Renaming corrupt DB to {}", backup.display());
            let _ = std::fs::rename(path, &backup);
            // Reopen fresh database
            let conn = Connection::open(path)?;
            conn.execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA busy_timeout = 5000;
                 PRAGMA foreign_keys = ON;",
            )?;
            Ok(conn)
        } else {
            Ok(conn)
        }
    }

    fn create_schema(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS snapshots (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                command_id  INTEGER NOT NULL,
                cwd         TEXT NOT NULL,
                created_at  INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE INDEX IF NOT EXISTS idx_snapshots_command ON snapshots(command_id);

            CREATE TABLE IF NOT EXISTS snapshot_files (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                snapshot_id INTEGER NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
                file_path   TEXT NOT NULL,
                blob_hash   TEXT,
                file_size   INTEGER,
                source      TEXT NOT NULL DEFAULT 'parser'
            );
            CREATE INDEX IF NOT EXISTS idx_sf_snapshot ON snapshot_files(snapshot_id);
            CREATE INDEX IF NOT EXISTS idx_sf_hash ON snapshot_files(blob_hash);",
        )?;
        Ok(())
    }

    /// Apply schema migrations based on PRAGMA user_version.
    fn migrate(conn: &Connection) -> Result<()> {
        let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version < 1 {
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        }
        Ok(())
    }

    /// Create a new snapshot record. Returns the snapshot id.
    pub fn create_snapshot(&self, command_id: i64, cwd: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO snapshots (command_id, cwd) VALUES (?1, ?2)",
            params![command_id, cwd],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Insert a file entry into a snapshot.
    pub fn insert_snapshot_file(
        &self,
        snapshot_id: i64,
        file_path: &Path,
        blob_hash: Option<&str>,
        file_size: Option<u64>,
        source: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO snapshot_files (snapshot_id, file_path, blob_hash, file_size, source)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                snapshot_id,
                file_path.to_string_lossy().as_ref(),
                blob_hash,
                file_size.map(|s| s as i64),
                source,
            ],
        )?;
        Ok(())
    }

    /// Get a snapshot record by id.
    pub fn get_snapshot(&self, id: i64) -> Result<Option<SnapshotRecord>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, command_id, cwd, created_at FROM snapshots WHERE id = ?1")?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(SnapshotRecord {
                id: row.get(0)?,
                command_id: row.get(1)?,
                cwd: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        match rows.next() {
            Some(Ok(record)) => Ok(Some(record)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// Get all file entries for a snapshot.
    pub fn get_snapshot_files(&self, snapshot_id: i64) -> Result<Vec<SnapshotFileRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, snapshot_id, file_path, blob_hash, file_size, source
             FROM snapshot_files WHERE snapshot_id = ?1",
        )?;
        let rows = stmt.query_map(params![snapshot_id], |row| {
            let file_size: Option<i64> = row.get(4)?;
            Ok(SnapshotFileRecord {
                id: row.get(0)?,
                snapshot_id: row.get(1)?,
                file_path: row.get(2)?,
                blob_hash: row.get(3)?,
                file_size: file_size.map(|s| s as u64),
                source: row.get(5)?,
            })
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Get all snapshots for a given command_id.
    pub fn get_snapshots_by_command(&self, command_id: i64) -> Result<Vec<SnapshotRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, command_id, cwd, created_at FROM snapshots WHERE command_id = ?1",
        )?;
        let rows = stmt.query_map(params![command_id], |row| {
            Ok(SnapshotRecord {
                id: row.get(0)?,
                command_id: row.get(1)?,
                cwd: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Delete a snapshot by id. Returns true if it existed.
    /// Cascades to snapshot_files rows via foreign key.
    pub fn delete_snapshot(&self, id: i64) -> Result<bool> {
        let deleted = self
            .conn
            .execute("DELETE FROM snapshots WHERE id = ?1", params![id])?;
        Ok(deleted > 0)
    }

    /// Get the most recent snapshot that has at least one parser-sourced file.
    /// Used by the undo engine to find the latest undoable command.
    pub fn get_latest_parser_snapshot(&self) -> Result<Option<SnapshotRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.command_id, s.cwd, s.created_at
             FROM snapshots s
             WHERE EXISTS (
                 SELECT 1 FROM snapshot_files sf
                 WHERE sf.snapshot_id = s.id AND sf.source = 'parser'
             )
             ORDER BY s.id DESC
             LIMIT 1",
        )?;
        let mut rows = stmt.query_map([], |row| {
            Ok(SnapshotRecord {
                id: row.get(0)?,
                command_id: row.get(1)?,
                cwd: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        match rows.next() {
            Some(Ok(record)) => Ok(Some(record)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// Count total number of snapshots.
    pub fn count_snapshots(&self) -> Result<u64> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM snapshots", [], |row| row.get(0))?;
        Ok(count as u64)
    }

    /// Delete all snapshots with created_at < epoch. Returns deleted IDs.
    pub fn delete_snapshots_before(&self, epoch: i64) -> Result<Vec<i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM snapshots WHERE created_at < ?1")?;
        let ids: Vec<i64> = stmt
            .query_map(params![epoch], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        if !ids.is_empty() {
            self.conn.execute(
                "DELETE FROM snapshots WHERE created_at < ?1",
                params![epoch],
            )?;
        }
        Ok(ids)
    }

    /// Get the N oldest snapshot IDs, ordered by created_at ASC.
    pub fn get_oldest_snapshot_ids(&self, limit: u32) -> Result<Vec<i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM snapshots ORDER BY created_at ASC LIMIT ?1")?;
        let ids: Vec<i64> = stmt
            .query_map(params![limit], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(ids)
    }

    /// Get all distinct blob hashes still referenced by snapshot_files.
    pub fn get_referenced_hashes(&self) -> Result<std::collections::HashSet<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT blob_hash FROM snapshot_files WHERE blob_hash IS NOT NULL")?;
        let hashes: std::collections::HashSet<String> =
            stmt.query_map([], |row| row.get(0))?
                .collect::<std::result::Result<std::collections::HashSet<_>, _>>()?;
        Ok(hashes)
    }

    /// Get the created_at timestamp of the Nth newest snapshot (1-indexed).
    /// Returns None if fewer than N snapshots exist.
    pub fn get_nth_newest_created_at(&self, n: u32) -> Result<Option<i64>> {
        let offset = n.saturating_sub(1);
        let mut stmt = self.conn.prepare(
            "SELECT created_at FROM snapshots ORDER BY created_at DESC LIMIT 1 OFFSET ?1",
        )?;
        let mut rows = stmt.query_map(params![offset], |row| row.get::<_, i64>(0))?;
        match rows.next() {
            Some(Ok(ts)) => Ok(Some(ts)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// Set the created_at timestamp of a snapshot (for testing).
    #[cfg(test)]
    pub fn set_created_at(&self, snapshot_id: i64, created_at: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE snapshots SET created_at = ?1 WHERE id = ?2",
            params![created_at, snapshot_id],
        )?;
        Ok(())
    }

    /// Get a parser snapshot by command_id (newest if multiple exist).
    /// Only returns snapshots that have at least one parser-sourced file.
    pub fn get_parser_snapshot_by_command(
        &self,
        command_id: i64,
    ) -> Result<Option<SnapshotRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.command_id, s.cwd, s.created_at
             FROM snapshots s
             WHERE s.command_id = ?1
               AND EXISTS (
                   SELECT 1 FROM snapshot_files sf
                   WHERE sf.snapshot_id = s.id AND sf.source = 'parser'
               )
             ORDER BY s.created_at DESC
             LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![command_id], |row| {
            Ok(SnapshotRecord {
                id: row.get(0)?,
                command_id: row.get(1)?,
                cwd: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;
        match rows.next() {
            Some(Ok(record)) => Ok(Some(record)),
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// Update the command_id on an existing snapshot.
    pub fn update_command_id(&self, snapshot_id: i64, command_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE snapshots SET command_id = ?1 WHERE id = ?2",
            params![command_id, snapshot_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_db() -> (SnapshotDb, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("snapshots.db");
        let db = SnapshotDb::open(&db_path).unwrap();
        (db, dir)
    }

    #[test]
    fn test_schema_creation() {
        let (db, _dir) = test_db();
        // Verify snapshots table exists with correct columns
        let mut stmt = db.conn.prepare("PRAGMA table_info(snapshots)").unwrap();
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(columns.contains(&"id".to_string()));
        assert!(columns.contains(&"command_id".to_string()));
        assert!(columns.contains(&"cwd".to_string()));
        assert!(columns.contains(&"created_at".to_string()));

        // Verify snapshot_files table exists with correct columns
        let mut stmt = db
            .conn
            .prepare("PRAGMA table_info(snapshot_files)")
            .unwrap();
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(columns.contains(&"id".to_string()));
        assert!(columns.contains(&"snapshot_id".to_string()));
        assert!(columns.contains(&"file_path".to_string()));
        assert!(columns.contains(&"blob_hash".to_string()));
        assert!(columns.contains(&"file_size".to_string()));
        assert!(columns.contains(&"source".to_string()));
    }

    #[test]
    fn test_persistence() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("persist.db");

        // Create and insert
        {
            let db = SnapshotDb::open(&db_path).unwrap();
            db.create_snapshot(1, "/home/user").unwrap();
        }

        // Reopen and verify
        {
            let db = SnapshotDb::open(&db_path).unwrap();
            let snapshot = db
                .get_snapshot(1)
                .unwrap()
                .expect("should exist after reopen");
            assert_eq!(snapshot.command_id, 1);
            assert_eq!(snapshot.cwd, "/home/user");
        }
    }

    #[test]
    fn test_command_id_link() {
        let (db, _dir) = test_db();
        let sid = db.create_snapshot(42, "/tmp").unwrap();
        let snapshots = db.get_snapshots_by_command(42).unwrap();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].id, sid);
        assert_eq!(snapshots[0].command_id, 42);
    }

    #[test]
    fn test_cascade_delete() {
        let (db, _dir) = test_db();
        let sid = db.create_snapshot(1, "/tmp").unwrap();
        db.insert_snapshot_file(
            sid,
            Path::new("/tmp/a.txt"),
            Some("abc123"),
            Some(100),
            "parser",
        )
        .unwrap();
        db.insert_snapshot_file(
            sid,
            Path::new("/tmp/b.txt"),
            Some("def456"),
            Some(200),
            "watcher",
        )
        .unwrap();

        // Verify files exist
        let files = db.get_snapshot_files(sid).unwrap();
        assert_eq!(files.len(), 2);

        // Delete snapshot -- files should cascade
        assert!(db.delete_snapshot(sid).unwrap());
        let files = db.get_snapshot_files(sid).unwrap();
        assert_eq!(files.len(), 0);
        assert!(db.get_snapshot(sid).unwrap().is_none());
    }

    #[test]
    fn test_get_latest_parser_snapshot_none_when_empty() {
        let (db, _dir) = test_db();
        let result = db.get_latest_parser_snapshot().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_get_latest_parser_snapshot_none_when_only_watcher() {
        let (db, _dir) = test_db();
        let sid = db.create_snapshot(1, "/tmp").unwrap();
        db.insert_snapshot_file(
            sid,
            Path::new("/tmp/a.txt"),
            Some("aaa"),
            Some(10),
            "watcher",
        )
        .unwrap();
        let result = db.get_latest_parser_snapshot().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_get_latest_parser_snapshot_returns_snapshot_with_parser_file() {
        let (db, _dir) = test_db();
        let sid = db.create_snapshot(1, "/tmp").unwrap();
        db.insert_snapshot_file(
            sid,
            Path::new("/tmp/a.txt"),
            Some("aaa"),
            Some(10),
            "parser",
        )
        .unwrap();
        let result = db.get_latest_parser_snapshot().unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, sid);
    }

    #[test]
    fn test_get_latest_parser_snapshot_returns_newest() {
        let (db, _dir) = test_db();
        // Older snapshot with parser file
        let sid1 = db.create_snapshot(1, "/tmp").unwrap();
        db.insert_snapshot_file(
            sid1,
            Path::new("/tmp/a.txt"),
            Some("aaa"),
            Some(10),
            "parser",
        )
        .unwrap();
        // Newer snapshot with only watcher files
        let sid2 = db.create_snapshot(2, "/tmp").unwrap();
        db.insert_snapshot_file(
            sid2,
            Path::new("/tmp/b.txt"),
            Some("bbb"),
            Some(20),
            "watcher",
        )
        .unwrap();
        // Newest snapshot with parser file
        let sid3 = db.create_snapshot(3, "/tmp").unwrap();
        db.insert_snapshot_file(
            sid3,
            Path::new("/tmp/c.txt"),
            Some("ccc"),
            Some(30),
            "parser",
        )
        .unwrap();

        let result = db.get_latest_parser_snapshot().unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, sid3);
    }

    #[test]
    fn test_get_latest_parser_snapshot_with_pending_command() {
        let (db, _dir) = test_db();
        // command_id=0 means pending pre-exec snapshot
        let sid = db.create_snapshot(0, "/home/user").unwrap();
        db.insert_snapshot_file(
            sid,
            Path::new("/home/user/file.rs"),
            Some("abc"),
            Some(100),
            "parser",
        )
        .unwrap();
        let result = db.get_latest_parser_snapshot().unwrap();
        assert!(result.is_some());
        let snap = result.unwrap();
        assert_eq!(snap.id, sid);
        assert_eq!(snap.command_id, 0);
    }

    #[test]
    fn test_corrupt_db_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        std::fs::write(&db_path, b"not a sqlite database at all").unwrap();
        let db = SnapshotDb::open(&db_path);
        assert!(db.is_ok(), "Should recover from corrupt DB");
        assert!(
            db_path.with_extension("db.corrupt").exists(),
            "Corrupt DB should be renamed"
        );
    }

    #[test]
    fn test_insert_snapshot_file_null_hash() {
        let (db, _dir) = test_db();
        let sid = db.create_snapshot(1, "/tmp").unwrap();
        db.insert_snapshot_file(sid, Path::new("/tmp/gone.txt"), None, None, "parser")
            .unwrap();

        let files = db.get_snapshot_files(sid).unwrap();
        assert_eq!(files.len(), 1);
        assert!(files[0].blob_hash.is_none());
        assert!(files[0].file_size.is_none());
    }
}
