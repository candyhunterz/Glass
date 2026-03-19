//! Storage pruning -- deletes old snapshots by age, count, and cleans up orphan blobs.

use anyhow::Result;

use crate::SnapshotStore;

/// Result of a prune operation.
#[derive(Debug, Clone, Default)]
pub struct PruneResult {
    /// Number of snapshots deleted.
    pub snapshots_deleted: u32,
    /// Number of orphan blob files deleted.
    pub blobs_deleted: u32,
}

/// Prunes old snapshots and orphan blobs based on retention policy.
pub struct Pruner<'a> {
    store: &'a SnapshotStore,
    retention_days: u32,
    max_count: u32,
    #[allow(dead_code)]
    max_size_mb: u32,
}

impl<'a> Pruner<'a> {
    /// Create a new Pruner with the given retention policy parameters.
    pub fn new(
        store: &'a SnapshotStore,
        retention_days: u32,
        max_count: u32,
        max_size_mb: u32,
    ) -> Self {
        Self {
            store,
            retention_days,
            max_count,
            max_size_mb,
        }
    }

    /// Run the pruning process: age-based, count-based, then orphan blob cleanup.
    pub fn prune(&self) -> Result<PruneResult> {
        let mut result = PruneResult::default();

        // Step 1: Delete by age (skip 10 most recent as safety margin)
        let total_count = self.store.db().count_snapshots()?;
        if total_count > 10 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or(std::time::Duration::ZERO)
                .as_secs() as i64;
            let age_epoch = now - (self.retention_days as i64) * 86400;

            // Get the 10th newest snapshot's created_at as safety floor
            // We want to protect the 10 newest, so get oldest among top 10
            let safe_epoch = self.store.db().get_nth_newest_created_at(10)?;
            let effective_epoch = match safe_epoch {
                Some(ts) => std::cmp::min(age_epoch, ts),
                None => age_epoch,
            };

            let deleted_ids = self.store.db().delete_snapshots_before(effective_epoch)?;
            result.snapshots_deleted += deleted_ids.len() as u32;
        }

        // Step 2: Delete by count
        let current_count = self.store.db().count_snapshots()?;
        if current_count > self.max_count as u64 {
            let excess = (current_count - self.max_count as u64) as u32;
            let oldest_ids = self.store.db().get_oldest_snapshot_ids(excess)?;
            for id in &oldest_ids {
                self.store.db().delete_snapshot(*id)?;
                result.snapshots_deleted += 1;
            }
        }

        // Step 3: Orphan blob cleanup
        let referenced = self.store.db().get_referenced_hashes()?;
        let all_blobs = self.store.blobs().list_blob_hashes()?;
        for hash in &all_blobs {
            if !referenced.contains(hash) && self.store.blobs().delete_blob(hash)? {
                result.blobs_deleted += 1;
            }
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (SnapshotStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let glass_dir = dir.path().join(".glass");
        std::fs::create_dir_all(&glass_dir).unwrap();
        let store = SnapshotStore::open(&glass_dir).unwrap();
        (store, dir)
    }

    /// Helper: create a snapshot with a specific created_at timestamp.
    fn create_snapshot_at(
        store: &SnapshotStore,
        command_id: i64,
        cwd: &str,
        created_at: i64,
    ) -> i64 {
        let sid = store.db().create_snapshot(command_id, cwd).unwrap();
        store.db().set_created_at(sid, created_at).unwrap();
        sid
    }

    #[test]
    fn test_delete_snapshots_before() {
        let (store, _dir) = setup();
        let now = 1_000_000;
        let s1 = create_snapshot_at(&store, 1, "/tmp", now - 200);
        let s2 = create_snapshot_at(&store, 2, "/tmp", now - 100);
        let _s3 = create_snapshot_at(&store, 3, "/tmp", now);

        let deleted = store.db().delete_snapshots_before(now - 50).unwrap();
        assert_eq!(deleted.len(), 2);
        assert!(deleted.contains(&s1));
        assert!(deleted.contains(&s2));

        // s3 should still exist
        assert_eq!(store.db().count_snapshots().unwrap(), 1);
    }

    #[test]
    fn test_count_snapshots() {
        let (store, _dir) = setup();
        assert_eq!(store.db().count_snapshots().unwrap(), 0);
        store.create_snapshot(1, "/tmp").unwrap();
        store.create_snapshot(2, "/tmp").unwrap();
        assert_eq!(store.db().count_snapshots().unwrap(), 2);
    }

    #[test]
    fn test_get_oldest_snapshot_ids() {
        let (store, _dir) = setup();
        let now = 1_000_000;
        let s1 = create_snapshot_at(&store, 1, "/tmp", now - 300);
        let s2 = create_snapshot_at(&store, 2, "/tmp", now - 200);
        let _s3 = create_snapshot_at(&store, 3, "/tmp", now - 100);

        let oldest = store.db().get_oldest_snapshot_ids(2).unwrap();
        assert_eq!(oldest.len(), 2);
        assert_eq!(oldest[0], s1);
        assert_eq!(oldest[1], s2);
    }

    #[test]
    fn test_get_referenced_hashes() {
        let (store, dir) = setup();
        let sid = store.create_snapshot(1, "/tmp").unwrap();

        // Insert files with known hashes
        let file_path = dir.path().join("a.txt");
        std::fs::write(&file_path, b"aaa").unwrap();
        store.store_file(sid, &file_path, "parser").unwrap();

        let file_path2 = dir.path().join("b.txt");
        std::fs::write(&file_path2, b"bbb").unwrap();
        store.store_file(sid, &file_path2, "parser").unwrap();

        let hashes = store.db().get_referenced_hashes().unwrap();
        assert_eq!(hashes.len(), 2);
    }

    #[test]
    fn test_prune_retention_days_zero_deletes_old() {
        let (store, _dir) = setup();
        // Create 15 snapshots (more than safety margin of 10)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        for i in 0..15 {
            // All created "now" minus various offsets
            create_snapshot_at(&store, i, "/tmp", now - (15 - i) * 100);
        }
        assert_eq!(store.db().count_snapshots().unwrap(), 15);

        // retention_days=0 means epoch = now, so all are "old"
        // But safety margin protects 10 newest
        let pruner = Pruner::new(&store, 0, 1000, 500);
        let result = pruner.prune().unwrap();

        // 5 oldest should be deleted, 10 newest kept (safety margin)
        assert_eq!(result.snapshots_deleted, 5);
        assert_eq!(store.db().count_snapshots().unwrap(), 10);
    }

