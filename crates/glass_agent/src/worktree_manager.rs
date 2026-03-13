//! WorktreeManager: create, diff, apply, dismiss, and prune agent worktrees.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::types::{WorktreeHandle, WorktreeKind};
use crate::worktree_db::WorktreeDb;

/// Manages the lifecycle of agent worktrees.
///
/// Each worktree isolates proposed file changes from the user's working tree
/// until they explicitly approve (apply) or reject (dismiss) the proposal.
pub struct WorktreeManager {
    /// Base directory for all worktrees (`~/.glass/worktrees/` by default).
    pub base_dir: PathBuf,
    /// Database for crash-recovery tracking (wrapped for interior mutability).
    db: RefCell<WorktreeDb>,
}

impl WorktreeManager {
    /// Create a new `WorktreeManager` with an explicit base directory and DB.
    pub fn new(base_dir: PathBuf, db: WorktreeDb) -> Self {
        Self {
            base_dir,
            db: RefCell::new(db),
        }
    }

    /// Create a new `WorktreeManager` using `~/.glass/worktrees/` and the default DB.
    pub fn new_default() -> Result<Self> {
        let base_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not resolve home directory"))?
            .join(".glass")
            .join("worktrees");
        let db = WorktreeDb::open_default()?;
        Ok(Self {
            base_dir,
            db: RefCell::new(db),
        })
    }

    /// Create a worktree for the given project, writing `file_changes` into it.
    ///
    /// `file_changes` is a slice of `(relative_path, new_content)` pairs.
    ///
    /// The "register-before-create" invariant is enforced: the SQLite row is
    /// inserted BEFORE the git worktree or directory is created on disk.
    pub fn create_worktree(
        &self,
        project_root: &Path,
        proposal_id: &str,
        file_changes: &[(String, String)],
    ) -> Result<WorktreeHandle> {
        let id = uuid::Uuid::new_v4().to_string();
        let worktree_path = self.base_dir.join(&id);

        // STEP 1: Register BEFORE creating (crash-safety invariant).
        self.db
            .borrow_mut()
            .insert_pending_worktree(&id, &worktree_path, project_root, proposal_id)?;

        // STEP 2: Create the worktree.
        let result = self.create_worktree_inner(project_root, &worktree_path, &id, file_changes);

        match result {
            Ok(kind) => {
                let changed_files = file_changes
                    .iter()
                    .map(|(rel, _)| PathBuf::from(rel))
                    .collect();
                Ok(WorktreeHandle {
                    id,
                    worktree_path,
                    project_root: project_root.to_path_buf(),
                    kind,
                    changed_files,
                })
            }
            Err(e) => {
                // Creation failed: remove the pending row (not an orphan).
                let _ = self.db.borrow_mut().delete_pending_worktree(&id);
                Err(e)
            }
        }
    }

    fn create_worktree_inner(
        &self,
        project_root: &Path,
        worktree_path: &Path,
        id: &str,
        file_changes: &[(String, String)],
    ) -> Result<WorktreeKind> {
        // Ensure the base directory exists before attempting git worktree add.
        if let Some(parent) = worktree_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Detect git vs non-git project.
        let kind = match git2::Repository::discover(project_root) {
            Ok(repo) => {
                // Git path: create a linked worktree.
                tracing::debug!("WorktreeManager: creating git worktree at {:?}", worktree_path);
                repo.worktree(id, worktree_path, None)?;
                WorktreeKind::Git {
                    repo_path: project_root.to_path_buf(),
                }
            }
            Err(_) => {
                // Non-git fallback: create a plain directory.
                tracing::info!(
                    "WorktreeManager: non-git project, using plain directory fallback at {:?}",
                    worktree_path
                );
                std::fs::create_dir_all(worktree_path)?;
                WorktreeKind::TempDir
            }
        };

        // Write proposed file changes into the worktree.
        for (rel_path, content) in file_changes {
            let dest = worktree_path.join(rel_path);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&dest, content)?;
        }

