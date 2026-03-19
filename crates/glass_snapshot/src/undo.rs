//! Undo engine -- restores files to pre-command state from snapshot data.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::types::{Confidence, FileOutcome, SnapshotFileRecord, SnapshotRecord, UndoResult};
use crate::SnapshotStore;

/// Engine that performs undo operations by restoring snapshotted files.
pub struct UndoEngine<'a> {
    store: &'a SnapshotStore,
    /// Optional project root for path validation.
    /// When set, restore_file will refuse to write/delete outside this directory.
    project_root: Option<PathBuf>,
}

impl<'a> UndoEngine<'a> {
    /// Create a new UndoEngine backed by the given SnapshotStore.
    pub fn new(store: &'a SnapshotStore) -> Self {
        Self {
            store,
            project_root: None,
        }
    }

    /// Create a new UndoEngine with project-root path validation.
    /// Restore operations will be rejected if the target path falls outside `project_root`.
    pub fn with_project_root(store: &'a SnapshotStore, project_root: PathBuf) -> Self {
        Self {
            store,
            project_root: Some(project_root),
        }
    }

    /// Undo the most recent file-modifying command.
    ///
    /// Returns `Ok(None)` if there are no parser snapshots to undo.
    /// Returns `Ok(Some(UndoResult))` with per-file outcomes otherwise.
    /// Deletes the snapshot after successful undo (one-shot).
    pub fn undo_latest(&self) -> Result<Option<UndoResult>> {
        let snapshot = match self.store.db().get_latest_parser_snapshot()? {
            Some(s) => s,
            None => return Ok(None),
        };

        let result = self.restore_snapshot(&snapshot)?;
        self.store.db().delete_snapshot(snapshot.id)?;
        Ok(Some(result))
    }

    /// Undo a specific command by its command_id.
    ///
    /// Returns `Ok(None)` if no parser snapshot exists for the given command.
    /// Returns `Ok(Some(UndoResult))` with per-file outcomes otherwise.
    /// Deletes the snapshot after successful undo (one-shot).
    pub fn undo_command(&self, command_id: i64) -> Result<Option<UndoResult>> {
        let snapshot = match self.store.db().get_parser_snapshot_by_command(command_id)? {
            Some(s) => s,
            None => return Ok(None),
        };

        let result = self.restore_snapshot(&snapshot)?;
        self.store.db().delete_snapshot(snapshot.id)?;
        Ok(Some(result))
    }

    /// Shared logic: restore files from a snapshot record.
    fn restore_snapshot(&self, snapshot: &SnapshotRecord) -> Result<UndoResult> {
        let files = self.store.db().get_snapshot_files(snapshot.id)?;
        let mut outcomes = Vec::new();

        for file_rec in &files {
            // Only restore parser-sourced files (pre-exec snapshots)
            if file_rec.source != "parser" {
                continue;
            }
            outcomes.push(self.restore_file(file_rec, snapshot.command_id));
        }

        Ok(UndoResult {
            snapshot_id: snapshot.id,
            command_id: snapshot.command_id,
            confidence: Confidence::High,
            files: outcomes,
        })
    }

    /// Check whether a file has been modified since the command ran.
    ///
    /// Compares the current on-disk hash against the watcher-recorded
    /// post-command hash for the same command_id.
    ///
    /// Returns `Ok(Some((current_hash, expected_hash)))` if conflict detected,
    /// `Ok(None)` if no conflict.
    fn check_conflict(
        &self,
        file_path: &Path,
        command_id: i64,
    ) -> Result<Option<(String, Option<String>)>> {
        if !file_path.exists() {
            return Ok(None);
        }

        let current_content = std::fs::read(file_path)?;
        let current_hash = blake3::hash(&current_content).to_hex().to_string();

        // Find watcher snapshots for the same command
        let watcher_snapshots = self.store.db().get_snapshots_by_command(command_id)?;
        for ws in &watcher_snapshots {
            let ws_files = self.store.db().get_snapshot_files(ws.id)?;
            for wf in &ws_files {
                if wf.source == "watcher" && wf.file_path == file_path.to_string_lossy().as_ref() {
                    if let Some(ref watcher_hash) = wf.blob_hash {
                        if current_hash != *watcher_hash {
                            return Ok(Some((current_hash, Some(watcher_hash.clone()))));
                        } else {
                            // Current matches watcher -- no conflict
                            return Ok(None);
                        }
                    }
                }
            }
        }

        // No watcher data for this file -- optimistic, no conflict
        Ok(None)
    }

