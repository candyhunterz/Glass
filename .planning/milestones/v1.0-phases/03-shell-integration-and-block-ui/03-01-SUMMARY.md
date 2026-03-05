---
phase: 03-shell-integration-and-block-ui
plan: 01
subsystem: terminal
tags: [osc, shell-integration, state-machine, tdd, git, url-parsing]

# Dependency graph
requires:
  - phase: 02-terminal-core
    provides: "glass_terminal crate with PTY, grid snapshot, input encoding"
provides:
  - "OscScanner byte parser for OSC 133/7/9;9 sequences"
  - "BlockManager state machine for command lifecycle tracking"
  - "StatusState with CWD and async-ready git info"
  - "format_duration() for human-readable elapsed time"
affects: [03-02-block-rendering, 03-03-status-bar, 03-04-wiring]

# Tech tracking
tech-stack:
  added: [url 2.x]
  patterns: [byte-level state machine, TDD red-green-refactor, split-buffer resilience]

key-files:
  created:
    - crates/glass_terminal/src/osc_scanner.rs
    - crates/glass_terminal/src/block_manager.rs
    - crates/glass_terminal/src/status.rs
  modified:
    - crates/glass_terminal/src/lib.rs
    - crates/glass_terminal/Cargo.toml
    - Cargo.toml
    - Cargo.lock

key-decisions:
  - "url crate v2 for OSC 7 file:// path parsing with Windows /C:/ prefix handling"
  - "OscScanner uses 3-state machine (Ground/Escape/Accumulating) not 4 — OscStart merged into Accumulating"
  - "BlockManager ignores events without prior PromptStart for resilience to partial streams"
  - "query_git_status runs synchronous git CLI with GIT_OPTIONAL_LOCKS=0"

patterns-established:
  - "TDD: write all tests first (RED), implement to pass (GREEN), clean up warnings (REFACTOR)"
  - "OscEvent enum as bridge between scanner and consumers (BlockManager, StatusState)"
  - "Split-buffer support via persistent state in OscScanner across scan() calls"

requirements-completed: [SHEL-01, SHEL-02, BLOK-02, BLOK-03, STAT-01, STAT-02]

# Metrics
duration: 6min
completed: 2026-03-05
---

# Phase 3 Plan 1: Shell Integration Data Layer Summary

**OscScanner byte parser, BlockManager command lifecycle state machine, and StatusState CWD/git tracker -- all TDD with 27 new tests**

## Performance

- **Duration:** 6 min
- **Started:** 2026-03-05T05:28:33Z
- **Completed:** 2026-03-05T05:34:44Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- OscScanner parses OSC 133 A/B/C/D, OSC 7 file:// URLs, and OSC 9;9 ConEmu CWD from raw byte streams
- Split-buffer edge cases handled correctly (sequences spanning PTY read boundaries)
- BlockManager tracks full command lifecycle with exit codes and duration timing
- StatusState stores CWD and git info with synchronous query_git_status()
- 27 new unit tests (63 total in glass_terminal crate), all passing

## Task Commits

Each task was committed atomically:

1. **Task 1: OscScanner with TDD** - `606fdad` (test+feat)
2. **Task 2: BlockManager and StatusState with TDD** - `83fbc84` (feat)

## Files Created/Modified
- `crates/glass_terminal/src/osc_scanner.rs` - OscScanner state machine, OscEvent enum, 13 tests
- `crates/glass_terminal/src/block_manager.rs` - BlockManager, Block, BlockState, format_duration, 12 tests
- `crates/glass_terminal/src/status.rs` - StatusState, GitInfo, query_git_status, 5 tests
- `crates/glass_terminal/src/lib.rs` - Module declarations and public re-exports
- `crates/glass_terminal/Cargo.toml` - Added url dependency
- `Cargo.toml` - Added url = "2" to workspace dependencies

## Decisions Made
- Used `url` crate v2 for OSC 7 file:// URL parsing with proper Windows path prefix handling
- OscScanner simplified to 3-state machine (Ground/Escape/Accumulating) -- OscStart state unnecessary since we transition directly from Escape+] to Accumulating
- BlockManager silently ignores events without a prior PromptStart rather than panicking, providing resilience to partial PTY streams
- query_git_status() is synchronous, meant to be called from a background thread; uses GIT_OPTIONAL_LOCKS=0 to avoid contention
- Custom percent_decode_str() for file path decoding (avoids pulling in percent-encoding crate directly)

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- OscScanner, BlockManager, and StatusState are public in glass_terminal, ready for:
  - Plan 02 (block rendering) to consume BlockManager for visual indicators
  - Plan 03 (status bar) to consume StatusState for CWD and git display
  - Plan 04 (wiring) to feed OscEvents from PTY output into BlockManager and StatusState

---
*Phase: 03-shell-integration-and-block-ui*
*Completed: 2026-03-05*
