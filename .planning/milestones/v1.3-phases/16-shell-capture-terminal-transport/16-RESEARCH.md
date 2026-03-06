# Phase 16: Shell Capture + Terminal Transport - Research

**Researched:** 2026-03-05
**Domain:** Shell integration pipeline rewriting, OSC protocol extension, terminal event wiring (Rust + Bash + PowerShell)
**Confidence:** HIGH

## Summary

Phase 16 bridges the gap between Phase 15's pure parsing library and the actual capture of intermediate pipe stage output. This phase has four distinct workstreams: (1) bash/zsh shell integration modifications to rewrite piped commands with tee-based interception, emitting captured data as OSC 133;P sequences, (2) PowerShell shell integration modifications to capture object pipeline output via Tee-Object and emit the same OSC sequences, (3) OscScanner extension in glass_terminal to parse the new OSC 133;S and 133;P sequences, and (4) wiring the parsed events into the Block struct and AppEvent pipeline so downstream phases (UI, storage) can consume pipeline stage data.

The core architectural challenge is that the shell integration scripts must rewrite the user's command transparently -- injecting tee between each pipe stage to capture intermediate output to temp files, then emitting that captured data as OSC escape sequences after the pipeline completes. Exit codes must be preserved through this rewriting (using `$PIPESTATUS` in bash, `$LASTEXITCODE` in PowerShell). The terminal side receives these OSC sequences through the existing OscScanner pre-scan path and converts them into ShellEvent variants that the main event loop processes.

This phase does NOT store stage data to the database (Phase 18) or render pipeline UI (Phase 17). It delivers captured stage data as in-memory events that the Block struct holds until consumed.

**Primary recommendation:** Extend the existing shell integration scripts (`glass.bash`, `glass.ps1`) with pipeline-aware hooks that rewrite commands before execution, capture stage output to temp files, and emit OSC 133;S/P sequences after completion. On the terminal side, extend OscScanner with two new event variants and add `pipeline_stages` to Block.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| CAPT-01 | Byte-stream capture points inserted between bash/zsh pipe stages via tee-based rewriting | Shell integration modifies PS0/DEBUG trap to rewrite `cmd1 \| cmd2 \| cmd3` into `cmd1 \| tee /tmp/glass_stage_0 \| cmd2 \| tee /tmp/glass_stage_1 \| cmd3`; after execution emits OSC 133;P with file contents |
| CAPT-02 | PowerShell pipe stages captured via post-hoc string representation after pipeline completes | PSReadLine Enter handler enhanced to wrap pipeline with Tee-Object at each stage or capture post-hoc via variable; emits OSC 133;P with string representation |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| glass_pipes | 0.1.0 (local) | Pipeline parsing and classification | Phase 15 output -- parse_pipeline() and classify_pipeline() |
| glass_terminal | 0.1.0 (local) | OscScanner extension, Block struct | Existing PTY reader infrastructure |
| glass_core | 0.1.0 (local) | ShellEvent enum extension | Cross-crate event types |
| tempfile | 3 (workspace dev-dep) | Temp file management for tee output | Already in dev-dependencies; use for integration tests |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| (no new Rust deps) | - | All Rust changes extend existing crates | Shell scripts do the heavy lifting |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Temp files for stage capture | Named pipes (FIFOs) | Temp files are simpler, work on Windows, avoid blocking issues. Named pipes would need cleanup and don't work on Windows. |
| Post-completion OSC emission | Live streaming during execution | Live streaming adds complexity (concurrent tee writes + OSC emission). Post-completion is explicitly in scope per REQUIREMENTS.md "show after completion" |
| Tee-Object for PowerShell | ForEach-Object with side-effect | Tee-Object is the idiomatic PowerShell approach; ForEach-Object is slower and more fragile |
| OSC 133;P (custom) | Separate OSC number (e.g., OSC 1337) | Keeping within OSC 133 namespace maintains consistency with existing shell integration protocol |

**Installation:**
```bash
# No new dependencies needed
```

## Architecture Patterns

### Recommended Changes by Component

```
shell-integration/
  glass.bash          # ADD: pipeline rewriting in DEBUG trap / PS0
  glass.ps1           # ADD: pipeline wrapping in PSReadLine Enter handler

crates/glass_core/src/
  event.rs            # ADD: ShellEvent::PipelineStart, ShellEvent::PipelineStage variants

crates/glass_terminal/src/
  osc_scanner.rs      # ADD: parse OSC 133;S and 133;P payloads
  block_manager.rs    # ADD: pipeline_stages field to Block struct

crates/glass_pipes/src/
  rewrite.rs          # NEW: generate tee-rewritten command strings for bash
  (types.rs)          # ADD: CapturedStage type for captured stage data
```

