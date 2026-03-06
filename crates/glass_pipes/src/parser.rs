use crate::types::{Pipeline, PipeStage, PipelineClassification};

/// Split a command string into pipe stages.
///
/// Respects single quotes, double quotes, backslash escapes, backtick escapes,
/// parenthesis depth, and distinguishes `|` (pipe) from `||` (logical OR).
/// Returns trimmed slices for each stage.
pub fn split_pipes(_command: &str) -> Vec<&str> {
    todo!("Implemented in GREEN phase")
}

/// Parse a command string into a Pipeline with typed stages.
///
/// Calls split_pipes to find stage boundaries, then tokenizes each stage
/// with shlex to extract the program name (first token, path-stripped).
pub fn parse_pipeline(_command: &str) -> Pipeline {
    todo!("Implemented in GREEN phase")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── split_pipes tests ──

    #[test]
    fn split_pipes_basic_multi_stage() {
        let result = split_pipes("cat file | grep foo | wc -l");
        assert_eq!(result, vec!["cat file", "grep foo", "wc -l"]);
    }

    #[test]
    fn split_pipes_single_command_no_pipe() {
        let result = split_pipes("echo hello");
        assert_eq!(result, vec!["echo hello"]);
    }

    #[test]
    fn split_pipes_pipe_in_single_quotes() {
        let result = split_pipes("echo 'hello | world'");
        assert_eq!(result, vec!["echo 'hello | world'"]);
    }

    #[test]
    fn split_pipes_pipe_in_double_quotes() {
        let result = split_pipes("echo \"hello | world\"");
        assert_eq!(result, vec!["echo \"hello | world\""]);
    }

    #[test]
    fn split_pipes_backslash_escaped_pipe() {
        let result = split_pipes("echo hello \\| world");
        assert_eq!(result, vec!["echo hello \\| world"]);
    }

    #[test]
    fn split_pipes_backtick_escaped_pipe() {
        let result = split_pipes("echo hello `| world");
        assert_eq!(result, vec!["echo hello `| world"]);
    }

    #[test]
    fn split_pipes_logical_or_not_pipe() {
        let result = split_pipes("cmd1 || cmd2");
        assert_eq!(result, vec!["cmd1 || cmd2"]);
    }

    #[test]
    fn split_pipes_logical_or_and_pipe_mixed() {
        let result = split_pipes("cmd1 || cmd2 | cmd3");
        assert_eq!(result, vec!["cmd1 || cmd2", "cmd3"]);
    }

    #[test]
    fn split_pipes_command_substitution() {
        let result = split_pipes("echo $(cat file | grep foo) | wc");
        assert_eq!(result, vec!["echo $(cat file | grep foo)", "wc"]);
    }

    #[test]
    fn split_pipes_parenthesized_subshell() {
        let result = split_pipes("(cmd1 | cmd2) | cmd3");
        assert_eq!(result, vec!["(cmd1 | cmd2)", "cmd3"]);
    }

    #[test]
    fn split_pipes_empty_string() {
        let result = split_pipes("");
        assert_eq!(result, vec![""]);
    }

    #[test]
    fn split_pipes_whitespace_trimming() {
        let result = split_pipes("  cat file  |  grep foo  ");
        assert_eq!(result, vec!["cat file", "grep foo"]);
    }

    // ── parse_pipeline tests ──

    #[test]
    fn parse_pipeline_multi_stage() {
        let pipeline = parse_pipeline("cat file | grep foo | wc -l");
        assert_eq!(pipeline.stages.len(), 3);
        assert_eq!(pipeline.stages[0].program, "cat");
        assert_eq!(pipeline.stages[1].program, "grep");
        assert_eq!(pipeline.stages[2].program, "wc");
        assert_eq!(pipeline.stages[0].index, 0);
        assert_eq!(pipeline.stages[1].index, 1);
        assert_eq!(pipeline.stages[2].index, 2);
    }

    #[test]
    fn parse_pipeline_single_command() {
        let pipeline = parse_pipeline("ls");
        assert_eq!(pipeline.stages.len(), 1);
        assert_eq!(pipeline.stages[0].program, "ls");
        assert_eq!(pipeline.stages[0].index, 0);
    }

    #[test]
    fn parse_pipeline_path_stripped_program() {
        let pipeline = parse_pipeline("/usr/bin/cat file | grep foo");
        assert_eq!(pipeline.stages[0].program, "cat");
    }

    #[test]
    fn parse_pipeline_windows_path_stripped() {
        let pipeline = parse_pipeline("C:\\Windows\\System32\\cmd.exe /c dir | findstr foo");
        assert_eq!(pipeline.stages[0].program, "cmd.exe");
        assert_eq!(pipeline.stages[1].program, "findstr");
    }

    #[test]
    fn parse_pipeline_empty_command() {
        let pipeline = parse_pipeline("");
        assert_eq!(pipeline.stages.len(), 1);
        assert_eq!(pipeline.stages[0].command, "");
        assert_eq!(pipeline.raw_command, "");
    }

    #[test]
    fn parse_pipeline_raw_command_preserved() {
        let cmd = "cat file | grep foo | wc -l";
        let pipeline = parse_pipeline(cmd);
        assert_eq!(pipeline.raw_command, cmd);
    }

    #[test]
    fn parse_pipeline_is_tty_defaults_false() {
        let pipeline = parse_pipeline("cat file | grep foo");
        assert!(!pipeline.stages[0].is_tty);
        assert!(!pipeline.stages[1].is_tty);
    }

    #[test]
    fn parse_pipeline_default_classification() {
        let pipeline = parse_pipeline("cat file | grep foo");
        assert!(pipeline.classification.should_capture);
        assert!(!pipeline.classification.has_tty_command);
        assert!(!pipeline.classification.opted_out);
        assert!(pipeline.classification.tty_stages.is_empty());
    }
}
