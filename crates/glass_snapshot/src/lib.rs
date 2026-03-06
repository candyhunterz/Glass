//! glass_snapshot — Content-addressed blob storage and snapshot metadata.

pub mod blob_store;
pub mod command_parser;
pub mod db;
pub mod ignore_rules;
pub mod types;

pub use blob_store::BlobStore;
pub use db::SnapshotDb;
pub use ignore_rules::IgnoreRules;
pub use types::{Confidence, ParseResult, SnapshotFileRecord, SnapshotRecord};

use std::path::{Path, PathBuf};

use anyhow::Result;

/// High-level API combining blob storage and metadata.
pub struct SnapshotStore {
    db: SnapshotDb,
    blobs: BlobStore,
}

impl SnapshotStore {
    /// Open (or create) a SnapshotStore under the given `.glass/` directory.
    pub fn open(glass_dir: &Path) -> Result<Self> {
        let db = SnapshotDb::open(&glass_dir.join("snapshots.db"))?;
        let blobs = BlobStore::new(glass_dir);
        Ok(Self { db, blobs })
    }

    /// Create a new snapshot record. Returns the snapshot id.
    pub fn create_snapshot(&self, command_id: i64, cwd: &str) -> Result<i64> {
        self.db.create_snapshot(command_id, cwd)
    }

    /// Store a file into the snapshot. Handles non-existent files (NULL hash)
    /// and skips symlinks.
    pub fn store_file(
        &self,
        snapshot_id: i64,
        path: &Path,
        source: &str,
    ) -> Result<()> {
        if !path.exists() {
            // File does not exist -- record NULL hash (file was absent before command)
            self.db
                .insert_snapshot_file(snapshot_id, path, None, None, source)?;
            return Ok(());
        }
        let metadata = std::fs::symlink_metadata(path)?;
        if metadata.is_symlink() {
            tracing::debug!("Skipping symlink: {}", path.display());
            return Ok(());
        }
        let (hash, size) = self.blobs.store_file(path)?;
        self.db
            .insert_snapshot_file(snapshot_id, path, Some(&hash), Some(size), source)?;
        Ok(())
    }

    /// Update the command_id on an existing snapshot.
    pub fn update_command_id(&self, snapshot_id: i64, command_id: i64) -> Result<()> {
        self.db.update_command_id(snapshot_id, command_id)
    }

    /// Get a reference to the underlying SnapshotDb.
    pub fn db(&self) -> &SnapshotDb {
        &self.db
    }

    /// Get a reference to the underlying BlobStore.
    pub fn blobs(&self) -> &BlobStore {
        &self.blobs
    }
}

/// Resolve the `.glass/` directory by walking up from `cwd`.
/// Falls back to `~/.glass/` if no ancestor has a `.glass/` directory.
pub fn resolve_glass_dir(cwd: &Path) -> PathBuf {
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        let glass_dir = d.join(".glass");
        if glass_dir.is_dir() {
            return glass_dir;
        }
        dir = d.parent();
    }
    let home = dirs::home_dir().expect("Could not determine home directory");
    let global_dir = home.join(".glass");
    std::fs::create_dir_all(&global_dir).ok();
    global_dir
}

/// Resolve the path to `snapshots.db` by walking up from `cwd`.
/// Falls back to `~/.glass/snapshots.db` if no ancestor has a `.glass/` directory.
pub fn resolve_snapshot_db_path(cwd: &Path) -> PathBuf {
    resolve_glass_dir(cwd).join("snapshots.db")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_snapshot_store_integration() {
        let dir = TempDir::new().unwrap();
        let glass_dir = dir.path().join(".glass");
        std::fs::create_dir_all(&glass_dir).unwrap();

        let store = SnapshotStore::open(&glass_dir).unwrap();

        // Create a snapshot
        let sid = store.create_snapshot(99, "/home/user/project").unwrap();
        assert!(sid > 0);

        // Write a test file and store it
        let file_path = dir.path().join("test_file.txt");
        std::fs::write(&file_path, b"snapshot content").unwrap();
        store.store_file(sid, &file_path, "parser").unwrap();

        // Verify blob on disk
        let files = store.db().get_snapshot_files(sid).unwrap();
        assert_eq!(files.len(), 1);
        let hash = files[0].blob_hash.as_ref().expect("hash should exist");
        assert!(store.blobs().blob_exists(hash));

        // Verify metadata in DB
        let snapshot = store.db().get_snapshot(sid).unwrap().unwrap();
        assert_eq!(snapshot.command_id, 99);
        assert_eq!(snapshot.cwd, "/home/user/project");

        // Verify blob content matches
        let blob_content = store.blobs().read_blob(hash).unwrap();
        assert_eq!(blob_content, b"snapshot content");
    }
}