    /// Restore a single file from its snapshot record.
    fn restore_file(
        &self,
        file_rec: &SnapshotFileRecord,
        command_id: i64,
    ) -> (PathBuf, FileOutcome) {
        let path = PathBuf::from(&file_rec.file_path);

        // Validate path is within project root (prevent path-traversal attacks)
        if let Some(ref root) = self.project_root {
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            if !canonical.starts_with(root) {
                let msg = format!("Path outside project root: {}", path.display());
                return (path, FileOutcome::Error(msg));
            }
        }

        // Check for conflicts before restoring
        match self.check_conflict(&path, command_id) {
            Ok(Some((current_hash, expected_hash))) => {
                return (
                    path,
                    FileOutcome::Conflict {
                        current_hash,
                        expected_hash,
                    },
                );
            }
            Err(e) => {
                return (path, FileOutcome::Error(e.to_string()));
            }
            Ok(None) => {}
        }

        match &file_rec.blob_hash {
            Some(hash) => {
                // File existed before command -- restore its content
                match self.store.blobs().read_blob(hash) {
                    Ok(content) => {
                        if let Err(e) = std::fs::write(&path, &content) {
                            (path, FileOutcome::Error(e.to_string()))
                        } else {
                            (path, FileOutcome::Restored)
                        }
                    }
                    Err(e) => (path, FileOutcome::Error(e.to_string())),
                }
            }
            None => {
                // File did not exist before command -- delete it
                if path.exists() {
                    if let Err(e) = std::fs::remove_file(&path) {
                        (path, FileOutcome::Error(e.to_string()))
                    } else {
                        (path, FileOutcome::Deleted)
                    }
                } else {
                    (path, FileOutcome::Skipped)
                }
            }
        }
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
        assert!(
            result.is_none(),
            "Should return None when no snapshots exist"
        );
    }

