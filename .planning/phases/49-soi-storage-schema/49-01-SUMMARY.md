---
phase: 49-soi-storage-schema
plan: 01
subsystem: database
tags: [sqlite, glass_history, glass_soi, schema-migration, serde_json]

requires:
  - phase: 48-soi-classifier-parser
    provides: glass_soi crate with ParsedOutput, OutputRecord, Severity, OutputType types

provides:
  - SOI schema v3 with command_output_records and output_records tables
  - insert_parsed_output inserts ParsedOutput atomically into both tables
  - get_output_summary retrieves per-command summary row
  - get_output_records retrieves records filtered by severity, file_path, record_type
  - v2->v3 migration that preserves all existing data

affects: [50-soi-mcp-tools, 51-soi-compression, 52-soi-trend-analysis]

tech-stack:
  added: [glass_soi (path dep), serde_json 1.0]
  patterns: [unchecked_transaction for atomic multi-table inserts, dynamic WHERE clause builder with positional params, Debug format for OutputType enum strings, explicit match for Severity strings]

key-files:
  created:
    - crates/glass_history/src/soi.rs
  modified:
    - crates/glass_history/Cargo.toml
    - crates/glass_history/src/db.rs
    - crates/glass_history/src/lib.rs

key-decisions:
  - "Severity strings use explicit match arms ('Error'/'Warning'/'Info'/'Success') not Debug format for future rename safety"
  - "OutputType strings use Debug format (format!(\"{:?}\", ...)) since identifiers are stable single-word variants"
  - "Dynamic WHERE clause built from positional params (numbered ?N) rather than string interpolation to avoid SQL injection"
  - "delete_command explicitly deletes SOI rows before CASCADE for belt-and-suspenders safety"
  - "test_migration_v1_to_v2 assertion changed from hardcoded 2 to SCHEMA_VERSION since cascade runs all pending migrations on open"

patterns-established:
  - "SOI insert: unchecked_transaction -> insert summary row -> iterate records with extract_record_meta -> serde_json::to_string -> commit"
  - "SOI query: dynamic SQL string built from conditions vec, Box<dyn ToSql> params built in parallel"
  - "Migration cascade: all if version < N blocks run in sequence on same open() call"

requirements-completed: [SOIS-01, SOIS-02, SOIS-03]

duration: 4min
completed: 2026-03-13
---

# Phase 49 Plan 01: SOI Storage Schema Summary

**SQLite schema v3 with command_output_records and output_records tables, v2->v3 migration, and typed insert/query helpers bridging glass_soi ParsedOutput into glass_history**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-13T05:55:40Z
- **Completed:** 2026-03-13T05:59:42Z
- **Tasks:** 1
- **Files modified:** 5 (4 modified + 1 created)

## Accomplishments
- Added glass_soi and serde_json as dependencies to glass_history crate
- Created soi.rs module with CommandOutputSummaryRow, OutputRecordRow, insert_parsed_output, get_output_summary, get_output_records, and extract_record_meta helpers
- Added v2->v3 schema migration in db.rs with command_output_records (summary), output_records (per-record), and 4 performance indexes
- Added HistoryDb delegation methods for all three SOI operations and explicit SOI row deletion in delete_command
- All 68 glass_history tests pass; full workspace test suite green; zero clippy warnings

## Task Commits

Each task was committed atomically:

1. **Task 1: Add glass_soi dependency, v3 migration, and soi.rs module with insert/query** - `c91cfa8` (feat)

## Files Created/Modified
- `crates/glass_history/src/soi.rs` - New module: CommandOutputSummaryRow, OutputRecordRow, insert_parsed_output, get_output_summary, get_output_records, extract_record_meta
- `crates/glass_history/src/db.rs` - v3 migration block, SCHEMA_VERSION=3, HistoryDb delegation methods, SOI deletions in delete_command, test_migration_v2_to_v3 test
- `crates/glass_history/src/lib.rs` - pub mod soi, re-exports for CommandOutputSummaryRow and OutputRecordRow
- `crates/glass_history/Cargo.toml` - Added glass_soi (path) and serde_json 1.0 dependencies
- `Cargo.lock` - Updated with new dependency resolutions

## Decisions Made
- Severity strings use explicit match arms not Debug format to guard against future rename churn in the glass_soi crate
- OutputType strings use Debug format since they are stable single-word identifiers (RustCompiler, Jest, etc.)
- Dynamic WHERE builder uses numbered positional params (?1, ?2...) with Box<dyn ToSql> param vec to avoid SQL injection

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed stale assertion in test_migration_v1_to_v2**
- **Found during:** Task 1 (verification run)
- **Issue:** test_migration_v1_to_v2 asserted `version == 2` (hardcoded). After v3 migration added, opening a v1 DB runs all pending migrations cascading to version 3, not 2.
- **Fix:** Changed assertion from hardcoded `2` to `SCHEMA_VERSION` to reflect correct cascade behavior. Updated comment from "user_version is now 1" to "current schema version".
- **Files modified:** crates/glass_history/src/db.rs
- **Verification:** Test passes after fix; all 68 tests green
- **Committed in:** c91cfa8 (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (Rule 1 bug fix)
**Impact on plan:** Stale hardcoded assertion was incorrect after schema addition. Fix improves test accuracy.

## Issues Encountered
None beyond the stale migration test assertion noted above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- SOI storage layer complete; downstream phases can call HistoryDb::insert_parsed_output with a ParsedOutput from glass_soi
- get_output_records supports severity/file_path/record_type filtering needed by MCP tools (Phase 50)
- No blockers

---
*Phase: 49-soi-storage-schema*
*Completed: 2026-03-13*