### Pattern 1: Bash Tee-Based Pipeline Rewriting
**What:** The shell integration script intercepts the user's command at PS0/DEBUG trap time, detects pipes using the glass_pipes parser (or a simpler shell-level check), and rewrites the command to insert `tee` between each stage. After execution, it reads temp files and emits OSC sequences.
**When to use:** bash and zsh pipelines.
**Design:**

The bash rewriting approach works as follows. When a user types:
```bash
cat file | grep foo | wc -l
```

The shell integration rewrites it (transparently) to:
```bash
cat file | tee /tmp/glass_s0_$$ | grep foo | tee /tmp/glass_s1_$$ | wc -l
```

After the pipeline completes, the script:
1. Captures `${PIPESTATUS[@]}` to preserve all exit codes
2. Reads each temp file and emits `OSC 133;P;{stage_index};{byte_count};{data}\a`
3. Cleans up temp files
4. Restores the correct exit code for `$?`

**Critical implementation detail:** The tee insertion happens in the PS0 handler (bash >= 4.4) or a DEBUG trap. PS0 is evaluated after Enter but before execution -- this is the existing hook point for OSC 133;C. The rewriting must happen here.

**Example bash integration addition:**
```bash
# In glass.bash, extend the PS0/DEBUG mechanism:

__glass_rewrite_pipeline() {
    local cmd="$1"
    local tmpdir="${TMPDIR:-/tmp}/glass_$$"
    mkdir -p "$tmpdir"

    # Simple pipe detection (not inside quotes)
    # For robustness, use the same logic as glass_pipes parser
    local IFS='|'
    local -a stages
    read -ra stages <<< "$cmd"

    if [[ ${#stages[@]} -le 1 ]]; then
        # Not a pipeline, no rewriting needed
        echo "$cmd"
        return 1
    fi

    # Build rewritten command with tee between stages
    local rewritten=""
    local i=0
    for stage in "${stages[@]}"; do
        stage=$(echo "$stage" | xargs)  # trim
        if [[ $i -gt 0 ]]; then
            rewritten+=" | "
        fi
        rewritten+="$stage"
        if [[ $i -lt $((${#stages[@]} - 1)) ]]; then
            rewritten+=" | tee ${tmpdir}/stage_${i}"
        fi
        ((i++))
    done

    echo "$rewritten"
    return 0
}

__glass_emit_stages() {
    local tmpdir="${TMPDIR:-/tmp}/glass_$$"
    local i=0
    while [[ -f "${tmpdir}/stage_${i}" ]]; do
        local size=$(wc -c < "${tmpdir}/stage_${i}")
        local data=$(head -c 1048576 "${tmpdir}/stage_${i}")  # cap at 1MB per OSC
        printf '\e]133;P;%d;%d;%s\e\\' "$i" "$size" "$data"
        ((i++))
    done
    rm -rf "$tmpdir"
}
```

**IMPORTANT caveat about PS0:** PS0 in bash is a prompt string, not a function. It is expanded and printed, but cannot modify the command being executed. Therefore, the actual pipeline rewriting must use a different mechanism:

Option A: **DEBUG trap** -- bash's `trap '...' DEBUG` fires before each simple command. The `BASH_COMMAND` variable contains the command about to run. However, DEBUG trap cannot modify the command.

Option B: **PSReadLine-style keystroke interception** -- not available in bash.

Option C: **Function wrapper / alias approach** -- define a function that wraps command execution. This is complex and fragile.

**Recommended approach for bash: Post-execution capture with process substitution.**

Instead of rewriting the command, use the DEBUG trap to detect pipeline commands, then use `PROMPT_COMMAND` (which fires after the command completes) to emit the stage data. The actual capture happens via a different mechanism:

The shell integration sets up a `preexec` hook (via DEBUG trap) that:
1. Parses the command to detect pipes
2. If pipeline detected and should_capture: rewrites the command using `eval` with tee insertion
3. Stores PIPESTATUS after execution
4. In PROMPT_COMMAND, emits OSC 133;P sequences with temp file contents

Actually, the most reliable bash approach is:

**DEBUG trap + eval rewrite:**
```bash
trap '__glass_preexec "$BASH_COMMAND"' DEBUG

__glass_preexec() {
    # Only intercept once per command (not for each pipeline stage)
    [[ -n "$__glass_intercepted" ]] && return
    __glass_intercepted=1

    local cmd="$1"
    # Check for pipes (simplified -- real implementation needs quote awareness)
    if [[ "$cmd" == *"|"* ]]; then
        # Mark for post-exec stage emission
        __glass_pipeline_active=1
        __glass_pipeline_tmpdir="${TMPDIR:-/tmp}/glass_$$"
        mkdir -p "$__glass_pipeline_tmpdir"
    fi
}
```

However, the DEBUG trap **cannot modify `BASH_COMMAND`** -- it can only observe it. To actually rewrite the pipeline, the approach must be different.