    #[test]
    fn test_undo_latest_restores_file() {
        let (store, dir) = setup();
        let engine = UndoEngine::new(&store);

        // Create a file with original content
        let file_path = dir.path().join("target.txt");
        std::fs::write(&file_path, b"original content").unwrap();

        // Create a parser snapshot capturing the original content
        let sid = store
            .create_snapshot(1, dir.path().to_str().unwrap())
            .unwrap();
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
        let sid = store
            .create_snapshot(1, dir.path().to_str().unwrap())
            .unwrap();
        store
            .db()
            .insert_snapshot_file(
                sid, &file_path, None, // NULL hash -- file didn't exist
                None, "parser",
            )
            .unwrap();

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
        let sid = store
            .create_snapshot(1, dir.path().to_str().unwrap())
            .unwrap();
        store
            .db()
            .insert_snapshot_file(sid, &file_path, None, None, "parser")
            .unwrap();

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
        let sid = store
            .create_snapshot(1, dir.path().to_str().unwrap())
            .unwrap();
        store.store_file(sid, &file_path, "parser").unwrap();

        // Watcher snapshot recording post-command state
        let watcher_sid = store
            .create_snapshot(1, dir.path().to_str().unwrap())
            .unwrap();
        std::fs::write(&file_path, b"after command").unwrap();
        store
            .store_file(watcher_sid, &file_path, "watcher")
            .unwrap();

        // User modifies file AFTER the command (creating a conflict)
        std::fs::write(&file_path, b"user edit after command").unwrap();

        // Undo should detect conflict
        let result = engine.undo_latest().unwrap().expect("should have a result");
        assert_eq!(result.files.len(), 1);
        match &result.files[0].1 {
            FileOutcome::Conflict {
                current_hash,
                expected_hash,
            } => {
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
        let sid = store
            .create_snapshot(1, dir.path().to_str().unwrap())
            .unwrap();
        store.store_file(sid, &file_path, "parser").unwrap();

        // Watcher snapshot recording post-command state
        let watcher_sid = store
            .create_snapshot(1, dir.path().to_str().unwrap())
            .unwrap();
        std::fs::write(&file_path, b"after command").unwrap();
        store
            .store_file(watcher_sid, &file_path, "watcher")
            .unwrap();

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
        let sid = store
            .create_snapshot(1, dir.path().to_str().unwrap())
            .unwrap();
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
        assert_eq!(
            std::fs::read_to_string(&parser_file).unwrap(),
            "parser original"
        );
        assert_eq!(
            std::fs::read_to_string(&watcher_file).unwrap(),
            "watcher modified"
        );
    }

    #[test]
    fn test_undo_command_valid_id() {
        let (store, dir) = setup();
        let engine = UndoEngine::new(&store);

        let file_path = dir.path().join("cmd_target.txt");
        std::fs::write(&file_path, b"original").unwrap();

        // Create a parser snapshot for command_id=42
        let sid = store
            .create_snapshot(42, dir.path().to_str().unwrap())
            .unwrap();
        store.store_file(sid, &file_path, "parser").unwrap();

        // Simulate command modifying file
        std::fs::write(&file_path, b"modified").unwrap();

        let result = engine
            .undo_command(42)
            .unwrap()
            .expect("should have result");
        assert_eq!(result.command_id, 42);
        assert_eq!(result.files.len(), 1);
        assert!(matches!(result.files[0].1, FileOutcome::Restored));

        // Verify file restored
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "original");
    }

    #[test]
    fn test_undo_command_nonexistent_id() {
        let (store, _dir) = setup();
        let engine = UndoEngine::new(&store);
        let result = engine.undo_command(999).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_undo_command_only_restores_parser_files() {
        let (store, dir) = setup();
        let engine = UndoEngine::new(&store);

        let parser_file = dir.path().join("p.txt");
        let watcher_file = dir.path().join("w.txt");
        std::fs::write(&parser_file, b"parser orig").unwrap();
        std::fs::write(&watcher_file, b"watcher orig").unwrap();

        let sid = store
            .create_snapshot(10, dir.path().to_str().unwrap())
            .unwrap();
        store.store_file(sid, &parser_file, "parser").unwrap();
        store.store_file(sid, &watcher_file, "watcher").unwrap();

        std::fs::write(&parser_file, b"parser mod").unwrap();
        std::fs::write(&watcher_file, b"watcher mod").unwrap();

        let result = engine
            .undo_command(10)
            .unwrap()
            .expect("should have result");
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].0, parser_file);
        assert_eq!(
            std::fs::read_to_string(&parser_file).unwrap(),
            "parser orig"
        );
        assert_eq!(
            std::fs::read_to_string(&watcher_file).unwrap(),
            "watcher mod"
        );
    }

    #[test]
    fn test_undo_command_deletes_snapshot() {
        let (store, dir) = setup();
        let engine = UndoEngine::new(&store);

        let file_path = dir.path().join("del_test.txt");
        std::fs::write(&file_path, b"content").unwrap();

        let sid = store
            .create_snapshot(77, dir.path().to_str().unwrap())
            .unwrap();
        store.store_file(sid, &file_path, "parser").unwrap();

        std::fs::write(&file_path, b"changed").unwrap();

        let result = engine
            .undo_command(77)
            .unwrap()
            .expect("should have result");
        assert_eq!(result.snapshot_id, sid);

        // Snapshot should be deleted after successful undo
        assert!(store.db().get_snapshot(sid).unwrap().is_none());
    }

    #[test]
    fn test_undo_latest_still_works_after_refactor() {
        // Re-verify undo_latest with the same pattern as test_undo_latest_restores_file
        // but also check snapshot deletion
        let (store, dir) = setup();
        let engine = UndoEngine::new(&store);

        let file_path = dir.path().join("latest.txt");
        std::fs::write(&file_path, b"orig").unwrap();

        let sid = store
            .create_snapshot(5, dir.path().to_str().unwrap())
            .unwrap();
        store.store_file(sid, &file_path, "parser").unwrap();

        std::fs::write(&file_path, b"mod").unwrap();

        let result = engine.undo_latest().unwrap().expect("should have result");
        assert_eq!(result.snapshot_id, sid);
        assert!(matches!(result.files[0].1, FileOutcome::Restored));
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "orig");

