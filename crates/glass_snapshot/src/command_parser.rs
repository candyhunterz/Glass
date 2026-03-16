//! POSIX command parser for identifying file modification targets.
//!
//! Pure function: `(command_text, cwd) -> ParseResult` with no state or DB access.
//! Uses a whitelist of known destructive commands and per-command argument extractors.

use std::path::{Path, PathBuf};

use crate::types::{Confidence, ParseResult};

/// Parse a shell command and extract file modification targets.
///
/// This is a heuristic parser -- it handles common destructive commands
/// (rm, mv, cp, sed -i, chmod, git checkout, truncate) and returns
/// `Confidence::Low` for anything it cannot parse.
pub fn parse_command(command_text: &str, cwd: &Path) -> ParseResult {
    let trimmed = command_text.trim();
    if trimmed.is_empty() {
        return ParseResult {
            targets: vec![],
            confidence: Confidence::ReadOnly,
        };
    }

    // Check for shell features we cannot parse
    if contains_unparseable_syntax(trimmed) {
        return ParseResult {
            targets: vec![],
            confidence: Confidence::Low,
        };
    }

    // Extract redirect targets (applies regardless of command)
    let mut redirect_targets = extract_redirect_targets(trimmed, cwd);

    // Tokenize
    let tokens = tokenize(trimmed);
    if tokens.is_empty() {
        let confidence = if redirect_targets.is_empty() {
            Confidence::Low
        } else {
            Confidence::High
        };
        return ParseResult {
            targets: redirect_targets,
            confidence,
        };
    }

    // Check if the command is a PowerShell cmdlet or alias
    let cmd = base_command(&tokens[0]);
    if is_powershell_cmdlet(cmd) {
        let ps_tokens = tokenize_powershell(trimmed);
        if ps_tokens.is_empty() {
            let confidence = if redirect_targets.is_empty() {
                Confidence::Low
            } else {
                Confidence::High
            };
            return ParseResult {
                targets: redirect_targets,
                confidence,
            };
        }
        let ps_cmd = base_command(&ps_tokens[0]);
        let ps_args: Vec<String> = ps_tokens[1..].to_vec();
        let mut result = parse_powershell_command(ps_cmd, &ps_args, cwd);
        result.targets.append(&mut redirect_targets);
        if result.confidence == Confidence::ReadOnly && !result.targets.is_empty() {
            result.confidence = Confidence::High;
        }
        return result;
    }

    // Dispatch to per-command parser (POSIX)
    let args: Vec<String> = tokens[1..].to_vec();
    let mut result = dispatch_command(cmd, &args, cwd);

    // Merge redirect targets
    result.targets.append(&mut redirect_targets);

    // If base command is read-only but we have redirect targets, upgrade confidence
    if result.confidence == Confidence::ReadOnly && !result.targets.is_empty() {
        result.confidence = Confidence::High;
    }

    result
}

/// Tokenize a command string using shlex for POSIX shell splitting.
/// Falls back to whitespace splitting if shlex cannot parse (e.g., unterminated quotes).
fn tokenize(command_text: &str) -> Vec<String> {
    // Strip redirect operators and their targets before tokenizing,
    // since shlex treats > as a normal character
    let cleaned = strip_redirects(command_text);
    shlex::split(&cleaned).unwrap_or_else(|| cleaned.split_whitespace().map(String::from).collect())
}

