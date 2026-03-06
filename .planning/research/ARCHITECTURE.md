# Architecture Patterns: Pipe Visualization Integration

**Domain:** Terminal emulator pipe visualization (v1.3 milestone)
**Researched:** 2026-03-05
**Confidence:** HIGH (based on direct codebase analysis of all 10 Glass crates + established Unix/Windows pipe patterns)

---

## Existing Architecture Summary

Glass is a 10-crate Rust workspace (12,214 LOC) with a clear event-driven data flow:

```
Shell (pwsh/bash)
  |  OSC 133 A/B/C/D sequences
  v
PTY reader thread (std::thread, blocking I/O)
  |  OscScanner pre-scans raw bytes
  |  OutputBuffer accumulates between C..D
  |  VTE parser updates terminal grid
  v
AppEvent enum (via winit EventLoopProxy)
  |  Shell { event, line }
  |  CommandOutput { raw_output }
  v
Main thread (winit event loop)
  |  BlockManager tracks lifecycle
  |  HistoryDb inserts CommandRecord
  |  SnapshotStore pre-exec snapshots
  v
FrameRenderer
  |  BlockRenderer generates rects + labels
  |  GridRenderer draws terminal cells
  v
wgpu DX12 GPU pipeline
```

**Key architectural constraints (from code analysis):**

1. **PTY reader** runs on `std::thread` (not tokio) -- blocking I/O must not block async executor. Holds `Term` lock briefly, scans OSC, sends AppEvent via EventLoopProxy.

2. **Crate boundary rule:** `glass_terminal` must NOT depend on `glass_history` -- raw bytes sent via AppEvent, processed on main thread.

3. **Command text extraction** happens at `CommandExecuted` time by reading the terminal grid between `block.command_start_line` and `block.output_start_line`.

4. **`command_parser.rs`** already marks `" | "` as unparseable syntax (returns `Confidence::Low`). This is correct for snapshot/undo; pipes are a different concern.

5. **Shell integration** works by wrapping existing prompts (Oh My Posh/Starship compatible). PowerShell uses PSReadLine Enter handler for OSC 133;C. Bash uses PS0.

6. **`glass_pipes` crate** exists as a stub (`//! glass_pipes -- stub crate, filled in future phases`).

---

## Recommended Architecture for Pipe Visualization

### Core Design Decision: Shell-Side Tee Insertion + Terminal-Side Detection

Pipe visualization has a fundamental challenge: the terminal sees only the FINAL output of a pipeline. Intermediate stage outputs are consumed by the next stage and never reach the PTY. There are two possible approaches:

**Option A (chosen for bash/zsh):** Shell integration rewrites the command to insert `tee` between stages, capturing intermediate output to temp files. After execution, shell reports captured data back via custom OSC sequences.

**Option B (chosen for PowerShell):** PowerShell's `Tee-Object -Variable` captures pipeline objects to variables. Shell integration rewrites the command to insert `Tee-Object` between stages, then reports variable contents post-execution.

**Fallback (all shells):** When rewriting is unsafe (TTY-sensitive commands) or disabled, Glass still detects pipes from the command text and displays the pipeline structure without intermediate output.

### Component Boundaries

| Component | Crate | Responsibility | Status |
|-----------|-------|---------------|--------|
| PipeParser | `glass_pipes` | Parse command text into pipeline stages | **NEW** |
| TtyDetector | `glass_pipes` | Identify TTY-sensitive commands that break with tee | **NEW** |
| PipeStage types | `glass_pipes` | Data types for stages, pipeline info | **NEW** |
| Shell integration (bash) | `shell-integration/glass.bash` | DEBUG trap rewrites piped commands with tee, PROMPT_COMMAND reports captured data via OSC | **MODIFIED** |
| Shell integration (PS) | `shell-integration/glass.ps1` | Enter handler inserts Tee-Object, prompt reports variables via OSC | **MODIFIED** |
| OscScanner | `glass_terminal` | Parse new OSC 133;P (pipe stage data) sequences | **MODIFIED** |
| AppEvent / ShellEvent | `glass_core` | New PipeStageOutput variant | **MODIFIED** |
| Block | `glass_terminal` | Block gains `pipeline_stages` field | **MODIFIED** |
| HistoryDb | `glass_history` | New `pipe_stages` table (schema v1 -> v2 migration) | **MODIFIED** |
| BlockRenderer | `glass_renderer` | Multi-row pipeline UI with stage indicators | **MODIFIED** |
| GlassServer | `glass_mcp` | New `glass_pipe_inspect` tool | **MODIFIED** |
| GlassConfig | `glass_core` | New `[pipes]` config section | **MODIFIED** |