        // Snapshot should be deleted after successful undo
        assert!(store.db().get_snapshot(sid).unwrap().is_none());
    }

    #[test]
    fn test_undo_missing_parent_directory() {
        let (store, dir) = setup();
        let engine = UndoEngine::new(&store);

        // Snapshot a file under a subdirectory
        let sub_dir = dir.path().join("sub");
        std::fs::create_dir_all(&sub_dir).unwrap();
        let file_path = sub_dir.join("target.txt");
        std::fs::write(&file_path, b"original").unwrap();

        let sid = store
            .create_snapshot(1, dir.path().to_str().unwrap())
            .unwrap();
        store.store_file(sid, &file_path, "parser").unwrap();

        // Remove the entire subdirectory after snapshot
        std::fs::remove_dir_all(&sub_dir).unwrap();

        // Undo should report error (parent dir missing), not panic
        let result = engine.undo_latest().unwrap().expect("should have a result");
        assert_eq!(result.files.len(), 1);
        assert!(
            matches!(result.files[0].1, FileOutcome::Error(_)),
            "Expected Error outcome when parent dir missing, got {:?}",
            result.files[0].1
        );
    }

    #[test]
    fn test_undo_missing_blob() {
        let (store, dir) = setup();
        let engine = UndoEngine::new(&store);

        let file_path = dir.path().join("orphan.txt");
        std::fs::write(&file_path, b"content").unwrap();

        let sid = store
            .create_snapshot(1, dir.path().to_str().unwrap())
            .unwrap();
        store.store_file(sid, &file_path, "parser").unwrap();

        // Get the blob hash, then delete the blob file from disk
        let files = store.db().get_snapshot_files(sid).unwrap();
        let hash = files[0].blob_hash.as_ref().unwrap();
        store.blobs().delete_blob(hash).unwrap();

        // Modify the file so there's something to undo
        std::fs::write(&file_path, b"modified").unwrap();

        // Undo should report error (blob missing), not panic
        let result = engine.undo_latest().unwrap().expect("should have a result");
        assert_eq!(result.files.len(), 1);
        assert!(
            matches!(result.files[0].1, FileOutcome::Error(_)),
            "Expected Error outcome when blob missing, got {:?}",
            result.files[0].1
        );
    }

    #[test]
    fn test_path_outside_project_root_rejected() {
        let (store, dir) = setup();

        // Use a subdirectory as the project root
        let project_root = dir.path().join("project");
        std::fs::create_dir_all(&project_root).unwrap();
        let engine = UndoEngine::with_project_root(&store, project_root.clone());

        // Create a file OUTSIDE the project root
        let outside_file = dir.path().join("outside.txt");
        std::fs::write(&outside_file, b"secret").unwrap();

        // Snapshot the outside file
        let sid = store
            .create_snapshot(1, dir.path().to_str().unwrap())
            .unwrap();
        store.store_file(sid, &outside_file, "parser").unwrap();

        // Modify the file
        std::fs::write(&outside_file, b"modified").unwrap();

        // Undo should reject the path
        let result = engine.undo_latest().unwrap().expect("should have a result");
        assert_eq!(result.files.len(), 1);
        match &result.files[0].1 {
            FileOutcome::Error(msg) => {
                assert!(
                    msg.contains("outside project root"),
                    "Error should mention project root, got: {}",
                    msg
                );
            }
            other => panic!("Expected Error for path outside root, got {:?}", other),
        }
    }

    #[test]
    fn test_path_within_project_root_allowed() {
        let (store, dir) = setup();

        // Project root is the whole temp dir
        let project_root = dir.path().canonicalize().unwrap();
        let engine = UndoEngine::with_project_root(&store, project_root);

        let file_path = dir.path().join("inside.txt");
        std::fs::write(&file_path, b"original").unwrap();

        let sid = store
            .create_snapshot(1, dir.path().to_str().unwrap())
            .unwrap();
        store.store_file(sid, &file_path, "parser").unwrap();

        std::fs::write(&file_path, b"modified").unwrap();

        let result = engine.undo_latest().unwrap().expect("should have a result");
        assert_eq!(result.files.len(), 1);
        assert!(
            matches!(result.files[0].1, FileOutcome::Restored),
            "Expected Restored for path inside root, got {:?}",
            result.files[0].1
        );
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "original");
    }
}
