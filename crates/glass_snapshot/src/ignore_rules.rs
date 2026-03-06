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
        let mut builder = GitignoreBuilder::new(cwd);

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
            GitignoreBuilder::new(cwd).build().unwrap()
        });

        Self { matcher }
    }

    /// Check whether a given path should be ignored.
    pub fn is_ignored(&self, path: &Path) -> bool {
        self.matcher
            .matched_path_or_any_parents(path, path.is_dir())
            .is_ignore()
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
        std::fs::write(
            dir.path().join(".glassignore"),
            "*.log\n!important.log\n",
        )
        .unwrap();
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
