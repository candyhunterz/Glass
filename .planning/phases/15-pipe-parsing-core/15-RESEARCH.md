# Phase 15: Pipe Parsing Core - Research

**Researched:** 2026-03-05
**Domain:** Shell command pipe parsing, TTY detection, buffer management (Rust)
**Confidence:** HIGH

## Summary

Phase 15 fills the `glass_pipes` stub crate with pipe-aware command parsing. The core task is splitting a raw command string like `cat file | grep foo | wc -l` into individual stages while respecting shell quoting rules, then classifying each stage for TTY sensitivity and opt-out flags. This phase also defines the buffer policy types (10MB cap with head/tail sampling) and binary detection for stage data, though actual capture happens in Phase 16.

The project already has strong precedents: `glass_snapshot::command_parser` uses `shlex` for POSIX tokenization and a custom tokenizer for PowerShell. The pipe parser should follow the same pattern -- `shlex` for quote-aware splitting within stages, byte-level scanning for pipe boundary detection. The existing `glass_history::output` module already implements `is_binary()` and `truncate_head_tail()` which can be reused or adapted for stage buffers.

**Primary recommendation:** Build `glass_pipes` with three focused modules: `parser.rs` (pipe splitting + stage extraction), `classify.rs` (TTY exclusion + opt-out flag), and `types.rs` (Pipeline, PipeStage, StageBuffer, BufferPolicy structs). Keep it a pure library crate with no I/O -- all functions take `&str` and return data types.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| PIPE-01 | User's piped commands are detected and parsed into individual stages | `parser.rs` module with `parse_pipeline()` function using byte-level pipe boundary detection with quote awareness |
| PIPE-02 | User can opt out of pipe capture per-command with `--no-glass` flag | `classify.rs` checks for `--no-glass` anywhere in command text before parsing |
| PIPE-03 | TTY-sensitive commands (less, vim, fzf, git log) are auto-excluded from interception | `classify.rs` with static TTY_COMMANDS list; classify each stage and flag pipeline |
| CAPT-03 | Per-stage buffer capped at 10MB with head/tail sampling for overflow | `types.rs` StageBuffer with `append()` and `finalize()` implementing head/tail sampling at 10MB |
| CAPT-04 | Binary data in pipe stages detected and shown as `[binary: <size>]` | Reuse `glass_history::output::is_binary()` pattern; detect in StageBuffer on finalize |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| shlex | 1.3.0 | POSIX shell tokenization within individual stages | Already in workspace; battle-tested for quote-aware splitting |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| (no new deps) | - | All logic is pure Rust string parsing | Phase 15 needs no external crates beyond shlex |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Byte-level pipe scanner | shlex for entire pipeline | shlex does not understand pipe operators -- it treats `|` as a normal char. Must split on pipes FIRST, then tokenize each stage with shlex |
| Full shell parser (e.g., tree-sitter-bash) | Custom byte scanner | Overkill for pipe splitting; project explicitly declares "no full shell parser" in Out of Scope |

**Installation:**
```bash
# No new dependencies needed -- shlex already in workspace
```

## Architecture Patterns

### Recommended Module Structure
```
crates/glass_pipes/src/
  lib.rs           # pub mod declarations, re-exports
  types.rs         # Pipeline, PipeStage, StageBuffer, BufferPolicy, PipelineClassification
  parser.rs        # parse_pipeline() -- pipe boundary detection + stage extraction
  classify.rs      # classify_pipeline() -- TTY detection, --no-glass opt-out
```

