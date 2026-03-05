---
phase: 02-terminal-core
plan: 03
subsystem: input
tags: [keyboard-encoding, escape-sequences, clipboard, scrollback, bracketed-paste, arboard, winit]

# Dependency graph
requires:
  - phase: 02-terminal-core/02
    provides: "FrameRenderer, GridRenderer, font-metrics resize, GPU text rendering"
  - phase: 01-scaffold
    provides: "PTY integration, winit event loop, WindowContext"
provides:
  - "encode_key(): full keyboard escape sequence encoding (Ctrl, Alt, arrows, function keys, APP_CURSOR mode)"
  - "Clipboard copy/paste via Ctrl+Shift+C/V with bracketed paste support"
  - "Scrollback interaction via mouse wheel and Shift+PageUp/Down"
  - "ModifiersState tracking for accurate modifier-aware input"
affects: [03-shell-intelligence]

# Tech tracking
tech-stack:
  added: [arboard]
  patterns: [keyboard-escape-encoding, modifier-param-calculation, bracketed-paste-wrapping]

key-files:
  created:
    - crates/glass_terminal/src/input.rs
  modified:
    - crates/glass_terminal/src/lib.rs
    - src/main.rs
    - Cargo.toml

key-decisions:
  - "encode_key returns None for Glass-handled keys (clipboard, scrollback) so main.rs can intercept them"
  - "Ctrl+C sends 0x03 to PTY (SIGINT), NOT intercepted for clipboard — Ctrl+Shift+C used for copy"
  - "Arrow keys use SS3 encoding in APP_CURSOR mode, CSI in normal mode, per xterm convention"
  - "arboard crate for cross-platform clipboard access"

patterns-established:
  - "encode_key() pattern: returns Option<Vec<u8>> with None meaning 'handled elsewhere'"
  - "Modifier param encoding: 1 + (shift|alt|ctrl bitmask) per CSI u convention"

requirements-completed: [CORE-03, CORE-04, CORE-05, CORE-06, CORE-07, CORE-08]

# Metrics
duration: 12min
completed: 2026-03-04
---

# Phase 2 Plan 03: Keyboard, Clipboard, and Scrollback Summary

**Full escape sequence keyboard encoder with Ctrl/Alt/arrow/function key support, Ctrl+Shift+C/V clipboard with bracketed paste, and mouse wheel/Shift+PageUp scrollback interaction**

## Performance

- **Duration:** 12 min
- **Started:** 2026-03-04T23:30:00Z
- **Completed:** 2026-03-04T23:42:00Z
- **Tasks:** 3
- **Files modified:** 4

## Accomplishments
- encode_key() handles all standard terminal key sequences: Ctrl+letter (0x01-0x1a), Alt+key (ESC prefix), arrows (CSI/SS3 mode-aware), function keys F1-F12, Home/End/Insert/Delete/PageUp/PageDown, Enter/Tab/Backspace/Escape
- Modifier parameter encoding for shifted/ctrl/alt variants of named keys
- Clipboard copy from terminal selection via Ctrl+Shift+C, paste via Ctrl+Shift+V with bracketed paste wrapping when BRACKETED_PASTE mode active
- Mouse wheel scrollback and Shift+PageUp/PageDown viewport scrolling
- Plain Ctrl+C correctly sends 0x03 (SIGINT) to PTY, not intercepted for clipboard
- Human-verified: keyboard, clipboard, scrollback, and UTF-8 rendering all functional

## Task Commits

Each task was committed atomically:

1. **Task 1: Create keyboard input encoder with full escape sequence support** - `1fc4ec2` (feat, TDD)
2. **Task 2: Wire keyboard encoder, clipboard, and scrollback into main.rs** - `af34571` (feat)
3. **Task 3: Verify full terminal functionality** - checkpoint:human-verify approved

## Files Created/Modified
- `crates/glass_terminal/src/input.rs` - Full keyboard escape sequence encoder (encode_key function with 19 unit tests)
- `crates/glass_terminal/src/lib.rs` - Added input module declaration and encode_key re-export
- `src/main.rs` - Replaced ASCII-only handler with encode_key, added clipboard copy/paste, mouse wheel scrollback, Shift+PageUp/Down, ModifiersState tracking
- `Cargo.toml` - Added arboard workspace dependency for clipboard

## Decisions Made
- encode_key returns None for Glass-handled keys (clipboard shortcuts, scrollback keys) so main.rs intercepts them before forwarding to PTY
- Ctrl+C sends 0x03 to PTY for SIGINT; Ctrl+Shift+C used for clipboard copy (standard terminal convention)
- Arrow keys use SS3 (ESC O) in APP_CURSOR mode, CSI (ESC [) in normal mode, matching xterm behavior
- arboard crate chosen for clipboard access (cross-platform, maintained)

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Phase 2 terminal core is complete: PTY, rendering, keyboard, clipboard, scrollback all functional
- Glass is now a usable daily-driver terminal on Windows
- Ready for Phase 3 (shell intelligence): PSReadLine integration, command indexing, session snapshots
- encode_key API stable for future enhancements (vi mode, custom keybindings)

## Self-Check: PASSED

All files verified present. Both task commits (1fc4ec2, af34571) confirmed in git history.

---
*Phase: 02-terminal-core*
*Completed: 2026-03-04*
