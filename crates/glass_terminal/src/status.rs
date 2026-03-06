//! Status state for shell integration — CWD and git info tracking.

use std::path::Path;
use std::process::Command;

/// Git repository information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitInfo {
    /// Current branch name
    pub branch: String,
    /// Number of dirty (modified/untracked) files
    pub dirty_count: usize,
}

/// Tracks current working directory and associated git state.
#[derive(Clone)]
pub struct StatusState {
    cwd: String,
    git_info: Option<GitInfo>,
    /// Whether a git query is currently in-flight
    pub git_query_pending: bool,
}

impl StatusState {
    pub fn new() -> Self {
        Self {
            cwd: String::new(),
            git_info: None,
            git_query_pending: false,
        }
    }

    /// Update the current working directory.
    pub fn set_cwd(&mut self, cwd: String) {
        self.cwd = cwd;
    }

    /// Get the current working directory.
    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    /// Update git info for the current directory.
    pub fn set_git_info(&mut self, info: Option<GitInfo>) {
        self.git_info = info;
    }

    /// Get current git info.
    pub fn git_info(&self) -> Option<&GitInfo> {
        self.git_info.as_ref()
    }

    /// Clear git info (e.g., when CWD leaves a git repo).
    pub fn clear_git_info(&mut self) {
        self.git_info = None;
    }
}

impl Default for StatusState {
    fn default() -> Self {
        Self::new()
    }
}

/// Query git status synchronously for a given directory.
///
/// Uses GIT_OPTIONAL_LOCKS=0 to avoid contention with other git processes.
/// Returns None if the directory is not inside a git repository.
pub fn query_git_status(cwd: &str) -> Option<GitInfo> {
    let path = Path::new(cwd);
    if !path.exists() {
        return None;
    }

    // Get current branch
    let branch_output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(cwd)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .ok()?;

    if !branch_output.status.success() {
        return None; // Not a git repo
    }

    let branch = String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .to_string();

    // Get dirty file count (modified + untracked)
    let status_output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(cwd)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .ok()?;

    let dirty_count = if status_output.status.success() {
        String::from_utf8_lossy(&status_output.stdout)
            .lines()
            .filter(|line| !line.is_empty())
            .count()
    } else {
        0
    };

    Some(GitInfo {
        branch,
        dirty_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_cwd_updates_directory() {
        let mut s = StatusState::new();
        s.set_cwd("C:\\Users\\test".to_string());
        assert_eq!(s.cwd(), "C:\\Users\\test");
    }

    #[test]
    fn set_git_info_stores_branch_and_dirty() {
        let mut s = StatusState::new();
        s.set_git_info(Some(GitInfo {
            branch: "main".to_string(),
            dirty_count: 3,
        }));
        let info = s.git_info().unwrap();
        assert_eq!(info.branch, "main");
        assert_eq!(info.dirty_count, 3);
    }

    #[test]
    fn clear_git_info_removes_it() {
        let mut s = StatusState::new();
        s.set_git_info(Some(GitInfo {
            branch: "main".to_string(),
            dirty_count: 0,
        }));
        s.clear_git_info();
        assert!(s.git_info().is_none());
    }

    #[test]
    fn query_git_status_non_git_dir() {
        // A directory that is definitely not a git repo
        let result = query_git_status("C:\\Windows\\System32");
        assert!(result.is_none());
    }

    #[test]
    fn query_git_status_this_repo() {
        // Integration test: use the Glass repo itself
        let result = query_git_status(env!("CARGO_MANIFEST_DIR"));
        // This test is running inside a git repo, so we should get info
        assert!(result.is_some());
        let info = result.unwrap();
        assert!(!info.branch.is_empty());
    }
}