**Final recommended approach: Alias/function wrapping is not practical. Use `eval` in PS0.**

Actually, the cleanest approach for bash is to NOT rewrite the command at all. Instead:

1. In the DEBUG trap, detect that a pipeline is about to run
2. Set up temp file paths
3. Let the command run unmodified
4. In PROMPT_COMMAND, the exit code is already captured (existing `$?` logic)
5. For CAPT-01, the tee insertion must happen. The only reliable way is **PS0 with command substitution that performs eval:**

Wait -- this is getting complex. Let me reconsider.

**The most practical bash approach:**

Use `BASH_COMMAND` in the DEBUG trap to detect pipelines. For the actual capture, rewrite the command by using bash's `history` mechanism or by intercepting the readline buffer. But these are fragile.

**Simplest correct approach: `preexec`-style with eval.**

The glass.bash script replaces PS0 (which already emits OSC 133;C) with a function that also detects pipelines and rewrites them via eval:

```bash
# PS0 is expanded, and can include command substitution
PS0='$(__glass_ps0_hook)'

__glass_ps0_hook() {
    printf '\e]133;C\e\\'
    # Pipeline detection and rewriting happens via DEBUG trap
    # PS0 cannot rewrite the command -- it just emits the OSC
}
```

**Actual recommended implementation:** For bash, the tee-based rewriting is best done by changing how the user invokes commands. Since we control the shell integration script that runs in the user's bash session, we can:

1. Override the `command_not_found_handle` -- no, that only fires for unknown commands
2. Use `bind` to intercept Enter key -- yes! Similar to the PowerShell PSReadLine approach

```bash
# Bind Enter to a custom function that rewrites pipelines
bind -x '"\C-m": __glass_accept_line'

__glass_accept_line() {
    local cmd="$READLINE_LINE"

    # Parse for pipes (simple version)
    if __glass_has_pipes "$cmd"; then
        local rewritten=$(__glass_tee_rewrite "$cmd")
        READLINE_LINE="$rewritten; __glass_emit_stages; __glass_restore_pipestatus"
    fi

    # Accept the line (execute it)
    # ... this is where it gets tricky -- bind -x doesn't have a clean "accept" action
}
```

This is also fragile. **Let me settle on the most robust approach used by real terminal emulators:**

### Final Recommended Bash Approach (HIGH confidence)

Based on how tools like `bash-preexec` work (used by iTerm2, VS Code terminal integration):

1. The `DEBUG` trap fires before each command with `BASH_COMMAND` containing the command text
2. The trap CANNOT modify the command, but it CAN set variables
3. After the command runs, `PROMPT_COMMAND` fires

For actual tee rewriting, the approach is:
- **Intercept at the readline level** using `bind -x` to capture the command line
- Rewrite it with tee insertions
- Replace `READLINE_LINE` with the rewritten version
- Let bash execute the rewritten command

```bash
if [[ "${BASH_VERSINFO[0]}" -ge 4 ]]; then
    __glass_original_accept_line() {
        # Store current line for potential rewriting
        local cmd="$READLINE_LINE"
        local cursor="$READLINE_POINT"

        if __glass_should_capture "$cmd"; then
            local tmpdir="${TMPDIR:-/tmp}/glass_$$_$(date +%s)"
            mkdir -p "$tmpdir" 2>/dev/null
            __glass_capture_tmpdir="$tmpdir"
            __glass_capture_stage_count=0

            READLINE_LINE="$(__glass_tee_rewrite "$cmd" "$tmpdir")"
            READLINE_POINT=${#READLINE_LINE}
        else
            __glass_capture_tmpdir=""
        fi
    }

    # Bind to Enter key -- this runs the function then accepts the line
    bind -x '"\e[glass-pre": __glass_original_accept_line'
    bind '"\C-m": "\e[glass-pre\C-j"'
fi
```

This `bind` approach is the same technique used by tools like `fzf` and `bash-preexec` for intercepting command input.

### Pattern 2: PowerShell Post-Hoc Capture
**What:** PowerShell's object pipeline makes mid-pipeline tee insertion less useful (objects lose their type info when serialized). Instead, use a post-hoc approach: after the pipeline completes, capture the string representation of what each stage produced.
**When to use:** PowerShell pipelines.
**Design:**

For PowerShell, the approach is simpler because PSReadLine already intercepts Enter:

```powershell
Set-PSReadLineKeyHandler -Key Enter -ScriptBlock {
    $line = $null
    $cursor = $null
    [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$line, [ref]$cursor)

    # Check for pipeline
    if ($line -match '\|' -and -not ($line -match '--no-glass')) {
        # Rewrite with Tee-Object at each stage
        $rewritten = __Glass-Rewrite-Pipeline $line
        [Microsoft.PowerShell.PSConsoleReadLine]::Replace(0, $line.Length, $rewritten)
    }

    [Microsoft.PowerShell.PSConsoleReadLine]::AcceptLine()
    [Console]::Write("$([char]0x1b)]133;C$([char]7)")
}
```

