//! glass_history -- SQLite-backed command history with FTS5 search.
//!
//! Provides a database for storing, searching, and managing command execution
//! history. Supports project-local databases (`.glass/history.db`) with
//! global fallback (`~/.glass/global-history.db`).

pub mod config;
pub mod db;
pub mod retention;
pub mod search;

pub use config::HistoryConfig;
pub use db::{CommandRecord, HistoryDb};
pub use search::SearchResult;

use std::path::{Path, PathBuf};

/// Resolve the database path for command history.
///
/// Walks up from `cwd` looking for a `.glass/` directory.
/// If found, returns `.glass/history.db` within that directory.
/// Otherwise, falls back to `~/.glass/global-history.db`.
pub fn resolve_db_path(cwd: &Path) -> PathBuf {
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        let glass_dir = d.join(".glass");
        if glass_dir.is_dir() {
            return glass_dir.join("history.db");
        }
        dir = d.parent();
    }
    // Global fallback
    let home = dirs::home_dir().expect("Could not determine home directory");
    let global_dir = home.join(".glass");
    std::fs::create_dir_all(&global_dir).ok();
    global_dir.join("global-history.db")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_resolve_db_path_project() {
        let dir = TempDir::new().unwrap();
        let glass_dir = dir.path().join(".glass");
        std::fs::create_dir_all(&glass_dir).unwrap();

        let result = resolve_db_path(dir.path());
        assert_eq!(result, glass_dir.join("history.db"));
    }

    #[test]
    fn test_resolve_db_path_ancestor() {
        let dir = TempDir::new().unwrap();
        let glass_dir = dir.path().join(".glass");
        std::fs::create_dir_all(&glass_dir).unwrap();

        // Create nested subdirectory
        let nested = dir.path().join("sub").join("sub2");
        std::fs::create_dir_all(&nested).unwrap();

        let result = resolve_db_path(&nested);
        assert_eq!(result, glass_dir.join("history.db"));
    }

    #[test]
    fn test_resolve_db_path_global_fallback() {
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("no_glass_here").join("deep");
        std::fs::create_dir_all(&nested).unwrap();

        let result = resolve_db_path(&nested);

        // The result should end with history.db. If the system has .glass/ in
        // an ancestor of the temp directory, it will find that instead of the
        // global fallback. Both behaviors are correct.
        let filename = result.file_name().unwrap().to_str().unwrap();
        assert!(
            filename == "history.db" || filename == "global-history.db",
            "Expected history.db or global-history.db, got: {:?}",
            result
        );
        // The parent directory should be named .glass
        let parent_name = result.parent().unwrap().file_name().unwrap().to_str().unwrap();
        assert_eq!(parent_name, ".glass");
    }
}
