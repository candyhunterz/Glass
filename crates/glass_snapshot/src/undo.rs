//! Undo engine -- restores files to pre-command state from snapshot data.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::types::{Confidence, FileOutcome, SnapshotFileRecord, UndoResult};
use crate::SnapshotStore;

/// Engine that performs undo operations by restoring snapshotted files.
pub struct UndoEngine<'a> {
    store: &'a SnapshotStore,
}

impl<'a> UndoEngine<'a> {
    /// Create a new UndoEngine backed by the given SnapshotStore.
    pub fn new(store: &'a SnapshotStore) -> Self {
        Self { store }
    }

    /// Undo the most recent file-modifying command.
    ///
    /// Returns `Ok(None)` if there are no parser snapshots to undo.
    /// Returns `Ok(Some(UndoResult))` with per-file outcomes otherwise.
    pub fn undo_latest(&self) -> Result<Option<UndoResult>> {
        todo!("implement in GREEN phase")
    }

    /// Check whether a file has been modified since the command ran.
    ///
    /// Compares the current on-disk hash against the watcher-recorded
    /// post-command hash for the same command_id.
    fn check_conflict(&self, _file_path: &Path, _command_id: i64) -> Result<Option<(String, Option<String>)>> {
        todo!("implement in GREEN phase")
    }

    /// Restore a single file from its snapshot record.
    fn restore_file(&self, _file_rec: &SnapshotFileRecord, _command_id: i64) -> (PathBuf, FileOutcome) {
        todo!("implement in GREEN phase")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Helper: create a SnapshotStore in a temp directory.
    fn setup() -> (SnapshotStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let glass_dir = dir.path().join(".glass");
        std::fs::create_dir_all(&glass_dir).unwrap();
        let store = SnapshotStore::open(&glass_dir).unwrap();
        (store, dir)
    }

    #[test]
    fn test_undo_latest_no_snapshots() {
        let (store, _dir) = setup();
        let engine = UndoEngine::new(&store);
        let result = engine.undo_latest().unwrap();
        assert!(result.is_none(), "Should return None when no snapshots exist");
    }

    #[test]
    fn test_undo_latest_restores_file() {
        let (store, dir) = setup();
        let engine = UndoEngine::new(&store);

        // Create a file with original content
        let file_path = dir.path().join("target.txt");
        std::fs::write(&file_path, b"original content").unwrap();

        // Create a parser snapshot capturing the original content
        let sid = store.create_snapshot(1, dir.path().to_str().unwrap()).unwrap();
        store.store_file(sid, &file_path, "parser").unwrap();

        // Simulate: command modifies the file
        std::fs::write(&file_path, b"modified by command").unwrap();

        // Undo should restore original content
        let result = engine.undo_latest().unwrap().expect("should have a result");
        assert_eq!(result.snapshot_id, sid);
        assert_eq!(result.command_id, 1);

        // Verify file was restored
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "original content");

        // Verify outcome is Restored
        assert_eq!(result.files.len(), 1);
        assert!(matches!(result.files[0].1, FileOutcome::Restored));
    }

    #[test]
    fn test_undo_deletes_new_file() {
        let (store, dir) = setup();
        let engine = UndoEngine::new(&store);

        let file_path = dir.path().join("new_file.txt");

        // Create a parser snapshot with NULL hash (file didn't exist before command)
        let sid = store.create_snapshot(1, dir.path().to_str().unwrap()).unwrap();
        store.db().insert_snapshot_file(
            sid,
            &file_path,
            None, // NULL hash -- file didn't exist
            None,
            "parser",
        ).unwrap();

        // Simulate: command creates the file
        std::fs::write(&file_path, b"created by command").unwrap();
        assert!(file_path.exists());

        // Undo should delete the file
        let result = engine.undo_latest().unwrap().expect("should have a result");
        assert!(!file_path.exists(), "File should be deleted after undo");

        assert_eq!(result.files.len(), 1);
        assert!(matches!(result.files[0].1, FileOutcome::Deleted));
    }

