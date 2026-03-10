---
phase: 38-structured-error-extraction
plan: 01
subsystem: parsing
tags: [regex, serde_json, error-extraction, compiler-output]

requires:
  - phase: none
    provides: standalone pure library crate
provides:
  - glass_errors crate with extract_errors() entry point
  - StructuredError and Severity types
  - Rust JSON, Rust human-readable, and generic parsers
  - Auto-detection logic for parser selection
affects: [38-02, glass_mcp]

tech-stack:
  added: [glass_errors crate]
  patterns: [OnceLock regex compilation, enum dispatch for parser selection, state machine for two-line patterns]

key-files:
  created:
    - crates/glass_errors/Cargo.toml
    - crates/glass_errors/src/lib.rs
    - crates/glass_errors/src/detect.rs
    - crates/glass_errors/src/generic.rs
    - crates/glass_errors/src/rust_json.rs
    - crates/glass_errors/src/rust_human.rs
  modified: []

key-decisions:
  - "Enum dispatch (ParserKind match) instead of trait objects for 3 parsers"
  - "OnceLock for compiled regexes per project conventions (not lazy_static)"
  - "State machine in rust_human parser pairs header lines with --> span lines"

patterns-established:
  - "Parser modules expose pub(crate) parse_* functions dispatched from lib.rs"
  - "Three-tier regex fallback in generic parser: full -> no-col -> no-severity"

requirements-completed: [ERR-02, ERR-03, ERR-04]

duration: 3min
completed: 2026-03-10
---

# Phase 38 Plan 01: Error Parser Library Summary

**Pure glass_errors crate with Rust JSON, human-readable, and generic parsers plus auto-detection dispatch**

## Performance

- **Duration:** 3 min
- **Started:** 2026-03-10T05:14:10Z
- **Completed:** 2026-03-10T05:17:20Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- Created glass_errors pure library crate with zero Glass internal dependencies
- Rust JSON parser handles both cargo wrapper and raw rustc diagnostic formats
- Rust human parser extracts error[Exxx] + --> file:line:col patterns via state machine
- Generic parser handles file:line:col with Windows path support via three regex tiers
- Auto-detection selects parser from command hint and output content sniffing
- 36 unit tests covering all parsers, detection paths, and end-to-end integration

## Task Commits

Each task was committed atomically:

1. **Task 1: Create glass_errors crate with types, detection, and generic parser** - `0c9eda6` (feat)
2. **Task 2: Implement Rust JSON and human-readable parsers** - `fb1162b` (feat)

## Files Created/Modified
- `crates/glass_errors/Cargo.toml` - Crate manifest with regex, serde, serde_json deps
- `crates/glass_errors/src/lib.rs` - Public API: extract_errors(), StructuredError, Severity types
- `crates/glass_errors/src/detect.rs` - Auto-detection: command hint + content sniffing
- `crates/glass_errors/src/generic.rs` - Generic file:line:col parser with Windows path support
- `crates/glass_errors/src/rust_json.rs` - Rust JSON diagnostic parser (cargo + raw rustc)
- `crates/glass_errors/src/rust_human.rs` - Rust human-readable error parser

## Decisions Made
- Used enum dispatch (ParserKind match) instead of trait objects -- simpler for 3 parsers
- OnceLock for compiled regexes per project conventions (not lazy_static)
- State machine in rust_human parser pairs header lines with --> span lines
- Modules are pub(crate), only extract_errors/types are public API

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- glass_errors crate fully tested and clippy clean
- Ready for Plan 02 to wire MCP tool integration via glass_extract_errors

---
*Phase: 38-structured-error-extraction*
*Completed: 2026-03-10*