        Ok(kind)
    }

    /// Generate a unified diff comparing the worktree files against working tree originals.
    ///
    /// Non-UTF-8 (binary) files emit a `"Binary file {path} changed\n"` placeholder.
    pub fn generate_diff(&self, handle: &WorktreeHandle) -> Result<String> {
        let mut full_diff = String::new();

        for rel_path in &handle.changed_files {
            let working_path = handle.project_root.join(rel_path);
            let wt_path = handle.worktree_path.join(rel_path);

            // Read original (empty string = new file).
            let original = match std::fs::read_to_string(&working_path) {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(_) => {
                    // Binary or unreadable file.
                    full_diff.push_str(&format!("Binary file {} changed\n", rel_path.display()));
                    continue;
                }
            };

            // Read modified.
            let modified = match std::fs::read_to_string(&wt_path) {
                Ok(s) => s,
                Err(_) => {
                    full_diff.push_str(&format!("Binary file {} changed\n", rel_path.display()));
                    continue;
                }
            };

            let patch = diffy::create_patch(&original, &modified);
            full_diff.push_str(&format!(
                "--- a/{}\n+++ b/{}\n",
                rel_path.display(),
                rel_path.display()
            ));
            full_diff.push_str(&patch.to_string());
            full_diff.push('\n');
        }

        Ok(full_diff)
    }

    /// Apply the worktree: copy changed files to the working tree, then clean up.
    pub fn apply(&self, handle: WorktreeHandle) -> Result<()> {
        for rel_path in &handle.changed_files {
            let src = handle.worktree_path.join(rel_path);
            let dst = handle.project_root.join(rel_path);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&src, &dst)?;
        }
        self.cleanup(handle)
    }

    /// Dismiss the worktree: remove it without touching the working tree.
    pub fn dismiss(&self, handle: WorktreeHandle) -> Result<()> {
        self.cleanup(handle)
    }

    /// Remove the worktree directory and (for git worktrees) prune the git reference.
    ///
    /// Also deletes the `pending_worktrees` row from SQLite.
    fn cleanup(&self, handle: WorktreeHandle) -> Result<()> {
        match &handle.kind {
            WorktreeKind::Git { repo_path } => {
                if let Ok(repo) = git2::Repository::open(repo_path) {
                    if let Ok(wt) = repo.find_worktree(&handle.id) {
                        let mut opts = git2::WorktreePruneOptions::new();
                        opts.valid(true); // force-prune even when path still exists
                        let _ = wt.prune(Some(&mut opts));
                    }
                }
                // Belt-and-suspenders: remove the directory if prune left it.
                if handle.worktree_path.exists() {
                    std::fs::remove_dir_all(&handle.worktree_path)?;
                }
            }
            WorktreeKind::TempDir => {
                if handle.worktree_path.exists() {
                    std::fs::remove_dir_all(&handle.worktree_path)?;
                }
            }
        }
        self.db.borrow_mut().delete_pending_worktree(&handle.id)?;
        Ok(())
    }

    /// Prune all orphaned worktrees recorded in the database.
    ///
    /// Called once on startup. Any surviving `pending_worktrees` rows indicate
    /// a crash occurred before `apply` or `dismiss` completed.
    pub fn prune_orphans(&self) -> Result<()> {
        let pending = self.db.borrow().list_pending_worktrees()?;
        for row in pending {
            tracing::warn!("WorktreeManager: pruning orphan worktree {}", row.id);
            if row.worktree_path.exists() {
                // Try git prune first; fall through to rm on error.
                if let Ok(repo) = git2::Repository::discover(&row.project_root) {
                    if let Ok(wt) = repo.find_worktree(&row.id) {
                        let mut opts = git2::WorktreePruneOptions::new();
                        opts.valid(true);
                        let _ = wt.prune(Some(&mut opts));
                    }
                }
                let _ = std::fs::remove_dir_all(&row.worktree_path);
            }
            self.db.borrow_mut().delete_pending_worktree(&row.id)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Create a test WorktreeManager using a temp directory for both base_dir and DB.
    fn make_manager(base_dir: &Path, db_path: &Path) -> WorktreeManager {
        let db = WorktreeDb::open(db_path).unwrap();
        WorktreeManager::new(base_dir.to_path_buf(), db)
    }

    /// Initialize a git repository in `dir` with an initial commit.
    fn init_git_repo(dir: &Path) {
        let repo = git2::Repository::init(dir).unwrap();
        // Set minimal git config to allow committing.
        {
            let mut config = repo.config().unwrap();
            config.set_str("user.name", "test").unwrap();
            config.set_str("user.email", "test@test.com").unwrap();
        }
        // Create an initial commit so HEAD is valid.
        let sig = repo.signature().unwrap();
        let tree_id = {
            let mut index = repo.index().unwrap();
            index.write_tree().unwrap()
        };
        {
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
                .unwrap();
        }
    }

    #[test]
    fn test_create_worktree_git() {
        let env = TempDir::new().unwrap();
        let project_dir = env.path().join("project");
        std::fs::create_dir_all(&project_dir).unwrap();
        init_git_repo(&project_dir);

        let wt_base = env.path().join("worktrees");
        let db_path = env.path().join("agents.db");
        let mgr = make_manager(&wt_base, &db_path);

        let file_changes = vec![("src/main.rs".to_string(), "fn main() {}".to_string())];
        let handle = mgr
            .create_worktree(&project_dir, "proposal-1", &file_changes)
            .unwrap();

        assert!(handle.worktree_path.exists(), "Worktree dir should exist");
        assert!(matches!(handle.kind, WorktreeKind::Git { .. }));
        assert_eq!(handle.changed_files, vec![PathBuf::from("src/main.rs")]);
    }

    #[test]
    fn test_create_worktree_writes_file_changes() {
        let env = TempDir::new().unwrap();
        let project_dir = env.path().join("project");
        std::fs::create_dir_all(&project_dir).unwrap();
        init_git_repo(&project_dir);

        let wt_base = env.path().join("worktrees");
        let db_path = env.path().join("agents.db");
        let mgr = make_manager(&wt_base, &db_path);

        let content = "pub fn hello() { println!(\"hello\"); }";
        let file_changes = vec![("src/lib.rs".to_string(), content.to_string())];
        let handle = mgr
            .create_worktree(&project_dir, "proposal-2", &file_changes)
            .unwrap();

        let written = std::fs::read_to_string(handle.worktree_path.join("src/lib.rs")).unwrap();
        assert_eq!(written, content);
    }

    #[test]
    fn test_create_worktree_non_git_fallback() {
        let env = TempDir::new().unwrap();
        let project_dir = env.path().join("non-git-project");
        std::fs::create_dir_all(&project_dir).unwrap();
        // No git init -- this should use the TempDir fallback.

        let wt_base = env.path().join("worktrees");
        let db_path = env.path().join("agents.db");
        let mgr = make_manager(&wt_base, &db_path);

        let file_changes = vec![("config.yaml".to_string(), "key: value".to_string())];
        let handle = mgr
            .create_worktree(&project_dir, "proposal-3", &file_changes)
            .unwrap();

        assert!(handle.worktree_path.exists(), "Fallback dir should exist");
        assert!(matches!(handle.kind, WorktreeKind::TempDir));
        let written =
            std::fs::read_to_string(handle.worktree_path.join("config.yaml")).unwrap();
        assert_eq!(written, "key: value");
    }

    #[test]
    fn test_generate_diff_text() {
        let env = TempDir::new().unwrap();
        let project_dir = env.path().join("project");
        std::fs::create_dir_all(&project_dir).unwrap();

        let wt_base = env.path().join("worktrees");
        let db_path = env.path().join("agents.db");
        let mgr = make_manager(&wt_base, &db_path);

        // Create a file in the project.
        std::fs::write(project_dir.join("hello.txt"), "hello world\n").unwrap();

        // Create worktree with a modification.
        let file_changes = vec![("hello.txt".to_string(), "hello rust\n".to_string())];
        let handle = mgr
            .create_worktree(&project_dir, "proposal-diff", &file_changes)
            .unwrap();

        let diff = mgr.generate_diff(&handle).unwrap();
        assert!(diff.contains("--- a/hello.txt"), "Diff should have header");
        assert!(diff.contains("+++ b/hello.txt"), "Diff should have header");
        assert!(diff.contains("-hello world"), "Diff should show removed line");
        assert!(diff.contains("+hello rust"), "Diff should show added line");
    }

    #[test]
    fn test_generate_diff_binary_placeholder() {
        let env = TempDir::new().unwrap();
        let project_dir = env.path().join("project");
        std::fs::create_dir_all(&project_dir).unwrap();

        let wt_base = env.path().join("worktrees");
        let db_path = env.path().join("agents.db");
        let mgr = make_manager(&wt_base, &db_path);

        // Write a binary file to the worktree manually.
        let wt_path = wt_base.join("test-binary-wt");
        std::fs::create_dir_all(wt_path.join("img")).unwrap();
        // Invalid UTF-8 bytes
        std::fs::write(wt_path.join("img/photo.png"), &[0xFF, 0xFE, 0x00, 0x01]).unwrap();

        let handle = WorktreeHandle {
            id: "test-binary-wt".to_string(),
            worktree_path: wt_path.clone(),
            project_root: project_dir.clone(),
            kind: WorktreeKind::TempDir,
            changed_files: vec![PathBuf::from("img/photo.png")],
        };

        let diff = mgr.generate_diff(&handle).unwrap();
        assert!(
            diff.contains("Binary file"),
            "Should contain binary placeholder, got: {diff}"
        );
    }

    #[test]
    fn test_apply_copies_files_to_working_tree() {
        let env = TempDir::new().unwrap();
        let project_dir = env.path().join("project");
        std::fs::create_dir_all(&project_dir).unwrap();

        let wt_base = env.path().join("worktrees");
        let db_path = env.path().join("agents.db");
        let mgr = make_manager(&wt_base, &db_path);

        let new_content = "fn applied() {}";
        let file_changes = vec![("src/mod.rs".to_string(), new_content.to_string())];
        let handle = mgr
            .create_worktree(&project_dir, "proposal-apply", &file_changes)
            .unwrap();
        let wt_path = handle.worktree_path.clone();

        mgr.apply(handle).unwrap();

        // File should now exist in working tree.
        let applied = std::fs::read_to_string(project_dir.join("src/mod.rs")).unwrap();
        assert_eq!(applied, new_content);
        // Worktree directory should be gone.
        assert!(!wt_path.exists(), "Worktree dir should be removed after apply");
    }

    #[test]
    fn test_dismiss_removes_worktree_without_touching_working_tree() {
        let env = TempDir::new().unwrap();
        let project_dir = env.path().join("project");
        std::fs::create_dir_all(&project_dir).unwrap();

        let wt_base = env.path().join("worktrees");
        let db_path = env.path().join("agents.db");
        let mgr = make_manager(&wt_base, &db_path);

        let file_changes = vec![("dismissed.rs".to_string(), "fn nope() {}".to_string())];
        let handle = mgr
            .create_worktree(&project_dir, "proposal-dismiss", &file_changes)
            .unwrap();
        let wt_path = handle.worktree_path.clone();

        mgr.dismiss(handle).unwrap();

        // Worktree dir should be gone.
        assert!(!wt_path.exists(), "Worktree dir should be removed after dismiss");
        // Working tree file should NOT exist.
        assert!(
            !project_dir.join("dismissed.rs").exists(),
            "Working tree should not be modified by dismiss"
        );
    }

    #[test]
    fn test_prune_orphans() {
        let env = TempDir::new().unwrap();
        let project_dir = env.path().join("project");
        std::fs::create_dir_all(&project_dir).unwrap();

        let wt_base = env.path().join("worktrees");
        let db_path = env.path().join("agents.db");

        // Simulate a crash: insert DB row and create directory but don't call apply/dismiss.
        let orphan_id = "orphan-uuid-001";
        let orphan_path = wt_base.join(orphan_id);
        std::fs::create_dir_all(&orphan_path).unwrap();
        std::fs::write(orphan_path.join("leftover.txt"), "leftover").unwrap();

        {
            let mut db = WorktreeDb::open(&db_path).unwrap();
            db.insert_pending_worktree(orphan_id, &orphan_path, &project_dir, "crashed-proposal")
                .unwrap();
        }

        // Now create the manager and call prune_orphans.
        let db = WorktreeDb::open(&db_path).unwrap();
        let mgr = WorktreeManager::new(wt_base.clone(), db);
        mgr.prune_orphans().unwrap();

        // Orphan directory should be gone.
        assert!(!orphan_path.exists(), "Orphan worktree dir should be pruned");

        // DB row should be gone.
        let db2 = WorktreeDb::open(&db_path).unwrap();
        let rows = db2.list_pending_worktrees().unwrap();
        assert!(rows.is_empty(), "DB row should be deleted after pruning");
    }

    #[test]
    fn test_register_before_create_invariant() {
        let env = TempDir::new().unwrap();
        let project_dir = env.path().join("project");
        std::fs::create_dir_all(&project_dir).unwrap();

        let wt_base = env.path().join("worktrees");
        let db_path = env.path().join("agents.db");

        // Open the DB independently to check row existence DURING create.
        // We simulate this by opening the DB before calling create_worktree
        // and checking rows exist after -- since we can't intercept mid-call
        // without unsafe tricks, we verify the row is present after success.
        let mgr = make_manager(&wt_base, &db_path);
        let file_changes = vec![("check.rs".to_string(), "".to_string())];
        let handle = mgr
            .create_worktree(&project_dir, "proposal-invariant", &file_changes)
            .unwrap();

        // After successful create, the pending row should still exist (not deleted yet).
        let db_check = WorktreeDb::open(&db_path).unwrap();
        let rows = db_check.list_pending_worktrees().unwrap();
        assert_eq!(rows.len(), 1, "Pending row should exist until apply/dismiss");
        assert_eq!(rows[0].id, handle.id);

        // Clean up.
        mgr.dismiss(handle).unwrap();
        let rows_after = db_check.list_pending_worktrees().unwrap();
        assert!(rows_after.is_empty(), "Row should be gone after dismiss");
    }
}
