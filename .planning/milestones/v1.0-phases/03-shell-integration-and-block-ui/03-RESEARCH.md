# Phase 3: Shell Integration and Block UI - Research

**Researched:** 2026-03-04
**Domain:** OSC escape sequence parsing, shell integration scripts, block-based terminal rendering, status bar with async git
**Confidence:** HIGH

## Summary

Phase 3 transforms Glass from a flat terminal into a block-structured command viewer. The shell emits OSC 133 sequences to mark prompt/command/output boundaries, and OSC 7 to report CWD. Glass parses these from the raw PTY byte stream, maintains a BlockManager state machine tracking command lifecycle, and renders each block with visual separators, exit code badges, and duration labels. A persistent status bar shows CWD and git branch info.

The critical architectural finding is that **alacritty_terminal 0.25.1 does NOT handle OSC 133 or OSC 7** -- both fall through to an `unhandled` debug log in the VTE parser. Glass must pre-scan the raw PTY byte stream with a lightweight custom scanner that extracts these sequences before (or alongside) alacritty_terminal's processing. This avoids forking alacritty_terminal while capturing the metadata Glass needs.

**Primary recommendation:** Build a `OscScanner` that processes the same PTY bytes the alacritty event loop reads, extracting OSC 133 A/B/C/D and OSC 7 events into a channel. The BlockManager consumes these events and maintains block state. The renderer queries BlockManager alongside GridSnapshot to draw block decorations.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SHEL-01 | Parse OSC 133 sequences for command lifecycle | OscScanner pre-scan architecture; OSC 133 A/B/C/D spec documented below |
| SHEL-02 | Parse OSC 7 sequences for CWD tracking | OscScanner handles OSC 7 file:// URLs; also support OSC 9;9 for Windows compat |
| SHEL-03 | PowerShell integration script emitting OSC 133/7 | Windows Terminal reference script documented; Oh My Posh/Starship wrapping pattern |
| SHEL-04 | Bash integration script emitting OSC 133/7 | PROMPT_COMMAND + PS0 pattern documented; bash >= 4.4 required for PS0 |
| BLOK-01 | Each command renders as a visually distinct block | BlockManager state machine + renderer block decorations (separator lines) |
| BLOK-02 | Exit code badge (green check / red X) | Block stores exit_code from OSC 133;D; renderer draws colored badge |
| BLOK-03 | Command wall-clock duration | Block tracks start_time (OSC 133;C) and end_time (OSC 133;D); renderer draws label |
| STAT-01 | Status bar shows CWD from OSC 7 | Status bar rendered as bottom-pinned rect; CWD updated from OscScanner events |
| STAT-02 | Status bar shows git branch + dirty count | Async subprocess spawns `git rev-parse --abbrev-ref HEAD` and `git status --porcelain`; debounced on CWD change |
</phase_requirements>

## Standard Stack

### Core (already in workspace)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| alacritty_terminal | =0.25.1 | VTE parsing, terminal grid | Already used; handles all terminal emulation except OSC 133/7 |
| wgpu | 28.0.0 | GPU rendering | Already used for rect + text rendering pipeline |
| glyphon | 0.10.0 | Text rendering | Already used for terminal text |
| tokio | 1.50.0 | Async runtime | Already in workspace; use for async git subprocess |

### Supporting (new for Phase 3)
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| std::time::Instant | stdlib | Command duration measurement | Start timer on OSC 133;C, stop on OSC 133;D |
| std::process::Command | stdlib | Sync git subprocess | For git branch/dirty queries (spawn on std::thread, not tokio) |
| url (crate) | 2.x | Parse file:// URLs from OSC 7 | Extract path from `file://hostname/path` |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Custom OscScanner | Fork alacritty_terminal | Forking adds maintenance burden; pre-scanning is non-invasive |
| url crate for OSC 7 | Manual string parsing | url crate handles percent-decoding and edge cases correctly |
| std::thread for git | tokio::process::Command | Project uses dedicated threads for I/O (PTY pattern); git queries are infrequent |