### Pattern 1: Two-Phase Parse (Split then Tokenize)
**What:** First scan bytes for unquoted `|` characters to split into stages, then use shlex on each stage for argument extraction.
**When to use:** Always -- shlex cannot detect pipe boundaries; pipes must be found at byte level first.
**Example:**
```rust
/// Split a command string into pipe stages.
/// Respects single quotes, double quotes, and backslash escapes.
/// Returns the raw command text for each stage (trimmed).
pub fn split_pipes(command: &str) -> Vec<&str> {
    let mut stages = Vec::new();
    let mut start = 0;
    let bytes = command.as_bytes();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;

    while i < bytes.len() {
        if escaped {
            escaped = false;
            i += 1;
            continue;
        }
        match bytes[i] {
            b'\\' if !in_single => { escaped = true; }
            b'\'' if !in_double => { in_single = !in_single; }
            b'"' if !in_single => { in_double = !in_double; }
            b'|' if !in_single && !in_double => {
                // Check it's not || (logical OR)
                if i + 1 < bytes.len() && bytes[i + 1] == b'|' {
                    i += 2; // skip ||
                    continue;
                }
                stages.push(command[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    stages.push(command[start..].trim());
    stages
}
```

### Pattern 2: Classification as Separate Pass
**What:** After splitting into stages, classify the entire pipeline: does it contain TTY-sensitive commands? Is `--no-glass` present?
**When to use:** Always -- classification is orthogonal to parsing.
**Example:**
```rust
pub struct PipelineClassification {
    /// Whether any stage contains a TTY-sensitive command
    pub has_tty_command: bool,
    /// Which stages are TTY-sensitive (by index)
    pub tty_stages: Vec<usize>,
    /// Whether --no-glass opt-out flag is present
    pub opted_out: bool,
    /// Whether the pipeline should be captured
    pub should_capture: bool,
}

pub fn classify_pipeline(stages: &[PipeStage]) -> PipelineClassification {
    // ...
}
```

### Pattern 3: StageBuffer with Sampling Policy
**What:** A buffer that accumulates bytes up to a limit, then switches to head+tail sampling mode.
**When to use:** For CAPT-03 -- the actual buffering happens in Phase 16's capture engine, but the data type and policy live here.
**Example:**
```rust
pub struct StageBuffer {
    head: Vec<u8>,       // First N bytes
    tail: Vec<u8>,       // Last N bytes (ring buffer)
    total_bytes: usize,  // Total bytes seen
    max_bytes: usize,    // Policy limit (default 10MB)
    overflow: bool,      // Whether we exceeded max_bytes
}

impl StageBuffer {
    pub fn new(max_bytes: usize) -> Self { /* ... */ }
    pub fn append(&mut self, data: &[u8]) { /* ... */ }
    pub fn finalize(self) -> FinalizedBuffer { /* ... */ }
}

pub enum FinalizedBuffer {
    Complete(Vec<u8>),
    Sampled { head: Vec<u8>, tail: Vec<u8>, total_bytes: usize },
    Binary { size: usize },
}
```

### Anti-Patterns to Avoid
- **Parsing pipes with regex:** Shell quoting makes regex unreliable. Use byte-level state machine.
- **Using shlex::split on the whole pipeline:** shlex treats `|` as a regular character, not a pipe boundary. It will merge `grep foo | wc` into tokens `["grep", "foo", "|", "wc"]` which loses stage boundaries.
- **Coupling parser to I/O:** The parser should be a pure function `(&str) -> Pipeline`. Buffer management is a data type, not an I/O operation. Actual capture wiring happens in Phase 16.
- **Hardcoding buffer size:** Use a configurable `BufferPolicy` struct that defaults to 10MB. Phase 19 will wire this to `[pipes]` config.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| POSIX shell tokenization | Custom whitespace splitter | `shlex::split()` | Handles single/double quotes, backslash escapes, edge cases |
| Binary detection | New implementation | Adapt `glass_history::output::is_binary()` pattern | Already proven in the codebase; same 8KB sample + 30% threshold |
| Head/tail text truncation | New implementation | Adapt `glass_history::output::truncate_head_tail()` pattern | Already handles UTF-8 boundaries correctly |

**Key insight:** The existing `glass_history::output` module solves binary detection and truncation for command output. Stage buffers need the same logic but at the byte level (pre-UTF8 conversion), so adapt rather than import directly.

## Common Pitfalls

