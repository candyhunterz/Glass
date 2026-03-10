---
phase: 34-gui-integration
plan: 01
subsystem: ui
tags: [coordination, status-bar, polling, wgpu]

# Dependency graph
requires:
  - phase: 31-core-db
    provides: CoordinationDb with list_agents and list_locks APIs
  - phase: 33-integration-testing
    provides: Verified coordination DB cross-connection behavior
provides:
  - CoordinationState type with agent/lock counts
  - Background coordination poller thread (5-second interval)
  - AppEvent::CoordinationUpdate variant for UI thread communication
  - Status bar coordination text rendering in soft purple
affects: [34-gui-integration]

# Tech tracking
tech-stack:
  added: []
  patterns: [background-poller-to-event-loop, open-per-call-db-polling]

key-files:
  created:
    - crates/glass_core/src/coordination_poller.rs
  modified:
    - crates/glass_core/src/event.rs
    - crates/glass_core/src/lib.rs
    - crates/glass_core/Cargo.toml
    - crates/glass_renderer/src/status_bar.rs
    - crates/glass_renderer/src/frame.rs
    - Cargo.toml
    - src/main.rs

key-decisions:
  - "Coordination text positioned left of git info in status bar, with 2-cell gap"
  - "Soft purple (180,140,255) color for coordination text to distinguish from git cyan"
  - "Poll sleep before first DB access so startup is not delayed by I/O"

patterns-established:
  - "Background poller pattern: spawn_coordination_poller follows spawn_update_checker convention"
  - "CoordinationState flows: poller thread -> AppEvent -> Processor field -> draw_frame parameter -> StatusLabel"

requirements-completed: [GUI-01, GUI-02, GUI-03]

# Metrics
duration: 5min
completed: 2026-03-09
---

# Phase 34 Plan 01: Coordination Poller and Status Bar Summary

**Background coordination poller thread with 5-second polling and soft purple agent/lock count rendering in status bar**

## Performance

- **Duration:** 5 min
- **Started:** 2026-03-09T23:55:16Z
- **Completed:** 2026-03-10T00:00:16Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- Created CoordinationState type with agent_count, lock_count, locks, and conflicts fields
- Built background poller thread that queries agents.db every 5 seconds via open-per-call pattern
- Wired coordination state through AppEvent -> Processor -> FrameRenderer -> StatusLabel pipeline
- Status bar shows "agents: N locks: M" in soft purple when agents are active

## Task Commits

Each task was committed atomically:

1. **Task 1: Create CoordinationState types and polling thread module** - `e519564` (feat)
2. **Task 2: Extend StatusLabel, wire poller and state through Processor and FrameRenderer** - `4872558` (feat)

## Files Created/Modified
- `crates/glass_core/src/coordination_poller.rs` - CoordinationState types and spawn_coordination_poller function
- `crates/glass_core/src/event.rs` - Added CoordinationUpdate variant to AppEvent
- `crates/glass_core/src/lib.rs` - Added coordination_poller module
- `crates/glass_core/Cargo.toml` - Added glass_coordination dependency
- `crates/glass_renderer/src/status_bar.rs` - Added coordination_text and coordination_color to StatusLabel
- `crates/glass_renderer/src/frame.rs` - Added coordination_text parameter to draw_frame and draw_multi_pane_frame
- `Cargo.toml` - Added glass_coordination dependency to root
- `src/main.rs` - Added coordination_state to Processor, CoordinationUpdate handler, poller spawn

## Decisions Made
- Coordination text positioned left of git info with 2-cell-width gap for visual separation
- Soft purple color (RGB 180,140,255) chosen to be visually distinct from cyan git info and yellow update text
- Poll thread sleeps before first poll to avoid startup I/O delay (matches spawn_update_checker pattern)
- Default CoordinationState (all zeros) displayed as no text -- status bar only shows coordination info when agents > 0

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

Pre-existing test failure: `test_utf8_codepage_65001_active` fails in non-console test environment (codepage 0 vs 65001). Not related to this plan's changes.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Coordination poller and status bar wiring complete
- Plan 02 can build lock conflict overlay using the ConflictInfo type and LockEntry data already flowing through CoordinationState

---
*Phase: 34-gui-integration*
*Completed: 2026-03-09*