The rewrite function for PowerShell:
```powershell
function Global:__Glass-Rewrite-Pipeline {
    param([string]$Command)

    $stages = $Command -split '(?<!\|)\|(?!\|)'  # split on | but not ||
    $tmpdir = Join-Path $env:TEMP "glass_$PID_$(Get-Date -Format 'yyyyMMddHHmmss')"
    New-Item -ItemType Directory -Path $tmpdir -Force | Out-Null
    $Global:__GlassCaptureDir = $tmpdir

    $rewritten = @()
    for ($i = 0; $i -lt $stages.Count; $i++) {
        $stage = $stages[$i].Trim()
        $rewritten += $stage
        if ($i -lt ($stages.Count - 1)) {
            $rewritten += "| Tee-Object -FilePath '$tmpdir\stage_$i.txt'"
        }
    }

    $result = $rewritten -join ' '
    # Append stage emission after pipeline
    $result += "; __Glass-Emit-Stages '$tmpdir' $($stages.Count - 1)"
    return $result
}
```

### Pattern 3: OSC 133;S and 133;P Protocol Design
**What:** Two new OSC 133 sub-sequences for pipeline metadata and stage data.
**When to use:** Terminal-side parsing of pipeline capture data.

**Protocol design:**
```
OSC 133;S;{stage_count}\a          -- Pipeline start marker (emitted before stages)
OSC 133;P;{index};{size}\a{data}   -- Pipeline stage data (one per captured stage)
```

Wait -- embedding arbitrary data inside an OSC sequence is problematic because the data may contain the BEL (0x07) or ST (ESC \) terminators. This would prematurely terminate the OSC sequence.

**Better approach: Base64-encode the stage data.**

```
OSC 133;S;{stage_count}\a
OSC 133;P;{index};{total_bytes};{base64_data}\a
```

Or even simpler: since the OscScanner already accumulates bytes until BEL/ST, we can use a delimiter that won't appear in base64:

```
OSC 133;P;{index};{total_bytes};{base64_encoded_data}\a
```

Base64 encoding is safe because its character set (A-Z, a-z, 0-9, +, /, =) never includes BEL (0x07) or ESC (0x1b).

**However**, base64-encoding large stage output (up to 10MB) into a single OSC sequence is impractical. The OscScanner accumulates the entire payload in memory before parsing.

**Revised approach: Use temp file paths instead of inline data.**

```
OSC 133;S;{stage_count}\a
OSC 133;P;{index};{total_bytes};{temp_file_path}\a
```

The terminal reads the temp file to get the stage data. This avoids embedding large data in OSC sequences. The terminal is responsible for reading and cleaning up the temp files.

**This is the recommended approach.** It:
- Avoids OSC payload size issues
- Works naturally with the existing tee-to-temp-file capture
- Keeps the OSC protocol lightweight
- Allows the terminal to apply StageBuffer policies when reading

### Pattern 4: Terminal-Side Event Wiring
**What:** OscScanner parses OSC 133;S and 133;P into OscEvent variants, which flow through the existing event pipeline to update Block structs.
**When to use:** Always -- this is the terminal-side of the capture.

```rust
// In osc_scanner.rs - new OscEvent variants:
pub enum OscEvent {
    // ... existing variants ...

    /// OSC 133;S;{stage_count} - Pipeline with N stages detected
    PipelineStart { stage_count: usize },
    /// OSC 133;P;{index};{total_bytes};{temp_path} - Stage data available
    PipelineStage {
        index: usize,
        total_bytes: usize,
        temp_path: String,
    },
}

// In glass_core/event.rs - new ShellEvent variants:
pub enum ShellEvent {
    // ... existing variants ...
    PipelineStart { stage_count: usize },
    PipelineStage {
        index: usize,
        total_bytes: usize,
        temp_path: String,
    },
}

// In block_manager.rs - Block gets pipeline_stages:
pub struct Block {
    // ... existing fields ...
    /// Pipeline stage data (populated by OSC 133;P events)
    pub pipeline_stages: Vec<CapturedStage>,
}

// In glass_pipes/types.rs - new type:
pub struct CapturedStage {
    pub index: usize,
    pub total_bytes: usize,
    /// Finalized buffer data (read from temp file and processed through StageBuffer)
    pub data: FinalizedBuffer,
}
```