    #[test]
    fn test_undo_skips_already_absent() {
        let (store, dir) = setup();
        let engine = UndoEngine::new(&store);

        let file_path = dir.path().join("ghost.txt");

        // Snapshot with NULL hash, file doesn't exist on disk either
        let sid = store.create_snapshot(1, dir.path().to_str().unwrap()).unwrap();
        store.db().insert_snapshot_file(sid, &file_path, None, None, "parser").unwrap();

        assert!(!file_path.exists());

        let result = engine.undo_latest().unwrap().expect("should have a result");
        assert_eq!(result.files.len(), 1);
        assert!(matches!(result.files[0].1, FileOutcome::Skipped));
    }

    #[test]
    fn test_conflict_detection() {
        let (store, dir) = setup();
        let engine = UndoEngine::new(&store);

        let file_path = dir.path().join("conflict.txt");

        // Pre-exec parser snapshot (original content)
        std::fs::write(&file_path, b"original").unwrap();
        let sid = store.create_snapshot(1, dir.path().to_str().unwrap()).unwrap();
        store.store_file(sid, &file_path, "parser").unwrap();

        // Watcher snapshot recording post-command state
        let watcher_sid = store.create_snapshot(1, dir.path().to_str().unwrap()).unwrap();
        std::fs::write(&file_path, b"after command").unwrap();
        store.store_file(watcher_sid, &file_path, "watcher").unwrap();

        // User modifies file AFTER the command (creating a conflict)
        std::fs::write(&file_path, b"user edit after command").unwrap();

        // Undo should detect conflict
        let result = engine.undo_latest().unwrap().expect("should have a result");
        assert_eq!(result.files.len(), 1);
        match &result.files[0].1 {
            FileOutcome::Conflict { current_hash, expected_hash } => {
                assert!(!current_hash.is_empty());
                assert!(expected_hash.is_some());
            }
            other => panic!("Expected Conflict, got {:?}", other),
        }
    }

    #[test]
    fn test_no_conflict_when_file_unchanged() {
        let (store, dir) = setup();
        let engine = UndoEngine::new(&store);

        let file_path = dir.path().join("unchanged.txt");

        // Pre-exec parser snapshot
        std::fs::write(&file_path, b"original").unwrap();
        let sid = store.create_snapshot(1, dir.path().to_str().unwrap()).unwrap();
        store.store_file(sid, &file_path, "parser").unwrap();

        // Watcher snapshot recording post-command state
        let watcher_sid = store.create_snapshot(1, dir.path().to_str().unwrap()).unwrap();
        std::fs::write(&file_path, b"after command").unwrap();
        store.store_file(watcher_sid, &file_path, "watcher").unwrap();

        // File still matches watcher hash (no user edit after command)
        // file_path already contains "after command"

        // Undo should NOT detect conflict -- file should be Restored
        let result = engine.undo_latest().unwrap().expect("should have a result");
        assert_eq!(result.files.len(), 1);
        assert!(
            matches!(result.files[0].1, FileOutcome::Restored),
            "Expected Restored, got {:?}",
            result.files[0].1
        );

        // Verify file was restored to original
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "original");
    }

    #[test]
    fn test_only_parser_files_restored() {
        let (store, dir) = setup();
        let engine = UndoEngine::new(&store);

        let parser_file = dir.path().join("parser_target.txt");
        let watcher_file = dir.path().join("watcher_only.txt");

        // Create both files
        std::fs::write(&parser_file, b"parser original").unwrap();
        std::fs::write(&watcher_file, b"watcher original").unwrap();

        // Snapshot with both parser and watcher files
        let sid = store.create_snapshot(1, dir.path().to_str().unwrap()).unwrap();
        store.store_file(sid, &parser_file, "parser").unwrap();
        store.store_file(sid, &watcher_file, "watcher").unwrap();

        // Modify both files
        std::fs::write(&parser_file, b"parser modified").unwrap();
        std::fs::write(&watcher_file, b"watcher modified").unwrap();

        // Undo should only restore parser file
        let result = engine.undo_latest().unwrap().expect("should have a result");

        // Only parser files should appear in outcomes
        assert_eq!(result.files.len(), 1);
        let (path, outcome) = &result.files[0];
        assert_eq!(path, &parser_file);
        assert!(matches!(outcome, FileOutcome::Restored));

        // Parser file restored, watcher file unchanged
        assert_eq!(std::fs::read_to_string(&parser_file).unwrap(), "parser original");
        assert_eq!(std::fs::read_to_string(&watcher_file).unwrap(), "watcher modified");
    }
}
