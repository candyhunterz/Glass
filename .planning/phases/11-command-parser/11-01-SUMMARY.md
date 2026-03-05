---
phase: 11-command-parser
plan: 01
subsystem: snapshot
tags: [shlex, command-parsing, posix, path-resolution]

# Dependency graph
requires:
  - phase: 10-content-store-db
    provides: glass_snapshot crate with BlobStore, SnapshotDb, types
provides:
  - ParseResult and Confidence types for command classification
  - parse_command pure function for extracting file modification targets
  - Whitelist-based dispatch for rm, mv, cp, sed, chmod, git, truncate
  - Redirect detection (>, >>) for file target extraction
affects: [12-fs-watcher, pre-exec-snapshot-integration]

# Tech tracking
tech-stack:
  added: [shlex 1.3.0]
  patterns: [whitelist-dispatch-parser, per-command-argument-extraction, tdd-red-green]

key-files:
  created: [crates/glass_snapshot/src/command_parser.rs]
  modified: [Cargo.toml, crates/glass_snapshot/Cargo.toml, crates/glass_snapshot/src/types.rs, crates/glass_snapshot/src/lib.rs]

key-decisions:
  - "Single-file parser (~350 lines impl + 250 lines tests) rather than splitting into submodules"
  - "Redirect targets merged into ParseResult regardless of base command classification"
  - "POSIX paths starting with / treated as absolute on Windows for WSL compatibility"
  - "Glob characters in arguments trigger Low confidence rather than attempting expansion"

patterns-established:
  - "Whitelist dispatch: match base command name against known destructive/read-only lists"
  - "Per-command extractors: each destructive command has its own argument parsing function"
  - "strip_redirects before tokenization to prevent redirect filenames from appearing as command args"

requirements-completed: [SNAP-03]

# Metrics
duration: 4min
completed: 2026-03-05
---

# Phase 11 Plan 01: Command Parser Summary

**POSIX command parser with whitelist dispatch, shlex tokenization, redirect detection, and 14 unit tests covering rm/mv/cp/sed/chmod/git/truncate**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-05T22:41:38Z
- **Completed:** 2026-03-05T22:45:51Z
- **Tasks:** 1 (TDD: RED + GREEN)
- **Files modified:** 5

## Accomplishments
- Pure function `parse_command(command_text, cwd) -> ParseResult` with zero state or DB coupling
- Whitelist dispatch for 7 destructive commands (rm, mv, cp, sed -i, chmod, git checkout/restore/clean/reset, truncate) and 30+ read-only commands
- Redirect detection extracts `>` and `>>` targets independent of base command
- Path resolution handles relative paths (joined to cwd) and absolute POSIX paths on Windows
- Unparseable syntax detection (pipes, subshells, semicolons, loops) returns Low confidence
- All 14 unit tests pass, 226 workspace tests green with zero regressions

## Task Commits

Each task was committed atomically (TDD):

1. **Task 1 RED: Types, dependency wiring, and test scaffold** - `d0107e0` (test)
2. **Task 1 GREEN: Implement parse_command** - `9bd3b43` (feat)

## Files Created/Modified
- `crates/glass_snapshot/src/command_parser.rs` - POSIX command parser with whitelist dispatch, per-command extractors, redirect detection, path resolution, and 14 inline tests
- `crates/glass_snapshot/src/types.rs` - Added ParseResult struct and Confidence enum
- `crates/glass_snapshot/src/lib.rs` - Added command_parser module and re-exports
- `crates/glass_snapshot/Cargo.toml` - Added shlex workspace dependency
- `Cargo.toml` - Added shlex 1.3.0 to workspace dependencies

## Decisions Made
- Single-file parser rather than splitting into submodules -- file is under 400 lines of core parsing logic (tests separate), per research recommendation
- Redirect targets merged into ParseResult after command dispatch -- echo with redirect gets High confidence even though echo is ReadOnly
- strip_redirects preprocessing before shlex tokenization -- prevents redirect filenames from appearing as command arguments
- POSIX `/` paths treated as absolute on Windows via `path_str.starts_with('/')` check for WSL compatibility
- Glob characters (`*`, `?`, `[`) in arguments trigger Low confidence rather than attempting expansion

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
- Rust borrow checker caught moved value in redirect_targets empty check -- fixed by extracting confidence before moving the Vec
- Windows path joining uses backslash separator -- tests use `resolved()` helper that matches platform behavior

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- parse_command is ready for integration with pre-exec snapshot trigger
- ParseResult and Confidence types are exported from glass_snapshot crate
- PowerShell tokenizer deferred (noted in STATE.md blockers) -- to be added in future plan if needed

---
*Phase: 11-command-parser*
*Completed: 2026-03-05*