### Anti-Patterns to Avoid
- **Embedding large data in OSC sequences:** OSC payloads should be lightweight metadata. Use temp file paths for actual data transfer.
- **Modifying BASH_COMMAND in DEBUG trap:** This is not possible in bash. The DEBUG trap is read-only for the command.
- **Synchronous temp file reads in PTY reader thread:** The PTY reader thread must not block on file I/O. Read temp files on the main thread or a background thread when PipelineStage events are received.
- **Forgetting PIPESTATUS:** After tee rewriting, `$?` reflects tee's exit code, not the original command's. Must capture `${PIPESTATUS[@]}` immediately after the pipeline and restore the intended exit code.
- **Not cleaning up temp files:** Every pipeline execution creates temp files. Must clean up in PROMPT_COMMAND (bash) or prompt function (PowerShell), even if the command fails.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Pipe detection in shell scripts | Custom bash regex | glass_pipes::split_pipes logic (adapt to shell) or simple heuristic | Quote-aware pipe detection is already solved in Rust |
| Base64 encoding for OSC data | Custom encoder | Don't use base64 at all -- use temp file paths | Avoids payload size issues entirely |
| Temp file lifecycle management | Custom temp dir logic | `mktemp -d` in bash, `[System.IO.Path]::GetTempPath()` in PowerShell | OS handles permissions and cleanup |
| Pipeline stage parsing in terminal | New parser | Extend existing OscScanner with two new match arms | OscScanner architecture handles this cleanly |

**Key insight:** The heavy lifting happens in the shell scripts, not in Rust. The Rust side just needs to parse two new OSC sequences and wire them into existing event infrastructure.

## Common Pitfalls

### Pitfall 1: PIPESTATUS Corruption After Tee Rewriting
**What goes wrong:** After `cmd1 | tee /tmp/s0 | cmd2`, `$?` is tee's exit code (usually 0), hiding cmd1's failure.
**Why it happens:** Tee insertion adds stages to the pipeline, changing PIPESTATUS indices.
**How to avoid:** Immediately after the rewritten pipeline executes, capture `__glass_pipestatus=("${PIPESTATUS[@]}")` before any other command runs. The original stage exit codes are at even indices (0, 2, 4...) while tee exit codes are at odd indices (1, 3, 5...). Restore `$?` using `(exit ${__glass_pipestatus[0]})` or similar.
**Warning signs:** Tests where `false | grep foo` should show exit code 1 but shows 0.

### Pitfall 2: Recursive Rewriting
**What goes wrong:** If PROMPT_COMMAND or the emit function itself contains pipes, the shell integration might try to rewrite those too.
**Why it happens:** The DEBUG trap fires for ALL commands, including internal shell integration functions.
**How to avoid:** Use a guard variable `__glass_rewriting=1` during pipeline processing. Skip rewriting when the guard is set. Also skip rewriting for commands that start with `__glass_` (internal functions).
**Warning signs:** Infinite loops or doubled temp files.

### Pitfall 3: Temp File Path Spaces and Special Characters
**What goes wrong:** Temp file paths with spaces break the OSC 133;P payload parsing.
**Why it happens:** The OscScanner splits on `;` -- if the path contains `;` it breaks parsing.
**How to avoid:** Use a controlled temp directory path (no spaces, no semicolons). On Windows, `$TEMP` can contain spaces. Use a subdirectory under a known-safe location, or encode the path.
**Warning signs:** Stage data not appearing for some users.

### Pitfall 4: PowerShell Object Pipeline vs Text Pipeline
**What goes wrong:** Tee-Object in PowerShell serializes objects to text (via Out-File format), losing type information. The captured output may not match what the next stage receives.
**Why it happens:** PowerShell pipelines pass .NET objects, not text bytes.
**How to avoid:** Accept this limitation -- CAPT-02 explicitly says "text representation." Document that PowerShell capture shows the formatted text output, not the raw object. Use `Out-String` or `ConvertTo-Json` for more useful representations.
**Warning signs:** Captured output looks different from what the user sees in terminal.

### Pitfall 5: Large Pipeline Output Slowing Shell
**What goes wrong:** Emitting large OSC sequences (multi-MB base64) freezes the terminal.
**Why it happens:** OSC emission is synchronous in the shell.
**How to avoid:** The temp-file-path approach avoids this -- OSC payload is just a file path (short string). The terminal reads the file asynchronously. Additionally, apply the 10MB StageBuffer cap when reading temp files on the terminal side.
**Warning signs:** Terminal hangs after running `find / -type f | grep something`.

### Pitfall 6: Bash Version Compatibility
**What goes wrong:** PS0 requires bash >= 4.4. `bind -x` requires bash >= 4.0. `READLINE_LINE` requires bash >= 4.0. Some features may not work on older bash versions.
**Why it happens:** Glass already requires bash >= 4.4 for PS0 (existing glass.bash checks version).
**How to avoid:** Guard pipeline capture behind the same version check. If bash < 4.4, disable pipeline capture silently.
**Warning signs:** Errors on macOS default bash (3.2) -- but Glass already handles this.

