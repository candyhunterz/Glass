---
phase: 21-session-extraction-platform-foundation
plan: 02
subsystem: terminal
tags: [session-id, event-routing, shell-integration, zsh, osc-133]

# Dependency graph
requires: []
provides:
  - "SessionId type in glass_core::event"
  - "AppEvent variants with session_id field for PTY-originated events"
  - "EventProxy carrying session_id for event routing"
  - "Zsh shell integration script with OSC 133 + OSC 7"
affects: [21-03-PLAN, 23-tabs]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "SessionId defined in glass_core (canonical location) to avoid circular deps"
    - "PTY-originated AppEvent variants carry session_id; TerminalDirty does not"
    - "Shell integration scripts follow consistent OSC 133 A/B/C/D + OSC 7 pattern"

key-files:
  created:
    - "shell-integration/glass.zsh"
  modified:
    - "crates/glass_core/src/event.rs"
    - "crates/glass_terminal/src/event_proxy.rs"
    - "crates/glass_terminal/src/pty.rs"
    - "src/main.rs"

key-decisions:
  - "SessionId defined in glass_core::event (not glass_mux) to avoid circular dependency"
  - "SessionId::new(0) used as placeholder in EventProxy::new and GitInfo construction until Plan 03 wires real sessions"
  - "TerminalDirty intentionally excluded from session_id (any dirty triggers full redraw)"

patterns-established:
  - "SessionId(u64) with new/val/Display in glass_core::event"
  - "EventProxy::new takes (proxy, window_id, session_id) triple"
  - "Zsh shell integration uses add-zsh-hook for precmd/preexec"

requirements-completed: [P21-03, P21-09]

# Metrics
duration: 3min
completed: 2026-03-06
---

# Phase 21 Plan 02: Event SessionId + Zsh Shell Integration Summary

**SessionId added to all PTY-originated AppEvent variants and EventProxy, plus zsh shell integration with OSC 133/OSC 7**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-06T22:32:15Z
- **Completed:** 2026-03-06T22:35:28Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments
- SessionId type defined in glass_core with Copy/Clone/Eq/Hash/Display traits
- All PTY-originated AppEvent variants (SetTitle, TerminalExit, Shell, GitInfo, CommandOutput) now carry session_id
- EventProxy updated to accept, store, and propagate session_id in all emitted events
- Zsh shell integration script created with OSC 133 A/B/C/D and OSC 7 CWD tracking
- Full workspace compiles cleanly with all existing tests passing

## Task Commits

Each task was committed atomically:

1. **Task 1: Add SessionId to AppEvent variants and EventProxy** - `8f8dc18` (feat)
2. **Task 2: Create zsh shell integration script** - `6a4d6cc` (feat)

## Files Created/Modified
- `crates/glass_core/src/event.rs` - SessionId type + session_id fields on AppEvent variants + new tests
- `crates/glass_terminal/src/event_proxy.rs` - EventProxy now carries session_id and propagates it
- `crates/glass_terminal/src/pty.rs` - Shell and CommandOutput events include session_id from EventProxy
- `src/main.rs` - Import SessionId, update all match arms with session_id: _, placeholder in EventProxy::new
- `shell-integration/glass.zsh` - Zsh shell integration with OSC 133 + OSC 7

## Decisions Made
- SessionId defined in glass_core::event (not glass_mux) to avoid circular crate dependency since glass_mux depends on glass_core
- SessionId::new(0) used as placeholder in all construction sites until Plan 03 wires real sessions from SessionMux
- TerminalDirty intentionally excluded from session_id because any dirty terminal triggers a full redraw regardless of session

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- SessionId in events enables Plan 03 to wire session routing through SessionMux
- Zsh script ready for cross-platform testing in Phase 22
- All match arms use `session_id: _` placeholder, ready for Plan 03 to use real routing

---
*Phase: 21-session-extraction-platform-foundation*
*Completed: 2026-03-06*
