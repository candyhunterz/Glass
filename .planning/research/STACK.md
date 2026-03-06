# Stack Research: Pipe Visualization

**Project:** Glass v1.3 -- Pipe Visualization
**Researched:** 2026-03-05
**Confidence:** HIGH (approach is custom code on existing crates, not new heavy dependencies)

## Scope

This document covers ONLY what is needed for v1.3 (pipe visualization: tee-based capture for bash/zsh, post-hoc capture for PowerShell, pipeline UI blocks, MCP tool). The existing validated stack is unchanged. v1.2 dependencies (notify, blake3, shlex) are already in the workspace.

---

## Key Finding: No New Heavy Dependencies Needed

Pipe visualization is primarily a **feature built on existing infrastructure**, not a dependency-heavy addition. The core work is:

1. Shell command rewriting (bash/zsh): string manipulation using existing `shlex` crate
2. PowerShell post-hoc capture: shell integration script changes (PowerShell code, not Rust)
3. Binary detection in pipe stages: already implemented in `glass_history::output::is_binary()`
4. Pipe stage storage: extend existing SQLite schema (existing `rusqlite`)
5. Pipeline UI blocks: extend existing renderer (existing `wgpu`/`glyphon`)
6. MCP tool: extend existing MCP server (existing `rmcp`)

---

## Existing Stack (Already in Workspace -- Reuse for v1.3)

| Technology | Version | How v1.3 Uses It |
|------------|---------|------------------|
| shlex | 1.3.0 | Split pipe commands at `|` boundaries, tokenize each stage for tee insertion |
| rusqlite | 0.38.0 | New `pipe_stages` table for intermediate output storage |
| blake3 | 1.8.3 | Hash pipe stage output for dedup in blob store (reuse CAS from v1.2) |
| strip-ansi-escapes | 0.2.1 | Strip ANSI from captured pipe stage output before storage |
| chrono | 0.4 | Timestamps on pipe stage records |
| tokio | 1.50.0 | Async temp file cleanup, MCP tool handlers |
| tempfile | 3.26.0 | Temp files for tee capture targets (already a dev-dep, promote to regular dep for glass_pipes) |
| rmcp | 1.1.0 | GlassPipeInspect MCP tool |
| serde | 1.0.228 | Serialize pipe stage data for MCP responses |
| clap | 4.5 | No new subcommands needed (pipe inspection is via MCP and UI) |

---

## New Dependencies for v1.3

### Required: Promote tempfile to Regular Dependency

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| tempfile | 3.26.0 | Named temp files for tee capture targets | Tee-based capture writes intermediate pipe stage output to temp files. `tempfile::NamedTempFile` provides auto-cleanup on drop, unique names (no collision between concurrent commands), and is already in the workspace as a dev-dep. Promote to regular dep in glass_pipes. |

**Confidence:** HIGH -- tempfile is the standard Rust crate for temp files. 400M+ downloads. Already in the workspace.

### No Other New Crates Needed

The v1.3 feature set does not require any crate not already in the workspace. This is deliberate -- pipe visualization is a feature layer built on top of existing infrastructure.

---

## Approach by Shell: How Pipe Capture Works

### Bash/Zsh: Tee-Based Transparent Capture

**Mechanism:** Rewrite the user's piped command before execution by inserting `tee <tmpfile>` between each pipe stage.

**Example rewrite:**
```
Original:  cat data.csv | grep ERROR | sort | head -5
Rewritten: cat data.csv | tee /tmp/glass_pipe_abc123_s0 | grep ERROR | tee /tmp/glass_pipe_abc123_s1 | sort | tee /tmp/glass_pipe_abc123_s2 | head -5
```

**Implementation (in glass.bash shell integration):**