**Installation:**
```bash
cargo add url@2
```

## Architecture Patterns

### Recommended Project Structure
```
crates/glass_terminal/src/
    osc_scanner.rs     # Lightweight byte scanner for OSC 133/7
    block_manager.rs   # State machine tracking command blocks
    status.rs          # CWD + git branch state

crates/glass_renderer/src/
    block_renderer.rs  # Block separators, badges, duration labels
    status_bar.rs      # Bottom status bar rendering

shell-integration/
    glass.ps1          # PowerShell integration script
    glass.bash         # Bash integration script
```

### Pattern 1: OSC Pre-Scanner (Byte Stream Tap)

**What:** A lightweight state machine that scans the raw PTY byte stream for `\x1b]133;` and `\x1b]7;` patterns, extracting structured events without interfering with alacritty_terminal's parsing.

**When to use:** Every time PTY bytes are read, before or after they pass through alacritty_terminal.

**Architecture decision:** The alacritty_terminal `EventLoop` reads PTY bytes in its own thread and calls `parser.advance(&mut **terminal, &buf[..unprocessed])`. Glass cannot easily intercept this without forking alacritty_terminal. The recommended approach is:

1. **Option A (Recommended): Fork the event_loop only** -- Create a custom `GlassEventLoop` that wraps alacritty_terminal's PTY reading but adds a pre-scan step. Copy the `pty_read` method pattern, adding an `osc_scanner.scan(&buf[..unprocessed])` call before `parser.advance()`.

2. **Option B: Dual-read with shared buffer** -- Impossible because the PTY is a single reader.

3. **Option C: Post-process via VTE Perform wrapper** -- Too complex; requires intercepting at the Processor/Handler layer which is deeply coupled.

**Option A detail:** The key lines in alacritty_terminal's event_loop.rs (line 154):
```rust
// Original alacritty_terminal code:
state.parser.advance(&mut **terminal, &buf[..unprocessed]);

// Glass modification (in custom GlassEventLoop):
osc_scanner.scan(&buf[..unprocessed], &event_sender);
state.parser.advance(&mut **terminal, &buf[..unprocessed]);
```

**Critical finding from source inspection:** The `EventLoop::spawn()` method consumes `self` and runs on a dedicated thread. The `pty_read` method is the hot path. Glass should create its own event loop struct that replicates this pattern but adds the scanner call. The alacritty_terminal `EventLoop` struct fields (`poll`, `pty`, `rx`, `tx`, `terminal`, `event_proxy`) are all constructible from the public API.

```rust
// OscScanner: lightweight state machine
pub struct OscScanner {
    state: ScanState,
    buffer: Vec<u8>,
}

enum ScanState {
    Ground,
    Escape,        // saw \x1b
    OscStart,      // saw \x1b]
    Accumulating,  // reading OSC payload until ST (\x1b\\ or \x07)
}

pub enum OscEvent {
    PromptStart,                          // 133;A
    CommandStart,                         // 133;B
    CommandExecuted,                      // 133;C
    CommandFinished { exit_code: Option<i32> }, // 133;D[;exit_code]
    CurrentDirectory(String),             // 7;file://host/path
}
```

### Pattern 2: BlockManager State Machine

**What:** Tracks the lifecycle of command blocks based on OSC events.

**When to use:** Consumes OscEvents from the scanner, maintains a list of blocks with their line ranges and metadata.

```rust
pub struct Block {
    pub prompt_start_line: usize,     // Line where 133;A was received
    pub command_start_line: usize,    // Line where 133;B was received
    pub output_start_line: Option<usize>,  // Line where 133;C was received
    pub output_end_line: Option<usize>,    // Line where 133;D was received
    pub exit_code: Option<i32>,
    pub started_at: Option<Instant>,  // Set on 133;C
    pub finished_at: Option<Instant>, // Set on 133;D
    pub state: BlockState,
}

pub enum BlockState {
    PromptActive,      // After A, before B
    InputActive,       // After B, before C
    Executing,         // After C, before D
    Complete,          // After D
}

pub struct BlockManager {
    blocks: Vec<Block>,
    current: Option<usize>,
}
```

