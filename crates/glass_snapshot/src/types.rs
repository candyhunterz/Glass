use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_outcome_variants_exist() {
        let _restored = FileOutcome::Restored;
        let _deleted = FileOutcome::Deleted;
        let _skipped = FileOutcome::Skipped;
        let _conflict = FileOutcome::Conflict {
            current_hash: "abc".to_string(),
            expected_hash: Some("def".to_string()),
        };
        let _error = FileOutcome::Error("something went wrong".to_string());
    }

    #[test]
    fn undo_result_construction() {
        let result = UndoResult {
            snapshot_id: 1,
            command_id: 42,
            confidence: Confidence::High,
            files: vec![
                (PathBuf::from("/tmp/a.txt"), FileOutcome::Restored),
                (PathBuf::from("/tmp/b.txt"), FileOutcome::Deleted),
            ],
        };
        assert_eq!(result.snapshot_id, 1);
        assert_eq!(result.command_id, 42);
        assert_eq!(result.files.len(), 2);
    }
}

/// How confident the parser is in its identification of file targets.
#[derive(Debug, Clone, PartialEq)]
pub enum Confidence {
    /// Known destructive command with clear file targets identified.
    High,
    /// Unknown command or ambiguous targets -- rely on FS watcher.
    Low,
    /// Command is read-only -- no snapshot needed.
    ReadOnly,
}

/// Result of parsing a shell command for file modification targets.
#[derive(Debug, Clone)]
pub struct ParseResult {
    /// Absolute paths of files the command may modify.
    pub targets: Vec<PathBuf>,
    /// How confident the parser is in its target identification.
    pub confidence: Confidence,
}

/// A snapshot metadata record from the database.
#[derive(Debug, Clone)]
pub struct SnapshotRecord {
    /// Database row id.
    pub id: i64,
    /// The command history id this snapshot is associated with.
    pub command_id: i64,
    /// Working directory where the command was run.
    pub cwd: String,
    /// When the snapshot was created (Unix epoch seconds).
    pub created_at: i64,
}

/// A file entry within a snapshot.
#[derive(Debug, Clone)]
pub struct SnapshotFileRecord {
    /// Database row id.
    pub id: i64,
    /// The snapshot this file belongs to.
    pub snapshot_id: i64,
    /// Absolute path of the file.
    pub file_path: String,
    /// BLAKE3 hex hash of the file contents, or None if the file did not exist.
    pub blob_hash: Option<String>,
    /// File size in bytes, or None if the file did not exist.
    pub file_size: Option<u64>,
    /// How this file entry was recorded (e.g., "parser", "watcher").
    pub source: String,
}

// ---------------------------------------------------------------------------
// Undo result types
// ---------------------------------------------------------------------------

/// Outcome for a single file during an undo operation.
#[derive(Debug, Clone)]
pub enum FileOutcome {
    /// File was successfully restored to its pre-command state.
    Restored,
    /// File was deleted (it did not exist before the command).
    Deleted,
    /// File was skipped (no action needed).
    Skipped,
    /// Conflict detected: the file has been modified since the snapshot.
    Conflict {
        current_hash: String,
        expected_hash: Option<String>,
    },
    /// An error occurred while processing this file.
    Error(String),
}

/// Result of an undo operation on a snapshot.
#[derive(Debug, Clone)]
pub struct UndoResult {
    /// The snapshot that was undone.
    pub snapshot_id: i64,
    /// The command that was undone.
    pub command_id: i64,
    /// How confident the parser was in the original snapshot.
    pub confidence: Confidence,
    /// Per-file outcomes.
    pub files: Vec<(PathBuf, FileOutcome)>,
}

// ---------------------------------------------------------------------------
// Watcher event types
// ---------------------------------------------------------------------------

/// A filesystem event detected by the watcher.
#[derive(Debug, Clone)]
pub struct WatcherEvent {
    /// Path of the affected file.
    pub path: PathBuf,
    /// What kind of change occurred.
    pub kind: WatcherEventKind,
}

/// The kind of filesystem change.
#[derive(Debug, Clone, PartialEq)]
pub enum WatcherEventKind {
    /// File was created.
    Create,
    /// File content was modified.
    Modify,
    /// File was deleted.
    Delete,
    /// File was renamed (contains the new path).
    Rename { to: PathBuf },
}

impl WatcherEvent {
    /// Convert a `notify::Event` into a `WatcherEvent` for a specific path.
    ///
    /// Returns `None` for events that do not represent content modifications
    /// (e.g., access, metadata-only changes).
    pub fn from_notify(event: &notify::Event, path: &Path) -> Option<Self> {
        use notify::event::{CreateKind, ModifyKind, RemoveKind, RenameMode};
        use notify::EventKind;

        let kind = match &event.kind {
            EventKind::Create(CreateKind::File) | EventKind::Create(CreateKind::Any) => {
                WatcherEventKind::Create
            }

            EventKind::Modify(ModifyKind::Data(_)) | EventKind::Modify(ModifyKind::Any) => {
                WatcherEventKind::Modify
            }

            EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
                // For rename events, notify puts [from, to] in event.paths
                let to = event
                    .paths
                    .get(1)
                    .cloned()
                    .unwrap_or_else(|| path.to_path_buf());
                WatcherEventKind::Rename { to }
            }

            EventKind::Remove(RemoveKind::File) | EventKind::Remove(RemoveKind::Any) => {
                WatcherEventKind::Delete
            }

            // Ignore Access, Metadata-only, Other
            _ => return None,
        };

        Some(Self {
            path: path.to_path_buf(),
            kind,
        })
    }
}