---

### Data Types (in `glass_pipes`)

```rust
/// A single stage in a pipeline.
#[derive(Debug, Clone)]
pub struct PipeStage {
    /// Zero-indexed position in the pipeline.
    pub index: usize,
    /// The command fragment for this stage (e.g., "grep foo").
    pub command: String,
    /// Captured output (ANSI-stripped, truncated). None if capture not possible.
    pub output: Option<String>,
    /// Number of output lines (for UI sizing).
    pub line_count: usize,
    /// Whether this stage's output was truncated.
    pub truncated: bool,
}

/// Result of parsing a command for pipe structure.
#[derive(Debug, Clone)]
pub struct PipelineInfo {
    /// The original command text.
    pub original: String,
    /// Individual stages (split on unquoted |).
    pub stages: Vec<String>,
    /// Whether tee insertion is safe (no TTY-sensitive commands).
    pub tee_safe: bool,
}

/// TTY-sensitive commands that should NOT have tee inserted.
/// These commands require direct terminal access and break when
/// their stdin/stdout is redirected through tee.
pub const TTY_COMMANDS: &[&str] = &[
    "vim", "nvim", "vi", "nano", "emacs",       // editors
    "less", "more", "most",                       // pagers
    "top", "htop", "btop",                        // monitors
    "ssh", "telnet",                              // remote shells
    "fzf", "sk",                                  // fuzzy finders
    "tmux", "screen",                             // multiplexers
    "python", "node", "irb", "ghci",             // interactive REPLs
];
```

---

### Data Flow: Shell to Storage to UI to MCP

```
1. USER TYPES: ls -la | grep foo | wc -l

2. SHELL INTEGRATION (at command execution time):
   a. Detect unquoted pipe character(s) in command text
   b. Parse into stages: ["ls -la", "grep foo", "wc -l"]
   c. Check each stage against TTY blocklist
   d. If all safe AND tee_rewrite enabled:
      BASH:  ls -la | tee /tmp/glass-pipe-$$/0 | grep foo | tee /tmp/glass-pipe-$$/1 | wc -l
      PS:    ls -la | Tee-Object -Variable __glass_0 | grep foo | Tee-Object -Variable __glass_1 | wc -l
   e. Execute rewritten command (user sees normal final output)

3. SHELL INTEGRATION (after command finishes, before next prompt):
   a. BASH: Read /tmp/glass-pipe-$$/* files, emit OSC 133;P per stage, clean up
      PS:   Read $__glass_0, $__glass_1 variables, emit OSC 133;P per stage
   b. Emit pipe stage count: ESC]133;S;<stage_count>;<original_command_b64> BEL
   c. For each captured stage:
      ESC]133;P;<stage_index>;<byte_count>;<base64_output> BEL
   d. Emit OSC 133;D as before (normal command finish)

4. PTY READER THREAD (glass_terminal):
   a. OscScanner detects 133;S -> OscEvent::PipelineStart { count, original_cmd }
   b. OscScanner detects 133;P -> OscEvent::PipeStageOutput { index, data }
   c. Converted to ShellEvent variants, sent via AppEvent

5. MAIN THREAD EVENT HANDLER:
   a. On ShellEvent::PipelineStart:
      - Parse original command into stage commands via glass_pipes::PipeParser
      - Initialize pending_pipe_stages Vec on WindowContext
   b. On ShellEvent::PipeStageOutput:
      - Base64-decode data
      - Process (ANSI strip, binary detect, truncate)
      - Store in pending_pipe_stages[index]
   c. On ShellEvent::CommandFinished:
      - If pending_pipe_stages is non-empty:
        - Move stages into Block.pipeline_stages
        - Insert into pipe_stages DB table
        - Clear pending_pipe_stages

6. RENDERER (glass_renderer):
   a. BlockRenderer checks block.pipeline_stages.is_empty()
   b. If non-empty: render [N stages] indicator on separator line
   c. If expanded: render multi-row stage headers with truncated output previews
   d. Click/keybinding toggles pipeline_expanded on the Block

7. MCP (glass_mcp):
   a. glass_pipe_inspect(command_id) queries pipe_stages table
   b. Returns per-stage command + full captured output for AI analysis
```