/// Remove redirect operators and their targets from command text.
/// This prevents redirect filenames from appearing as command arguments.
fn strip_redirects(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
                result.push('\'');
                i += 1;
            }
            b'"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
                result.push('"');
                i += 1;
            }
            b'>' if !in_single_quote && !in_double_quote => {
                // Skip the > or >>
                i += 1;
                if i < bytes.len() && bytes[i] == b'>' {
                    i += 1;
                }
                // Skip whitespace after redirect
                while i < bytes.len() && bytes[i] == b' ' {
                    i += 1;
                }
                // Skip the filename token
                if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                    let quote = bytes[i];
                    i += 1;
                    while i < bytes.len() && bytes[i] != quote {
                        i += 1;
                    }
                    if i < bytes.len() {
                        i += 1; // skip closing quote
                    }
                } else {
                    while i < bytes.len()
                        && bytes[i] != b' '
                        && bytes[i] != b'\t'
                        && bytes[i] != b'>'
                        && bytes[i] != b'|'
                        && bytes[i] != b';'
                    {
                        i += 1;
                    }
                }
            }
            // Also skip fd redirects like 2>
            b'0'..=b'9' if !in_single_quote && !in_double_quote => {
                // Check if next char is >
                if i + 1 < bytes.len() && bytes[i + 1] == b'>' {
                    // Skip the digit and let the > handler above deal with it
                    i += 1;
                    // Now skip > or >>
                    i += 1;
                    if i < bytes.len() && bytes[i] == b'>' {
                        i += 1;
                    }
                    while i < bytes.len() && bytes[i] == b' ' {
                        i += 1;
                    }
                    while i < bytes.len()
                        && bytes[i] != b' '
                        && bytes[i] != b'\t'
                        && bytes[i] != b'>'
                    {
                        i += 1;
                    }
                } else {
                    result.push(bytes[i] as char);
                    i += 1;
                }
            }
            _ => {
                result.push(bytes[i] as char);
                i += 1;
            }
        }
    }
    result
}

/// Extract the base command name, stripping path prefixes.
/// `/usr/bin/rm` -> `rm`, `./script.sh` -> `script.sh`
fn base_command(cmd: &str) -> &str {
    // Handle both forward and back slashes
    cmd.rsplit(['/', '\\']).next().unwrap_or(cmd)
}

