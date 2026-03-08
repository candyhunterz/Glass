//! Ignore-rule engine for `.glassignore` pattern matching.
//!
//! Wraps the `ignore` crate's gitignore module to provide
//! hardcoded exclusions (.git, node_modules, target) plus
//! user-defined patterns from a `.glassignore` file.

use std::path::Path;

use ignore::gitignore::{Gitignore, GitignoreBuilder};

/// Matches file paths against ignore rules (hardcoded + user-defined).
pub struct IgnoreRules {
    matcher: Gitignore,
}

impl IgnoreRules {
    /// Load ignore rules rooted at `cwd`.
    ///
    /// Always adds hardcoded exclusions for `.git/`, `node_modules/`, and
    /// `target/`. If a `.glassignore` file exists in `cwd`, its patterns
    /// are loaded on top.
    pub fn load(cwd: &Path) -> Self {
        // Canonicalize to resolve symlinks (e.g. macOS /var -> /private/var)
        // so paths from notify events match the gitignore root.
        let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
        let mut builder = GitignoreBuilder::new(&cwd);

        // Hardcoded exclusions
        builder.add_line(None, ".git/").ok();
        builder.add_line(None, "node_modules/").ok();
        builder.add_line(None, "target/").ok();

        // User-defined patterns from .glassignore
        let glassignore = cwd.join(".glassignore");
        if glassignore.exists() {
            builder.add(&glassignore);
        }

        let matcher = builder.build().unwrap_or_else(|_| {
            // Fallback to empty matcher on error
            GitignoreBuilder::new(&cwd).build().unwrap()
        });

        Self { matcher }
    }

    /// Check whether a given path should be ignored.
    ///
    /// The path is canonicalized to match the canonical root used by the
    /// `ignore` crate. For paths that don't exist on disk, the longest
    /// existing ancestor is canonicalized and the remaining components
    /// are appended.
    pub fn is_ignored(&self, path: &Path) -> bool {
        let canonical = self.canonicalize_path(path);
        self.matcher
            .matched_path_or_any_parents(&canonical, canonical.is_dir())
            .is_ignore()
    }

    /// Canonicalize a path, handling non-existent files by canonicalizing
    /// the deepest existing ancestor and appending the remaining components.
    fn canonicalize_path(&self, path: &Path) -> std::path::PathBuf {
        // Fast path: the path exists and can be fully canonicalized.
        if let Ok(canon) = path.canonicalize() {
            return canon;
        }
        // Walk up to find the deepest existing ancestor, then append the rest.
        let mut existing = path.to_path_buf();
        let mut suffix = Vec::new();
        while !existing.exists() {
            if let Some(name) = existing.file_name() {
                suffix.push(name.to_os_string());
            } else {
                return path.to_path_buf();
            }
            existing.pop();
        }
        if let Ok(mut canon) = existing.canonicalize() {
            for component in suffix.into_iter().rev() {
                canon.push(component);
            }
            canon
        } else {
            path.to_path_buf()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_excludes_git() {
        let dir = TempDir::new().unwrap();
        let rules = IgnoreRules::load(dir.path());
        assert!(rules.is_ignored(&dir.path().join(".git/objects/abc")));
    }

    #[test]
    fn test_default_excludes_node_modules() {
        let dir = TempDir::new().unwrap();
        let rules = IgnoreRules::load(dir.path());
        assert!(rules.is_ignored(&dir.path().join("node_modules/foo/bar.js")));
    }

    #[test]
    fn test_default_excludes_target() {
        let dir = TempDir::new().unwrap();
        let rules = IgnoreRules::load(dir.path());
        assert!(rules.is_ignored(&dir.path().join("target/debug/build")));
    }

    #[test]
    fn test_regular_file_not_ignored() {
        let dir = TempDir::new().unwrap();
        let rules = IgnoreRules::load(dir.path());
        assert!(!rules.is_ignored(&dir.path().join("src/main.rs")));
    }

    #[test]
    fn test_glassignore_pattern() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(".glassignore"), "*.log\n").unwrap();
        let rules = IgnoreRules::load(dir.path());
        assert!(rules.is_ignored(&dir.path().join("foo.log")));
    }

    #[test]
    fn test_glassignore_negation() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(".glassignore"), "*.log\n!important.log\n").unwrap();
        let rules = IgnoreRules::load(dir.path());
        assert!(!rules.is_ignored(&dir.path().join("important.log")));
        assert!(rules.is_ignored(&dir.path().join("other.log")));
    }

    #[test]
    fn test_glassignore_directory_pattern() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(".glassignore"), "build/\n").unwrap();
        let rules = IgnoreRules::load(dir.path());
        assert!(rules.is_ignored(&dir.path().join("build/output.js")));
    }

    #[test]
    fn test_no_glassignore_still_works() {
        let dir = TempDir::new().unwrap();
        // No .glassignore file -- should still have defaults
        let rules = IgnoreRules::load(dir.path());
        assert!(rules.is_ignored(&dir.path().join(".git/HEAD")));
        assert!(!rules.is_ignored(&dir.path().join("README.md")));
    }
}