---

### Shell Integration Changes (Detailed)

#### Bash (`glass.bash`) -- DEBUG Trap Approach

**Critical insight:** PS0 cannot rewrite the command -- it only emits text before execution. The command is already parsed by bash. Instead, use the `DEBUG` trap with `BASH_COMMAND`.

```bash
# Pipe detection and tee rewriting via DEBUG trap
__glass_is_tee_safe() {
    local cmd="$1"
    local IFS='|'
    for stage in $cmd; do
        local base=$(echo "$stage" | awk '{print $1}')
        case "$base" in
            vim|nvim|vi|nano|less|more|top|htop|ssh|fzf|tmux|screen|python|node)
                return 1 ;;  # Not safe
        esac
    done
    return 0  # All stages safe
}

__glass_rewrite_pipeline() {
    local cmd="$1"
    local tmpdir="/tmp/glass-pipe-$$"
    mkdir -p "$tmpdir"
    local result=""
    local idx=0
    local IFS='|'
    local stages=($cmd)
    local last=$((${#stages[@]} - 1))
    for i in "${!stages[@]}"; do
        local stage="${stages[$i]}"
        result+="$stage"
        if [[ $i -lt $last ]]; then
            result+=" | tee $tmpdir/$idx |"
            ((idx++))
        fi
    done
    echo "$result"
}

__glass_report_stages() {
    local tmpdir="/tmp/glass-pipe-$$"
    [[ -d "$tmpdir" ]] || return
    local count=$(ls "$tmpdir" 2>/dev/null | wc -l)
    [[ $count -eq 0 ]] && return
    # Emit pipeline start marker with original command
    local orig_b64=$(echo -n "$__GLASS_PIPE_ORIGINAL" | base64 -w0)
    printf '\e]133;S;%d;%s\a' "$count" "$orig_b64"
    # Emit each stage's captured output
    for f in "$tmpdir"/*; do
        local idx=$(basename "$f")
        local data=$(head -c 51200 "$f" | base64 -w0)  # 50KB limit
        local size=$(wc -c < "$f")
        printf '\e]133;P;%s;%s;%s\a' "$idx" "$size" "$data"
    done
    rm -rf "$tmpdir"
}

# Integrate into PROMPT_COMMAND (before OSC 133;D)
__glass_prompt_command() {
    local exit_code=$?
    __glass_report_stages  # Report pipe stages BEFORE 133;D
    printf '\e]133;D;%d\e\\' "$exit_code"
    __glass_osc7
    PS1='\[\e]133;A\e\\\]'"${__GLASS_ORIGINAL_PS1:-\\s-\\v\\$ }"'\[\e]133;B\e\\\]'
}
```

**Bash limitation:** The DEBUG trap approach requires careful handling to avoid recursion. The trap sees every simple command, not just user-typed pipelines. Guard with a flag (`__GLASS_PIPE_ACTIVE`) and only rewrite when the command contains ` | ` and is at the top-level interactive prompt.

**Simpler alternative considered:** Wrapping the pipeline in a function at PS0 time. Rejected because PS0 output is prepended to the command line but doesn't actually modify the command bash executes.

**Recommended practical approach:** Since bash's DEBUG trap is fragile and complex, start with **post-hoc detection only** for bash: detect pipes in the command text, split into stages, but don't capture intermediate output. Add tee rewriting as an opt-in feature in a later iteration. This is safer for v1.3.

#### PowerShell (`glass.ps1`) -- PSReadLine Replace

PowerShell is significantly cleaner because PSReadLine's `Replace()` method can rewrite the command buffer BEFORE `AcceptLine()`.