## Code Examples

### Bash Pipeline Rewriting (shell-side)
```bash
# Detect if a command is a pipeline that should be captured
__glass_should_capture() {
    local cmd="$1"
    # Skip if --no-glass flag present
    [[ "$cmd" == *"--no-glass"* ]] && return 1
    # Skip internal functions
    [[ "$cmd" == __glass_* ]] && return 1
    # Skip if no unquoted pipes (simplified check)
    # Real implementation needs quote awareness
    echo "$cmd" | grep -qP '(?<![|\\])\|(?!\|)' || return 1
    return 0
}

# Rewrite a pipeline command with tee insertion
__glass_tee_rewrite() {
    local cmd="$1"
    local tmpdir="$2"
    local result=""
    local stage_idx=0
    local in_quote=0
    local quote_char=""
    local current=""
    local i=0
    local len=${#cmd}

    while [[ $i -lt $len ]]; do
        local c="${cmd:$i:1}"
        local next="${cmd:$((i+1)):1}"

        if [[ $in_quote -eq 1 ]]; then
            current+="$c"
            [[ "$c" == "$quote_char" ]] && in_quote=0
        elif [[ "$c" == "'" || "$c" == '"' ]]; then
            current+="$c"
            in_quote=1
            quote_char="$c"
        elif [[ "$c" == '|' && "$next" != '|' ]]; then
            # Pipe boundary -- insert tee before the pipe
            result+="${current} | tee '${tmpdir}/stage_${stage_idx}' |"
            current=""
            ((stage_idx++))
            ((i++))  # skip the pipe char
        elif [[ "$c" == '|' && "$next" == '|' ]]; then
            # Logical OR -- pass through
            current+="||"
            ((i+=2))
            continue
        else
            current+="$c"
        fi
        ((i++))
    done
    result+="$current"
    echo "$result"
}

# Emit captured stage data as OSC 133;P sequences
__glass_emit_stages() {
    local tmpdir="$1"
    [[ -z "$tmpdir" || ! -d "$tmpdir" ]] && return

    # Count stages
    local count=0
    while [[ -f "${tmpdir}/stage_${count}" ]]; do
        ((count++))
    done

    if [[ $count -gt 0 ]]; then
        # Emit pipeline start marker
        printf '\e]133;S;%d\e\\' "$count"

        # Emit each stage
        local i=0
        while [[ $i -lt $count ]]; do
            local size=$(wc -c < "${tmpdir}/stage_${i}" 2>/dev/null || echo 0)
            local path="${tmpdir}/stage_${i}"
            printf '\e]133;P;%d;%d;%s\e\\' "$i" "$size" "$path"
            ((i++))
        done
    fi

    # Don't clean up yet -- terminal needs to read the files
    # Terminal will clean up after reading, or cleanup happens on next prompt
}
```

### PowerShell Pipeline Rewriting
```powershell
function Global:__Glass-Rewrite-Pipeline {
    param([string]$Command)

    # Split on unquoted pipes (simplified)
    $stages = @()
    $current = ""
    $inSingle = $false
    $inDouble = $false

    for ($i = 0; $i -lt $Command.Length; $i++) {
        $c = $Command[$i]
        if ($c -eq "'" -and -not $inDouble) { $inSingle = -not $inSingle }
        elseif ($c -eq '"' -and -not $inSingle) { $inDouble = -not $inDouble }
        elseif ($c -eq '|' -and -not $inSingle -and -not $inDouble) {
            # Check for ||
            if ($i + 1 -lt $Command.Length -and $Command[$i + 1] -eq '|') {
                $current += '||'
                $i++
                continue
            }
            $stages += $current.Trim()
            $current = ""
            continue
        }
        $current += $c
    }
    $stages += $current.Trim()

    if ($stages.Count -le 1) { return $Command }

    $tmpdir = Join-Path ([System.IO.Path]::GetTempPath()) "glass_$PID"
    [System.IO.Directory]::CreateDirectory($tmpdir) | Out-Null
    $Global:__GlassCaptureDir = $tmpdir
    $Global:__GlassCaptureStageCount = $stages.Count - 1

    $parts = @()
    for ($i = 0; $i -lt $stages.Count; $i++) {
        $parts += $stages[$i]
        if ($i -lt ($stages.Count - 1)) {
            $path = Join-Path $tmpdir "stage_$i.txt"
            $parts += "| Tee-Object -FilePath '$path'"
        }
    }

    return ($parts -join ' ') + "; __Glass-Emit-Stages"
}

function Global:__Glass-Emit-Stages {
    $tmpdir = $Global:__GlassCaptureDir
    if (-not $tmpdir -or -not (Test-Path $tmpdir)) { return }

    $E = [char]0x1b
    $count = $Global:__GlassCaptureStageCount

    # Pipeline start marker
    [Console]::Write("$E]133;S;$count$([char]7)")

    for ($i = 0; $i -lt $count; $i++) {
        $path = Join-Path $tmpdir "stage_$i.txt"
        if (Test-Path $path) {
            $size = (Get-Item $path).Length
            [Console]::Write("$E]133;P;$i;$size;$path$([char]7)")
        }
    }

    $Global:__GlassCaptureDir = $null
}
```

