use std::path::{Path, PathBuf};

use anyhow::Result;

/// Content-addressed file storage using BLAKE3 hashing with deduplication.
///
/// Blobs are stored under `{blob_dir}/{hash[0:2]}/{hash}.blob` using
/// 2-character hex prefix directory sharding.
pub struct BlobStore {
    blob_dir: PathBuf,
}

impl BlobStore {
    /// Create a new BlobStore rooted at `glass_dir/blobs/`.
    pub fn new(glass_dir: &Path) -> Self {
        let blob_dir = glass_dir.join("blobs");
        Self { blob_dir }
    }

    /// Store file contents, returning the BLAKE3 hex hash and file size.
    /// Deduplicates: if blob already exists, skips the write.
    pub fn store_file(&self, source_path: &Path) -> Result<(String, u64)> {
        let content = std::fs::read(source_path)?;
        let file_size = content.len() as u64;
        let hash = blake3::hash(&content);
        let hex = hash.to_hex().to_string();

        let shard_dir = self.blob_dir.join(&hex[..2]);
        let blob_path = shard_dir.join(format!("{}.blob", &hex));

        if !blob_path.exists() {
            std::fs::create_dir_all(&shard_dir)?;
            std::fs::write(&blob_path, &content)?;
        }

        Ok((hex, file_size))
    }

    /// Read blob contents by hash.
    pub fn read_blob(&self, hash: &str) -> Result<Vec<u8>> {
        let blob_path = self
            .blob_dir
            .join(&hash[..2])
            .join(format!("{}.blob", hash));
        Ok(std::fs::read(&blob_path)?)
    }

    /// Check if a blob exists.
    pub fn blob_exists(&self, hash: &str) -> bool {
        let blob_path = self
            .blob_dir
            .join(&hash[..2])
            .join(format!("{}.blob", hash));
        blob_path.exists()
    }

    /// List all blob hashes stored on disk by walking the blobs/ directory.
    pub fn list_blob_hashes(&self) -> Result<Vec<String>> {
        let mut hashes = Vec::new();
        if !self.blob_dir.exists() {
            return Ok(hashes);
        }
        for shard_entry in std::fs::read_dir(&self.blob_dir)? {
            let shard_entry = shard_entry?;
            let shard_path = shard_entry.path();
            if !shard_path.is_dir() {
                continue;
            }
            for blob_entry in std::fs::read_dir(&shard_path)? {
                let blob_entry = blob_entry?;
                let file_name = blob_entry.file_name();
                let name = file_name.to_string_lossy();
                if let Some(hash) = name.strip_suffix(".blob") {
                    hashes.push(hash.to_string());
                }
            }
        }
        Ok(hashes)
    }

    /// Delete a blob by hash. Returns true if it existed.
    pub fn delete_blob(&self, hash: &str) -> Result<bool> {
        let blob_path = self
            .blob_dir
            .join(&hash[..2])
            .join(format!("{}.blob", hash));
        if blob_path.exists() {
            std::fs::remove_file(&blob_path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> (BlobStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::new(dir.path());
        (store, dir)
    }

    #[test]
    fn test_hash_correctness() {
        // blake3::hash of "hello world" should produce a known hex string
        let hash = blake3::hash(b"hello world");
        let hex = hash.to_hex().to_string();
        assert_eq!(
            hex,
            "d74981efa70a0c880b8d8c1985d075dbcbf679b99a5f9914e5aaf96b831a9e24"
        );
    }

    #[test]
    fn test_store_and_read() {
        let (store, dir) = setup();
        let content = b"hello world";
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, content).unwrap();

        let (hash, size) = store.store_file(&file_path).unwrap();
        assert_eq!(size, 11);

        let read_back = store.read_blob(&hash).unwrap();
        assert_eq!(read_back, content);
    }

    #[test]
    fn test_dedup() {
        let (store, dir) = setup();
        let content = b"duplicate content";
        let file1 = dir.path().join("file1.txt");
        let file2 = dir.path().join("file2.txt");
        std::fs::write(&file1, content).unwrap();
        std::fs::write(&file2, content).unwrap();

        let (hash1, _) = store.store_file(&file1).unwrap();
        let (hash2, _) = store.store_file(&file2).unwrap();
        assert_eq!(hash1, hash2);

        // Count .blob files on disk -- should be exactly 1
        let blob_count = count_blob_files(dir.path());
        assert_eq!(blob_count, 1);
    }

    #[test]
    fn test_blob_exists() {
        let (store, dir) = setup();
        let file_path = dir.path().join("exists.txt");
        std::fs::write(&file_path, b"exists").unwrap();

        let (hash, _) = store.store_file(&file_path).unwrap();
        assert!(store.blob_exists(&hash));
        assert!(
            !store.blob_exists("0000000000000000000000000000000000000000000000000000000000000000")
        );
    }

    #[test]
    fn test_delete_blob() {
        let (store, dir) = setup();
        let file_path = dir.path().join("delete_me.txt");
        std::fs::write(&file_path, b"delete me").unwrap();

        let (hash, _) = store.store_file(&file_path).unwrap();
        assert!(store.blob_exists(&hash));

        assert!(store.delete_blob(&hash).unwrap());
        assert!(!store.blob_exists(&hash));
        assert!(store.read_blob(&hash).is_err());
    }

    #[test]
    fn test_shard_directory() {
        let (store, dir) = setup();
        let file_path = dir.path().join("shard_test.txt");
        std::fs::write(&file_path, b"shard test content").unwrap();

        let (hash, _) = store.store_file(&file_path).unwrap();
        let expected_path = dir
            .path()
            .join("blobs")
            .join(&hash[..2])
            .join(format!("{}.blob", &hash));
        assert!(expected_path.exists());
    }

    /// Helper to count .blob files recursively under a directory.
    fn count_blob_files(dir: &Path) -> usize {
        let mut count = 0;
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    count += count_blob_files(&path);
                } else if path.extension().and_then(|e| e.to_str()) == Some("blob") {
                    count += 1;
                }
            }
        }
        count
    }
}
