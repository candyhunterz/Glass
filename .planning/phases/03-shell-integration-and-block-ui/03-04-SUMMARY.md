---
phase: 03-shell-integration-and-block-ui
plan: 04
subsystem: terminal
tags: [pty, osc, shell-integration, block-ui, status-bar, conpty, vte]

# Dependency graph
requires:
  - phase: 03-shell-integration-and-block-ui
    provides: OscScanner, BlockManager, StatusState, BlockRenderer, StatusBarRenderer, shell scripts
provides:
  - Custom PTY read loop with OscScanner pre-scanning
  - ShellEvent and GitStatus types in glass_core (no circular deps)
  - Full end-to-end wiring: PTY -> OscScanner -> ShellEvent -> BlockManager/StatusState -> FrameRenderer
  - Async git status querying on CWD change
  - Status bar grid height adjustment (PTY resize reflects content area minus 1 line)
affects: [phase-04]

# Tech tracking
tech-stack:
  added: [polling 3, vte 0.15]
  patterns: [custom PTY read loop with pre-scanning, ShellEvent/OscEvent conversion to avoid circular deps, async git query via background thread]

key-files:
  created: []
  modified:
    - crates/glass_terminal/src/pty.rs
    - crates/glass_core/src/event.rs
    - crates/glass_terminal/Cargo.toml
    - crates/glass_terminal/src/lib.rs
    - src/main.rs

key-decisions:
  - "ShellEvent enum in glass_core mirrors OscEvent to avoid circular crate dependency"
  - "Custom PTY read loop replaces alacritty_terminal PtyEventLoop to intercept bytes for OscScanner"
  - "PtySender wraps mpsc::Sender + polling::Poller to wake PTY thread on send"
  - "Grid height reduced by 1 line for status bar; PTY resize reflects actual content area"
  - "Git status queried on background thread with git_query_pending flag to avoid duplicate queries"

patterns-established:
  - "ShellEvent/OscEvent conversion pattern: PTY thread converts OscEvent to ShellEvent, main thread converts back for BlockManager"
  - "Async side-effect pattern: CWD change triggers background git query, result sent back via AppEvent::GitInfo"

requirements-completed: [SHEL-01, SHEL-02, SHEL-03, SHEL-04, BLOK-01, STAT-01, STAT-02]

# Metrics
duration: 7min
completed: 2026-03-05
---

# Phase 3 Plan 4: End-to-End Wiring Summary

**Custom PTY read loop with OscScanner pre-scanning, BlockManager/StatusState wiring, and status bar grid height adjustment**

## Performance

- **Duration:** 7 min
- **Started:** 2026-03-05T05:43:27Z
- **Completed:** 2026-03-05T05:50:43Z
- **Tasks:** 2 of 2 auto tasks completed (Task 3 is checkpoint:human-verify)
- **Files modified:** 5

## Accomplishments
- Replaced alacritty_terminal's PtyEventLoop with custom glass_pty_loop that pre-scans PTY bytes through OscScanner before VTE parsing
- Defined ShellEvent and GitStatus types in glass_core to avoid circular crate dependencies between glass_core and glass_terminal
- Wired BlockManager and StatusState into main.rs, handling Shell and GitInfo AppEvents
- Passed visible blocks and status data to FrameRenderer's draw_frame for block decoration and status bar rendering
- Adjusted terminal grid height by 1 line for the status bar so PTY resize reflects actual content area

## Task Commits

Each task was committed atomically:

1. **Task 1: Custom PTY read loop with OscScanner and extended AppEvent** - `318cec7` (feat)
2. **Task 2: Wire BlockManager, StatusState, and FrameRenderer in main.rs** - `0293eea` (feat)

## Files Created/Modified
- `crates/glass_terminal/src/pty.rs` - Custom PTY read loop with OscScanner integration, PtySender/PtyMsg types
- `crates/glass_core/src/event.rs` - ShellEvent enum, GitStatus struct, AppEvent::Shell and AppEvent::GitInfo variants
- `crates/glass_terminal/Cargo.toml` - Added polling 3 and vte 0.15 dependencies
- `crates/glass_terminal/src/lib.rs` - Re-export PtyMsg and PtySender
- `src/main.rs` - Full wiring: BlockManager, StatusState, Shell/GitInfo event handling, grid height adjustment

## Decisions Made
- ShellEvent enum in glass_core mirrors OscEvent to avoid circular crate dependency (glass_terminal depends on glass_core, not vice versa)
- Custom PTY read loop closely follows alacritty's event_loop.rs structure (polling, lease-based locking, MAX_LOCKED_READ) for correctness
- PtySender wraps mpsc::Sender + polling::Poller notification to properly wake the PTY event loop
- Git status queried on dedicated background thread with git_query_pending flag to prevent duplicate concurrent queries
- Grid height reduced by 1 line using saturating_sub(1) with max(2.0) floor to ensure at least 1 content line

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
- vte::ansi::Processor requires explicit type parameter annotation (StdSyncHandler) when the generic cannot be inferred from context - resolved by specifying `Processor::<ansi::StdSyncHandler>::new()`

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All Phase 3 components are wired end-to-end
- Pending: Task 3 human verification of visual rendering (checkpoint)
- Phase 4 can proceed once visual verification confirms blocks, badges, duration, and status bar render correctly

---
*Phase: 03-shell-integration-and-block-ui*
*Completed: 2026-03-05*