**Line tracking:** The scanner needs to know the current terminal line when each OSC event fires. Since the scanner runs on the PTY reader thread, it can query `term.lock().cursor().point.line` at the moment each OSC event is detected. This maps OSC events to grid lines.

### Pattern 3: Block-Aware Rendering

**What:** The renderer queries BlockManager to draw visual separators, exit code badges, and duration labels between/within blocks.

**When to use:** During `draw_frame()`, after building the normal grid content.

```rust
// In FrameRenderer::draw_frame(), after normal grid rendering:
for block in block_manager.visible_blocks(display_offset, screen_lines) {
    // 1. Draw horizontal separator line above block
    rects.push(separator_rect(block.prompt_start_line, viewport_width));

    // 2. Draw exit code badge (right-aligned on the separator line)
    if let Some(exit_code) = block.exit_code {
        let badge_color = if exit_code == 0 { GREEN } else { RED };
        let badge_text = if exit_code == 0 { "✓" } else { "✗" };
        // Render badge rect + text
    }

    // 3. Draw duration label
    if let (Some(start), Some(end)) = (block.started_at, block.finished_at) {
        let duration = end - start;
        let label = format_duration(duration); // "1.2s", "45ms", etc.
        // Render duration text right-aligned
    }
}
```

### Pattern 4: Status Bar (Bottom-Pinned)

**What:** A fixed-height bar at the bottom of the viewport showing CWD and git info.

**Architecture:** Reserve the bottom N pixels (1 cell height) of the viewport for the status bar. Reduce the terminal grid height by 1 line to make room.

```rust
pub struct StatusBar {
    cwd: String,
    git_branch: Option<String>,
    git_dirty_count: Option<usize>,
    git_query_pending: bool,
}
```

**Git info fetching:** Spawn a background thread on each CWD change (debounced). Run:
- `git rev-parse --abbrev-ref HEAD` for branch name
- `git status --porcelain` and count lines for dirty count

Send results back via the winit event loop proxy (new `AppEvent::GitStatus` variant).

### Anti-Patterns to Avoid
- **Forking alacritty_terminal entirely:** Only replicate the event loop; keep using alacritty_terminal for all VTE parsing and grid management
- **Blocking the render thread for git queries:** Always run git subprocesses on a background thread
- **Parsing OSC sequences after they reach the terminal grid:** The sequences are consumed/dropped by the VTE parser; they never appear in the grid
- **Using PSReadLine hooks for OSC 133;C:** PSReadLine's `CommandValidation` and `AcceptLine` handlers are unreliable across versions; use PowerShell's native prompt function wrapping instead

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| URL parsing for OSC 7 | Custom file:// parser | `url` crate | Handles percent-encoding, hostname, edge cases |
| Duration formatting | Custom formatter | Simple match on seconds/millis | Only need "1.2s", "45ms", "2m 30s" -- trivial |
| VTE escape parsing | Custom full VTE parser | `vte` crate (already in alacritty_terminal) | Battle-tested state machine; only pre-scan for OSC 133/7 |
| Git branch detection | libgit2 / git2 crate | `git` CLI subprocess | CLI is simpler, always matches user's git version, no native dep |

**Key insight:** The OscScanner is intentionally NOT a full VTE parser. It only needs to detect `\x1b]133;...(\x07|\x1b\\)` and `\x1b]7;...(\x07|\x1b\\)` patterns. This is a simple state machine, not a complex parser.

## Common Pitfalls

### Pitfall 1: OSC Sequences Split Across PTY Read Buffers
**What goes wrong:** An OSC 133 sequence may be split across two consecutive `read()` calls from the PTY. The scanner sees `\x1b]133;` in one buffer and `A\x07` in the next.
**Why it happens:** PTY reads are arbitrary-length; escape sequences have no alignment guarantees.
**How to avoid:** The OscScanner must buffer partial sequences. When it sees `\x1b]` but no terminator (`\x07` or `\x1b\\`) before end of buffer, it must save the partial data and continue scanning on the next buffer.
**Warning signs:** Intermittent missed block boundaries, especially with fast command output.

