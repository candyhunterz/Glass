---
phase: 15-pipe-parsing-core
plan: 01
subsystem: parsing
tags: [shlex, pipe-parsing, state-machine, shell-quoting]

# Dependency graph
requires: []
provides:
  - "Pipeline, PipeStage, PipelineClassification, BufferPolicy, StageBuffer, FinalizedBuffer types"
  - "split_pipes() byte-level pipe boundary detection with shell quoting awareness"
  - "parse_pipeline() function producing typed Pipeline structs"
affects: [15-02-classify-buffer, 16-pipe-capture, 17-pipe-ui, 18-pipe-storage]

# Tech tracking
tech-stack:
  added: [shlex]
  patterns: [two-phase-parse, byte-level-state-machine]

key-files:
  created:
    - crates/glass_pipes/src/types.rs
    - crates/glass_pipes/src/parser.rs
  modified:
    - crates/glass_pipes/Cargo.toml
    - crates/glass_pipes/src/lib.rs

key-decisions:
  - "Whitespace splitting for program extraction instead of shlex, because shlex treats backslash as escape which mangles Windows paths"
  - "Backtick escape support alongside backslash for PowerShell compatibility in pipe parser"
  - "Parenthesis depth tracking for subshell and $() command substitution awareness"

patterns-established:
  - "Two-phase parse: split on pipes first (byte scanner), then tokenize stages"
  - "StageBuffer stub pattern: define full type contract now, implement logic in later plan"

requirements-completed: [PIPE-01]

# Metrics
duration: 3min
completed: 2026-03-06
---

# Phase 15 Plan 01: Pipe Parsing Core Summary

**Byte-level pipe splitter with quote/escape/subshell awareness and typed Pipeline data structures using shlex for stage tokenization**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-06T06:22:58Z
- **Completed:** 2026-03-06T06:25:47Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- All pipe visualization data types defined (Pipeline, PipeStage, PipelineClassification, BufferPolicy, StageBuffer, FinalizedBuffer) serving as the contract for the entire v1.3 milestone
- Byte-level state machine pipe splitter handling single/double quotes, backslash escapes, backtick escapes (PowerShell), logical OR (||), parenthesized subshells, and $() command substitution
- parse_pipeline function producing typed Pipeline structs with path-stripped program names
- 20 comprehensive unit tests covering all edge cases

## Task Commits

Each task was committed atomically:

1. **Task 1: Create crate manifest, types, and module structure** - `c65e9f7` (feat)
2. **Task 2 RED: Add failing tests for pipe parser** - `e848f44` (test)
3. **Task 2 GREEN: Implement pipe parser** - `e9a6a20` (feat)

## Files Created/Modified
- `crates/glass_pipes/Cargo.toml` - Added shlex workspace dependency
- `crates/glass_pipes/src/lib.rs` - Module declarations and re-exports
- `crates/glass_pipes/src/types.rs` - All data types for pipe visualization feature
- `crates/glass_pipes/src/parser.rs` - split_pipes() and parse_pipeline() with 20 unit tests

## Decisions Made
- Used whitespace splitting instead of shlex for program name extraction because shlex interprets backslashes as escape characters, which mangles Windows paths like `C:\Windows\System32\cmd.exe`
- Added backtick escape support alongside backslash to handle PowerShell pipe escaping (`|`)
- Tracked parenthesis depth in scanner to correctly handle `$(cmd | grep)` and `(cmd1 | cmd2)` subshell patterns

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed Windows path stripping in program extraction**
- **Found during:** Task 2 (TDD GREEN phase)
- **Issue:** shlex::split treats backslash as escape character, converting `C:\Windows\System32\cmd.exe` to `C:WindowsSystem32cmd.exe`, breaking path stripping
- **Fix:** Switched to whitespace splitting for first-token extraction (preserves backslashes), then path-strip on the raw token
- **Files modified:** crates/glass_pipes/src/parser.rs
- **Verification:** parse_pipeline_windows_path_stripped test passes
- **Committed in:** e9a6a20 (Task 2 GREEN commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Necessary for correct Windows path handling. No scope creep.

## Issues Encountered
None beyond the auto-fixed deviation above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All data types exported and ready for Plan 02 (classify.rs and StageBuffer implementation)
- parser.rs provides split_pipes and parse_pipeline for downstream consumers
- StageBuffer has stub append/finalize methods ready for Plan 02 to implement fully

---
*Phase: 15-pipe-parsing-core*
*Completed: 2026-03-06*
