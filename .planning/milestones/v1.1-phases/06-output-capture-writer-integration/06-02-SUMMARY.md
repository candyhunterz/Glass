---
phase: 06-output-capture-writer-integration
plan: 02
subsystem: terminal
tags: [pty, output-capture, alt-screen, event-loop, tdd]

# Dependency graph
requires:
  - phase: 06-output-capture-writer-integration
    plan: 01
    provides: output processing pipeline (process_output, strip_ansi, is_binary, truncate_head_tail)
provides:
  - OutputBuffer struct for accumulating PTY bytes during command execution
  - AppEvent::CommandOutput variant for routing captured output to main thread
  - Alt-screen detection via raw byte scanning (ESC[?1049h/l)
  - HistorySection in GlassConfig for max_output_capture_kb setting
  - End-to-end capture pipeline: PTY bytes -> OutputBuffer -> AppEvent -> process_output
affects: [glass_terminal consumers, main.rs event handling, future HistoryDb write integration]

# Tech tracking
tech-stack:
  added: []
  patterns: [OutputBuffer in PTY reader thread, raw bytes via AppEvent for cross-crate processing]

key-files:
  created: [crates/glass_terminal/src/output_capture.rs]
  modified: [crates/glass_terminal/src/pty.rs, crates/glass_terminal/src/lib.rs, crates/glass_core/src/event.rs, crates/glass_core/src/config.rs, src/main.rs]

key-decisions:
  - "Raw bytes sent via AppEvent to main thread for processing, avoiding glass_terminal -> glass_history dependency"
  - "Alt-screen detection via raw byte scanning instead of locking terminal for TermMode flag"
  - "HistorySection as Option in GlassConfig for backward-compatible config parsing"

patterns-established:
  - "OutputBuffer lives entirely in PTY reader thread -- no mutex needed"
  - "Raw bytes via AppEvent, processing on main thread to keep dependency graph clean"
  - "Alt-screen detection via ESC[?1049h/l byte scanning in PTY read loop"

requirements-completed: [HIST-02]

# Metrics
duration: 5min
completed: 2026-03-05
---

# Phase 6 Plan 2: PTY Output Capture Pipeline Summary

**OutputBuffer in PTY reader thread accumulating bytes between CommandExecuted/CommandFinished, with alt-screen detection and AppEvent routing to main thread for processing**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-05T17:38:50Z
- **Completed:** 2026-03-05T17:43:53Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- OutputBuffer struct with 14 unit tests covering accumulation, alt-screen, max cap, and finish semantics
- Full PTY integration: OutputBuffer created in reader thread, wired into pty_read_with_scan
- AppEvent::CommandOutput routes raw bytes to main thread where process_output handles ANSI stripping, binary detection, and truncation
- GlassConfig extended with optional HistorySection for max_output_capture_kb
- 126 workspace tests passing with zero regressions

## Task Commits

Each task was committed atomically:

1. **Task 1: OutputBuffer struct (RED)** - `23bcb52` (test)
2. **Task 1: OutputBuffer struct (GREEN)** - `1866f49` (feat)
3. **Task 2: Wire into PTY and AppEvent** - `3a23e43` (feat)

_TDD: Task 1 had separate RED/GREEN commits._

## Files Created/Modified
- `crates/glass_terminal/src/output_capture.rs` - OutputBuffer struct with 14 unit tests
- `crates/glass_terminal/src/pty.rs` - OutputBuffer integration in read loop, spawn_pty gains max_output_capture_kb param
- `crates/glass_terminal/src/lib.rs` - Added pub mod output_capture
- `crates/glass_core/src/event.rs` - AppEvent::CommandOutput variant with raw bytes
- `crates/glass_core/src/config.rs` - HistorySection with max_output_capture_kb for GlassConfig
- `src/main.rs` - CommandOutput event handler calling process_output, passing max_kb to spawn_pty

## Decisions Made
- Sent raw bytes via AppEvent instead of processed String to avoid glass_terminal depending on glass_history -- keeps dependency graph clean (terminal -> core, history -> core, main -> both)
- Alt-screen detection via byte scanning rather than locking terminal mutex for TermMode flag -- more performant in the hot PTY read path
- GlassConfig.history is Option<HistorySection> rather than required -- backward-compatible with existing config files

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Output capture pipeline is fully wired: PTY bytes -> OutputBuffer -> AppEvent -> process_output
- Actual HistoryDb write (INSERT CommandRecord with output) deferred to when command record write path is established
- tracing::debug logs CommandOutput receipt for verification during development
- All workspace tests passing, clean build

---
*Phase: 06-output-capture-writer-integration*
*Completed: 2026-03-05*