### Pitfall 2: Line Number Tracking During Rapid Output
**What goes wrong:** The scanner detects an OSC event but associates it with the wrong terminal line because the grid has scrolled since the bytes were written.
**Why it happens:** The PTY reader thread processes bytes in chunks. By the time the scanner runs, multiple lines may have been written.
**How to avoid:** Track line numbers at scan time by querying the terminal cursor position immediately when the OSC sequence is found in the byte stream (before `parser.advance()` processes the subsequent bytes).
**Warning signs:** Block separators appear at wrong positions; blocks overlap.

### Pitfall 3: Oh My Posh / Starship Prompt Wrapping Order
**What goes wrong:** Glass's shell integration script runs before Oh My Posh/Starship initializes, so the prompt customizer overwrites Glass's prompt function, losing OSC 133 markers.
**Why it happens:** PowerShell profile execution order matters. If `oh-my-posh init` runs after Glass's prompt wrapper, it replaces the prompt function entirely.
**How to avoid:** Glass's integration script must wrap AROUND the existing prompt, not replace it. The pattern from Windows Terminal docs:
1. Let Oh My Posh/Starship set their prompt first
2. Stash `$function:Prompt` into `$Global:__OriginalPrompt`
3. Define new `prompt` that emits OSC 133;A, calls `$Global:__OriginalPrompt.Invoke()`, then emits OSC 133;B
**Warning signs:** Prompt appears but blocks are never detected; OSC 133 sequences missing from PTY output.

### Pitfall 4: PowerShell Exit Code Detection is Complex
**What goes wrong:** `$LASTEXITCODE` is only set by external programs; PowerShell cmdlet errors set `$?` to `$false` but leave `$LASTEXITCODE` unchanged.
**Why it happens:** PowerShell has two separate error mechanisms: `$?` (success/failure of last operation) and `$LASTEXITCODE` (exit code of last external program).
**How to avoid:** Use the Windows Terminal pattern: check `$?` first; if true, exit code is 0. If false, check `$Error[0].InvocationInfo.HistoryId` against the last history entry to distinguish PowerShell errors (-1) from external program errors (`$LASTEXITCODE`).
**Warning signs:** All commands show exit code 0 even when they fail; or all show -1.

### Pitfall 5: Status Bar Steals Terminal Line
**What goes wrong:** Reducing terminal grid height by 1 for the status bar causes content to reflow, and the PTY sees a different terminal size than expected.
**Why it happens:** The PTY was told the terminal is N lines, but only N-1 are available for content.
**How to avoid:** Actually resize the PTY to N-1 lines and reserve the bottom line for the status bar in the renderer. The PTY resize message must reflect the actual content area.
**Warning signs:** Last line of terminal content hidden behind status bar; scrollback offset wrong.

### Pitfall 6: Git Queries on Non-Git Directories
**What goes wrong:** Running `git rev-parse` in a non-git directory returns exit code 128 with error output to stderr.
**Why it happens:** CWD changes to a directory outside any git repo.
**How to avoid:** Check git exit code; if non-zero, clear git info in status bar. Also set `GIT_OPTIONAL_LOCKS=0` env var to prevent git from taking locks during read-only queries.
**Warning signs:** Error messages in logs every time CWD changes; status bar shows stale git info.

### Pitfall 7: Custom Event Loop vs alacritty_terminal's EventLoop
**What goes wrong:** Glass creates its own event loop but misses behavior from alacritty_terminal's event loop (drain_on_exit, sync handling, poll registration).
**Why it happens:** alacritty_terminal's EventLoop has subtle platform-specific behavior (polling, read buffering, sync byte tracking).
**How to avoid:** Study the alacritty_terminal EventLoop source carefully. The key functionality to replicate: (1) PTY registration with poller, (2) read buffer management with MAX_LOCKED_READ, (3) sync bytes tracking for batch updates, (4) drain_on_exit behavior, (5) write queue management. Consider keeping alacritty_terminal's EventLoop but wrapping the PTY reader to inject scanning.
**Warning signs:** Terminal hangs, missed output, or excessive CPU usage.