```powershell
function Global:__Glass-Is-Tee-Safe {
    param([string]$Command)
    $stages = $Command -split '\|'
    foreach ($stage in $stages) {
        $base = ($stage.Trim() -split '\s+')[0]
        if ($base -in @('vim','nvim','less','more','top','htop','ssh','fzf','python','node')) {
            return $false
        }
    }
    return $true
}

function Global:__Glass-Rewrite-Pipeline {
    param([string]$Command)
    $stages = $Command -split '\|'
    $result = @()
    for ($i = 0; $i -lt $stages.Count; $i++) {
        $result += $stages[$i].Trim()
        if ($i -lt $stages.Count - 1) {
            $result += "| Tee-Object -Variable __glass_pipe_$i |"
        }
    }
    return ($result -join ' ')
}

# Modified Enter handler
Set-PSReadLineKeyHandler -Key Enter -ScriptBlock {
    $line = $null
    [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$line, [ref]$null)

    $Global:__GlassPipeOriginal = $null
    if ($line -match '\|' -and (__Glass-Is-Tee-Safe $line)) {
        $Global:__GlassPipeOriginal = $line
        $rewritten = __Glass-Rewrite-Pipeline $line
        [Microsoft.PowerShell.PSConsoleReadLine]::Replace(0, $line.Length, $rewritten)
    }

    [Microsoft.PowerShell.PSConsoleReadLine]::AcceptLine()
    [Console]::Write("$([char]0x1b)]133;C$([char]7)")
}

# In prompt function, report pipe stages before 133;D
function __Glass-Report-Stages {
    if ($null -eq $Global:__GlassPipeOriginal) { return }
    $stages = $Global:__GlassPipeOriginal -split '\|'
    $count = $stages.Count - 1  # Number of intermediate stages (not final)
    $origB64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Global:__GlassPipeOriginal))
    [Console]::Write("$([char]0x1b)]133;S;$count;$origB64$([char]7)")
    for ($i = 0; $i -lt $count; $i++) {
        $varName = "__glass_pipe_$i"
        $value = Get-Variable -Name $varName -ValueOnly -ErrorAction SilentlyContinue
        if ($null -ne $value) {
            $text = $value | Out-String
            if ($text.Length -gt 51200) { $text = $text.Substring(0, 51200) }
            $b64 = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($text))
            [Console]::Write("$([char]0x1b)]133;P;$i;$($text.Length);$b64$([char]7)")
            Remove-Variable -Name $varName -Scope Global -ErrorAction SilentlyContinue
        }
    }
    $Global:__GlassPipeOriginal = $null
}
```

**PowerShell advantage:** PSReadLine's `Replace()` modifies the actual command buffer. The shell parses and executes the rewritten version. This is clean and reliable. The `Tee-Object -Variable` approach captures PowerShell objects serialized to string, which is exactly what a user would see if they examined intermediate output.

**PowerShell caveat:** `Tee-Object` works on PowerShell pipeline objects, not raw byte streams. For native commands (e.g., `git log | grep fix`), the output passes through PowerShell's pipeline as strings, which works fine. But for pure cmdlet pipelines (e.g., `Get-Process | Sort-Object CPU`), `Tee-Object` captures object representations, not formatted table output. This is actually MORE useful for debugging.

---

### OSC Protocol Extension

Two new OSC 133 sub-markers:

```
Pipeline start (emitted once before stage data):
ESC]133;S;<stage_count>;<base64_original_command> BEL

Per-stage output (emitted once per intermediate stage):
ESC]133;P;<stage_index>;<byte_count>;<base64_output> BEL
```

- `stage_count`: number of intermediate stages (pipeline length - 1)
- `stage_index`: 0-based
- `byte_count`: original byte count before base64
- `base64_output`: base64-encoded to avoid BEL/ESC conflicts in raw output

**Why base64:** Pipe stage output may contain arbitrary bytes including BEL (0x07) which would prematurely terminate the OSC sequence, and ESC (0x1b) which could trigger false OSC detection in the scanner.

**Size constraint:** Each stage is truncated to `max_stage_capture_kb` (default 50KB) before base64 encoding. Base64 inflates by ~33%, so max OSC payload per stage is ~67KB. The OscScanner already handles split-buffer accumulation, so large payloads spanning multiple PTY reads are handled correctly.

---

### OscScanner Extension

Add to `parse_osc133`:

```rust
fn parse_osc133(params: &str) -> Option<OscEvent> {
    let mut parts = params.splitn(2, ';');
    let marker = parts.next()?;
    match marker {
        "A" => Some(OscEvent::PromptStart),
        "B" => Some(OscEvent::CommandStart),
        "C" => Some(OscEvent::CommandExecuted),
        "D" => { /* existing */ }
        // NEW
        "S" => {
            // Pipeline start: S;<count>;<base64_cmd>
            let rest = parts.next()?;
            let mut sub = rest.splitn(2, ';');
            let count = sub.next()?.parse::<usize>().ok()?;
            let cmd_b64 = sub.next().unwrap_or("");
            Some(OscEvent::PipelineStart {
                stage_count: count,
                original_command: cmd_b64.to_string(),
            })
        }
        "P" => {
            // Pipe stage output: P;<index>;<byte_count>;<base64_data>
            let rest = parts.next()?;
            let mut sub = rest.splitn(3, ';');
            let index = sub.next()?.parse::<usize>().ok()?;
            let _byte_count = sub.next()?.parse::<usize>().ok()?;
            let data_b64 = sub.next().unwrap_or("");
            Some(OscEvent::PipeStageOutput {
                index,
                data: data_b64.to_string(),
            })
        }
        _ => None,
    }
}
```

This follows the existing scanner pattern exactly -- small, additive changes to the match arm.

---

### Database Schema Extension

```sql
-- New table linked to existing commands table
-- Added in schema migration v1 -> v2
CREATE TABLE IF NOT EXISTS pipe_stages (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    command_id  INTEGER NOT NULL,
    stage_index INTEGER NOT NULL,
    stage_cmd   TEXT NOT NULL,
    output      TEXT,
    line_count  INTEGER NOT NULL DEFAULT 0,
    truncated   INTEGER NOT NULL DEFAULT 0,
    UNIQUE(command_id, stage_index)
);
CREATE INDEX IF NOT EXISTS idx_pipe_stages_command ON pipe_stages(command_id);
```

This lives in `glass_history` because pipe stage data is command metadata. Migration follows the existing `PRAGMA user_version` pattern:

```rust
if version < 2 {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS pipe_stages (...);
         CREATE INDEX IF NOT EXISTS idx_pipe_stages_command ON pipe_stages(command_id);"
    )?;
    conn.pragma_update(None, "user_version", 2)?;
}
```

**Why not CASCADE delete:** The `commands` table doesn't enforce foreign keys on `pipe_stages` because existing code deletes commands without knowing about pipe_stages. Instead, retention pruning in `glass_history` adds cleanup: `DELETE FROM pipe_stages WHERE command_id NOT IN (SELECT id FROM commands)`.

---

### Renderer Changes

The `BlockRenderer` currently produces separator lines, exit code badges, duration labels, and `[undo]` labels. For pipeline blocks:

**Collapsed state (default):**
```
 ls -la | grep foo | wc -l                    [3 stages] 1.2s  OK
```

**Expanded state:**
```
 ls -la | grep foo | wc -l                    [3 stages] 1.2s  OK
 +-- Stage 1: ls -la ------------------------------------------+
 | total 48                                                     |
 | drwxr-xr-x  5 user user  4096 Mar  5 10:00 .               |
 | -rw-r--r--  1 user user  1234 Mar  5 09:00 foo.txt          |
 +-- Stage 2: grep foo ----------------------------------------+
 | -rw-r--r--  1 user user  1234 Mar  5 09:00 foo.txt          |
 +--------------------------------------------------------------+
 1                                                    (final output)
```

**Implementation changes to `Block`:**

```rust
pub struct Block {
    // ... existing fields ...
    /// Pipeline stages with captured intermediate output.
    pub pipeline_stages: Vec<PipeStage>,
    /// Whether the pipeline view is expanded in the UI.
    pub pipeline_expanded: bool,
}
```

**Rendering approach:**
- `[N stages]` is a `BlockLabel` positioned left of `[undo]` / duration
- Expanded view uses additional `RectInstance` rows for stage backgrounds (slightly different shade)
- Stage headers and output are rendered as `BlockLabel` text
- Stage output is limited to a configurable max lines (default 10 per stage, scrollable)
- The expanded area does NOT use the terminal grid -- it's a separate rendering layer drawn by `BlockRenderer`, using the same `glyphon` text rendering as other block labels

**Interaction:**
- Click `[N stages]` label or press a keybinding to toggle `pipeline_expanded`
- `auto_expand` config option (default false) expands all pipeline blocks automatically

---

### Crate Dependency Graph (After v1.3)

