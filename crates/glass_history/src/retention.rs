use anyhow::Result;
use rusqlite::{params, Connection};

/// Prune old records by age and database size.
///
/// 1. Age pruning: delete records with started_at older than max_age_days.
/// 2. Size pruning: if database exceeds max_size_bytes, delete oldest 10%.
///
/// Returns total number of records deleted.
pub fn prune(conn: &Connection, max_age_days: u32, max_size_bytes: u64) -> Result<u64> {
    let mut total_deleted = 0u64;

    // 1. Age-based pruning
    let cutoff = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        - (max_age_days as i64 * 86400);

    // Collect ids to delete first
    let ids_to_delete: Vec<i64> = {
        let mut stmt = conn.prepare("SELECT id FROM commands WHERE started_at < ?1")?;
        let result = stmt
            .query_map(params![cutoff], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        result
    };

    if !ids_to_delete.is_empty() {
        let tx = conn.unchecked_transaction()?;
        for &id in &ids_to_delete {
            tx.execute("DELETE FROM pipe_stages WHERE command_id = ?1", params![id])?;
        }
        for &id in &ids_to_delete {
            // Delete from FTS table (standard FTS5 -- just DELETE by rowid)
            tx.execute("DELETE FROM commands_fts WHERE rowid = ?1", params![id])?;
        }
        for &id in &ids_to_delete {
            tx.execute("DELETE FROM commands WHERE id = ?1", params![id])?;
        }
        tx.commit()?;
        total_deleted += ids_to_delete.len() as u64;
    }

    // 2. Size-based pruning
    let db_size: i64 = conn.query_row(
        "SELECT page_count * page_size FROM pragma_page_count, pragma_page_size",
        [],
        |row| row.get(0),
    )?;
    let db_size = db_size as u64;

    if db_size > max_size_bytes {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM commands", [], |row| row.get(0))?;
        let count = count as u64;
        let to_delete = (count / 10).max(1) as i64;

        // Collect ids of oldest records
        let old_ids: Vec<i64> = {
            let mut stmt =
                conn.prepare("SELECT id FROM commands ORDER BY started_at ASC LIMIT ?1")?;
            let result = stmt
                .query_map(params![to_delete], |row| row.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            result
        };

        if !old_ids.is_empty() {
            let tx = conn.unchecked_transaction()?;
            for &id in &old_ids {
                tx.execute("DELETE FROM pipe_stages WHERE command_id = ?1", params![id])?;
            }
            for &id in &old_ids {
                tx.execute("DELETE FROM commands_fts WHERE rowid = ?1", params![id])?;
            }
            for &id in &old_ids {
                tx.execute("DELETE FROM commands WHERE id = ?1", params![id])?;
            }
            tx.commit()?;
            total_deleted += old_ids.len() as u64;
        }
    }

    Ok(total_deleted)
}

#[cfg(test)]
mod tests {
    use crate::db::{CommandRecord, HistoryDb};
    use tempfile::TempDir;

    fn test_db() -> (HistoryDb, TempDir) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let db = HistoryDb::open(&db_path).unwrap();
        (db, dir)
    }

    #[test]
    fn test_prune_by_age() {
        let (db, _dir) = test_db();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Insert old records (2 days ago)
        let old_time = now - 2 * 86400;
        for i in 0..3 {
            db.insert_command(&CommandRecord {
                id: None,
                command: format!("old command {}", i),
                cwd: "/tmp".to_string(),
                exit_code: Some(0),
                started_at: old_time,
                finished_at: old_time + 1,
                duration_ms: 1000,
                output: None,
            })
            .unwrap();
        }

        // Insert recent record
        db.insert_command(&CommandRecord {
            id: None,
            command: "recent command".to_string(),
            cwd: "/tmp".to_string(),
            exit_code: Some(0),
            started_at: now,
            finished_at: now + 1,
            duration_ms: 1000,
            output: None,
        })
        .unwrap();

        assert_eq!(db.command_count().unwrap(), 4);

        // Prune with max_age = 1 day
        let deleted = db.prune(1, u64::MAX).unwrap();
        assert_eq!(deleted, 3);
        assert_eq!(db.command_count().unwrap(), 1);
    }

    #[test]
    fn test_prune_by_age_keeps_recent() {
        let (db, _dir) = test_db();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Insert only recent records
        for i in 0..3 {
            db.insert_command(&CommandRecord {
                id: None,
                command: format!("recent command {}", i),
                cwd: "/tmp".to_string(),
                exit_code: Some(0),
                started_at: now,
                finished_at: now + 1,
                duration_ms: 1000,
                output: None,
            })
            .unwrap();
        }

        let deleted = db.prune(1, u64::MAX).unwrap();
        assert_eq!(deleted, 0);
        assert_eq!(db.command_count().unwrap(), 3);
    }

    #[test]
    fn test_prune_fts_sync() {
        let (db, _dir) = test_db();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Insert old record with distinctive text
        let old_time = now - 2 * 86400;
        db.insert_command(&CommandRecord {
            id: None,
            command: "uniqueoldcommand_xyz".to_string(),
            cwd: "/tmp".to_string(),
            exit_code: Some(0),
            started_at: old_time,
            finished_at: old_time + 1,
            duration_ms: 1000,
            output: None,
        })
        .unwrap();

        // Verify FTS finds it before pruning
        let results = db.search("uniqueoldcommand_xyz", 10).unwrap();
        assert_eq!(results.len(), 1);

        // Prune
        db.prune(1, u64::MAX).unwrap();

        // FTS should now return empty
        let results = db.search("uniqueoldcommand_xyz", 10).unwrap();
        assert!(results.is_empty());
    }
}
