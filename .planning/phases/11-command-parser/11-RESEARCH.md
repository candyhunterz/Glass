# Phase 11: Command Parser - Research

**Researched:** 2026-03-05
**Domain:** Shell command text parsing, file target extraction, POSIX tokenization
**Confidence:** HIGH

## Summary

Phase 11 builds a heuristic command parser that extracts file modification targets from command text. This is explicitly NOT a full shell parser -- the project's Out of Scope section states "Full shell command parser: Shell syntax is Turing-complete; heuristic whitelist approach instead." The parser's role is to provide "bonus" pre-exec snapshots for obvious destructive commands while the FS watcher (Phase 12) serves as the safety net for everything else.

The parser receives command text (already extracted at `CommandExecuted` time per Phase 10's work) and the current working directory (available via `ctx.status.cwd()`). It tokenizes using `shlex` for POSIX shell splitting, identifies the base command against a whitelist of known destructive commands, extracts file arguments based on per-command argument patterns, resolves relative paths to absolute, and returns a `ParseResult` with targets and confidence level.

The key design constraint from STATE.md: "shlex for POSIX tokenization, separate PowerShell tokenizer needed." However, PowerShell tokenizer design was explicitly "deferred to Phase 11." Glass currently runs on Windows, so both bash/zsh (via WSL or Git Bash) and PowerShell commands must be handled. The recommended approach is: detect the shell type from the command syntax, then dispatch to the appropriate tokenizer.

**Primary recommendation:** Build a `CommandParser` module in glass_snapshot with a whitelist of ~15 destructive commands, `shlex` 1.3.0 for POSIX tokenization, a simple hand-rolled PowerShell tokenizer (~30 lines), and path resolution via `std::path::Path::join` + `std::fs::canonicalize`. Cap total parser complexity at ~400 lines.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SNAP-03 | Command text is parsed to identify file targets for pre-exec snapshot (rm, mv, sed -i, cp, chmod, git checkout, etc.) | CommandParser module with per-command extraction functions, shlex tokenization, confidence levels, path resolution against CWD |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| shlex | 1.3.0 | POSIX shell tokenization (split command into words) | Handles quoting (`"foo bar"`, `'hello'`), escaping (`foo\ bar`), multi-word arguments. Zero dependencies. Provides `Shlex` iterator for lazy tokenization. Project decision in STATE.md. |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| anyhow | 1.0.102 (workspace) | Error handling | Fallible path operations |
| tracing | 0.1.44 (workspace) | Logging | Debug logging for parse decisions |
| tempfile | 3 (dev-dependency) | Test infrastructure | Tests needing temp directories for path resolution |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| shlex | shell-words | shell-words forces full allocation upfront; shlex has lazy `Shlex` iterator to inspect command name before parsing all args |
| shlex | tree-sitter-bash | Massive dependency for AST parsing; we need tokenization, not syntax trees |
| shlex | regex-based splitting | Breaks on quoted strings, escaped characters, nested quotes; regex cannot correctly parse shell quoting |
| Hand-rolled PowerShell tokenizer | None available | PowerShell quoting is simple enough (backtick escapes, `"` and `'` quoting) that a 30-line function handles it |

**Installation:**
```toml
# Add to workspace Cargo.toml [workspace.dependencies]
shlex = "1.3.0"

# Add to glass_snapshot/Cargo.toml [dependencies]
shlex = { workspace = true }
```

## Architecture Patterns

### Recommended Project Structure
```
crates/glass_snapshot/
  src/
    lib.rs              # Existing - add re-export of command_parser
    command_parser.rs   # NEW - main module: ParseResult, parse_command(), dispatch
    command_parser/
      posix.rs          # POSIX shell command patterns (rm, mv, sed, cp, chmod, git)
      powershell.rs     # PowerShell command patterns (Remove-Item, Move-Item, etc.)
      path_resolver.rs  # Relative-to-absolute path resolution
    blob_store.rs       # Existing
    db.rs               # Existing
    types.rs            # Existing - extend with ParseResult, Confidence
```

**Alternative (simpler, recommended for Phase 11 scope):** Single `command_parser.rs` file containing all logic. Split into submodules only if the file exceeds ~400 lines. The parser is intentionally simple -- a whitelist of commands with argument extraction. Do not over-architect.

```
crates/glass_snapshot/
  src/
    lib.rs              # Add re-export
    command_parser.rs   # NEW - everything in one file (~300-400 lines)
    blob_store.rs       # Existing
    db.rs               # Existing
    types.rs            # Existing - add ParseResult, Confidence
```

### Pattern 1: Whitelist Command Dispatch
**What:** Match the first token (command name) against a known list of destructive commands, then delegate to per-command argument extraction.
**When to use:** Every call to `parse_command()`.
**Example:**
```rust
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum Confidence {
    /// Known destructive command with clear file targets identified.
    High,
    /// Unknown command or ambiguous targets -- rely on FS watcher.
    Low,
    /// Command is read-only -- no snapshot needed.
    ReadOnly,
}

#[derive(Debug, Clone)]
pub struct ParseResult {
    /// Absolute paths of files the command may modify.
    pub targets: Vec<PathBuf>,
    /// How confident the parser is in its target identification.
    pub confidence: Confidence,
}

pub fn parse_command(command_text: &str, cwd: &Path) -> ParseResult {
    let trimmed = command_text.trim();
    if trimmed.is_empty() {
        return ParseResult { targets: vec![], confidence: Confidence::ReadOnly };
    }

    // Detect shell type and tokenize
    let tokens = tokenize(trimmed);
    if tokens.is_empty() {
        return ParseResult { targets: vec![], confidence: Confidence::Low };
    }

    let cmd = base_command(&tokens[0]);
    let args = &tokens[1..];

    match cmd {
        // Destructive commands -- extract file targets
        "rm" | "del" => parse_rm(args, cwd),
        "mv" | "move" | "rename" => parse_mv(args, cwd),
        "cp" | "copy" => parse_cp(args, cwd),
        "sed" => parse_sed(args, cwd),
        "chmod" | "chown" => parse_chmod(args, cwd),
        "git" => parse_git(args, cwd),
        "truncate" => parse_truncate(args, cwd),

        // Read-only commands -- skip snapshot
        "ls" | "dir" | "cat" | "type" | "head" | "tail" | "less" | "more"
        | "grep" | "rg" | "find" | "which" | "where" | "echo" | "printf"
        | "pwd" | "whoami" | "date" | "env" | "set" | "wc" | "file"
        | "stat" | "df" | "du" | "ps" | "top" | "htop" => {
            ParseResult { targets: vec![], confidence: Confidence::ReadOnly }
        }

        // Git read-only subcommands handled in parse_git
        // Everything else -- unknown, rely on watcher
        _ => ParseResult { targets: vec![], confidence: Confidence::Low },
    }
}

/// Extract the base command name, stripping path prefixes.
/// "/usr/bin/rm" -> "rm", "./script.sh" -> "script.sh"
fn base_command(cmd: &str) -> &str {
    Path::new(cmd)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(cmd)
}
```

### Pattern 2: Per-Command Argument Extraction
**What:** Each destructive command has its own extraction function that understands its argument structure (flags vs file arguments, positional semantics).
**When to use:** After command dispatch identifies a known destructive command.
**Example:**
```rust
/// Parse `rm [-rfiv] file1 [file2 ...]`
fn parse_rm(args: &[String], cwd: &Path) -> ParseResult {
    let mut targets = Vec::new();
    let mut skip_next = false;

    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg.starts_with('-') {
            // Skip flags. rm has no flag-value pairs to worry about.
            continue;
        }
        targets.push(resolve_path(arg, cwd));
    }

    ParseResult {
        targets,
        confidence: Confidence::High,
    }
}

/// Parse `mv [-fivn] source... dest`
/// Source files are modification targets (they get moved/deleted from original location).
fn parse_mv(args: &[String], cwd: &Path) -> ParseResult {
    let non_flag_args: Vec<&String> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .collect();

    if non_flag_args.len() < 2 {
        return ParseResult { targets: vec![], confidence: Confidence::Low };
    }

    // All arguments are targets: source(s) get removed, dest may be overwritten
    let mut targets: Vec<PathBuf> = non_flag_args.iter()
        .map(|a| resolve_path(a, cwd))
        .collect();

    ParseResult {
        targets,
        confidence: Confidence::High,
    }
}

/// Parse `sed [-i[suffix]] 'pattern' file...`
fn parse_sed(args: &[String], cwd: &Path) -> ParseResult {
    let has_inplace = args.iter().any(|a| a == "-i" || a.starts_with("-i"));
    if !has_inplace {
        // sed without -i is read-only (outputs to stdout)
        return ParseResult { targets: vec![], confidence: Confidence::ReadOnly };
    }

    // File arguments come after the pattern. Heuristic: skip flags and the
    // first non-flag argument (the sed expression), remaining are files.
    let mut targets = Vec::new();
    let mut past_expression = false;
    let mut skip_next = false;

    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg.starts_with('-') {
            // -e and -f take a following argument
            if arg == "-e" || arg == "-f" {
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

    ParseResult {
        targets,
        confidence: if targets.is_empty() { Confidence::Low } else { Confidence::High },
    }
}
```

### Pattern 3: Path Resolution
**What:** Convert relative paths from command arguments to absolute paths using the shell's CWD.
**When to use:** Every extracted file path.
**Example:**
```rust
/// Resolve a path argument against the working directory.
/// Does NOT check if the file exists -- the file may not exist yet
/// (e.g., cp destination) or may be deleted by the command.
fn resolve_path(path_str: &str, cwd: &Path) -> PathBuf {
    let path = Path::new(path_str);
    if path.is_absolute() {
        // Already absolute -- normalize but don't canonicalize
        // (file may not exist yet)
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}
```

### Pattern 4: Shell-Aware Tokenization
**What:** Use shlex for POSIX shells, simple hand-rolled splitter for PowerShell.
**When to use:** Before command dispatch.
**Example:**
```rust
fn tokenize(command_text: &str) -> Vec<String> {
    // Try POSIX tokenization first (covers bash, zsh, sh)
    // shlex::split returns None on unterminated quotes -- fall back to
    // whitespace splitting in that case
    shlex::split(command_text).unwrap_or_else(|| {
        command_text.split_whitespace().map(String::from).collect()
    })
}
```

### Pattern 5: Redirect Detection
**What:** Detect output redirections (`>`, `>>`) which modify the target file.
**When to use:** Before tokenization, scan for redirect operators.
**Example:**
```rust
/// Check for output redirections: `> file`, `>> file`, `2> file`
/// These modify the redirect target regardless of the command.
fn extract_redirect_targets(command_text: &str, cwd: &Path) -> Vec<PathBuf> {
    let mut targets = Vec::new();
    // Simple regex-free approach: scan for > not inside quotes
    // This is a heuristic -- handles common cases
    let bytes = command_text.as_bytes();
    let mut i = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' if !in_double_quote => in_single_quote = !in_single_quote,
            b'"' if !in_single_quote => in_double_quote = !in_double_quote,
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
                while j < bytes.len() && bytes[j] != b' ' && bytes[j] != b'\t'
                    && bytes[j] != b'>' && bytes[j] != b'|' && bytes[j] != b';'
                {
                    j += 1;
                }
                if start < j {
                    let filename = &command_text[start..j];
                    targets.push(resolve_path(filename, cwd));
                }
            }
            _ => {}
        }
        i += 1;
    }
    targets
}
```

### Anti-Patterns to Avoid
- **Full shell parsing:** Do not attempt to handle variable expansion (`$var`), command substitution (`$(...)`, backticks), heredocs, process substitution, or arithmetic expansion. These require executing the shell. Return `Confidence::Low` when detected.
- **Glob expansion in the parser:** The shell expands globs before Glass sees the command text in most cases (bash expands `*.txt` before sending to the PTY). Do NOT attempt glob expansion. If you see `*` or `?` in arguments, treat the command as `Confidence::Low` and let the watcher handle it.
- **Parser exceeding 400 lines:** If the parser grows beyond this, you are over-engineering. The v1.2 pitfalls research explicitly warns: "the parser code must not exceed ~300 lines per shell."
- **Coupling to glass_history:** The parser is a pure function: `(command_text, cwd) -> ParseResult`. No database access, no state.
- **Attempting pipeline parsing:** For `cmd1 | cmd2`, only the last command in a pipeline can modify files (stdout of earlier commands goes to stdin, not files). But parsing pipelines adds complexity. Instead, for commands containing `|`, return `Confidence::Low`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| POSIX shell tokenization | Custom quote/escape parser | shlex 1.3.0 | Handles single quotes, double quotes, backslash escapes, multi-word arguments correctly. 15+ years of edge case fixes. |
| Command argument splitting | Regex-based splitter | shlex::split() | Regex cannot correctly parse nested/mixed quoting like `rm "file with 'quotes'"` |
| Path joining | Manual string concatenation | `Path::join()` | Handles OS-specific separators, `..` components, root detection |

**Key insight:** The parser itself is intentionally hand-rolled because no crate provides "extract file targets from shell commands." But the tokenization layer should use shlex, not be hand-rolled.

## Common Pitfalls

### Pitfall 1: Scope Creep -- Trying to Parse Everything
**What goes wrong:** Starting with `rm` and `mv`, then adding pipelines, subshells, loops, variable expansion, eventually spending weeks on a parser that still misses cases.
**Why it happens:** Each new command pattern reveals another shell feature to handle.
**How to avoid:** Hard cap: whitelist of ~15 commands. Unknown commands get `Confidence::Low`. The FS watcher is the safety net. Accept 60-70% coverage of common destructive patterns.
**Warning signs:** Parser file exceeds 400 lines. Adding `if` statement handling. Trying to resolve `$HOME`.

### Pitfall 2: Globs Appearing in Command Text
**What goes wrong:** Parser sees `rm *.txt` and tries to expand the glob, but `*.txt` was already expanded by the shell before Glass captured the command text. The terminal grid shows the expanded form (e.g., `rm a.txt b.txt c.txt`), not `rm *.txt`.
**Why it happens:** Confusion about when glob expansion occurs. The shell expands globs, then Glass reads the command line from the terminal grid. However, in some shells or configurations (like `set -o noglob`), globs pass through unexpanded.
**How to avoid:** Do NOT expand globs. If an argument contains `*`, `?`, or `[`, either: (a) treat it as a literal filename (may work), or (b) mark as `Confidence::Low`. The shell has already expanded globs in the vast majority of cases.
**Warning signs:** Importing a glob crate. Writing glob expansion code.

### Pitfall 3: PowerShell Named Parameters
**What goes wrong:** PowerShell uses `-Path`, `-Destination`, `-Force` named parameters, not positional arguments. `Remove-Item -Path "foo.txt" -Force` requires recognizing `-Path` as the parameter name and the next token as its value.
**Why it happens:** POSIX and PowerShell have fundamentally different argument conventions.
**How to avoid:** Separate PowerShell dispatch path. For PowerShell commands, look for `-Path`, `-LiteralPath`, `-Destination` parameter names and extract their values. Support both positional (first non-flag arg) and named forms.
**Warning signs:** Trying to handle PowerShell with the same flag-skipping logic as POSIX commands.

### Pitfall 4: Commands with Complex Flag-Value Pairs
**What goes wrong:** `cp --target-directory=dest src1 src2` or `chmod --reference=ref file` -- flags that take values via `=` or as the next argument confuse the simple "skip flags starting with -" logic.
**Why it happens:** Not all flags are boolean. Some consume the next argument.
**How to avoid:** For each known command, maintain a list of flags that take values (e.g., `cp`: `--target-directory`, `-t`; `sed`: `-e`, `-f`). Skip the flag AND its value argument.
**Warning signs:** File paths showing up as empty or containing flag characters.

### Pitfall 5: Absolute Paths on Windows vs POSIX
**What goes wrong:** `Path::is_absolute()` on Windows requires a drive letter prefix (`C:\...`). A path like `/usr/bin/file` from WSL bash is not absolute on Windows but IS absolute in the WSL context.
**Why it happens:** Glass runs as a Windows application but the shell inside may be WSL bash.
**How to avoid:** For POSIX-style paths starting with `/` on Windows, treat them as absolute even though `Path::is_absolute()` returns false. Add a helper: `fn is_effectively_absolute(p: &str) -> bool { Path::new(p).is_absolute() || p.starts_with('/') }`.
**Warning signs:** WSL paths being resolved as `C:\Users\...\usr\bin\file` instead of being treated as absolute.

## Code Examples

### Complete parse_command Flow
```rust
// Source: Architecture patterns from project research documents
pub fn parse_command(command_text: &str, cwd: &Path) -> ParseResult {
    let trimmed = command_text.trim();
    if trimmed.is_empty() {
        return ParseResult { targets: vec![], confidence: Confidence::ReadOnly };
    }

    // Check for shell features we cannot parse
    if contains_unparseable_syntax(trimmed) {
        return ParseResult { targets: vec![], confidence: Confidence::Low };
    }

    // Extract redirect targets (applies regardless of command)
    let mut redirect_targets = extract_redirect_targets(trimmed, cwd);

    // Tokenize
    let tokens = tokenize(trimmed);
    if tokens.is_empty() {
        return ParseResult {
            targets: redirect_targets,
            confidence: if redirect_targets.is_empty() { Confidence::Low } else { Confidence::High },
        };
    }

    // Dispatch to per-command parser
    let cmd = base_command(&tokens[0]);
    let args = &tokens[1..];
    let mut result = dispatch_command(cmd, args, cwd);

    // Merge redirect targets
    result.targets.append(&mut redirect_targets);
    result
}

/// Detect syntax we cannot reliably parse.
fn contains_unparseable_syntax(text: &str) -> bool {
    // Variable expansion, command substitution, subshells
    text.contains("$(") || text.contains('`')
        || text.contains("${")
        // Pipelines (last command may write, but parsing is complex)
        || text.contains(" | ")
        // Logical operators (multiple commands)
        || text.contains(" && ") || text.contains(" || ")
        // Semicolons (multiple commands)
        || text.contains(';')
        // Loops and conditionals
        || text.starts_with("for ") || text.starts_with("while ")
        || text.starts_with("if ")
}
```

### git Subcommand Handling
```rust
fn parse_git(args: &[String], cwd: &Path) -> ParseResult {
    let subcommand = match args.first() {
        Some(s) => s.as_str(),
        None => return ParseResult { targets: vec![], confidence: Confidence::Low },
    };

    match subcommand {
        // Destructive subcommands
        "checkout" => parse_git_checkout(&args[1..], cwd),
        "restore" => parse_git_restore(&args[1..], cwd),
        "clean" => parse_git_clean(&args[1..], cwd),
        "reset" => parse_git_reset(&args[1..], cwd),

        // Read-only subcommands
        "status" | "log" | "diff" | "show" | "branch" | "remote"
        | "fetch" | "stash" | "tag" | "blame" | "reflog" => {
            ParseResult { targets: vec![], confidence: Confidence::ReadOnly }
        }

        // Unknown git subcommands -- could modify files
        _ => ParseResult { targets: vec![], confidence: Confidence::Low },
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
        return ParseResult { targets, confidence: Confidence::High };
    }

    // Without `--`, ambiguous (could be branch name or file)
    // Be conservative: return Low confidence
    ParseResult { targets: vec![], confidence: Confidence::Low }
}
```

### PowerShell Command Patterns
```rust
/// Simple PowerShell tokenizer: split on whitespace, respecting quotes.
fn tokenize_powershell(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_double_quote = false;
    let mut in_single_quote = false;

    for ch in text.chars() {
        match ch {
            '"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            '\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
            }
            ' ' | '\t' if !in_double_quote && !in_single_quote => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Parse PowerShell destructive commands.
fn parse_powershell_command(cmd: &str, args: &[String], cwd: &Path) -> ParseResult {
    match cmd.to_lowercase().as_str() {
        "remove-item" | "ri" | "del" | "erase" | "rd" | "rmdir" => {
            extract_powershell_path_args(args, cwd)
        }
        "move-item" | "mi" | "move" => {
            extract_powershell_path_args(args, cwd)
        }
        "copy-item" | "ci" | "copy" => {
            extract_powershell_destination_args(args, cwd)
        }
        "set-content" | "sc" => {
            extract_powershell_path_args(args, cwd)
        }
        "clear-content" | "clc" => {
            extract_powershell_path_args(args, cwd)
        }
        // Read-only
        "get-content" | "gc" | "cat" | "type"
        | "get-childitem" | "gci" | "ls" | "dir"
        | "get-location" | "gl" | "pwd"
        | "get-item" | "gi"
        | "test-path"
        | "select-string" | "sls" => {
            ParseResult { targets: vec![], confidence: Confidence::ReadOnly }
        }
        _ => ParseResult { targets: vec![], confidence: Confidence::Low },
    }
}

/// Extract `-Path` or `-LiteralPath` values, or first positional argument.
fn extract_powershell_path_args(args: &[String], cwd: &Path) -> ParseResult {
    let mut targets = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg.eq_ignore_ascii_case("-Path") || arg.eq_ignore_ascii_case("-LiteralPath") {
            if i + 1 < args.len() {
                targets.push(resolve_path(&args[i + 1], cwd));
                i += 2;
                continue;
            }
        } else if !arg.starts_with('-') {
            // Positional argument -- treat as path
            targets.push(resolve_path(arg, cwd));
        }
        i += 1;
    }
    ParseResult {
        targets,
        confidence: if targets.is_empty() { Confidence::Low } else { Confidence::High },
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Full shell AST parsing | Heuristic whitelist + FS watcher safety net | Project design decision | Bounded complexity, honest about limitations |
| shlex < 1.2.1 | shlex 1.3.0 | 2024 security fix | Safe quoting, no curly brace injection |
| Single parser for all shells | Separate POSIX + PowerShell paths | Project design decision (STATE.md) | Correct handling of fundamentally different syntax |

**Deprecated/outdated:**
- `shlex::quote()` and `shlex::join()` -- deprecated in 1.3.0, replaced by `try_quote()` and `try_join()`. We only use `shlex::split()` which is unaffected.

## Open Questions

1. **Shell type detection: how to know if command is PowerShell vs bash?**
   - What we know: Glass runs on Windows. The shell could be PowerShell, cmd.exe, bash (Git Bash/WSL), or others.
   - What's unclear: Whether Glass tracks which shell is running in the PTY.
   - Recommendation: Heuristic detection from command text. PowerShell cmdlets have a `Verb-Noun` pattern (e.g., `Remove-Item`). If the command matches known PowerShell cmdlets or aliases, use PowerShell parsing. Otherwise, default to POSIX parsing. This handles 95%+ of cases without needing shell type metadata.

2. **Should `echo "text" > file` be High or Low confidence?**
   - What we know: Redirect targets are file modification targets. The redirect `>` creates/overwrites the file.
   - What's unclear: echo is listed as read-only, but with redirect it modifies files.
   - Recommendation: Redirect detection runs independently before command dispatch. If redirects are found, those targets are always included regardless of the base command's classification. The base command is still classified (echo = ReadOnly for its own args), but the overall result is High if redirect targets were found.

3. **WSL paths: should `/home/user/file` be resolved differently on Windows?**
   - What we know: WSL paths like `/home/user/file` start with `/` but are not accessible via the Windows filesystem at that path. They are accessible via `\\wsl$\Ubuntu\home\user\file`.
   - What's unclear: Whether Glass should attempt WSL path translation.
   - Recommendation: Do NOT translate WSL paths. Record them as-is. The SnapshotStore's `store_file` will fail gracefully if the path is not accessible from Windows (the file simply won't be snapshotted). The FS watcher will catch actual modifications. This is an acceptable limitation.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[cfg(test)]` + tempfile 3 |
| Config file | None -- Rust's built-in test harness |
| Quick run command | `cargo test -p glass_snapshot -- command_parser` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SNAP-03 (rm) | `rm foo.txt bar.txt` returns correct file paths | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_rm` | Wave 0 |
| SNAP-03 (mv) | `mv src dst` returns both paths as targets | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_mv` | Wave 0 |
| SNAP-03 (cp) | `cp src dst` returns dst as target | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_cp` | Wave 0 |
| SNAP-03 (sed) | `sed -i 's/a/b/' file` returns file as target | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_sed_inplace` | Wave 0 |
| SNAP-03 (chmod) | `chmod 755 file` returns file as target | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_chmod` | Wave 0 |
| SNAP-03 (git) | `git checkout -- file` returns file as target | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_git_checkout` | Wave 0 |
| SNAP-03 (readonly) | `ls`, `cat`, `grep` return ReadOnly confidence | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_readonly` | Wave 0 |
| SNAP-03 (paths) | Relative paths resolved to absolute against cwd | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_path_resolution` | Wave 0 |
| SNAP-03 (redirect) | `echo x > file.txt` returns file.txt as target | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_redirect` | Wave 0 |
| SNAP-03 (quoted) | `rm "file with spaces.txt"` correctly unquotes | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_quoted_args` | Wave 0 |
| SNAP-03 (powershell) | `Remove-Item -Path "file"` returns file as target | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_powershell` | Wave 0 |
| SNAP-03 (unknown) | Unknown command returns Low confidence | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_unknown_command` | Wave 0 |
| SNAP-03 (unparseable) | Commands with `$()`, pipes return Low confidence | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_unparseable` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_snapshot -- command_parser`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_snapshot/src/command_parser.rs` -- new module with all parser logic + tests
- [ ] Update `crates/glass_snapshot/src/types.rs` -- add `ParseResult`, `Confidence` types
- [ ] Update `crates/glass_snapshot/src/lib.rs` -- add `pub mod command_parser;` and re-exports
- [ ] Update `crates/glass_snapshot/Cargo.toml` -- add `shlex = { workspace = true }`
- [ ] Update root `Cargo.toml` -- add `shlex = "1.3.0"` to `[workspace.dependencies]`

## Sources

### Primary (HIGH confidence)
- Glass v1.1/v1.2 source code -- Direct analysis of glass_snapshot crate (lib.rs, types.rs), main.rs (command text extraction, CWD access)
- Glass .planning/research/ARCHITECTURE.md -- CommandParser design, ParseResult struct, known command patterns, confidence levels
- Glass .planning/research/STACK.md -- shlex 1.3.0 selection, PowerShell tokenizer decision
- Glass .planning/research/PITFALLS.md -- Pitfall 5 (scope creep), parser line limits, shell syntax limitations
- Glass .planning/STATE.md -- "shlex for POSIX tokenization, separate PowerShell tokenizer needed"
- [shlex 1.3.0 docs.rs](https://docs.rs/shlex/latest/shlex/) -- Shlex iterator, split() function, POSIX shell tokenization
- [shlex crates.io](https://crates.io/crates/shlex/1.3.0) -- Version 1.3.0, security fixes

### Secondary (MEDIUM confidence)
- Glass .planning/REQUIREMENTS.md -- Out of Scope: "Full shell command parser: Shell syntax is Turing-complete; heuristic whitelist approach instead"
- [shlex GitHub](https://github.com/comex/rust-shlex) -- API overview, POSIX compatibility

### Tertiary (LOW confidence)
- None.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- shlex is the locked project decision, version verified at 1.3.0
- Architecture: HIGH -- CommandParser design specified in project architecture research, ParseResult struct defined, known command whitelist enumerated
- Pitfalls: HIGH -- directly from project pitfalls research (Pitfall 5), reinforced by Out of Scope requirements

**Research date:** 2026-03-05
**Valid until:** 2026-04-05 (stable domain, no fast-moving dependencies)