    #[test]
    fn test_prune_max_count_keeps_only_n() {
        let (store, _dir) = setup();
        let now = 1_000_000;
        for i in 0..5 {
            create_snapshot_at(&store, i, "/tmp", now + i * 100);
        }
        assert_eq!(store.db().count_snapshots().unwrap(), 5);

        // max_count=2, retention_days high enough to not trigger age pruning
        let pruner = Pruner::new(&store, 365, 2, 500);
        let result = pruner.prune().unwrap();

        // 3 should be deleted by count enforcement
        assert_eq!(store.db().count_snapshots().unwrap(), 2);
        assert!(result.snapshots_deleted >= 3);
    }

    #[test]
    fn test_prune_cleans_orphan_blobs() {
        let (store, dir) = setup();

        // Create a snapshot with a file (creates a blob)
        let sid = store.create_snapshot(1, "/tmp").unwrap();
        let file_path = dir.path().join("keep.txt");
        std::fs::write(&file_path, b"keep this").unwrap();
        store.store_file(sid, &file_path, "parser").unwrap();

        // Create an orphan blob manually
        let orphan_hash = "deadbeef00000000000000000000000000000000000000000000000000000000";
        let shard_dir = dir
            .path()
            .join(".glass")
            .join("blobs")
            .join(&orphan_hash[..2]);
        std::fs::create_dir_all(&shard_dir).unwrap();
        std::fs::write(shard_dir.join(format!("{}.blob", orphan_hash)), b"orphan").unwrap();

        // Verify orphan blob exists
        assert!(store.blobs().blob_exists(orphan_hash));

        let pruner = Pruner::new(&store, 365, 1000, 500);
        let result = pruner.prune().unwrap();

        // Orphan should be deleted, referenced blob should remain
        assert_eq!(result.blobs_deleted, 1);
        assert!(!store.blobs().blob_exists(orphan_hash));

        // Referenced blob should still exist
        let files = store.db().get_snapshot_files(sid).unwrap();
        let kept_hash = files[0].blob_hash.as_ref().unwrap();
        assert!(store.blobs().blob_exists(kept_hash));
    }

    #[test]
    fn test_prune_skips_10_most_recent() {
        let (store, _dir) = setup();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Create exactly 10 snapshots, all very old
        for i in 0..10 {
            create_snapshot_at(&store, i, "/tmp", now - 86400 * 365 + i * 10);
        }
        assert_eq!(store.db().count_snapshots().unwrap(), 10);

        // retention_days=0 means all are "old", but safety margin should protect all 10
        let pruner = Pruner::new(&store, 0, 1000, 500);
        let result = pruner.prune().unwrap();

        // None should be deleted (count <= 10, safety margin)
        assert_eq!(result.snapshots_deleted, 0);
        assert_eq!(store.db().count_snapshots().unwrap(), 10);
    }
}