The shell integration script hooks into bash's `PS0` (pre-execution). When a pipe is detected in the command text:
1. Parse command at `|` boundaries (quote-aware splitting)
2. Generate temp file paths via a naming convention (not Rust tempfile -- this is in bash)
3. Insert `tee <tmpfile>` after each stage except the last
4. Emit a custom OSC sequence with the temp file paths so Glass can collect them after command completion
5. On `PROMPT_COMMAND` (post-execution), Glass reads the temp files and stores stage output

**Why tee, not process substitution:**
- `tee` is POSIX, works in bash and zsh identically
- Process substitution (`>(...)`) is bash-specific, not available in sh
- `tee` preserves the pipeline exit code chain correctly
- `tee` is a separate process -- it does not buffer the entire stage output in memory

**Temp file naming convention (in bash, not Rust):**
```bash
# Pattern: /tmp/glass_pipe_{command_epoch}_{stage_index}
__glass_pipe_dir="/tmp/glass_pipes_$$"
mkdir -p "$__glass_pipe_dir"
```

**How Glass reads the captures:** After command completion (OSC 133;D), Glass reads the temp files listed in the pre-exec OSC metadata, processes them through the existing output pipeline (binary detection, ANSI stripping, truncation), stores to pipe_stages table, and cleans up temp files.

**shlex for pipe splitting in Rust (validation/fallback):**
```rust
/// Split a command string at unquoted pipe characters.
/// Uses shlex for quote awareness, custom logic for pipe detection.
fn split_pipes(command: &str) -> Vec<String> {
    let mut stages = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let bytes = command.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' if !in_double => { in_single = !in_single; current.push('\''); }
            b'"' if !in_single => { in_double = !in_double; current.push('"'); }
            b'|' if !in_single && !in_double => {
                // Check for || (logical OR) -- not a pipe
                if i + 1 < bytes.len() && bytes[i + 1] == b'|' {
                    current.push_str("||");
                    i += 2;
                    continue;
                }
                stages.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(bytes[i] as char),
        }
        i += 1;
    }
    if !current.trim().is_empty() {
        stages.push(current.trim().to_string());
    }
    stages
}
```

### PowerShell: Post-Hoc Variable Capture

**Mechanism:** PowerShell pipelines pass .NET objects, not byte streams. Tee insertion would change behavior (objects get serialized to text). Instead, use `Tee-Object -Variable` to capture each stage into a PowerShell variable, then emit the captured data via a custom OSC sequence after command completion.

**Example rewrite:**
```powershell
# Original:
Get-Process | Where-Object { $_.CPU -gt 100 } | Sort-Object CPU

# Rewritten (transparent):
Get-Process | Tee-Object -Variable __glass_s0 | Where-Object { $_.CPU -gt 100 } | Tee-Object -Variable __glass_s1 | Sort-Object CPU
# Post-execution: emit captured variables via OSC
```

**Key difference from bash:** PowerShell pipelines pass objects. `Tee-Object -Variable` captures the object array at each stage. For Glass display, we call `.ToString()` or `Out-String` on captured variables to get text representation.

**Implementation (in glass.ps1 shell integration):**

The PSReadLine Enter handler detects pipes, rewrites with `Tee-Object -Variable`, and after execution, the prompt function reads the variables and emits them via OSC or writes to temp files.

**Why post-hoc, not pre-exec temp files:**
- PowerShell objects lose fidelity when serialized to files mid-pipeline
- `Tee-Object -Variable` preserves the object pipeline exactly
- Variable capture adds negligible overhead
- The variable contents are serialized to text only at collection time (in the prompt function)

### TTY-Sensitive Command Detection

**Commands that break when tee is inserted:**
- Interactive editors: `vim`, `nano`, `emacs`, `vi`
- Pagers: `less`, `more`, `bat` (when interactive)
- Terminal UIs: `htop`, `top`, `ncdu`, `fzf`
- Password prompts: `sudo`, `ssh`, `passwd`
- REPLs: `python`, `node`, `irb` (when interactive)

**Detection approach:** Maintain a hardcoded denylist in the shell integration script. Before rewriting, check if any pipe stage contains a denylisted command. If so, skip tee insertion entirely for that pipeline.