## Code Examples

### OSC 133 Escape Sequences (verified from Windows Terminal docs + FinalTerm spec)
```
ESC ] 133 ; A BEL          -- Prompt start (FTCS_PROMPT)
ESC ] 133 ; B BEL          -- Command start / prompt end (FTCS_COMMAND_START)
ESC ] 133 ; C BEL          -- Command executed / output start (FTCS_COMMAND_EXECUTED)
ESC ] 133 ; D ; <code> BEL -- Command finished with exit code (FTCS_COMMAND_FINISHED)
ESC ] 133 ; D BEL          -- Command finished, no exit code

Where:
  ESC = \x1b
  BEL = \x07
  ST  = \x1b\\  (alternative terminator)
```

### OSC 7 CWD Reporting
```
ESC ] 7 ; file://hostname/path/to/dir BEL
ESC ] 7 ; file://hostname/path/to/dir ST

Note: On Windows, path may be /C:/Users/... (leading slash before drive letter)
Characters in path should be percent-encoded per RFC 3986.
```

### PowerShell Integration Script (adapted from Windows Terminal reference)
```powershell
# Glass shell integration for PowerShell
# Source: Windows Terminal docs, adapted for Glass

$Global:__GlassLastHistoryId = -1

function Global:__Glass-Get-LastExitCode {
    if ($? -eq $True) { return 0 }
    $LastHistoryEntry = $(Get-History -Count 1)
    if ($Error.Count -gt 0 -and $Error[0].InvocationInfo.HistoryId -eq $LastHistoryEntry.Id) {
        return -1
    }
    if ($null -ne $LastExitCode) { return $LastExitCode }
    return -1
}

# Stash existing prompt (Oh My Posh / Starship compatibility)
if ($function:Prompt) {
    $Global:__GlassOriginalPrompt = $function:Prompt
}

function prompt {
    $gle = $(__Glass-Get-LastExitCode)
    $LastHistoryEntry = $(Get-History -Count 1)

    $out = ""

    # End previous command (133;D with exit code)
    if ($Global:__GlassLastHistoryId -ne -1) {
        if ($LastHistoryEntry.Id -eq $Global:__GlassLastHistoryId) {
            $out += "`e]133;D`a"
        } else {
            $out += "`e]133;D;$gle`a"
        }
    }

    # Report CWD via OSC 7
    $loc = $executionContext.SessionState.Path.CurrentLocation
    $out += "`e]7;file://$($env:COMPUTERNAME)/$($loc.Path.Replace('\','/'))`a"

    # Prompt start (133;A)
    $out += "`e]133;A`a"

    # Call original prompt (preserves Oh My Posh / Starship styling)
    if ($Global:__GlassOriginalPrompt) {
        $out += & $Global:__GlassOriginalPrompt
    } else {
        $out += "PS $loc> "
    }

    # Command start (133;B)
    $out += "`e]133;B`a"

    $Global:__GlassLastHistoryId = $LastHistoryEntry.Id
    return $out
}

# PSReadLine hook for 133;C (command executed)
# AcceptLine handler emits 133;C just before the command runs
if (Get-Module PSReadLine) {
    Set-PSReadLineKeyHandler -Key Enter -ScriptBlock {
        [Microsoft.PowerShell.PSConsoleReadLine]::AcceptLine()
        [Console]::Write("`e]133;C`a")
    }
}
```

### Bash Integration Script
```bash
# Glass shell integration for Bash (>= 4.4 for PS0 support)

__glass_osc7() {
    printf '\e]7;file://%s%s\e\\' "$HOSTNAME" "$PWD"
}