### Pitfall 1: Confusing `|` and `||`
**What goes wrong:** Pipe splitter treats `||` (logical OR) as a pipe boundary, splitting `cmd1 || cmd2` into stages.
**Why it happens:** Byte-level scanner that only checks for `|` without lookahead.
**How to avoid:** When a `|` is found, peek at the next byte. If it's also `|`, skip both bytes (it's logical OR, not a pipe).
**Warning signs:** Tests with `echo a || echo b` should produce 1 stage (the whole command), not 2.

### Pitfall 2: Backslash-escaped pipes in POSIX
**What goes wrong:** `echo "hello \| world"` is treated as a pipe.
**Why it happens:** Backslash escape handling missing from the scanner.
**How to avoid:** Track an `escaped` flag. When `\` is seen outside single quotes, set escaped=true and skip the next character.
**Warning signs:** Tests with escaped pipes should produce 1 stage.

### Pitfall 3: PowerShell pipe semantics differ
**What goes wrong:** PowerShell uses `|` for object pipeline, which behaves differently from text pipe.
**Why it happens:** Same character, different semantics.
**How to avoid:** The parser's job is only to split on `|` boundaries -- semantic differences are Phase 16's problem (PowerShell capture uses Tee-Object, not tee). The parser should work identically for both shells at the syntax level.
**Warning signs:** No special PowerShell handling needed in the parser itself.

### Pitfall 4: Process substitution and subshells
**What goes wrong:** `cat <(ls) | grep foo` -- the `<(ls)` contains potential pipe-like syntax.
**Why it happens:** Process substitution `<()` and `>()` create subshell contexts.
**How to avoid:** For v1.3, process substitution decomposition is explicitly deferred (PIPE-04 in Future Requirements). The parser should treat `<(...)` and `>(...)` as opaque text within a stage. Parentheses don't need special handling since pipes inside `$(...)` or backticks are inside subshells and won't be at the top level -- but subshells `(cmd1 | cmd2)` should ideally not split. For v1.3, a simple heuristic: only split on `|` that is NOT inside parentheses, or accept the limitation and document it.
**Warning signs:** Commands with `$(cmd | grep)` or `<(cmd)` should not cause incorrect stage splits.

### Pitfall 5: TTY command list incompleteness
**What goes wrong:** Missing a TTY-sensitive command causes Glass to try capturing its output via tee, which breaks the command's terminal interaction.
**Why it happens:** The list of TTY-sensitive commands is manually maintained.
**How to avoid:** Start with a comprehensive list and make it configurable. Include: `less`, `more`, `vim`, `vi`, `nvim`, `nano`, `emacs`, `fzf`, `htop`, `top`, `man`, `git log`, `git diff` (when pager enabled), `ssh`, `tmux`, `screen`. Note: `git log` and `git diff` use a pager by default so they are TTY-sensitive.
**Warning signs:** Running piped commands through Glass that include any interactive program should be flagged.

## Code Examples

### Pipeline Data Types
```rust
/// A parsed pipeline with its stages and classification.
#[derive(Debug, Clone)]
pub struct Pipeline {
    /// Original full command text
    pub raw_command: String,
    /// Individual pipe stages
    pub stages: Vec<PipeStage>,
    /// Classification result
    pub classification: PipelineClassification,
}

/// A single stage in a pipeline (one command between pipe operators).
#[derive(Debug, Clone)]
pub struct PipeStage {
    /// The raw command text for this stage (trimmed)
    pub command: String,
    /// Index of this stage (0-based)
    pub index: usize,
    /// The base command name (first token, path-stripped)
    pub program: String,
    /// Whether this stage's program is TTY-sensitive
    pub is_tty: bool,
}
```

### TTY Command Detection
```rust
/// Commands that require a TTY and should not have their I/O intercepted.
const TTY_COMMANDS: &[&str] = &[
    "less", "more", "most",
    "vim", "vi", "nvim", "nano", "emacs", "emacsclient",
    "fzf", "sk",                    // fuzzy finders
    "htop", "top", "btop", "gtop",  // system monitors
    "man",                          // pager
    "ssh", "mosh",                  // remote shells
    "tmux", "screen", "zellij",     // multiplexers
    "python", "python3", "ipython", "node", // REPLs (when no args)
    "psql", "mysql", "sqlite3",     // database CLIs
    "gdb", "lldb",                  // debuggers
];