```bash
__GLASS_TTY_COMMANDS="vim|vi|nano|emacs|less|more|bat|htop|top|ncdu|fzf|sudo|ssh|passwd"

__glass_has_tty_command() {
    local cmd="$1"
    echo "$cmd" | grep -qE "(^|\|)\s*($__GLASS_TTY_COMMANDS)(\s|$)"
}
```

**Opt-out flag:** Allow users to prefix with `# nopipe` or set a shell variable to disable pipe capture for a specific command.

---

## Binary Detection in Pipe Stage Output

**Already implemented:** `glass_history::output::is_binary()` samples the first 8KB and checks if >30% of bytes are non-printable (excluding `\n`, `\r`, `\t`). This is sufficient for pipe stage output.

**Enhancement for v1.3:** The existing 30% threshold is conservative. For pipe stages, the same function works because:
- Text pipelines (grep, awk, sed, sort) produce text output
- Binary pipelines (tar, gzip, openssl) produce binary output
- Mixed pipelines (strings on binary) produce text from binary input

**content_inspector crate (NOT recommended):**
- v0.2.4, last updated 7+ years ago
- Uses NULL-byte detection only (checks first 1024 bytes)
- Glass's existing `is_binary()` is actually more sophisticated (30% threshold on 8KB sample vs NULL scan on 1KB)
- Adding a dependency for a simpler algorithm would be a downgrade

**Recommendation:** Reuse `glass_history::output::is_binary()` directly. Extract it to a shared utility if glass_pipes needs it without depending on glass_history.

---

## DB Schema Extension

Extend the existing history database with a new migration (v2 -> v3):

```sql
-- v3 migration: pipe stage storage
CREATE TABLE pipe_stages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    command_id INTEGER NOT NULL REFERENCES commands(id) ON DELETE CASCADE,
    stage_index INTEGER NOT NULL,       -- 0-based position in pipeline
    stage_command TEXT NOT NULL,         -- the command text for this stage
    output TEXT,                         -- captured text output (NULL if binary/unavailable)
    output_bytes INTEGER,               -- original byte count before truncation
    is_binary INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    UNIQUE(command_id, stage_index)
);

CREATE INDEX idx_pipe_stages_command ON pipe_stages(command_id);
```

**Why not store in blob store:** Pipe stage output is ephemeral display data, not file content for restoration. It belongs in SQLite directly (simpler queries, automatic cleanup via CASCADE delete, retention policy integration).

**Size management:** Reuse the existing `process_output()` function with the same `max_kb` truncation. Each stage output is capped at the configured limit (default 50KB). Total per-pipeline cap = stages * 50KB.

---

## Crate Architecture for glass_pipes

### glass_pipes/Cargo.toml

```toml
[package]
name = "glass_pipes"
version = "0.1.0"
edition = "2021"

[dependencies]
rusqlite = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
chrono = { workspace = true }
serde = { workspace = true }
tempfile = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
```

### Crate Responsibility

```
glass_terminal (detects pipe in command text via OSC 133;C pre-exec)
    |
    v  command text + metadata via AppEvent::PipelineDetected
glass_pipes (pipe parsing, stage management, DB storage)
    |-- pipe_parser.rs    -- split_pipes(), is_tty_sensitive(), generate_rewrite()
    |-- stage_store.rs    -- SQLite storage for pipe_stages table
    |-- capture.rs        -- read temp files, process output, store stages
    |
    v  pipe stage data via query API
glass_mcp (GlassPipeInspect tool)
glass_renderer (pipeline UI blocks)
```

### Shared Utilities

The `is_binary()` and `process_output()` functions in `glass_history::output` are needed by glass_pipes. Two options:

**Option A (RECOMMENDED): glass_pipes depends on glass_history**
- Import `glass_history::output::{is_binary, process_output, strip_ansi}`
- Matches the v1.2 pattern where glass_snapshot depends on glass_history
- These functions are pure (no DB access), so the coupling is minimal