### OscScanner Extension (Rust)
```rust
// In parse_osc133():
fn parse_osc133(params: &str) -> Option<OscEvent> {
    let mut parts = params.splitn(2, ';');
    let marker = parts.next()?;

    match marker {
        "A" => Some(OscEvent::PromptStart),
        "B" => Some(OscEvent::CommandStart),
        "C" => Some(OscEvent::CommandExecuted),
        "D" => {
            let exit_code = parts.next().and_then(|s| s.parse::<i32>().ok());
            Some(OscEvent::CommandFinished { exit_code })
        }
        "S" => {
            // Pipeline start: OSC 133;S;{count}
            let count = parts.next()?.parse::<usize>().ok()?;
            Some(OscEvent::PipelineStart { stage_count: count })
        }
        "P" => {
            // Pipeline stage: OSC 133;P;{index};{size};{path}
            let rest = parts.next()?;
            let mut sub = rest.splitn(3, ';');
            let index = sub.next()?.parse::<usize>().ok()?;
            let total_bytes = sub.next()?.parse::<usize>().ok()?;
            let temp_path = sub.next()?.to_string();
            Some(OscEvent::PipelineStage {
                index,
                total_bytes,
                temp_path,
            })
        }
        _ => None,
    }
}
```

### Block Struct Pipeline Stage Addition (Rust)
```rust
// In block_manager.rs:
use glass_pipes::CapturedStage;

pub struct Block {
    // ... existing fields ...
    /// Captured pipeline stages (empty for non-pipeline commands)
    pub pipeline_stages: Vec<CapturedStage>,
    /// Expected number of pipeline stages (from OSC 133;S)
    pub pipeline_stage_count: Option<usize>,
}

// In handle_event:
OscEvent::PipelineStart { stage_count } => {
    if let Some(idx) = self.current {
        if let Some(block) = self.blocks.get_mut(idx) {
            block.pipeline_stage_count = Some(stage_count);
            block.pipeline_stages = Vec::with_capacity(stage_count);
        }
    }
}
OscEvent::PipelineStage { index, total_bytes, temp_path } => {
    if let Some(idx) = self.current {
        if let Some(block) = self.blocks.get_mut(idx) {
            // Don't read file here -- just store the metadata
            // Main thread will read files when needed
            block.pipeline_stages.push(CapturedStage {
                index,
                total_bytes,
                data: FinalizedBuffer::Complete(Vec::new()), // placeholder
                temp_path: Some(temp_path),
            });
        }
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| shell integration emits only A/B/C/D/7 | Add S and P sub-sequences | Phase 16 | Two new OscEvent variants, backward compatible |
| Block has no pipeline awareness | Block gains pipeline_stages field | Phase 16 | Enables Phase 17 UI and Phase 18 storage |
| OutputBuffer captures whole-command output | pipeline stages captured per-stage | Phase 16 | Coexists with existing OutputBuffer (different granularity) |

## Open Questions

1. **Bash readline interception robustness**
   - What we know: `bind -x` with READLINE_LINE modification works in bash >= 4.0. This is how fzf and bash-preexec work.
   - What's unclear: Whether modifying READLINE_LINE in a bound function reliably changes the executed command in all bash versions (4.4+). Edge cases with multi-line commands.
   - Recommendation: Test with bash 4.4 and 5.x. Fall back to no-capture if READLINE_LINE modification doesn't work. This is flagged in STATE.md as a research concern.

2. **Temp file cleanup responsibility**
   - What we know: Shell creates temp files, terminal needs to read them.
   - What's unclear: Race condition between terminal reading and shell cleanup. Who cleans up if terminal crashes?
   - Recommendation: Shell defers cleanup to next PROMPT_COMMAND cycle (after terminal has had time to read). Terminal reads files immediately on PipelineStage event. Include a TMPDIR-based cleanup for stale files on terminal startup.

3. **PowerShell Tee-Object object serialization format**
   - What we know: Tee-Object serializes to file using Out-File formatting (text table format).
   - What's unclear: Whether ConvertTo-Json or Export-Csv would be more useful for AI inspection via MCP.
   - Recommendation: Use default Tee-Object (Out-File format) for v1.3. This matches what users see in their terminal. Future enhancement (PS-01, PS-02) can add object type info.

4. **Should glass_pipes crate gain a `rewrite` module?**
   - What we know: The tee rewriting logic could live in Rust (glass_pipes::rewrite) and be called from shell scripts, or live entirely in shell scripts.
   - What's unclear: Whether it's worth the complexity of having Rust generate rewritten bash/PS commands.
   - Recommendation: Keep rewriting logic in shell scripts. It's simpler and avoids needing `glass pipes rewrite "cmd"` CLI subcommand. The shell scripts already handle quote-aware logic.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[cfg(test)]` + cargo test; shell script manual validation |