```
glass_core (events, config, error)
    ^           ^           ^
    |           |           |
glass_terminal  |     glass_snapshot (unchanged)
    ^           |           ^
    |           |           |
glass_renderer  |     glass_mcp
    ^           |           ^
    |           |           |
    +-----+-----+----------+
          |
       root binary (Processor coordinates everything)
          |
     +----+----+
     |         |
glass_history  glass_pipes [FILLED]
```

**Key dependency rules:**
- `glass_pipes` depends on NOTHING (pure logic crate: strings in, data types out)
- `glass_pipes` does NOT depend on `glass_core` -- it defines its own `PipeStage` type
- The root binary imports `glass_pipes` for parsing, bridges data to other crates
- `glass_terminal` does NOT import `glass_pipes` -- OscScanner handles raw OSC bytes; conversion to `PipeStage` happens in the root binary

**glass_pipes Cargo.toml:**
```toml
[package]
name = "glass_pipes"
version = "0.1.0"
edition = "2021"

[dependencies]
# None -- pure logic crate

[dev-dependencies]
# Test-only dependencies if needed
```

---

### Integration Points with Existing Code (Explicit)

| Existing Code | Change Required | Risk |
|--------------|----------------|------|
| `command_parser.rs` (glass_snapshot) | **None.** Still returns `Confidence::Low` for piped commands. Correct -- undo shouldn't try to parse pipes. | None |
| `output_capture.rs` (glass_terminal) | **None.** OutputBuffer captures FINAL output unchanged. Pipe stage data arrives via separate OSC sequences. | None |
| `osc_scanner.rs` (glass_terminal) | **Add** `S` and `P` markers to `parse_osc133`. Small additive change. | Low |
| `event.rs` (glass_core) | **Add** `ShellEvent::PipelineStart` and `ShellEvent::PipeStageOutput`. Follows existing pattern. | Low |
| `block_manager.rs` (glass_terminal) | **Add** `pipeline_stages` and `pipeline_expanded` fields to `Block`. | Low |
| `db.rs` (glass_history) | **Add** `pipe_stages` table, schema migration v1->v2, insert/query methods. | Low |
| `main.rs` event handler | **Add** handling for PipelineStart and PipeStageOutput in Shell match arm. Buffer stages, flush on CommandFinished. | Medium |
| `block_renderer.rs` (glass_renderer) | **Extend** `build_block_text` and `build_block_rects` for pipeline UI. Most complex change. | Medium |
| `tools.rs` (glass_mcp) | **Add** `glass_pipe_inspect` tool. Follows existing pattern exactly. | Low |
| `config.rs` (glass_core) | **Add** `PipesSection` to `GlassConfig`. Follows `SnapshotSection` pattern. | Low |
| `shell-integration/glass.bash` | **Add** pipe rewriting via DEBUG trap (opt-in), stage reporting in PROMPT_COMMAND. | High |
| `shell-integration/glass.ps1` | **Modify** Enter handler for tee insertion, add stage reporting to prompt. | Medium |

---

## Patterns to Follow

### Pattern 1: Raw Bytes via AppEvent (Boundary Preservation)

**What:** Send raw data through AppEvent to avoid crate dependency cycles.
**When:** Pipe stage data flowing from PTY reader to main thread.
**Why:** Established in v1.1. glass_terminal must not depend on glass_history or glass_pipes.

```rust
// In glass_core/event.rs -- new ShellEvent variants
ShellEvent::PipelineStart { stage_count: usize, original_command_b64: String },
ShellEvent::PipeStageOutput { index: usize, data_b64: String },
```

### Pattern 2: Schema Migration via PRAGMA user_version

**What:** Bump `user_version` from 1 to 2, add pipe_stages table.
**When:** Adding pipe stage storage.
**Example:** Exactly follows the v0->v1 migration in `db.rs`.

### Pattern 3: Config Section with Serde Defaults