__glass_prompt_command() {
    local exit_code=$?
    # End previous command
    printf '\e]133;D;%d\e\\' "$exit_code"
    # Report CWD
    __glass_osc7
    # Mark prompt start
    PS1='\[\e]133;A\e\\\]'"${__GLASS_ORIGINAL_PS1:-\\s-\\v\\$ }"'\[\e]133;B\e\\\]'
}

# Stash original PS1 for prompt customizer compatibility
__GLASS_ORIGINAL_PS1="$PS1"

PROMPT_COMMAND="__glass_prompt_command${PROMPT_COMMAND:+;$PROMPT_COMMAND}"

# PS0 emits 133;C when command starts executing (bash >= 4.4)
if [[ "${BASH_VERSINFO[0]}" -ge 5 ]] || \
   [[ "${BASH_VERSINFO[0]}" -eq 4 && "${BASH_VERSINFO[1]}" -ge 4 ]]; then
    PS0='\[\e]133;C\e\\\]'
fi
```

### Git Status Query (background thread)
```rust
use std::process::Command;

pub struct GitInfo {
    pub branch: String,
    pub dirty_count: usize,
}

pub fn query_git_status(cwd: &str) -> Option<GitInfo> {
    // Get branch name
    let branch_output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(cwd)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .ok()?;

    if !branch_output.status.success() {
        return None; // Not a git repo
    }

    let branch = String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .to_string();

    // Get dirty file count
    let status_output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(cwd)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .ok()?;

    let dirty_count = status_output.stdout
        .split(|&b| b == b'\n')
        .filter(|line| !line.is_empty())
        .count();

    Some(GitInfo { branch, dirty_count })
}
```

### Duration Formatting
```rust
fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 0.001 { return "<1ms".to_string(); }
    if secs < 1.0 { return format!("{:.0}ms", secs * 1000.0); }
    if secs < 60.0 { return format!("{:.1}s", secs); }
    let mins = secs as u64 / 60;
    let rem = secs as u64 % 60;
    format!("{}m {}s", mins, rem)
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| No semantic prompts | OSC 133 (FinalTerm spec) adopted by all major terminals | 2022-2023 | Standard way for shells to communicate command boundaries |
| OSC 9;9 for CWD (ConEmu) | OSC 7 file:// URL (macOS Terminal convention) | Ongoing | OSC 7 is more widely adopted; support both for Windows compat |
| PSReadLine PreExecution handler | PSReadLine AcceptLine key handler + prompt wrapping | 2023+ | More reliable across PSReadLine versions |

**Deprecated/outdated:**
- ConEmu's OSC 9;9 for CWD: Still used by Windows Terminal's PowerShell examples, but OSC 7 is the cross-platform standard. Glass should support both.
- FinalTerm's original broader semantic prompt spec: Only A/B/C/D markers are widely adopted. Other FinalTerm sequences (like `133;P`) are WezTerm-specific extensions.

## Open Questions

1. **PSReadLine AcceptLine hook reliability**
   - What we know: The Enter key handler approach works in Windows Terminal's integration
   - What's unclear: Whether this conflicts with PSReadLine's predictive IntelliSense or custom AcceptLine handlers users may have
   - Recommendation: Test with PSReadLine 2.x on PowerShell 7; provide fallback that omits 133;C if PSReadLine is not loaded (blocks will still work but won't have precise output-start timing)

2. **Custom EventLoop vs wrapping alacritty_terminal's EventLoop**
   - What we know: alacritty_terminal's `EventLoop::spawn()` consumes `self` and runs on its own thread. The `pty_read` method is the insertion point.
   - What's unclear: Whether we can inject scanning without replicating the entire event loop
   - Recommendation: The most maintainable approach is to replicate the `pty_read` + `spawn` pattern in a `GlassEventLoop` that wraps alacritty_terminal's PTY but uses its own read loop. The `polling` crate, `FairMutex`, and VTE parser are all publicly accessible. This avoids forking but requires ~150 lines of event loop code.

3. **Block line range accuracy with scrollback**
   - What we know: Blocks are identified by terminal line numbers at the time OSC events fire
   - What's unclear: How scrollback affects line numbering; whether alacritty_terminal's `display_offset` is sufficient
   - Recommendation: Store absolute line indices (history line + screen line) in Block structs, not display-relative indices. Convert to display-relative during rendering.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in Rust test framework) |
