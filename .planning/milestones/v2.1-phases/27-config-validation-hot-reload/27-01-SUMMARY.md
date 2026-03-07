---
phase: 27-config-validation-hot-reload
plan: 01
subsystem: config
tags: [toml, validation, config-error, partial-eq, notify]

requires:
  - phase: none
    provides: n/a
provides:
  - ConfigError struct with line/column/snippet for actionable error messages
  - load_validated() returning Result<GlassConfig, ConfigError>
  - PartialEq on GlassConfig and all sub-structs
  - font_changed() diff helper for selective font rebuilds
  - notify dependency available in glass_core
affects: [27-02-config-watcher, config-hot-reload]

tech-stack:
  added: [notify 8.2]
  patterns: [structured-config-errors, config-diffing]

key-files:
  created: []
  modified:
    - crates/glass_core/src/config.rs
    - crates/glass_core/Cargo.toml

key-decisions:
  - "Used toml span() API for byte-offset-to-line/col conversion rather than regex parsing"
  - "Direct f32 comparison in font_changed() since values are parsed from TOML, not computed"

patterns-established:
  - "ConfigError pattern: structured errors with source location for user-facing config messages"
  - "Config diffing: targeted field comparison methods (font_changed) over full PartialEq for selective rebuild"

requirements-completed: [CONF-01, CONF-02]

duration: 2min
completed: 2026-03-07
---

# Phase 27 Plan 01: Config Validation Summary

**ConfigError with line/column info, load_validated() returning Result, PartialEq on GlassConfig, and font_changed() diff helper**

## Performance

- **Duration:** 2 min
- **Started:** 2026-03-07T17:57:36Z
- **Completed:** 2026-03-07T17:59:21Z
- **Tasks:** 1 (TDD: RED + GREEN)
- **Files modified:** 2

## Accomplishments
- ConfigError struct with message, line, column, snippet fields and Display impl
- load_validated() parses TOML and returns structured errors with span-based line/col info
- PartialEq derived on GlassConfig, HistorySection, SnapshotSection, PipesSection
- font_changed() method for selective font rebuild detection
- notify 8.0 dependency added to glass_core for Plan 02 config watcher
- All 27 tests pass (10 new + 17 existing)

## Task Commits

Each task was committed atomically:

1. **Task 1 (RED): Failing tests** - `acaaeeb` (test)
2. **Task 1 (GREEN): Implementation** - `27b2040` (feat)

_TDD task: test-first then implementation._

## Files Created/Modified
- `crates/glass_core/src/config.rs` - Added ConfigError, load_validated(), font_changed(), PartialEq derives
- `crates/glass_core/Cargo.toml` - Added notify dependency

## Decisions Made
- Used toml crate's span() API to convert byte offsets to line/column numbers rather than separate regex parsing
- Direct f32 comparison in font_changed() is safe since values come from TOML parsing, not floating-point arithmetic

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- ConfigError and load_validated() ready for Plan 02 config watcher to use
- notify dependency available for config file watching
- font_changed() ready for selective font rebuild on config changes
- PartialEq enables full config comparison in watcher logic

---
*Phase: 27-config-validation-hot-reload*
*Completed: 2026-03-07*