**Option B: Extract to a shared glass_common crate**
- Move output processing to `glass_common::output`
- Both glass_history and glass_pipes depend on glass_common
- Cleaner but adds a new crate for 3 functions
- Defer this refactor unless the dependency graph becomes unwieldy

---

## Workspace Changes

### Root Cargo.toml

```toml
[workspace.dependencies]
# v1.3: Promote tempfile from dev-only to regular dependency
tempfile = "3"
# All other deps already in workspace -- no additions needed
```

### Root binary Cargo.toml

```toml
[dependencies]
# Add to existing:
glass_pipes = { path = "crates/glass_pipes" }
```

---

## Alternatives Considered

| Category | Recommended | Alternative | Why Not |
|----------|-------------|-------------|---------|
| Pipe splitting | Custom quote-aware splitter (~40 LOC) | shlex + post-process | shlex does not expose pipe boundaries. It treats `|` as a regular character. Custom splitter is simple and correct. |
| Pipe splitting | Custom splitter | tree-sitter-bash | Massive dependency. We need to find `|` outside quotes, not build an AST. |
| Pipe splitting | Custom splitter | regex `\|` | Breaks on pipes inside quoted strings: `echo "a|b" | grep a`. Must be quote-aware. |
| Bash capture | tee insertion | Process substitution `>(cat > file)` | Bash-specific (not POSIX), more complex rewrite, same result. |
| Bash capture | tee to temp files | Named pipes (mkfifo) | Race conditions if consumer is slower than producer. Temp files are simpler and sufficient. |
| Bash capture | Shell-side rewrite | Rust-side PTY interception | Would require spawning sub-PTYs for each pipe stage. Enormously complex, fragile, and not portable. Shell-side tee insertion is the standard approach used by pipe debuggers. |
| PowerShell capture | Tee-Object -Variable | Trace-Command PipelineExecution | Trace-Command output is engine debug text, not the actual pipeline data. Unparseable for display. |
| PowerShell capture | Tee-Object -Variable | Out-File per stage | Serializes objects to text mid-pipeline, changing behavior (Format-Table vs raw objects). |
| PowerShell capture | Post-hoc variable read | Real-time streaming | PowerShell variables are available only after pipeline completion. Real-time would require custom cmdlets. Not worth the complexity. |
| Binary detection | Existing is_binary() (8KB, 30%) | content_inspector crate (v0.2.4) | 7+ years unmaintained. NULL-byte only detection on 1KB. Our existing implementation is better. |
| Stage storage | SQLite pipe_stages table | Blob store (files) | Pipe output is small, ephemeral, query-friendly. SQLite is the right tool. Blob store is for large file snapshots. |
| Temp file management | tempfile crate | Manual /tmp files | tempfile handles unique naming, auto-cleanup, and cross-platform temp dirs. Already in workspace. |
| Temp file management | tempfile crate | In-memory buffers | Pipe stages can be arbitrarily large. Temp files avoid memory pressure. |

---

## What NOT to Add

| Temptation | Why Not |
|------------|---------|
| content_inspector | Existing `is_binary()` is more sophisticated. Adding a dependency for a downgrade. |
| shell-words (for pipe parsing) | Same limitation as shlex -- neither understands pipe operators. Custom splitter needed regardless. |
| nix crate (for mkfifo/named pipes) | Named pipes are unnecessary. Temp files are simpler and sufficient for batch capture. |
| pty-process or portable-pty | Sub-PTY spawning for each pipe stage would be the "correct" Unix approach but is massively over-engineered for this use case. |
| serde_json (for MCP pipe data) | Already available transitively through rmcp. No direct dependency needed in glass_pipes. |
| tree-sitter-bash | AST parsing for pipe detection is like using a chainsaw to cut butter. |
| A PowerShell module (.psm1) | Keep all PowerShell integration in the single glass.ps1 script for simplicity. |
| async-tempfile | Temp files are written by the shell (tee), read once by Glass, then deleted. No async needed. |

