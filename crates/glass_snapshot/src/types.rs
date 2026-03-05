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