/// Git subcommands that invoke a pager by default.
const TTY_GIT_SUBCOMMANDS: &[&str] = &[
    "log", "diff", "show", "blame", "reflog",
];

fn is_tty_command(program: &str, args: &[&str]) -> bool {
    let base = program.rsplit(['/', '\\']).next().unwrap_or(program);
    if TTY_COMMANDS.contains(&base) {
        return true;
    }
    // Special case: git with pager subcommands
    if base == "git" {
        if let Some(sub) = args.first() {
            return TTY_GIT_SUBCOMMANDS.contains(sub);
        }
    }
    false
}
```

### --no-glass Flag Detection
```rust
/// Check if a command contains the --no-glass opt-out flag.
/// Checks the raw command text before any parsing (flag could be anywhere).
pub fn has_opt_out(command: &str) -> bool {
    // Simple text scan -- --no-glass must appear as a separate token
    command.split_whitespace().any(|token| token == "--no-glass")
}
```

### StageBuffer with Head/Tail Sampling
```rust
const DEFAULT_MAX_BYTES: usize = 10 * 1024 * 1024; // 10MB
const SAMPLE_SIZE: usize = 512 * 1024; // 512KB for head and tail each

impl StageBuffer {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            head: Vec::new(),
            tail: Vec::new(),
            total_bytes: 0,
            max_bytes,
            overflow: false,
        }
    }

    pub fn append(&mut self, data: &[u8]) {
        self.total_bytes += data.len();

        if !self.overflow {
            self.head.extend_from_slice(data);
            if self.head.len() > self.max_bytes {
                // Transition to overflow mode
                self.overflow = true;
                // Keep first SAMPLE_SIZE as head
                self.head.truncate(SAMPLE_SIZE);
                // Start collecting tail
                self.tail = data[data.len().saturating_sub(SAMPLE_SIZE)..].to_vec();
            }
        } else {
            // In overflow mode, maintain rolling tail buffer
            self.tail.extend_from_slice(data);
            if self.tail.len() > SAMPLE_SIZE {
                let excess = self.tail.len() - SAMPLE_SIZE;
                self.tail.drain(..excess);
            }
        }
    }

    pub fn finalize(self) -> FinalizedBuffer {
        // Check for binary content
        let check_data = if self.overflow { &self.head } else { &self.head };
        if is_binary_data(check_data) {
            return FinalizedBuffer::Binary { size: self.total_bytes };
        }

        if self.overflow {
            FinalizedBuffer::Sampled {
                head: self.head,
                tail: self.tail,
                total_bytes: self.total_bytes,
            }
        } else {
            FinalizedBuffer::Complete(self.head)
        }
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `contains_unparseable_syntax()` rejects pipes | New: pipes are parsed into stages | Phase 15 | The command_parser in glass_snapshot currently returns Low confidence for commands with `\| `. Phase 15 introduces pipe-aware parsing in glass_pipes. The snapshot parser remains unchanged -- it still sees pipes as unparseable (by design). |

**Important design note:** The existing `glass_snapshot::command_parser::contains_unparseable_syntax()` explicitly rejects commands containing ` | `. This is correct for the snapshot use case (can't reliably determine file targets across piped commands). The new `glass_pipes` parser complements it -- it focuses on stage splitting, not file target extraction.

## Open Questions

1. **Should `--no-glass` be stripped from the command before execution?**
   - What we know: The flag needs to be detected. The roadmap says "opts it out of pipe interception."
   - What's unclear: Should the shell integration script strip it before passing to the shell? If not, commands like `ls --no-glass | grep foo` would fail because `ls` doesn't recognize `--no-glass`.
   - Recommendation: The shell integration script (Phase 16) should strip `--no-glass` before execution. Phase 15 just detects its presence. Document this for Phase 16 planner.

2. **Parenthesized subshells containing pipes**
   - What we know: `(cmd1 | cmd2) | cmd3` has a pipe inside parentheses that shouldn't create a stage boundary at the top level.
   - What's unclear: How common this pattern is. Full parenthesis tracking adds complexity.
   - Recommendation: Track parenthesis depth in the byte scanner. Only split on `|` at depth 0. Low implementation cost, prevents incorrect splits.

3. **PowerShell backtick escaping for pipes**
   - What we know: PowerShell uses backtick (`` ` ``) as escape character, not backslash. `` echo `| `` would be an escaped pipe.
   - What's unclear: Whether the same parser should handle both or use shell-type dispatch.
   - Recommendation: Add backtick escape handling alongside backslash. Since the parser only does syntax-level splitting, both escape characters can be handled in a single pass. The parser doesn't need to know which shell is active -- both `\|` and `` `| `` are escaped pipes.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[cfg(test)]` + cargo test |
| Config file | None needed -- Cargo.toml handles it |
| Quick run command | `cargo test -p glass_pipes` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| PIPE-01 | Piped commands parsed into stages | unit | `cargo test -p glass_pipes -- parser` | No -- Wave 0 |
| PIPE-02 | --no-glass flag detection | unit | `cargo test -p glass_pipes -- classify::opt_out` | No -- Wave 0 |
| PIPE-03 | TTY-sensitive command detection | unit | `cargo test -p glass_pipes -- classify::tty` | No -- Wave 0 |
| CAPT-03 | 10MB buffer with head/tail sampling | unit | `cargo test -p glass_pipes -- buffer` | No -- Wave 0 |
| CAPT-04 | Binary data detection in stages | unit | `cargo test -p glass_pipes -- buffer::binary` | No -- Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_pipes`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_pipes/src/types.rs` -- Pipeline, PipeStage, StageBuffer, FinalizedBuffer types
- [ ] `crates/glass_pipes/src/parser.rs` -- parse_pipeline() with #[cfg(test)] module
- [ ] `crates/glass_pipes/src/classify.rs` -- classify_pipeline() with #[cfg(test)] module
- [ ] `crates/glass_pipes/Cargo.toml` -- needs shlex dependency added

## Sources

### Primary (HIGH confidence)
- `crates/glass_snapshot/src/command_parser.rs` -- existing pipe-as-unparseable pattern, shlex usage, quote-aware byte scanning
- `crates/glass_history/src/output.rs` -- existing binary detection (`is_binary()`) and head/tail truncation (`truncate_head_tail()`)
- `crates/glass_terminal/src/block_manager.rs` -- Block struct and command lifecycle
- `src/main.rs:744-774` -- command text extraction from terminal grid at CommandExecuted time
- `.planning/REQUIREMENTS.md` -- PIPE-01/02/03, CAPT-03/04 requirement definitions
- `.planning/ROADMAP.md` -- Phase 15 scope and Phase 16 downstream expectations

### Secondary (MEDIUM confidence)
- `Cargo.toml` workspace -- shlex 1.3.0, confirming workspace dependency management
- `.planning/PROJECT.md` -- "shlex for POSIX, custom for PowerShell" decision

### Tertiary (LOW confidence)
- TTY command list -- assembled from common knowledge; should be validated against actual terminal behavior. Some commands (python, node) are only TTY-sensitive when run without arguments.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - no new dependencies, shlex already proven in codebase
- Architecture: HIGH - follows established crate patterns (pure library, types.rs + logic modules)
- Pitfalls: HIGH - pipe parsing edge cases well-understood from existing command_parser.rs code
- TTY command list: MEDIUM - comprehensive but may miss niche commands; designed to be extensible

**Research date:** 2026-03-05
**Valid until:** 2026-04-05 (stable domain, no fast-moving dependencies)
