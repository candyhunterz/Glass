---
phase: 03-shell-integration-and-block-ui
verified: 2026-03-04T23:00:00Z
status: human_needed
score: 7/7 must-haves verified (automated)
must_haves:
  truths:
    - "OscScanner correctly parses OSC 133 A/B/C/D sequences from raw byte streams"
    - "OscScanner correctly parses OSC 7 file:// CWD sequences"
    - "OscScanner handles sequences split across buffer boundaries"
    - "BlockManager tracks command lifecycle through PromptActive -> InputActive -> Executing -> Complete"
    - "Block stores exit code from OSC 133;D and timestamps for duration calculation"
    - "StatusState tracks CWD from OSC 7 events"
    - "Git info can be queried asynchronously from a CWD path"
    - "Each command block is visually separated by a horizontal line"
    - "Exit code badge renders green checkmark for 0 and red X for non-zero"
    - "Duration label renders right-aligned on the separator line"
    - "Status bar renders at the bottom of the viewport with CWD text"
    - "Status bar shows git branch and dirty count when available"
    - "PowerShell integration script emits OSC 133 A/B/C/D sequences around prompt and command execution"
    - "PowerShell integration script emits OSC 7 with CWD on each prompt"
    - "PowerShell integration script preserves Oh My Posh and Starship prompt styling"
    - "PowerShell integration script correctly detects exit codes for both cmdlets and external programs"
    - "Bash integration script emits OSC 133 A/B/C/D sequences via PROMPT_COMMAND and PS0"
    - "Bash integration script emits OSC 7 with CWD"
    - "Bash integration script preserves existing PS1 for prompt customizer compatibility"
    - "OscScanner pre-scans PTY bytes before alacritty_terminal processes them"
    - "OscEvents flow from PTY reader thread to main thread via AppEvent channel"
    - "BlockManager and StatusState are updated from OscEvents on the main thread"
    - "FrameRenderer receives block and status data during draw_frame"
    - "Status bar reduces terminal grid height by 1 line"
    - "Git info is queried asynchronously on CWD change and sent back via AppEvent"
human_verification:
  - test: "Source glass.ps1, run commands, verify block separators with exit code badges and duration labels render correctly"
    expected: "Each command shows as a visually distinct block with green OK badge for success, red X for failure, and wall-clock duration"
    why_human: "Visual rendering correctness cannot be verified programmatically"
  - test: "Change directory and verify status bar updates with CWD and git info"
    expected: "Status bar at bottom shows current directory and git branch with dirty count"
    why_human: "Requires live terminal interaction and visual inspection"
  - test: "Scroll back through output and verify block decorations stay aligned"
    expected: "Block separators and badges remain aligned with their content during scrollback"
    why_human: "display_offset=0 hardcoded in frame.rs block rendering -- scroll alignment may be broken"
---

# Phase 3: Shell Integration and Block UI Verification Report

