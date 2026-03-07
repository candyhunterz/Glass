---
phase: 21-session-extraction-platform-foundation
plan: 01
subsystem: terminal
tags: [session, mux, platform, cfg-gate, newtype, winit]

requires:
  - phase: none
    provides: "New crate, no dependencies on prior phases"
provides:
  - "glass_mux crate with Session struct (all 15 WindowContext fields)"
  - "SessionMux single-session multiplexer"
  - "SessionId/TabId newtype wrappers"
  - "Platform helpers (default_shell, is_action_modifier, is_glass_shortcut, config_dir, data_dir)"
  - "Stub types: Tab, SplitNode, ViewportLayout"
  - "SearchOverlay copied into glass_mux for per-session ownership"
affects: [21-02, 21-03, 23-tabs, 24-split-panes]

tech-stack:
  added: [glass_mux crate]
  patterns: [newtype-id-wrapper, cfg-gated-platform, session-extraction]

key-files:
  created:
    - crates/glass_mux/Cargo.toml
    - crates/glass_mux/src/lib.rs
    - crates/glass_mux/src/types.rs
    - crates/glass_mux/src/session.rs
    - crates/glass_mux/src/session_mux.rs
    - crates/glass_mux/src/platform.rs
    - crates/glass_mux/src/search_overlay.rs
    - crates/glass_mux/src/tab.rs
    - crates/glass_mux/src/split_tree.rs
    - crates/glass_mux/src/layout.rs
  modified:
    - Cargo.toml

key-decisions:
  - "Copied SearchOverlay into glass_mux rather than re-exporting from src/ -- enables per-session ownership"
  - "SessionId/TabId use u64 wrapper (no uuid dependency needed for Phase 21)"
  - "Platform helpers use cfg-gated function definitions (not runtime detection)"

patterns-established:
  - "Newtype ID pattern: SessionId(u64) with new/val/Display"
  - "cfg-gated platform module for cross-platform behavior"
  - "Session struct as extracted WindowContext fields"

requirements-completed: [P21-01, P21-02, P21-04, P21-06, P21-07, P21-08]

duration: 4min
completed: 2026-03-06
---

# Phase 21 Plan 01: Create glass_mux Crate Summary

**glass_mux crate with Session struct (15 WindowContext fields), SessionMux single-session multiplexer, platform cfg-gated helpers, and stub types for tabs/splits**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-06T22:32:28Z
- **Completed:** 2026-03-06T22:36:35Z
- **Tasks:** 2
- **Files modified:** 11

## Accomplishments
- Created glass_mux crate with all module structure and dependencies
- Session struct mirrors all 15 WindowContext fields plus id and title
- SessionMux provides single-session mode with focused_session/session lookup
- Platform helpers with cfg-gated implementations for Windows/macOS/Linux
- 14 unit tests covering types, session_mux, and platform helpers

## Task Commits

Each task was committed atomically:

1. **Task 1: Create glass_mux crate with types, Session, and SessionMux** - `378be9d` (feat)
2. **Task 2: Add platform helpers with cfg-gated implementations** - `331e60b` (test)

## Files Created/Modified
- `crates/glass_mux/Cargo.toml` - Crate manifest with dependencies on glass_core, glass_terminal, glass_history, glass_snapshot
- `crates/glass_mux/src/lib.rs` - Module declarations and re-exports
- `crates/glass_mux/src/types.rs` - SessionId, TabId, SplitDirection, FocusDirection with tests
- `crates/glass_mux/src/session.rs` - Session struct with all 15 extracted WindowContext fields
- `crates/glass_mux/src/session_mux.rs` - SessionMux with single-session mode
- `crates/glass_mux/src/platform.rs` - cfg-gated default_shell, is_action_modifier, is_glass_shortcut, config_dir, data_dir
- `crates/glass_mux/src/search_overlay.rs` - SearchOverlay/SearchOverlayData/SearchResultDisplay copied for per-session ownership
- `crates/glass_mux/src/tab.rs` - Tab stub struct (Phase 23)
- `crates/glass_mux/src/split_tree.rs` - SplitNode stub enum (Phase 24)
- `crates/glass_mux/src/layout.rs` - ViewportLayout stub struct (Phase 24)
- `Cargo.toml` - Added glass_mux to root binary dependencies

## Decisions Made
- Copied SearchOverlay into glass_mux rather than re-exporting from src/ -- enables per-session ownership without circular dependencies
- SessionId/TabId use u64 wrapper per research recommendation (no uuid dependency needed for Phase 21)
- Platform helpers use cfg-gated function definitions with separate implementations per OS

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- glass_mux crate compiles and all 14 tests pass
- Root binary (cargo check -p glass) still compiles
- Ready for Plan 02 (SessionId routing in AppEvent/EventProxy + zsh integration)
- Ready for Plan 03 (WindowContext refactor to use SessionMux)

## Self-Check: PASSED

All 10 created files verified present on disk. Commits 378be9d and 331e60b verified in git log.

---
*Phase: 21-session-extraction-platform-foundation*
*Completed: 2026-03-06*
