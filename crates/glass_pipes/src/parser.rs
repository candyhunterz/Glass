use crate::types::{PipeStage, Pipeline};

/// Split a command string into pipe stages.
///
/// Byte-level state machine that scans for unquoted, unescaped `|` characters
/// at parenthesis depth 0. Respects single quotes, double quotes, backslash
/// escapes (POSIX), backtick escapes (PowerShell), and parenthesized subshells
/// including `$(...)` command substitution.
///
/// Distinguishes `|` (pipe) from `||` (logical OR).
/// Returns trimmed slices for each stage.
pub fn split_pipes(command: &str) -> Vec<&str> {
    let bytes = command.as_bytes();
    let len = bytes.len();
    let mut stages = Vec::new();
    let mut start = 0;
    let mut i = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escaped = false;
    let mut paren_depth: usize = 0;

    while i < len {
        let b = bytes[i];

        if escaped {
            escaped = false;
            i += 1;
            continue;
        }

        match b {
            // Backslash escape (not inside single quotes)
            b'\\' if !in_single_quote => {
                escaped = true;
                i += 1;
                continue;
            }
            // Backtick escape (PowerShell -- not inside quotes)
            b'`' if !in_single_quote && !in_double_quote => {
                escaped = true;
                i += 1;
                continue;
            }
            // Single quote toggle (not inside double quotes)
            b'\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
            }
            // Double quote toggle (not inside single quotes)
            b'"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            // Open paren (not inside quotes) -- increases depth
            b'(' if !in_single_quote && !in_double_quote => {
                paren_depth += 1;
            }
            // Close paren (not inside quotes) -- decreases depth
            b')' if !in_single_quote && !in_double_quote => {
                paren_depth = paren_depth.saturating_sub(1);
            }
            // Pipe character -- only split at depth 0, not in quotes
            b'|' if !in_single_quote && !in_double_quote && paren_depth == 0 => {
                // Check for || (logical OR) -- peek at next byte
                if i + 1 < len && bytes[i + 1] == b'|' {
                    // Skip both characters of ||
                    i += 2;
                    continue;
                }
                // It's a real pipe boundary
                stages.push(command[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }

    // Push the final stage
    stages.push(command[start..].trim());
    stages
}

/// Extract the program name from a command string.
///
/// Takes the first whitespace-delimited token and strips any path prefix
/// (both Unix `/` and Windows `\` separators). Uses raw whitespace splitting
/// rather than shlex for program extraction because shlex interprets
/// backslashes as escape characters, which mangles Windows paths like
/// `C:\Windows\System32\cmd.exe`.
fn extract_program(stage_command: &str) -> String {
    let trimmed = stage_command.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Use raw whitespace split to get the first token (preserves backslashes)
    if let Some(first) = trimmed.split_whitespace().next() {
        return strip_path(first);
    }

    String::new()
}

/// Strip directory path from a program name.
/// Handles both Unix (`/`) and Windows (`\`) path separators.
fn strip_path(program: &str) -> String {
    program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .to_string()
}

/// Parse a command string into a Pipeline with typed stages.
///
/// Calls `split_pipes` to find stage boundaries, then tokenizes each stage
/// with shlex to extract the program name (first token, path-stripped).
/// Returns a Pipeline with default classification (not yet classified).
pub fn parse_pipeline(command: &str) -> Pipeline {
    let raw_stages = split_pipes(command);

    let stages: Vec<PipeStage> = raw_stages
        .iter()
        .enumerate()
        .map(|(index, &stage_text)| {
            let program = extract_program(stage_text);
            PipeStage {
                command: stage_text.to_string(),
                index,
                program,
                is_tty: false,
            }
        })
        .collect();

    Pipeline {
        raw_command: command.to_string(),
        stages,
    }
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

    // --- Audit: edge-case tests ---

    #[test]
    fn split_pipes_no_spaces_around_pipe() {
        let result = split_pipes("cmd1|cmd2");
        assert_eq!(result, vec!["cmd1", "cmd2"]);
    }

    #[test]
    fn split_pipes_with_redirection() {
        let result = split_pipes("cat file | grep foo > out.txt");
        assert_eq!(result, vec!["cat file", "grep foo > out.txt"]);
    }

    #[test]
    fn split_pipes_stderr_redirect() {
        let result = split_pipes("cmd1 2>&1 | cmd2");
        assert_eq!(result, vec!["cmd1 2>&1", "cmd2"]);
    }

    #[test]
    fn split_pipes_backgrounded_pipeline() {
        let result = split_pipes("cmd1 | cmd2 &");
        assert_eq!(result, vec!["cmd1", "cmd2 &"]);
    }

    #[test]
    fn split_pipes_process_substitution() {
        // <( starts a subshell; inner pipe should not split
        let result = split_pipes("diff <(cmd1 | sort) <(cmd2 | sort)");
        assert_eq!(
            result,
            vec!["diff <(cmd1 | sort) <(cmd2 | sort)"]
        );
    }

    #[test]
    fn split_pipes_arithmetic_expansion() {
        let result = split_pipes("echo $((1+1)) | cat");
        assert_eq!(result, vec!["echo $((1+1))", "cat"]);
    }

    #[test]
    fn split_pipes_unmatched_quote_treats_as_single_stage() {
        // Malformed: unclosed single quote — parser conservatively returns one stage
        let result = split_pipes("echo 'unclosed | pipe");
        assert_eq!(result, vec!["echo 'unclosed | pipe"]);
    }

    #[test]
    fn split_pipes_pipe_at_start() {
        let result = split_pipes("| cmd");
        assert_eq!(result, vec!["", "cmd"]);
    }

    #[test]
    fn split_pipes_pipe_at_end() {
        let result = split_pipes("cmd |");
        assert_eq!(result, vec!["cmd", ""]);
    }

    #[test]
    fn extract_program_empty() {
        assert_eq!(extract_program(""), "");
    }

    #[test]
    fn extract_program_whitespace_only() {
        assert_eq!(extract_program("   "), "");
    }
}