/// Resolve a path argument against the working directory.
/// Does NOT check if the file exists -- the file may not exist yet
/// (e.g., cp destination) or may be deleted by the command.
fn resolve_path(path_str: &str, cwd: &Path) -> PathBuf {
    let path = Path::new(path_str);
    // On Windows, POSIX paths starting with `/` are not considered absolute
    // by Path::is_absolute(), but they ARE absolute in WSL/bash context.
    if path.is_absolute() || path_str.starts_with('/') {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

/// Detect syntax we cannot reliably parse.
fn contains_unparseable_syntax(text: &str) -> bool {
    text.contains("$(")
        || text.contains('`')
        || text.contains("${")
        || text.contains(" | ")
        || text.contains(" && ")
        || text.contains(" || ")
        || text.contains(';')
        || text.starts_with("for ")
        || text.starts_with("while ")
        || text.starts_with("if ")
}

/// Check for output redirections: `> file`, `>> file`, `2> file`.
/// These modify the redirect target regardless of the command.
fn extract_redirect_targets(command_text: &str, cwd: &Path) -> Vec<PathBuf> {
    let mut targets = Vec::new();
    let bytes = command_text.as_bytes();
    let mut i = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
            }
            b'"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            b'>' if !in_single_quote && !in_double_quote => {
                // Skip >> (append) -- still a target
                let mut j = i + 1;
                if j < bytes.len() && bytes[j] == b'>' {
                    j += 1;
                }
                // Skip whitespace after >
                while j < bytes.len() && bytes[j] == b' ' {
                    j += 1;
                }
                // Extract the filename token
                let start = j;
                while j < bytes.len()
                    && bytes[j] != b' '
                    && bytes[j] != b'\t'
                    && bytes[j] != b'>'
                    && bytes[j] != b'|'
                    && bytes[j] != b';'
                {
                    j += 1;
                }
                if start < j {
                    let filename = &command_text[start..j];
                    targets.push(resolve_path(filename, cwd));
                }
                i = j;
                continue;
            }
            // Handle fd redirects like 2>
            b'0'..=b'9' if !in_single_quote && !in_double_quote => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'>' {
                    let mut j = i + 2;
                    if j < bytes.len() && bytes[j] == b'>' {
                        j += 1;
                    }
                    while j < bytes.len() && bytes[j] == b' ' {
                        j += 1;
                    }
                    let start = j;
                    while j < bytes.len()
                        && bytes[j] != b' '
                        && bytes[j] != b'\t'
                        && bytes[j] != b'>'
                    {
                        j += 1;
                    }
                    if start < j {
                        let filename = &command_text[start..j];
                        targets.push(resolve_path(filename, cwd));
                    }
                    i = j;
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    targets
}

// --- PowerShell support ---

/// Known PowerShell aliases that don't match the Verb-Noun pattern.
const PS_ALIASES: &[&str] = &[
    "ri", "mi", "ci", "si", "gc", "gci", "gl", "gi", "sc", "clc", "sls", "del", "erase", "rd",
    "rmdir", "move", "copy",
];

/// Check if a command is a PowerShell cmdlet (Verb-Noun pattern) or known alias.
fn is_powershell_cmdlet(cmd: &str) -> bool {
    let lower = cmd.to_ascii_lowercase();

    // Check known aliases first
    if PS_ALIASES.contains(&lower.as_str()) {
        return true;
    }

    // Check Verb-Noun pattern: contains `-` with letters on both sides
    // e.g., Remove-Item, Get-Content, Set-Content
    if let Some(hyphen_pos) = cmd.find('-') {
        if hyphen_pos > 0 && hyphen_pos < cmd.len() - 1 {
            let before = &cmd[..hyphen_pos];
            let after = &cmd[hyphen_pos + 1..];
            return before.chars().all(|c| c.is_ascii_alphabetic())
                && after.chars().all(|c| c.is_ascii_alphabetic());
        }
    }

    false
}

/// Simple quote-aware tokenizer for PowerShell commands.
/// Splits on whitespace, respects `"` and `'` quotes.
fn tokenize_powershell(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    for ch in text.chars() {
        match ch {
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
                // Don't include the quote character in the token
            }
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            ' ' | '\t' if !in_single_quote && !in_double_quote => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
            _ => {
                current.push(ch);
            }
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Parse a PowerShell cmdlet and extract file modification targets.
fn parse_powershell_command(cmd: &str, args: &[String], cwd: &Path) -> ParseResult {
    let lower = cmd.to_ascii_lowercase();

    match lower.as_str() {
        // Destructive cmdlets
        "remove-item" | "ri" | "del" | "erase" | "rd" | "rmdir" => {
            extract_powershell_path_args(args, cwd)
        }
        "move-item" | "mi" | "move" => extract_powershell_destination_args(args, cwd),
        "copy-item" | "ci" | "copy" => extract_powershell_destination_args(args, cwd),
        "set-content" | "sc" => extract_powershell_path_args(args, cwd),
        "clear-content" | "clc" => extract_powershell_path_args(args, cwd),

        // Read-only cmdlets
        "get-content" | "gc" | "cat" | "type" => ParseResult {
            targets: vec![],
            confidence: Confidence::ReadOnly,
        },
        "get-childitem" | "gci" | "ls" | "dir" => ParseResult {
            targets: vec![],
            confidence: Confidence::ReadOnly,
        },
        "get-location" | "gl" | "pwd" => ParseResult {
            targets: vec![],
            confidence: Confidence::ReadOnly,
        },
        "set-location" | "sl" | "cd" | "chdir" => ParseResult {
            targets: vec![],
            confidence: Confidence::ReadOnly,
        },
        "get-item" | "gi" => ParseResult {
            targets: vec![],
            confidence: Confidence::ReadOnly,
        },
        "test-path" => ParseResult {
            targets: vec![],
            confidence: Confidence::ReadOnly,
        },
        "select-string" | "sls" => ParseResult {
            targets: vec![],
            confidence: Confidence::ReadOnly,
        },

        // Unknown PowerShell cmdlet
        _ => ParseResult {
            targets: vec![],
            confidence: Confidence::Low,
        },
    }
}

/// Extract -Path or -LiteralPath named parameter values, or first positional argument.
fn extract_powershell_path_args(args: &[String], cwd: &Path) -> ParseResult {
    let mut targets = Vec::new();
    let mut i = 0;

    // First pass: look for named -Path or -LiteralPath parameters
    let mut found_named = false;
    while i < args.len() {
        let lower = args[i].to_ascii_lowercase();
        if lower == "-path" || lower == "-literalpath" {
            found_named = true;
            if i + 1 < args.len() {
                targets.push(resolve_path(&args[i + 1], cwd));
                i += 2;
                continue;
            }
        }
        i += 1;
    }

    // If no named parameter found, treat first positional (non-flag) argument as path
    if !found_named {
        for arg in args {
            if !arg.starts_with('-') {
                targets.push(resolve_path(arg, cwd));
                break; // Only take the first positional for path
            }
        }
    }

    ParseResult {
        targets,
        confidence: Confidence::High,
    }
}

/// Extract both -Path and -Destination parameter values (for Move-Item, Copy-Item).
fn extract_powershell_destination_args(args: &[String], cwd: &Path) -> ParseResult {
    let mut targets = Vec::new();
    let mut i = 0;
    let mut found_named_path = false;
    let mut found_named_dest = false;

    // Named parameter scan
    while i < args.len() {
        let lower = args[i].to_ascii_lowercase();
        if lower == "-path" || lower == "-literalpath" {
            found_named_path = true;
            if i + 1 < args.len() {
                targets.push(resolve_path(&args[i + 1], cwd));
                i += 2;
                continue;
            }
        } else if lower == "-destination" {
            found_named_dest = true;
            if i + 1 < args.len() {
                targets.push(resolve_path(&args[i + 1], cwd));
                i += 2;
                continue;
            }
        }
        i += 1;
    }

    // Fallback to positional: first positional = source, second = destination
    if !found_named_path && !found_named_dest {
        let positionals: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
        for p in &positionals {
            targets.push(resolve_path(p, cwd));
        }
    }

    ParseResult {
        targets,
        confidence: Confidence::High,
    }
}

/// Dispatch to per-command parser based on command name.
fn dispatch_command(cmd: &str, args: &[String], cwd: &Path) -> ParseResult {
    match cmd {
        // Destructive commands -- extract file targets
        "rm" | "del" | "unlink" => parse_rm(args, cwd),
        "mv" | "move" | "rename" => parse_mv(args, cwd),
        "cp" | "copy" => parse_cp(args, cwd),
        "sed" => parse_sed(args, cwd),
        "chmod" | "chown" => parse_chmod(args, cwd),
        "git" => parse_git(args, cwd),
        "truncate" => parse_truncate(args, cwd),

        // Read-only commands -- skip snapshot
        "ls" | "dir" | "cat" | "type" | "head" | "tail" | "less" | "more" | "grep" | "rg"
        | "find" | "which" | "where" | "echo" | "printf" | "pwd" | "whoami" | "date" | "env"
        | "set" | "wc" | "file" | "stat" | "df" | "du" | "ps" | "top" | "htop"
        // Shell builtins that never modify files
        | "cd" | "pushd" | "popd" | "dirs" | "export" | "unset" | "source" | "alias"
        | "unalias" | "history" | "exit" | "return" | "true" | "false" | "test" | "help"
        | "builtin" | "command" | "hash" | "ulimit" | "umask" | "times" | "wait"
        | "jobs" | "fg" | "bg" | "kill" | "man" | "info" | "clear" | "reset" | "tput"
        // PowerShell builtins for cd
        | "Set-Location" | "sl" => ParseResult {
            targets: vec![],
            confidence: Confidence::ReadOnly,
        },

        // Everything else -- unknown, rely on watcher
        _ => ParseResult {
            targets: vec![],
            confidence: Confidence::Low,
        },
    }
}

/// Parse `rm [-rfiv] file1 [file2 ...]`
fn parse_rm(args: &[String], cwd: &Path) -> ParseResult {
    let mut targets = Vec::new();

    for arg in args {
        if arg.starts_with('-') {
            continue;
        }
        // If arg contains glob characters, return Low confidence
        if contains_glob(arg) {
            return ParseResult {
                targets: vec![],
                confidence: Confidence::Low,
            };
        }
        targets.push(resolve_path(arg, cwd));
    }

    ParseResult {
        targets,
        confidence: Confidence::High,
    }
}

/// Parse `mv [-fivn] source... dest`
/// All arguments (sources + dest) are targets.
fn parse_mv(args: &[String], cwd: &Path) -> ParseResult {
    let non_flag_args: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();

    if non_flag_args.len() < 2 {
        return ParseResult {
            targets: vec![],
            confidence: Confidence::Low,
        };
    }

    let targets: Vec<PathBuf> = non_flag_args.iter().map(|a| resolve_path(a, cwd)).collect();

    ParseResult {
        targets,
        confidence: Confidence::High,
    }
}

/// Parse `cp [-rfiv] source... dest`
/// Destination file is the target (may be overwritten).
fn parse_cp(args: &[String], cwd: &Path) -> ParseResult {
    let non_flag_args: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();

    if non_flag_args.len() < 2 {
        return ParseResult {
            targets: vec![],
            confidence: Confidence::Low,
        };
    }

    // Destination is the last non-flag argument
    let dest = non_flag_args.last().unwrap();
    let targets = vec![resolve_path(dest, cwd)];

    ParseResult {
        targets,
        confidence: Confidence::High,
    }
}

/// Parse `sed [-i[suffix]] [-e expr] [-f script] 'pattern' file...`
fn parse_sed(args: &[String], cwd: &Path) -> ParseResult {
    let has_inplace = args.iter().any(|a| a == "-i" || a.starts_with("-i"));
    if !has_inplace {
        return ParseResult {
            targets: vec![],
            confidence: Confidence::ReadOnly,
        };
    }

    let mut targets = Vec::new();
    // When -e or -f is used, the expression is supplied as a flag argument,
    // so there is no bare expression to skip among positional args.
    let has_explicit_expr = args
        .iter()
        .any(|a| a == "-e" || a == "-f" || a.starts_with("-e") || a.starts_with("-f"));
    let mut past_expression = has_explicit_expr;
    let mut skip_next = false;

    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg.starts_with('-') {
            // -e and -f take a following argument (unless value is attached: -e's/a/b/')
            if (arg == "-e" || arg == "-f") && arg.len() <= 2 {
                skip_next = true;
            }
            continue;
        }
        if !past_expression {
            past_expression = true; // First non-flag arg is the sed expression
            continue;
        }
        targets.push(resolve_path(arg, cwd));
    }

    let confidence = if targets.is_empty() {
        Confidence::Low
    } else {
        Confidence::High
    };
    ParseResult {
        targets,
        confidence,
    }
}

/// Parse `chmod/chown mode/owner file...`
/// First non-flag argument is mode/owner, rest are files.
fn parse_chmod(args: &[String], cwd: &Path) -> ParseResult {
    let mut targets = Vec::new();
    let mut past_mode = false;

    for arg in args {
        if arg.starts_with('-') {
            continue;
        }
        if !past_mode {
            past_mode = true; // First non-flag arg is mode/owner
            continue;
        }
        targets.push(resolve_path(arg, cwd));
    }

    let confidence = if targets.is_empty() {
        Confidence::Low
    } else {
        Confidence::High
    };
    ParseResult {
        targets,
        confidence,
    }
}

/// Parse git subcommands.
fn parse_git(args: &[String], cwd: &Path) -> ParseResult {
    let subcommand = match args.first() {
        Some(s) => s.as_str(),
        None => {
            return ParseResult {
                targets: vec![],
                confidence: Confidence::Low,
            }
        }
    };

    match subcommand {
        // Destructive subcommands
        "checkout" => parse_git_checkout(&args[1..], cwd),
        "restore" => parse_git_restore(&args[1..], cwd),
        "clean" => parse_git_clean(&args[1..], cwd),
        "reset" => parse_git_reset(&args[1..], cwd),

        // Read-only subcommands
        "status" | "log" | "diff" | "show" | "branch" | "remote" | "fetch" | "stash" | "tag"
        | "blame" | "reflog" => ParseResult {
            targets: vec![],
            confidence: Confidence::ReadOnly,
        },

        // Unknown git subcommands -- could modify files
        _ => ParseResult {
            targets: vec![],
            confidence: Confidence::Low,
        },
    }
}

/// `git checkout -- file1 file2` or `git checkout branch -- file1`
fn parse_git_checkout(args: &[String], cwd: &Path) -> ParseResult {
    // After `--`, everything is a file path
    if let Some(dashdash_pos) = args.iter().position(|a| a == "--") {
        let targets: Vec<PathBuf> = args[dashdash_pos + 1..]
            .iter()
            .filter(|a| !a.starts_with('-'))
            .map(|a| resolve_path(a, cwd))
            .collect();
        return ParseResult {
            targets,
            confidence: Confidence::High,
        };
    }

    // Without `--`, ambiguous (could be branch name or file)
    ParseResult {
        targets: vec![],
        confidence: Confidence::Low,
    }
}

/// `git restore [--source=ref] [--staged] [--worktree] -- file...`
fn parse_git_restore(args: &[String], cwd: &Path) -> ParseResult {
    // After `--`, everything is a file path
    if let Some(dashdash_pos) = args.iter().position(|a| a == "--") {
        let targets: Vec<PathBuf> = args[dashdash_pos + 1..]
            .iter()
            .filter(|a| !a.starts_with('-'))
            .map(|a| resolve_path(a, cwd))
            .collect();
        return ParseResult {
            targets,
            confidence: Confidence::High,
        };
    }

    // Without --, files could be at the end after flags
    let non_flag_args: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    let targets: Vec<PathBuf> = non_flag_args.iter().map(|a| resolve_path(a, cwd)).collect();
    let confidence = if targets.is_empty() {
        Confidence::Low
    } else {
        Confidence::High
    };
    ParseResult {
        targets,
        confidence,
    }
}

/// `git clean [-fdxn]` -- removes untracked files
fn parse_git_clean(_args: &[String], _cwd: &Path) -> ParseResult {
    // git clean operates on untracked files -- we cannot predict which ones
    ParseResult {
        targets: vec![],
        confidence: Confidence::Low,
    }
}

/// `git reset [--hard|--soft|--mixed] [ref] [-- file...]`
fn parse_git_reset(args: &[String], cwd: &Path) -> ParseResult {
    if let Some(dashdash_pos) = args.iter().position(|a| a == "--") {
        let targets: Vec<PathBuf> = args[dashdash_pos + 1..]
            .iter()
            .filter(|a| !a.starts_with('-'))
            .map(|a| resolve_path(a, cwd))
            .collect();
        return ParseResult {
            targets,
            confidence: Confidence::High,
        };
    }

    // git reset --hard is destructive but affects all tracked files
    if args.iter().any(|a| a == "--hard") {
        return ParseResult {
            targets: vec![],
            confidence: Confidence::Low,
        };
    }

    ParseResult {
        targets: vec![],
        confidence: Confidence::Low,
    }
}

/// Parse `truncate [-s size] file...`
fn parse_truncate(args: &[String], cwd: &Path) -> ParseResult {
    let mut targets = Vec::new();
    let mut skip_next = false;

    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg == "-s" || arg == "--size" {
            skip_next = true;
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        targets.push(resolve_path(arg, cwd));
    }

    let confidence = if targets.is_empty() {
        Confidence::Low
    } else {
        Confidence::High
    };
    ParseResult {
        targets,
        confidence,
    }
}

/// Check if a string contains glob characters.
fn contains_glob(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cwd() -> PathBuf {
        PathBuf::from("/home/user/project")
    }

    /// Helper to build expected resolved path.
    /// On Windows, PathBuf::from("/home/user/project").join("foo.txt")
    /// uses backslash separators. This helper matches that behavior.
    fn resolved(relative: &str) -> PathBuf {
        cwd().join(relative)
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
        assert_eq!(result.targets, vec![resolved("foo.txt")]);
    }

    #[test]
    fn test_rm_multiple_files() {
        let result = parse_command("rm foo.txt bar.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert_eq!(
            result.targets,
            vec![resolved("foo.txt"), resolved("bar.txt")]
        );
    }

    #[test]
    fn test_readonly_commands() {
        for cmd in &[
            "ls foo",
            "cat file.txt",
            "grep pattern file",
            "echo hello",
            "pwd",
        ] {
            let result = parse_command(cmd, &cwd());
            assert_eq!(
                result.confidence,
                Confidence::ReadOnly,
                "Expected ReadOnly for '{cmd}'"
            );
            assert!(result.targets.is_empty(), "Expected no targets for '{cmd}'");
        }
    }

    #[test]
    fn test_shell_builtins_are_readonly() {
        for cmd in &[
            "cd /tmp",
            "cd app\\Glass",
            "pushd /tmp",
            "popd",
            "export PATH=/usr/bin",
            "source ~/.bashrc",
            "alias ll='ls -la'",
            "history",
            "exit 0",
            "true",
            "false",
            "clear",
            "kill 1234",
            "jobs",
            "fg %1",
            "bg %1",
            "Set-Location C:\\Users",
        ] {
            let result = parse_command(cmd, &cwd());
            assert_eq!(
                result.confidence,
                Confidence::ReadOnly,
                "Expected ReadOnly for '{cmd}'"
            );
            assert!(result.targets.is_empty(), "Expected no targets for '{cmd}'");
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
        assert_eq!(result.targets, vec![resolved("file with spaces.txt")]);
    }

    #[test]
    fn test_path_resolution() {
        // Relative path resolved against cwd
        let result = parse_command("rm foo.txt", &cwd());
        assert_eq!(result.targets, vec![resolved("foo.txt")]);

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
            vec![resolved("src.txt"), resolved("dst.txt")]
        );
    }

    #[test]
    fn test_cp_dest() {
        let result = parse_command("cp src.txt dst.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert!(result.targets.contains(&resolved("dst.txt")));
    }

    #[test]
    fn test_sed_inplace() {
        // sed -i is destructive
        let result = parse_command("sed -i 's/a/b/' file.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert!(result.targets.contains(&resolved("file.txt")));

        // sed without -i is read-only
        let result = parse_command("sed 's/a/b/' file.txt", &cwd());
        assert_eq!(result.confidence, Confidence::ReadOnly);
    }

    #[test]
    fn test_chmod() {
        let result = parse_command("chmod 755 file.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert!(result.targets.contains(&resolved("file.txt")));
    }

    #[test]
    fn test_git_checkout_with_dashdash() {
        let result = parse_command("git checkout -- file.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert!(result.targets.contains(&resolved("file.txt")));
    }

    #[test]
    fn test_redirect() {
        let result = parse_command("echo hello > out.txt", &cwd());
        assert!(result.targets.contains(&resolved("out.txt")));
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

    // --- PowerShell tests ---

    #[test]
    fn test_powershell_remove_item_named() {
        let result = parse_command("Remove-Item -Path \"file.txt\"", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert_eq!(result.targets, vec![resolved("file.txt")]);
    }

    #[test]
    fn test_powershell_remove_item_positional() {
        let result = parse_command("Remove-Item file.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert_eq!(result.targets, vec![resolved("file.txt")]);
    }

    #[test]
    fn test_powershell_move_item() {
        let result = parse_command("Move-Item -Path src.txt -Destination dst.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert!(result.targets.contains(&resolved("src.txt")));
        assert!(result.targets.contains(&resolved("dst.txt")));
    }

    #[test]
    fn test_powershell_copy_item() {
        let result = parse_command("Copy-Item src.txt dst.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert!(result.targets.contains(&resolved("dst.txt")));
    }

    #[test]
    fn test_powershell_set_content() {
        let result = parse_command("Set-Content -Path file.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert_eq!(result.targets, vec![resolved("file.txt")]);
    }

    #[test]
    fn test_powershell_readonly_cmdlets() {
        for cmd in &[
            "Get-Content file.txt",
            "Get-ChildItem",
            "Test-Path file.txt",
        ] {
            let result = parse_command(cmd, &cwd());
            assert_eq!(
                result.confidence,
                Confidence::ReadOnly,
                "Expected ReadOnly for '{cmd}'"
            );
        }
    }

    #[test]
    fn test_powershell_aliases() {
        // ri = Remove-Item alias
        let result = parse_command("ri file.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert_eq!(result.targets, vec![resolved("file.txt")]);

        // mi = Move-Item alias (needs source and dest)
        let result = parse_command("mi src.txt dst.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert!(result.targets.contains(&resolved("src.txt")));
        assert!(result.targets.contains(&resolved("dst.txt")));
    }

    #[test]
    fn test_powershell_unknown_cmdlet() {
        let result = parse_command("Invoke-CustomScript file.txt", &cwd());
        assert_eq!(result.confidence, Confidence::Low);
    }

    // --- Audit area 4: edge-case tests ---

    #[test]
    fn test_sed_with_explicit_e_flag() {
        // sed -i -e 'expr' file.txt — expression is supplied via -e,
        // so file.txt must be treated as a target, not as the expression.
        let result = parse_command("sed -i -e 's/a/b/' file.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert!(
            result.targets.contains(&resolved("file.txt")),
            "file.txt should be a target when -e is used, got: {:?}",
            result.targets
        );
    }

    #[test]
    fn test_sed_with_multiple_e_flags() {
        let result = parse_command("sed -i -e 's/a/b/' -e 's/c/d/' file.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert!(result.targets.contains(&resolved("file.txt")));
    }

    #[test]
    fn test_sed_with_f_flag() {
        let result = parse_command("sed -i -f script.sed file.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert!(result.targets.contains(&resolved("file.txt")));
    }

    #[test]
    fn test_rm_with_glob_returns_low() {
        let result = parse_command("rm *.txt", &cwd());
        assert_eq!(result.confidence, Confidence::Low);
    }

    #[test]
    fn test_rm_with_flags() {
        let result = parse_command("rm -rf dir/", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert_eq!(result.targets, vec![resolved("dir/")]);
    }

    #[test]
    fn test_mv_single_arg_returns_low() {
        // mv with only one arg is malformed
        let result = parse_command("mv file.txt", &cwd());
        assert_eq!(result.confidence, Confidence::Low);
    }

    #[test]
    fn test_git_status_is_readonly() {
        let result = parse_command("git status", &cwd());
        assert_eq!(result.confidence, Confidence::ReadOnly);
    }

    #[test]
    fn test_git_reset_hard_is_low() {
        let result = parse_command("git reset --hard HEAD", &cwd());
        assert_eq!(result.confidence, Confidence::Low);
    }

    #[test]
    fn test_git_restore_without_dashdash() {
        let result = parse_command("git restore file.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert!(result.targets.contains(&resolved("file.txt")));
    }

    #[test]
    fn test_truncate_with_size_flag() {
        let result = parse_command("truncate -s 0 file.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert!(result.targets.contains(&resolved("file.txt")));
    }

    #[test]
    fn test_redirect_append() {
        let result = parse_command("echo hello >> out.txt", &cwd());
        assert!(result.targets.contains(&resolved("out.txt")));
    }

    #[test]
    fn test_chmod_multiple_files() {
        let result = parse_command("chmod 644 a.txt b.txt", &cwd());
        assert_eq!(result.confidence, Confidence::High);
        assert!(result.targets.contains(&resolved("a.txt")));
        assert!(result.targets.contains(&resolved("b.txt")));
    }
}
