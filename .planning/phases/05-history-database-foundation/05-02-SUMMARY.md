---
phase: 05-history-database-foundation
plan: 02
subsystem: infra
tags: [clap, subcommand-routing, cli]

# Dependency graph
requires:
  - phase: 05-history-database-foundation/01
    provides: glass_history crate (used as dependency in root binary)
provides:
  - Clap-based subcommand routing in glass binary
  - Stub handlers for `glass history` and `glass mcp serve`
  - Option<Commands> pattern preserving no-arg terminal launch
affects: [07-cli-query-interface, 09-mcp-server]

# Tech tracking
tech-stack:
  added: [clap (derive)]
  patterns: [Option<Subcommand> for default-to-terminal routing]

key-files:
  created: []
  modified: [src/main.rs, src/tests.rs, Cargo.toml, Cargo.lock]

key-decisions:
  - "Used Option<Commands> with clap derive so None = terminal launch, preserving zero-arg behavior"
  - "Clap parse happens before EventLoop creation to avoid window flash on subcommands"
  - "Derived PartialEq on Commands and McpAction for unit test assertions"

patterns-established:
  - "Subcommand routing: Cli::parse() at top of main(), match on cli.command before any GUI code"
  - "Stub pattern: eprintln + process::exit(1) for unimplemented subcommands"

requirements-completed: [INFR-01]

# Metrics
duration: 12min
completed: 2026-03-05
---

# Phase 5 Plan 2: Clap Subcommand Routing Summary

**Clap derive-based subcommand routing with Option<Commands> preserving no-arg terminal launch, stub handlers for history and mcp serve**

## Performance

- **Duration:** 12 min (continuation only; original execution ~15 min total)
- **Started:** 2026-03-05T15:20:46Z
- **Completed:** 2026-03-05T15:33:00Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Added clap dependency and derive-based CLI struct with Option<Subcommand> pattern
- Subcommand routing dispatches `glass history` and `glass mcp serve` to stub handlers
- Zero-argument `glass` invocation continues to launch terminal GUI (no regression)
- Unit tests verify all parse paths: no-args, history, mcp serve, help, unknown subcommand errors

## Task Commits

Each task was committed atomically:

1. **Task 1: Add clap subcommand routing to main.rs** - `d43bcd2` (feat), `762133b` (fix: add clap and glass_history to root dependencies)
2. **Task 2: Verify subcommand routing works end-to-end** - checkpoint: human-verify (approved)

## Files Created/Modified
- `src/main.rs` - Added Cli/Commands/McpAction structs, refactored main() to parse CLI before event loop
- `src/tests.rs` - Added 5 subcommand routing unit tests
- `Cargo.toml` - Added clap and glass_history dependencies
- `Cargo.lock` - Updated lockfile

## Decisions Made
- Used `Option<Commands>` with clap derive so `None` maps to terminal launch -- simplest pattern for default behavior
- Clap parse placed before `EventLoop::build()` to prevent window flash on subcommand invocations
- Derived `PartialEq` on command enums for direct assertion in unit tests

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed glass_history u64/FromSql compile errors**
- **Found during:** Task 1
- **Issue:** glass_history crate used u64 types that don't implement FromSql in rusqlite
- **Fix:** Changed u64 to i64 in glass_history types
- **Committed in:** d43bcd2 (part of task commit)

**2. [Rule 1 - Bug] Fixed glass_history lifetime issue in retention.rs**
- **Found during:** Task 1
- **Issue:** Lifetime error in retention module
- **Fix:** Corrected lifetime annotations
- **Committed in:** d43bcd2 (part of task commit)

**3. [Rule 3 - Blocking] Missing clap and glass_history in root Cargo.toml**
- **Found during:** Task 1
- **Issue:** Dependencies not in root [dependencies] section, build failed
- **Fix:** Added clap = { workspace = true } and glass_history = { path = "crates/glass_history" }
- **Committed in:** 762133b

---

**Total deviations:** 3 auto-fixed (2 bugs, 1 blocking)
**Impact on plan:** All fixes necessary for compilation. No scope creep.

### Known Issues (Out of Scope)

- `test_resolve_db_path_global_fallback` in glass_history fails on machines with existing `~/.glass/` directory (pre-existing, not caused by this plan)

## Issues Encountered
None beyond the deviations listed above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 5 complete: glass_history crate (05-01) and subcommand routing (05-02) both done
- Phase 6 (Output Capture + Writer Integration) can begin
- Phase 7 will replace the `glass history` stub with real CLI query logic
- Phase 9 will replace the `glass mcp serve` stub with the MCP server

## Self-Check: PASSED

All files exist. All commit hashes verified.

---
*Phase: 05-history-database-foundation*
*Completed: 2026-03-05*
