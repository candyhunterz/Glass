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
        .unwrap_or(std::time::Duration::ZERO)
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
        let placeholders: String = ids_to_delete
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        let tx = conn.unchecked_transaction()?;
        // Batch DELETE for each dependent table, then the main table.
        // Order: dependents first (pipe_stages, output_records, command_output_records, FTS), then commands.
        for &(table, col) in &[
            ("pipe_stages", "command_id"),
            ("output_records", "command_id"),
            ("command_output_records", "command_id"),
            ("commands_fts", "rowid"),
            ("commands", "id"),
        ] {
            let sql = format!("DELETE FROM {} WHERE {} IN ({})", table, col, placeholders);
            let params: Vec<&dyn rusqlite::types::ToSql> = ids_to_delete
                .iter()
                .map(|id| id as &dyn rusqlite::types::ToSql)
                .collect();
            tx.execute(&sql, params.as_slice())?;
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
            let placeholders: String = old_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let tx = conn.unchecked_transaction()?;
            for &(table, col) in &[
                ("pipe_stages", "command_id"),
                ("output_records", "command_id"),
                ("command_output_records", "command_id"),
                ("commands_fts", "rowid"),
                ("commands", "id"),
            ] {
                let sql = format!("DELETE FROM {} WHERE {} IN ({})", table, col, placeholders);
                let params: Vec<&dyn rusqlite::types::ToSql> = old_ids
                    .iter()
                    .map(|id| id as &dyn rusqlite::types::ToSql)
                    .collect();
                tx.execute(&sql, params.as_slice())?;
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
    use glass_soi::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity};
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

    fn make_parsed_output() -> ParsedOutput {
        ParsedOutput {
            output_type: OutputType::RustCompiler,
            summary: OutputSummary {
                one_line: "2 errors".to_string(),
                token_estimate: 10,
                severity: Severity::Error,
            },
            records: vec![
                OutputRecord::CompilerError {
                    file: "src/main.rs".to_string(),
                    line: 1,
                    column: None,
                    severity: Severity::Error,
                    code: None,
                    message: "type mismatch".to_string(),
                    context_lines: None,
                },
                OutputRecord::CompilerError {
                    file: "src/lib.rs".to_string(),
                    line: 5,
                    column: Some(3),
                    severity: Severity::Error,
                    code: Some("E0308".to_string()),
                    message: "mismatched types".to_string(),
                    context_lines: None,
                },
            ],
            raw_line_count: 10,
            raw_byte_count: 200,
        }
    }

    #[test]
    fn test_prune_cascades_to_soi() {
        let (db, _dir) = test_db();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Insert old command (2 days ago) with SOI data
        let old_time = now - 2 * 86400;
        let old_id = db
            .insert_command(&CommandRecord {
                id: None,
                command: "old_build_cmd".to_string(),
                cwd: "/tmp".to_string(),
                exit_code: Some(1),
                started_at: old_time,
                finished_at: old_time + 1,
                duration_ms: 1000,
                output: None,
            })
            .unwrap();
        db.insert_parsed_output(old_id, &make_parsed_output())
            .unwrap();

        // Insert recent command with SOI data
        let recent_id = db
            .insert_command(&CommandRecord {
                id: None,
                command: "recent_build_cmd".to_string(),
                cwd: "/tmp".to_string(),
                exit_code: Some(0),
                started_at: now,
                finished_at: now + 1,
                duration_ms: 500,
                output: None,
            })
            .unwrap();
        db.insert_parsed_output(recent_id, &make_parsed_output())
            .unwrap();

        // Prune by age (1 day max) -- old command should go
        let deleted = db.prune(1, u64::MAX).unwrap();
        assert_eq!(deleted, 1, "one command should be pruned");

        // Old command's SOI records must be gone
        let old_summary = db.get_output_summary(old_id).unwrap();
        assert!(
            old_summary.is_none(),
            "old command_output_records row must be deleted"
        );
        let old_records = db
            .get_output_records(old_id, None, None, None, 100)
            .unwrap();
        assert!(
            old_records.is_empty(),
            "old output_records rows must be deleted"
        );

        // Recent command's SOI records must survive
        let recent_summary = db.get_output_summary(recent_id).unwrap();
        assert!(
            recent_summary.is_some(),
            "recent command_output_records row must survive"
        );
        let recent_records = db
            .get_output_records(recent_id, None, None, None, 100)
            .unwrap();
        assert_eq!(
            recent_records.len(),
            2,
            "recent output_records rows must survive"
        );
    }

    #[test]
    fn test_size_prune_cascades_to_soi() {
        let (db, _dir) = test_db();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Insert 10 commands each with SOI data, oldest first
        let mut ids = Vec::new();
        for i in 0..10i64 {
            let ts = now - (10 - i) * 1000; // oldest has smallest ts
            let id = db
                .insert_command(&CommandRecord {
                    id: None,
                    command: format!("build_{}", i),
                    cwd: "/tmp".to_string(),
                    exit_code: Some(0),
                    started_at: ts,
                    finished_at: ts + 1,
                    duration_ms: 100,
                    output: Some("x".repeat(1000)),
                })
                .unwrap();
            db.insert_parsed_output(id, &make_parsed_output()).unwrap();
            ids.push(id);
        }

        // Prune by size (max_size_bytes=1 forces pruning)
        let deleted = db.prune(u32::MAX, 1).unwrap();
        assert!(deleted > 0, "size prune should delete at least one command");

        // The oldest command (ids[0]) should have its SOI records gone
        let oldest_summary = db.get_output_summary(ids[0]).unwrap();
        assert!(
            oldest_summary.is_none(),
            "oldest command_output_records row must be deleted by size prune"
        );
        let oldest_records = db
            .get_output_records(ids[0], None, None, None, 100)
            .unwrap();
        assert!(
            oldest_records.is_empty(),
            "oldest output_records rows must be deleted by size prune"
        );
    }

    #[test]
    fn test_delete_command_cascades_soi() {
        let (db, _dir) = test_db();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let id = db
            .insert_command(&CommandRecord {
                id: None,
                command: "some_command".to_string(),
                cwd: "/tmp".to_string(),
                exit_code: Some(0),
                started_at: now,
                finished_at: now + 1,
                duration_ms: 500,
                output: None,
            })
            .unwrap();
        db.insert_parsed_output(id, &make_parsed_output()).unwrap();

        // Verify SOI data was inserted
        assert!(db.get_output_summary(id).unwrap().is_some());
        assert_eq!(
            db.get_output_records(id, None, None, None, 100)
                .unwrap()
                .len(),
            2
        );

        // Delete the command
        db.delete_command(id).unwrap();

        // SOI records must be gone
        let summary = db.get_output_summary(id).unwrap();
        assert!(
            summary.is_none(),
            "delete_command must cascade to command_output_records"
        );
        let records = db.get_output_records(id, None, None, None, 100).unwrap();
        assert!(
            records.is_empty(),
            "delete_command must cascade to output_records"
        );
    }
}