| Config file | Cargo.toml workspace test configuration |
| Quick run command | `cargo test -p glass_terminal --lib` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SHEL-01 | OSC 133 parsing | unit | `cargo test -p glass_terminal osc_scanner -- --nocapture` | No - Wave 0 |
| SHEL-02 | OSC 7 CWD parsing | unit | `cargo test -p glass_terminal osc_scanner -- --nocapture` | No - Wave 0 |
| SHEL-03 | PowerShell script correctness | manual | Run Glass with pwsh, execute commands, observe blocks | N/A manual |
| SHEL-04 | Bash script correctness | manual | Run Glass with bash, execute commands, observe blocks | N/A manual |
| BLOK-01 | Block visual separation | manual | Visual inspection: blocks have separator lines | N/A manual |
| BLOK-02 | Exit code badge | unit + manual | `cargo test -p glass_terminal block_manager -- --nocapture` + visual | No - Wave 0 |
| BLOK-03 | Duration display | unit | `cargo test -p glass_terminal block_manager -- --nocapture` | No - Wave 0 |
| STAT-01 | Status bar CWD | unit + manual | `cargo test -p glass_terminal status -- --nocapture` + visual | No - Wave 0 |
| STAT-02 | Git branch/dirty | unit | `cargo test -p glass_terminal status -- --nocapture` | No - Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_terminal --lib`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_terminal/src/osc_scanner.rs` + tests -- OSC 133/7 byte parsing
- [ ] `crates/glass_terminal/src/block_manager.rs` + tests -- Block state machine
- [ ] `crates/glass_terminal/src/status.rs` + tests -- CWD/git state management
- [ ] `shell-integration/glass.ps1` -- PowerShell integration script
- [ ] `shell-integration/glass.bash` -- Bash integration script

## Sources

### Primary (HIGH confidence)
- alacritty_terminal 0.25.1 source: `/c/Users/nkngu/.cargo/registry/src/.../alacritty_terminal-0.25.1/` -- verified OSC handler does NOT support 133/7
- vte 0.15.0 source: verified `osc_dispatch` function handles OSC 0/2/4/8/10-12/22/50/52/104/110/111/112 only
- [Windows Terminal Shell Integration Docs](https://learn.microsoft.com/en-us/windows/terminal/tutorials/shell-integration) -- Reference PowerShell + Bash scripts, OSC 133 spec
- [WezTerm Shell Integration](https://wezterm.org/shell-integration.html) -- Cross-reference implementation

### Secondary (MEDIUM confidence)
- [FinalTerm Semantic Prompts Spec](https://gitlab.freedesktop.org/Per_Bothner/specifications/blob/master/proposals/semantic-prompts.md) -- Original spec (referenced but not directly fetched)
- [Oh My Posh shell integration issue #3795](https://github.com/JanDeDobbeleer/oh-my-posh/issues/3795) -- OMP OSC 133 compatibility status
- [Starship advanced config](https://starship.rs/advanced-config/) -- Invoke-Starship-PreCommand hook

### Tertiary (LOW confidence)
- PSReadLine AcceptLine handler for 133;C -- based on Windows Terminal docs pattern, needs testing with current PSReadLine versions

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - using existing workspace deps; only adding `url` crate
- Architecture: HIGH - verified alacritty_terminal source; OSC pre-scanner is well-understood pattern
- OSC 133/7 spec: HIGH - verified against Windows Terminal docs and multiple terminal implementations
- Shell integration scripts: MEDIUM - adapted from Windows Terminal; Oh My Posh/Starship wrapping needs runtime testing
- Pitfalls: HIGH - identified from source code inspection and cross-terminal implementation patterns

**Research date:** 2026-03-04
**Valid until:** 2026-04-04 (stable domain; OSC 133 spec is settled)
