//! glass_coordination -- Multi-agent coordination via shared SQLite database.
//!
//! Provides agent registration, file locking, and inter-agent messaging
//! for coordinating multiple AI coding agents working on the same project.
//! Uses a global SQLite database at `~/.glass/agents.db`.

pub mod db;
pub mod event_log;
pub mod pid;
pub mod types;

pub use db::CoordinationDb;
pub use event_log::CoordinationEvent;
pub use pid::is_pid_alive;
pub use types::{AgentInfo, FileLock, LockConflict, LockResult, Message};

use std::path::{Path, PathBuf};

/// Resolve the path to the global coordination database.
///
/// Returns `~/.glass/agents.db`, creating the `.glass` directory if needed.
pub fn resolve_db_path() -> PathBuf {
    let home = dirs::home_dir().expect("Could not determine home directory");
    let glass_dir = home.join(".glass");
    std::fs::create_dir_all(&glass_dir).ok();
    glass_dir.join("agents.db")
}

/// Canonicalize a filesystem path for consistent cross-platform comparison.
///
/// Uses `dunce::canonicalize` to avoid UNC path prefixes on Windows.
/// On Windows, the result is lowercased for case-insensitive matching.
pub fn canonicalize_path(path: &Path) -> anyhow::Result<String> {
    let canonical = dunce::canonicalize(path)?;
    let path_str = canonical.to_string_lossy().to_string();

    #[cfg(target_os = "windows")]
    {
        Ok(path_str.to_lowercase())
    }

    #[cfg(not(target_os = "windows"))]
    {
        Ok(path_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_db_path() {
        let path = resolve_db_path();
        assert!(path.ends_with("agents.db"));
        let parent_name = path
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(parent_name, ".glass");
    }

    #[test]
    fn test_canonicalize_path_current_dir() {
        let cwd = std::env::current_dir().unwrap();
        let result = canonicalize_path(&cwd).unwrap();
        // Should be non-empty and absolute
        assert!(!result.is_empty());
        #[cfg(target_os = "windows")]
        {
            // On Windows, result should be lowercased
            assert_eq!(result, result.to_lowercase());
        }
    }
}