| Config file | None needed |
| Quick run command | `cargo test -p glass_terminal -- osc_scanner` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| CAPT-01 | Bash tee rewriting produces correct command | unit (shell) | Manual: `source glass.bash; __glass_tee_rewrite "cat f \| grep x" /tmp/test` | No -- Wave 0 |
| CAPT-01 | PIPESTATUS preserved after tee pipeline | integration | Manual: run pipeline in Glass terminal, check exit code | No -- Wave 0 |
| CAPT-01 | OSC 133;P sequences emitted with correct format | unit | `cargo test -p glass_terminal -- osc_scanner::pipeline` | No -- Wave 0 |
| CAPT-02 | PowerShell Tee-Object captures stage text | integration | Manual: run pipeline in Glass terminal with PowerShell | No -- Wave 0 |
| CAPT-02 | OSC 133;S and 133;P parsed by OscScanner | unit | `cargo test -p glass_terminal -- osc_scanner::pipeline` | No -- Wave 0 |
| CAPT-01/02 | Block.pipeline_stages populated from events | unit | `cargo test -p glass_terminal -- block_manager::pipeline` | No -- Wave 0 |
| CAPT-01/02 | ShellEvent::PipelineStart/Stage variants exist | unit | `cargo test -p glass_core` | No -- Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_terminal && cargo test -p glass_core`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green + manual shell integration test before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_terminal/src/osc_scanner.rs` -- tests for OSC 133;S and 133;P parsing
- [ ] `crates/glass_terminal/src/block_manager.rs` -- tests for PipelineStart/PipelineStage handling
- [ ] `crates/glass_core/src/event.rs` -- new ShellEvent variants (compile-time validated)
- [ ] `crates/glass_pipes/src/types.rs` -- CapturedStage type
- [ ] `shell-integration/glass.bash` -- manual validation of pipeline rewriting
- [ ] `shell-integration/glass.ps1` -- manual validation of Tee-Object capture

## Sources

### Primary (HIGH confidence)
- `shell-integration/glass.bash` -- existing bash integration, PS0 pattern, PROMPT_COMMAND chain
- `shell-integration/glass.ps1` -- existing PowerShell integration, PSReadLine Enter handler
- `crates/glass_terminal/src/osc_scanner.rs` -- existing OscScanner with parse_osc133()
- `crates/glass_terminal/src/block_manager.rs` -- Block struct and handle_event()
- `crates/glass_terminal/src/pty.rs` -- PTY reader thread, OscScanner integration
- `crates/glass_core/src/event.rs` -- ShellEvent and AppEvent enums
- `crates/glass_pipes/src/types.rs` -- Pipeline, StageBuffer, FinalizedBuffer types
- `src/main.rs:726-950` -- Shell event handling, command text extraction, history recording

### Secondary (MEDIUM confidence)
- [Bash PIPESTATUS documentation](https://www.baeldung.com/linux/exit-status-piped-processes) -- PIPESTATUS array behavior
- [PowerShell Tee-Object docs](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.utility/tee-object?view=powershell-7.5) -- Tee-Object -FilePath usage
- [OSC 133 shell integration protocol](https://gist.github.com/tep/e3f3d384de40dbda932577c7da576ec3) -- FinalTerm/iTerm2 OSC 133 specification
- [WezTerm shell integration](https://wezterm.org/shell-integration.html) -- Modern terminal OSC 133 implementation

### Tertiary (LOW confidence)
- Bash `bind -x` with READLINE_LINE modification -- technique used by fzf and bash-preexec, but exact behavior across bash versions needs validation
- DEBUG trap capabilities -- cannot modify BASH_COMMAND, confirmed by multiple sources

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - no new dependencies, extending existing infrastructure
- Architecture (OSC protocol + terminal side): HIGH - follows existing OscScanner patterns exactly
- Architecture (bash rewriting): MEDIUM - bind -x approach is well-known but Glass-specific integration needs testing
- Architecture (PowerShell capture): HIGH - PSReadLine handler already exists, Tee-Object is straightforward
- Pitfalls: HIGH - exit code preservation and temp file lifecycle are well-documented concerns

**Research date:** 2026-03-05
**Valid until:** 2026-04-05 (stable domain, no fast-moving dependencies)