---

## Version Compatibility

| Package | Compatible With | Notes |
|---------|-----------------|-------|
| tempfile 3.26.0 | tokio 1.x, rusqlite 0.38.0 | No conflicts. Pure std::fs operations. |
| All existing workspace deps | Unchanged | No version bumps needed for v1.3. |

---

## Compile & Binary Size Impact

| Change | Compile Impact | Binary Size | Notes |
|--------|---------------|-------------|-------|
| glass_pipes crate (new code) | MODERATE (new crate) | ~50 KB | Pure Rust logic: pipe parsing, DB queries, file I/O |
| tempfile promotion | NONE | Already compiled | Already in workspace as dev-dep; same binary |
| **Total v1.3 addition** | **~50 KB new code** | Negligible. No new external dependencies. |

---

## Shell Integration Script Changes

### glass.bash additions

```bash
# Pipe detection and tee insertion
__glass_rewrite_pipes() {
    local cmd="$1"
    # Skip if no pipes or TTY-sensitive command
    [[ "$cmd" != *"|"* ]] && return 1
    __glass_has_tty_command "$cmd" && return 1

    # Create capture directory for this command
    local epoch=$(date +%s%N)
    __glass_pipe_dir="/tmp/glass_pipes_${epoch}"
    mkdir -p "$__glass_pipe_dir"

    # Split on unquoted pipes, insert tee
    # (simplified -- real implementation needs quote awareness)
    local IFS='|'
    local -a stages=($cmd)
    local rewritten=""
    local i=0
    local last=$((${#stages[@]} - 1))

    for stage in "${stages[@]}"; do
        if [[ $i -lt $last ]]; then
            rewritten+="$stage | tee ${__glass_pipe_dir}/stage_${i} |"
        else
            rewritten+="$stage"
        fi
        ((i++))
    done

    # Emit OSC with pipe metadata
    printf '\e]133;P;dir=%s;stages=%d\e\\' "$__glass_pipe_dir" "${#stages[@]}"

    echo "$rewritten"
}
```

### glass.ps1 additions

```powershell
# Pipe detection and Tee-Object insertion
function __Glass-Rewrite-Pipes {
    param([string]$CommandLine)

    if ($CommandLine -notmatch '\|') { return $null }

    # Split on unquoted pipes
    $stages = $CommandLine -split '\s*\|\s*'
    $rewritten = @()
    $Global:__GlassPipeVars = @()

    for ($i = 0; $i -lt $stages.Count; $i++) {
        $rewritten += $stages[$i]
        if ($i -lt $stages.Count - 1) {
            $varName = "__glass_ps_$i"
            $rewritten += "| Tee-Object -Variable $varName |"
            $Global:__GlassPipeVars += $varName
        }
    }

    return ($rewritten -join ' ')
}
```

---

## Sources

- [shlex 1.3.0 (docs.rs)](https://docs.rs/shlex/latest/shlex/) -- POSIX shell tokenization, already in workspace
- [content_inspector 0.2.4 (docs.rs)](https://docs.rs/content_inspector/latest/content_inspector/) -- considered and rejected; existing is_binary() is better
- [tempfile (crates.io)](https://crates.io/crates/tempfile) -- v3.26.0, 400M+ downloads, already in workspace
- [tee command (GNU Coreutils)](https://www.gnu.org/software/coreutils/tee) -- POSIX standard pipe splitter
- [PowerShell Tee-Object (Microsoft Learn)](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.utility/tee-object?view=powershell-7.5) -- Variable capture in PowerShell pipelines
- [Trace-Command (Microsoft Learn)](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.utility/trace-command?view=powershell-7.4) -- considered and rejected for pipe capture
- [strip-ansi-escapes 0.2.1 (crates.io)](https://crates.io/crates/strip-ansi-escapes) -- already in workspace via glass_history

---
*Stack research for: Glass v1.3 Pipe Visualization*
*Researched: 2026-03-05*