**Phase Goal:** Shell integration with OSC parsing, command block tracking, block UI decorations, status bar, and shell scripts
**Verified:** 2026-03-04
**Status:** human_needed
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | OscScanner correctly parses OSC 133 A/B/C/D sequences | VERIFIED | 13 passing tests in osc_scanner.rs covering all variants |
| 2 | OscScanner correctly parses OSC 7 file:// CWD sequences | VERIFIED | Tests for BEL and ST terminators, Windows path prefix handling |
| 3 | OscScanner handles split-buffer edge cases | VERIFIED | split_buffer_at_payload and split_buffer_mid_osc tests pass |
| 4 | BlockManager tracks full command lifecycle | VERIFIED | 8 tests covering all state transitions, multiple blocks, resilience |
| 5 | Block stores exit code and timestamps for duration | VERIFIED | Tests verify exit_code, started_at, finished_at, duration() |
| 6 | StatusState tracks CWD from OSC 7 | VERIFIED | set_cwd test, integration with main.rs ShellEvent::CurrentDirectory |
| 7 | Git info queried asynchronously | VERIFIED | query_git_status() tested, async thread spawn in main.rs confirmed |
| 8 | Block separators render | VERIFIED | BlockRenderer.build_block_rects() generates separator rects, wired in frame.rs |
| 9 | Exit code badge renders green/red | VERIFIED | Badge color logic (exit_code==0 green, else red) in block_renderer.rs |
| 10 | Duration label renders | VERIFIED | build_block_text() with format_duration(), overlay buffer in frame.rs |
| 11 | Status bar renders with CWD | VERIFIED | StatusBarRenderer.build_status_text() + overlay buffers in frame.rs |
| 12 | Status bar shows git branch and dirty count | VERIFIED | build_status_text() formats "branch +N", right-aligned overlay text |
| 13 | PowerShell script emits OSC 133 A/B/C/D | VERIFIED | glass.ps1 contains prompt function with 133;A/B/D, PSReadLine Enter for 133;C |
| 14 | PowerShell script emits OSC 7 CWD | VERIFIED | glass.ps1 emits file://COMPUTERNAME/path |
| 15 | PowerShell script preserves existing prompt | VERIFIED | $Global:__GlassOriginalPrompt stash-and-wrap pattern |
| 16 | PowerShell exit code detection | VERIFIED | __Glass-Get-LastExitCode unifies $? and $LASTEXITCODE |
| 17 | Bash script emits OSC 133 A/B/C/D | VERIFIED | glass.bash uses PROMPT_COMMAND for A/B/D, PS0 for C |
| 18 | Bash script emits OSC 7 CWD | VERIFIED | __glass_osc7 function with file://HOSTNAME/PWD |
| 19 | Bash script preserves PS1 | VERIFIED | __GLASS_ORIGINAL_PS1 stash, PROMPT_COMMAND chaining |
| 20 | OscScanner pre-scans PTY bytes | VERIFIED | pty.rs: scanner.scan(data) before parser.advance() |
| 21 | OscEvents flow via AppEvent channel | VERIFIED | AppEvent::Shell variant, app_proxy.send_event() in pty.rs |
| 22 | BlockManager/StatusState updated on main thread | VERIFIED | main.rs handles AppEvent::Shell, calls handle_event() and set_cwd() |
| 23 | FrameRenderer receives block and status data | VERIFIED | draw_frame() signature accepts blocks/status, main.rs passes them |
| 24 | Status bar reduces grid height by 1 | VERIFIED | main.rs: saturating_sub(1) on num_lines in both resumed() and Resized |
| 25 | Git info queried async on CWD change | VERIFIED | main.rs spawns "Glass git query" thread, sends AppEvent::GitInfo back |

**Score:** 25/25 truths verified (automated)

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `crates/glass_terminal/src/osc_scanner.rs` | OscScanner state machine, OscEvent enum | VERIFIED | 337 lines, 13 tests, exports OscScanner + OscEvent |
| `crates/glass_terminal/src/block_manager.rs` | BlockManager, Block, BlockState, format_duration | VERIFIED | 335 lines, 12 tests, full state machine |
| `crates/glass_terminal/src/status.rs` | StatusState, GitInfo, query_git_status | VERIFIED | 163 lines, 5 tests, sync git CLI query |
| `crates/glass_renderer/src/block_renderer.rs` | BlockRenderer for separators/badges/duration | VERIFIED | 157 lines, build_block_rects + build_block_text |
| `crates/glass_renderer/src/status_bar.rs` | StatusBarRenderer for bottom-pinned bar | VERIFIED | 105 lines, build_status_rects + build_status_text |
| `shell-integration/glass.ps1` | PowerShell shell integration | VERIFIED | 117 lines, OSC 133/7, prompt stash, PSReadLine hook |
| `shell-integration/glass.bash` | Bash shell integration | VERIFIED | 78 lines, PROMPT_COMMAND, PS0, double-source guard |
| `crates/glass_terminal/src/pty.rs` | Custom PTY read loop with OscScanner | VERIFIED | 406 lines, glass_pty_loop with pre-scan |
| `crates/glass_core/src/event.rs` | ShellEvent, GitStatus, AppEvent variants | VERIFIED | 37 lines, Shell + GitInfo AppEvent variants |
| `src/main.rs` | Full wiring: events, BlockManager, StatusState, draw_frame | VERIFIED | 476 lines, complete pipeline |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| osc_scanner.rs | block_manager.rs | OscEvent consumed by handle_event() | WIRED | block_manager.rs imports OscEvent, handle_event matches all variants |
| osc_scanner.rs | status.rs | CurrentDirectory -> set_cwd() | WIRED | Indirectly via main.rs Shell event handler |
| frame.rs | block_renderer.rs | BlockRenderer called in draw_frame | WIRED | build_block_rects() and build_block_text() called in draw pipeline |
| frame.rs | status_bar.rs | StatusBarRenderer called in draw_frame | WIRED | build_status_rects() and build_status_text() called in draw pipeline |
| glass.ps1 | osc_scanner.rs | Emits OSC 133/7 sequences | WIRED | Script uses 133;A/B/C/D and 7;file:// matching scanner patterns |
| glass.bash | osc_scanner.rs | Emits OSC 133/7 sequences | WIRED | Script uses 133;A/B/C/D and 7;file:// matching scanner patterns |
| pty.rs | osc_scanner.rs | scanner.scan() before parser.advance() | WIRED | pty_read_with_scan calls scanner.scan(data) |
| pty.rs | event.rs | Sends AppEvent::Shell from PTY thread | WIRED | app_proxy.send_event(AppEvent::Shell{...}) |
| main.rs | block_manager.rs | handle_event() on Shell events | WIRED | ctx.block_manager.handle_event(&osc_event, line) |
| main.rs | frame.rs | Passes blocks + status to draw_frame | WIRED | draw_frame(..., &visible_blocks, Some(&ctx.status)) |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| SHEL-01 | 03-01, 03-04 | Glass parses OSC 133 for command lifecycle | SATISFIED | OscScanner with tests + PTY pre-scan wiring |
| SHEL-02 | 03-01, 03-04 | Glass parses OSC 7 for CWD tracking | SATISFIED | OscScanner OSC 7 parsing + StatusState wiring |
| SHEL-03 | 03-03, 03-04 | PowerShell integration script | SATISFIED | glass.ps1 with prompt wrapping, PSReadLine hook |
| SHEL-04 | 03-03, 03-04 | Bash integration script | SATISFIED | glass.bash with PROMPT_COMMAND, PS0 |
| BLOK-01 | 03-02, 03-04 | Each command renders as visually distinct block | SATISFIED | BlockRenderer separators + FrameRenderer integration |
| BLOK-02 | 03-01, 03-02 | Exit code badge (green/red) | SATISFIED | BlockRenderer badge rects + text labels |
| BLOK-03 | 03-01, 03-02 | Command duration display | SATISFIED | format_duration() + BlockRenderer duration labels |
| STAT-01 | 03-01, 03-02, 03-04 | Status bar shows CWD | SATISFIED | StatusBarRenderer + main.rs CWD wiring |
| STAT-02 | 03-01, 03-02, 03-04 | Status bar shows git info | SATISFIED | Async git query + StatusBarRenderer git display |

