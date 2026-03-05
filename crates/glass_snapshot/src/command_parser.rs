//! POSIX command parser for identifying file modification targets.
//!
//! Pure function: `(command_text, cwd) -> ParseResult` with no state or DB access.
//! Uses a whitelist of known destructive commands and per-command argument extractors.

use std::path::Path;

use crate::types::{Confidence, ParseResult};

/// Parse a shell command and extract file modification targets.
///
/// This is a heuristic parser -- it handles common destructive commands
/// (rm, mv, cp, sed -i, chmod, git checkout, truncate) and returns
/// `Confidence::Low` for anything it cannot parse.
pub fn parse_command(command_text: &str, cwd: &Path) -> ParseResult {
    let _trimmed = command_text.trim();
    // Stub: return Low confidence for everything
    ParseResult {
        targets: vec![],
        confidence: Confidence::Low,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn cwd() -> PathBuf {
        PathBuf::from("/home/user/project")
    }

    #[test]
    fn test_empty_command() {
        let result = parse_command("", &cwd());
        assert_eq!(result.confidence, Confidence::ReadOnly);
        assert!(result.targets.is_empty());
    }

    #[test]
    fn test_rm_single_file() {
        let result = parse_command("rm foo.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert_eq!(result.targets, vec![cwd().join("foo.txt")]);
    }

    #[test]
    fn test_rm_multiple_files() {
        let result = parse_command("rm foo.txt bar.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert_eq!(
            result.targets,
            vec![cwd().join("foo.txt"), cwd().join("bar.txt")]
        );
    }

    #[test]
    fn test_readonly_commands() {
        for cmd in &["ls foo", "cat file.txt", "grep pattern file", "echo hello", "pwd"] {
            let result = parse_command(cmd, &cwd());
            assert_eq!(
                result.confidence,
                Confidence::ReadOnly,
                "Expected ReadOnly for '{cmd}'"
            );
            assert!(
                result.targets.is_empty(),
                "Expected no targets for '{cmd}'"
            );
        }
    }

    #[test]
    fn test_unknown_command() {
        let result = parse_command("somecustomscript arg", &cwd());
        assert_eq!(result.confidence, Confidence::Low);
    }

    #[test]
    fn test_quoted_args() {
        let result = parse_command("rm \"file with spaces.txt\"", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert_eq!(
            result.targets,
            vec![cwd().join("file with spaces.txt")]
        );
    }

    #[test]
    fn test_path_resolution() {
        // Relative path resolved against cwd
        let result = parse_command("rm foo.txt", &cwd());
        assert_eq!(result.targets, vec![cwd().join("foo.txt")]);

        // Absolute path kept as-is
        let result = parse_command("rm /tmp/foo.txt", &cwd());
        assert_eq!(result.targets, vec![PathBuf::from("/tmp/foo.txt")]);
    }

    #[test]
    fn test_mv_source_and_dest() {
        let result = parse_command("mv src.txt dst.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert_eq!(
            result.targets,
            vec![cwd().join("src.txt"), cwd().join("dst.txt")]
        );
    }

    #[test]
    fn test_cp_dest() {
        let result = parse_command("cp src.txt dst.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert!(result.targets.contains(&cwd().join("dst.txt")));
    }

    #[test]
    fn test_sed_inplace() {
        // sed -i is destructive
        let result = parse_command("sed -i 's/a/b/' file.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert!(result.targets.contains(&cwd().join("file.txt")));

        // sed without -i is read-only
        let result = parse_command("sed 's/a/b/' file.txt", &cwd());
        assert_eq!(result.confidence, Confidence::ReadOnly);
    }

    #[test]
    fn test_chmod() {
        let result = parse_command("chmod 755 file.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert!(result.targets.contains(&cwd().join("file.txt")));
    }

    #[test]
    fn test_git_checkout_with_dashdash() {
        let result = parse_command("git checkout -- file.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert!(result.targets.contains(&cwd().join("file.txt")));
    }

    #[test]
    fn test_redirect() {
        let result = parse_command("echo hello > out.txt", &cwd());
        assert!(result.targets.contains(&cwd().join("out.txt")));
    }

    #[test]
    fn test_unparseable_syntax() {
        for cmd in &[
            "cat file | grep pattern",
            "echo $(date)",
            "cmd1 && cmd2",
            "cmd1; cmd2",
        ] {
            let result = parse_command(cmd, &cwd());
            assert_eq!(
                result.confidence,
                Confidence::Low,
                "Expected Low for '{cmd}'"
            );
        }
    }
}