**What:** New `[pipes]` section with all fields having defaults.
**When:** Adding pipe visualization configuration.

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct PipesSection {
    #[serde(default = "default_true")]
    pub enabled: bool,                    // default: true
    #[serde(default = "default_50")]
    pub max_stage_capture_kb: u32,        // default: 50
    #[serde(default = "default_false")]
    pub auto_expand: bool,                // default: false
    #[serde(default = "default_true")]
    pub tee_rewrite: bool,               // default: true (PS), false (bash initially)
}
```

### Pattern 4: MCP Tool with spawn_blocking

**What:** New `glass_pipe_inspect` follows existing tool structure exactly.
**When:** Adding pipe inspection for AI assistants.
**Example:** Same structure as `glass_file_diff` -- open DB in spawn_blocking, query, return JSON.

### Pattern 5: Non-Fatal Degradation

**What:** Pipe capture failures log warnings and continue. Terminal remains usable.
**When:** Tee rewriting fails, temp files can't be created, OSC parsing fails.
**Example:** Same pattern as snapshot_store and history_db initialization.

---

## Anti-Patterns to Avoid

### Anti-Pattern 1: Modifying the PTY Byte Stream

**What:** Intercepting PTY writes to inject tee into commands at the terminal level.
**Why bad:** The PTY write path is a thin `write_list` queue of raw bytes. Commands arrive as keystroke-by-keystroke bytes, not as complete strings. Rewriting at this layer would require maintaining a stateful command parser that tracks readline editing, multi-line commands, escape sequences, and command boundaries. This is fundamentally impossible without reimplementing a shell parser.
**Instead:** Shell integration scripts rewrite commands at the shell level, where the command text is known and complete.

### Anti-Pattern 2: glass_pipes Depending on glass_terminal

**What:** Importing terminal types into the pipe parsing crate.
**Why bad:** Creates dependency cycle risk and conflates parsing logic with terminal state.
**Instead:** `glass_pipes` is pure logic -- takes strings, returns strings and data structs.

### Anti-Pattern 3: Storing Stage Output in the commands Table

**What:** Adding pipe stage data as a JSON blob in the existing `output` column.
**Why bad:** Breaks the single-output-per-command model. Makes queries complex. Violates schema normalization.
**Instead:** Separate `pipe_stages` table with `command_id` foreign key.

### Anti-Pattern 4: Always-On Tee Insertion Without TTY Detection

**What:** Rewriting every piped command with tee, regardless of what commands are in the pipeline.
**Why bad:** TTY-sensitive commands (vim, less, fzf) break when their stdin/stdout is redirected through tee. `less` loses its ability to detect terminal height. `fzf` loses interactive selection.
**Instead:** Check each pipeline stage against TTY blocklist. If ANY stage is TTY-sensitive, skip rewriting for the entire pipeline.

### Anti-Pattern 5: Using Temp Files Without Session Scoping

**What:** Writing captured stage output to fixed-name temp files like `/tmp/glass-pipe-0`.
**Why bad:** Multiple Glass sessions would overwrite each other's capture files.
**Instead:** Use `$$` (PID) scoping: `/tmp/glass-pipe-$$/0`. Clean up in PROMPT_COMMAND and via `trap EXIT`.

### Anti-Pattern 6: Rendering Expanded Stages in the Terminal Grid

**What:** Inserting expanded pipe stage output into the VTE terminal grid as if the shell printed it.
**Why bad:** Corrupts the terminal scrollback. Confuses cursor positioning. Breaks copy-paste of actual command output.
**Instead:** Render expanded stages as block decorations in the renderer layer, overlay-style, separate from the terminal grid.

---

## Scalability Considerations

| Concern | Normal use | Heavy pipelines | Edge cases |
|---------|-----------|----------------|------------|
| Stage output size | 50KB total shared across stages | Configurable per-stage limit | Binary output detected + skipped (same as OutputBuffer) |
| DB storage | <1KB per command with stages | Retention prunes with parent command | Orphan cleanup: `DELETE FROM pipe_stages WHERE command_id NOT IN (...)` |
| Temp files (bash) | /tmp/glass-pipe-$$/ cleaned in PROMPT_COMMAND | $$-scoped prevents cross-session conflicts | Cleanup on shell exit via `trap EXIT` |
| OSC payload size | <67KB base64 per stage | OscScanner split-buffer handling works (tested to arbitrary sizes) | Scanner buffer growth bounded by truncation at shell level |
| UI rendering | Collapsed by default (single label, no extra GPU work) | Expanded shows max 10 lines per stage | Scrollable stage output for very long captures |
| Pipeline depth | Typical: 2-4 stages | Extreme: 10+ stages | Cap at 20 stages; warn and skip rewriting beyond that |

---

## Build Order (Considering Existing Dependencies)

### Phase 1: Core Data Types + Pipe Parsing (`glass_pipes`)
- `PipeStage`, `PipelineInfo` data types
- `PipeParser::parse(command_text) -> Option<PipelineInfo>` (split on unquoted `|`)
- `TtyDetector::is_tty_sensitive(command_fragment) -> bool`
- Zero dependencies, fully unit-testable
- **Builds on:** Nothing (empty stub crate)
- **Blocks:** Phases 3, 5

### Phase 2: Shell Integration + OSC Protocol
- Define OSC 133;S and 133;P protocol
- Modify `glass.ps1` Enter handler with Tee-Object insertion
- Modify `glass.ps1` prompt function with stage reporting
- Modify `glass.bash` PROMPT_COMMAND with stage reporting
- Bash tee rewriting as opt-in (DEBUG trap approach, disabled by default)
- Manual testing across pwsh 7, PowerShell 5.1, bash 4.4+
- **Builds on:** Protocol definition (no Rust code dependency)
- **Blocks:** Phase 3

### Phase 3: Terminal-Side Detection + Event Transport
- Extend `OscScanner` with 133;S and 133;P parsing
- Extend `OscEvent` / `ShellEvent` with pipeline variants
- Extend `AppEvent` with new Shell event types
- Extend `Block` with `pipeline_stages` and `pipeline_expanded` fields
- Wire up in main.rs: buffer stages on PipelineStart/PipeStageOutput, flush on CommandFinished
- Integrate pipe detection via `glass_pipes::PipeParser` in main.rs at CommandExecuted time
- **Builds on:** Phase 1 (PipeParser), Phase 2 (OSC protocol)
- **Blocks:** Phases 4, 5

### Phase 4: Database Storage + Retention
- Schema migration v1->v2 in `glass_history`
- `PipeStageRecord` struct + insert/query methods
- Orphan cleanup in retention pruning
- Integration with main.rs CommandFinished handler (insert stages)
- **Builds on:** Phase 3 (data types flowing through events)
- **Blocks:** Phase 6

### Phase 5: Pipeline UI Rendering
- `[N stages]` label in `BlockRenderer::build_block_text`
- Expanded multi-row view with stage headers and output preview
- Expand/collapse toggle (click on label or keybinding)
- Stage background rects in `build_block_rects`
- `auto_expand` config support
- **Builds on:** Phase 3 (Block.pipeline_stages populated)
- **Blocks:** Nothing (can parallel with Phase 4/6)

### Phase 6: MCP Tool + Config
- `glass_pipe_inspect(command_id)` tool in glass_mcp
- `[pipes]` config section in glass_core
- Config gating for tee_rewrite, auto_expand, max_stage_capture_kb
- **Builds on:** Phase 4 (DB queries), Phase 3 (config types)

**Phase ordering rationale:**
- Phase 1 has zero dependencies -- start here to unblock everything
- Phase 2 is highest-risk (shell integration is fragile) -- tackle early so issues surface before dependent phases
- Phase 3 is the integration backbone that connects shell output to Rust data structures
- Phases 4 and 5 can run in parallel after Phase 3
- Phase 6 is lowest risk, pure integration of existing patterns

---

## Sources

- Direct codebase analysis of all 10 Glass crates, 12,214 LOC (PRIMARY, HIGH confidence)
- [Windows ConPTY documentation](https://devblogs.microsoft.com/commandline/windows-command-line-introducing-the-windows-pseudo-console-conpty/) -- ConPTY pipe architecture
- [Creating a Pseudoconsole session](https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session) -- ConPTY threading model
- [GNU Bash Process Substitution](https://www.gnu.org/software/bash/manual/html_node/Process-Substitution.html) -- Process substitution mechanics
- [PowerShell Tee-Object](https://adamtheautomator.com/tee-object/) -- Tee-Object for intermediate pipeline capture
- [tee command in Linux](https://www.geeksforgeeks.org/linux-unix/tee-command-linux-example/) -- tee usage in pipelines
- [ForEach-Object documentation](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/foreach-object?view=powershell-7.5) -- PowerShell pipeline mechanics

---

*Architecture research for: Glass v1.3 Pipe Visualization*
*Researched: 2026-03-05*
