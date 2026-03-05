---
phase: 06-output-capture-writer-integration
verified: 2026-03-05T23:30:00Z
status: human_needed
score: 3/4 success criteria verified
re_verification:
  previous_status: gaps_found
  previous_score: 2/4
  gaps_closed:
    - "After a command completes, its stdout/stderr output (up to the configured max, default 50KB) is stored in the history database"
  gaps_remaining: []
  regressions: []
human_verification:
  - test: "Verify block decorations scroll correctly during scrollback"
    expected: "Separator lines and exit code badges move with their commands when scrolling up/down"
    why_human: "Visual rendering behavior cannot be verified programmatically"
  - test: "Verify PTY throughput is not regressed"
    expected: "Large output commands (e.g., seq 100000) render at similar speed to v1.0"
    why_human: "Performance feel requires interactive testing"
  - test: "Verify commands and output are persisted to SQLite at runtime"
    expected: "After running commands in Glass, .glass/history.db contains CommandRecords with cwd, exit_code, timestamps, and output"
    why_human: "End-to-end runtime behavior requires running the application"
---

# Phase 6: Output Capture + Writer Integration Verification Report

**Phase Goal:** Wire output capture into the PTY pipeline and persist commands+output to SQLite via HistoryDb
**Verified:** 2026-03-05T23:30:00Z
**Status:** human_needed
**Re-verification:** Yes -- after gap closure (Plan 06-04)

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | After a command completes, its stdout/stderr output (up to the configured max, default 50KB) is stored in the history database | VERIFIED | Gap closed by Plan 06-04. HistoryDb opened in WindowContext (main.rs:213). insert_command called on CommandFinished (main.rs:485). update_output called on CommandOutput (main.rs:542). update_output method added to HistoryDb (db.rs:173). No TODOs remain. |
| 2 | Output from alternate-screen applications (vim, less, top) is not captured | VERIFIED | OutputBuffer.check_alt_screen() scans for ESC[?1049h/l sequences. When alt_screen_seen=true, finish() returns None. 14 unit tests cover this behavior. No regression. |
| 3 | Block decorations (separator lines, exit code badges) render at correct positions during scrollback navigation | VERIFIED | frame.rs uses snapshot.display_offset (lines 116, 170) with viewport_abs_start calculation. No regression from previous verification. |
| 4 | PTY throughput does not regress measurably compared to v1.0 baseline (output capture is non-blocking) | UNCERTAIN | Architecture is sound: OutputBuffer lives in PTY thread, no mutex, DB writes happen on main thread via AppEvent. No benchmark exists to verify the throughput claim quantitatively. |

**Score:** 3/4 truths verified (1 needs human testing)

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `crates/glass_history/src/output.rs` | Output processing pipeline | VERIFIED | 4 exported functions, 16 tests passing. No regression. |
| `crates/glass_history/src/db.rs` | Schema migration v0->v1, CommandRecord with output field, update_output | VERIFIED | output: Option<String> field, PRAGMA user_version migration, insert/get/update_output support. update_output added at line 173. test_update_output passes. |
| `crates/glass_history/src/config.rs` | max_output_capture_kb config field | VERIFIED | Field present with serde default of 50. No regression. |
| `crates/glass_terminal/src/output_capture.rs` | OutputBuffer struct | VERIFIED | Full implementation with start_capture, append, check_alt_screen, finish. 14 unit tests passing. No regression. |
| `crates/glass_core/src/event.rs` | CommandOutput AppEvent variant | VERIFIED | `CommandOutput { window_id, raw_output: Vec<u8> }` variant present. No regression. |
| `crates/glass_renderer/src/frame.rs` | Real display_offset from GridSnapshot | VERIFIED | snapshot.display_offset used at lines 116 and 170. No regression. |
| `src/main.rs` | HistoryDb in WindowContext, insert on CommandFinished, update on CommandOutput | VERIFIED | Previously STUB, now fully wired. HistoryDb opened at line 213 (non-fatal). insert_command at line 485. update_output at line 542. No TODOs remain. |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| pty.rs | output_capture.rs | OutputBuffer::append() in pty_read_with_scan | WIRED | No regression from previous verification |
| pty.rs | event.rs | AppEvent::CommandOutput sent on CommandFinished | WIRED | No regression from previous verification |
| main.rs (Shell::CommandFinished) | glass_history db.rs | insert_command with CommandRecord | WIRED | Lines 474-494: builds CommandRecord with cwd, exit_code, timestamps, duration_ms; calls db.insert_command; stores last_command_id |
| main.rs (CommandOutput handler) | glass_history db.rs | update_output on last inserted row | WIRED | Lines 539-554: calls process_output then db.update_output(cmd_id, &output) |
| frame.rs | grid_snapshot.rs | snapshot.display_offset field | WIRED | No regression from previous verification |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| HIST-02 | 06-01, 06-02, 06-04 | Command output is captured and stored (truncated to configurable max, default 50KB) | SATISFIED | Full pipeline: OutputBuffer captures in PTY thread, sends via AppEvent, process_output strips ANSI/detects binary/truncates, HistoryDb persists via insert_command + update_output. |
| INFR-02 | 06-03 | Fix display_offset tech debt so block decorations scroll correctly | SATISFIED | frame.rs uses snapshot.display_offset instead of hardcoded 0. Human-verified during Plan 03 execution. |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/main.rs | 472 | Command text is empty string (deferred to Phase 7) | Info | Known limitation documented in code comment. Metadata (cwd, exit_code, timestamps, output) is the high-value data. Not a blocker for HIST-02. |

### Test Results

All 127 workspace tests pass with zero failures:
- glass_core: 6 passed
- glass_cli: 5 passed
- glass_history: 39 passed (includes test_update_output, test_migration_v0_to_v1, output processing tests)
- glass_terminal: 77 passed (includes 14 OutputBuffer tests)

### Human Verification Required

### 1. Block decoration scrollback

**Test:** Run Glass, execute several commands, scroll up/down through history
**Expected:** Separator lines and exit code badges move with their commands, not pinned to screen
**Why human:** Visual rendering behavior

### 2. PTY throughput regression check

**Test:** Run `seq 100000` or similar large-output command in Glass
**Expected:** Output speed similar to v1.0 baseline
**Why human:** Performance feel requires interactive testing

### 3. End-to-end history persistence

**Test:** Run Glass, execute several commands (echo hello, ls, false), then inspect .glass/history.db
**Expected:** CommandRecords exist with cwd, exit_code, timestamps, duration, and captured output
**Why human:** Runtime behavior requires running the application

### Gaps Summary

The critical gap from the initial verification has been closed. Plan 06-04 added the last-mile wiring:
1. `update_output` method on HistoryDb
2. HistoryDb opened in WindowContext at window creation (non-fatal on failure)
3. CommandRecord inserted on every CommandFinished with cwd, exit_code, wall-clock timestamps, and duration
4. Output updated on the last inserted record when CommandOutput arrives

The only remaining item is the empty command text (deferred to Phase 7), which is a known limitation and not a blocker for HIST-02 since the requirement specifies "command output is captured and stored" -- the output storage is now complete.

All automated checks pass. Three items require human verification: visual scrollback behavior, throughput regression, and end-to-end runtime persistence.

---

_Verified: 2026-03-05T23:30:00Z_
_Verifier: Claude (gsd-verifier)_