All 9 phase requirements accounted for. No orphaned requirements.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| crates/glass_renderer/src/frame.rs | 102, 156 | `display_offset = 0; // TODO: wired in Plan 04` | Warning | Block decorations will not scroll with content; misaligned during scrollback |

The hardcoded `display_offset = 0` in frame.rs is a warning-level issue. The Plan 04 summary claims wiring is complete, but frame.rs still hardcodes this value instead of using `snapshot.display_offset`. The correct data is available (snapshot has display_offset), but it is not passed to `build_block_rects()` and `build_block_text()`. Block decorations will render at wrong vertical positions when the user scrolls back. Note: main.rs correctly passes `snapshot.display_offset` to `block_manager.visible_blocks()` for filtering, so the right blocks are selected -- they just render at wrong pixel positions in frame.rs.

### Human Verification Required

### 1. End-to-End Block Rendering

**Test:** Build and run Glass (`cargo run`). Source the PowerShell integration: `. ./shell-integration/glass.ps1`. Run `echo hello`, then `Get-Item nonexistent`, then several more commands.
**Expected:** Each command appears in a visually distinct block with horizontal separator line. Successful commands show green "OK" badge, failed commands show red "X" badge. Duration labels appear right-aligned next to badges.
**Why human:** Visual rendering on GPU surface cannot be verified programmatically.

### 2. Status Bar CWD and Git Info

**Test:** Navigate to a git repository directory (`cd` commands). Observe the status bar at the bottom of the window.
**Expected:** Status bar background is slightly lighter than terminal. Left side shows current directory path. Right side shows git branch name and dirty file count (e.g., "master +3").
**Why human:** Requires live terminal interaction with shell integration active.

### 3. Scrollback Block Alignment

**Test:** Run many commands to fill the screen, then scroll up using Shift+PageUp.
**Expected:** Block separators and badges remain aligned with their corresponding command content.
**Why human:** The `display_offset=0` hardcoding in frame.rs may cause misalignment -- this needs visual confirmation. Block decorations may stay pinned to viewport-relative positions instead of scrolling with content.

### Gaps Summary

No blocking gaps found. All artifacts exist, are substantive (not stubs), and are fully wired end-to-end. All 63 tests pass (27 new for Phase 3 modules). Workspace compiles cleanly.

One warning-level issue: `display_offset` is hardcoded to 0 in `frame.rs` for block rect and text building, which means block decorations may not scroll correctly with content. This does not prevent the core goal (working shell integration and block UI at zero scroll), but affects the scroll use case.

Three items require human visual verification before the phase can be fully confirmed: block rendering appearance, status bar display, and scroll alignment.

---

_Verified: 2026-03-04_
_Verifier: Claude (gsd-verifier)_
