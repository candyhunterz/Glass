//! Pipeline classification: TTY detection and opt-out flag checking.

use crate::types::{PipeStage, PipelineClassification};

/// TTY-sensitive commands that require a terminal and should not be captured.
const TTY_COMMANDS: &[&str] = &[
    "less", "more", "most", "vim", "vi", "nvim", "nano", "emacs", "emacsclient",
    "fzf", "sk", "htop", "top", "btop", "gtop", "man", "ssh", "mosh",
    "tmux", "screen", "zellij", "python", "python3", "ipython", "node",
    "psql", "mysql", "sqlite3", "gdb", "lldb",
];

/// Git subcommands that invoke a pager (TTY-sensitive).
const TTY_GIT_SUBCOMMANDS: &[&str] = &[
    "log", "diff", "show", "blame", "reflog",
];

/// Check if a program (with its args) is TTY-sensitive.
fn is_tty_command(program: &str, args: &[&str]) -> bool {
    // Strip to basename: handle both / and \ path separators
    let basename = program
        .rsplit('/')
        .next()
        .unwrap_or(program);
    let basename = basename
        .rsplit('\\')
        .next()
        .unwrap_or(basename);

    if TTY_COMMANDS.contains(&basename) {
        return true;
    }

    // Special case: git with pager subcommands
    if basename == "git" {
        if let Some(first_arg) = args.first() {
            return TTY_GIT_SUBCOMMANDS.contains(first_arg);
        }
    }

    false
}

/// Check if a command string contains the --no-glass opt-out flag.
///
/// The flag must be an exact whitespace-delimited token (not a substring).
pub fn has_opt_out(command: &str) -> bool {
    command.split_whitespace().any(|token| token == "--no-glass")
}

/// Classify a pipeline's stages for capture decisions.
///
/// Mutates each stage's `is_tty` field and returns the overall classification.
pub fn classify_pipeline(stages: &mut [PipeStage], raw_command: &str) -> PipelineClassification {
    let opted_out = has_opt_out(raw_command);
    let mut has_tty_command = false;
    let mut tty_stages = Vec::new();

    for stage in stages.iter_mut() {
        // Parse stage command to get args (skip the program name which is token 0)
        let tokens = shlex::split(&stage.command).unwrap_or_default();
        let args: Vec<&str> = tokens.iter().skip(1).map(|s| s.as_str()).collect();

        let is_tty = is_tty_command(&stage.program, &args);
        stage.is_tty = is_tty;

        if is_tty {
            has_tty_command = true;
            tty_stages.push(stage.index);
        }
    }

    let should_capture = !has_tty_command && !opted_out;

    PipelineClassification {
        has_tty_command,
        tty_stages,
        opted_out,
        should_capture,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- has_opt_out tests --

    #[test]
    fn test_opt_out_present() {
        assert!(has_opt_out("ls --no-glass | grep foo"));
    }

    #[test]
    fn test_opt_out_absent() {
        assert!(!has_opt_out("ls | grep foo"));
    }

    #[test]
    fn test_opt_out_substring_not_matched() {
        assert!(!has_opt_out("ls --no-glass-thing"));
    }

    // -- is_tty_command tests --

    #[test]
    fn test_tty_less() {
        assert!(is_tty_command("less", &[]));
    }

    #[test]
    fn test_tty_vim() {
        assert!(is_tty_command("vim", &[]));
    }

    #[test]
    fn test_tty_fzf() {
        assert!(is_tty_command("fzf", &[]));
    }

    #[test]
    fn test_tty_git_log() {
        assert!(is_tty_command("git", &["log"]));
    }

    #[test]
    fn test_not_tty_git_status() {
        assert!(!is_tty_command("git", &["status"]));
    }

    #[test]
    fn test_not_tty_grep() {
        assert!(!is_tty_command("grep", &[]));
    }

    #[test]
    fn test_tty_path_stripped() {
        assert!(is_tty_command("/usr/bin/less", &[]));
    }

    #[test]
    fn test_tty_windows_path_stripped() {
        assert!(is_tty_command("C:\\Program Files\\vim", &[]));
    }

    // -- classify_pipeline tests --

    fn make_stage(command: &str, index: usize, program: &str) -> PipeStage {
        PipeStage {
            command: command.to_string(),
            index,
            program: program.to_string(),
            is_tty: false,
        }
    }

    #[test]
    fn test_classify_tty_sets_should_capture_false() {
        let mut stages = vec![
            make_stage("ls -la", 0, "ls"),
            make_stage("less", 1, "less"),
        ];
        let result = classify_pipeline(&mut stages, "ls -la | less");
        assert!(result.has_tty_command);
        assert!(!result.should_capture);
        assert_eq!(result.tty_stages, vec![1]);
        assert!(stages[1].is_tty);
        assert!(!stages[0].is_tty);
    }

    #[test]
    fn test_classify_opt_out_sets_should_capture_false() {
        let mut stages = vec![
            make_stage("ls --no-glass", 0, "ls"),
            make_stage("grep foo", 1, "grep"),
        ];
        let result = classify_pipeline(&mut stages, "ls --no-glass | grep foo");
        assert!(result.opted_out);
        assert!(!result.should_capture);
    }

    #[test]
    fn test_classify_clean_pipeline() {
        let mut stages = vec![
            make_stage("ls -la", 0, "ls"),
            make_stage("grep foo", 1, "grep"),
            make_stage("wc -l", 2, "wc"),
        ];
        let result = classify_pipeline(&mut stages, "ls -la | grep foo | wc -l");
        assert!(!result.has_tty_command);
        assert!(!result.opted_out);
        assert!(result.should_capture);
        assert!(result.tty_stages.is_empty());
    }

    #[test]
    fn test_classify_git_pager_subcommand() {
        let mut stages = vec![
            make_stage("git log --oneline", 0, "git"),
        ];
        let result = classify_pipeline(&mut stages, "git log --oneline");
        assert!(result.has_tty_command);
        assert!(!result.should_capture);
        assert_eq!(result.tty_stages, vec![0]);
    }

    #[test]
    fn test_classify_git_non_pager_subcommand() {
        let mut stages = vec![
            make_stage("git status", 0, "git"),
        ];
        let result = classify_pipeline(&mut stages, "git status");
        assert!(!result.has_tty_command);
        assert!(result.should_capture);
    }
}
