---
phase: 52-soi-display
plan: 01
subsystem: ui
tags: [soi, config, block-renderer, events, toml]

# Dependency graph
requires:
  - phase: 50-soi-pipeline-integration
    provides: AppEvent::SoiReady variant with command_id, summary, severity
  - phase: 51-soi-compression-engine
    provides: ParsedOutput with raw_line_count field
provides:
  - SoiSection config struct with enabled/shell_summary/format/min_lines fields
  - soi_summary and soi_severity fields on Block struct
  - raw_line_count on AppEvent::SoiReady event
  - soi_color_for_severity helper in block_renderer
  - SOI label emission in build_block_text for Complete blocks
affects: [52-02-soi-display, glass_renderer, glass_terminal, glass_core]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - SoiSection follows PipesSection optional config pattern with serde defaults
    - SOI label is left-anchored (x=cell_width) to avoid collision with right-side badges

key-files:
  created: []
  modified:
    - crates/glass_core/src/config.rs
    - crates/glass_core/src/event.rs
    - crates/glass_terminal/src/block_manager.rs
    - crates/glass_renderer/src/block_renderer.rs
    - src/main.rs

key-decisions:
  - "SoiSection uses Option<SoiSection> in GlassConfig (None when absent) matching PipesSection pattern"
  - "raw_line_count is i64 on SoiReady (not usize) to match HistoryDb i64 row types"
  - "SOI label placed at x=cell_width*1.0 left-anchored to avoid right-side badge/duration/undo collisions"
  - "soi_color_for_severity is a module-level fn not a method -- pure mapping, no self needed"

patterns-established:
  - "Block SOI fields default to None and are populated post-command via SoiReady event handler (Plan 02)"
  - "SOI label only emitted when block.state == Complete AND soi_summary is Some"

requirements-completed: [SOID-01, SOID-03]

# Metrics
duration: 12min
completed: 2026-03-13
---

# Phase 52 Plan 01: SOI Display Data Model Summary

**SoiSection config, Block SOI fields, SoiReady raw_line_count, and severity-colored left-anchored SOI label rendering in build_block_text**

## Performance

- **Duration:** 12 min
- **Started:** 2026-03-13T07:55:00Z
- **Completed:** 2026-03-13T08:07:00Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments

- Added SoiSection struct to glass_core config with enabled/shell_summary/format/min_lines defaults
- Added soi_summary/soi_severity Option<String> fields to Block struct
- Added raw_line_count: i64 to AppEvent::SoiReady, wired in main.rs SOI worker
- Implemented soi_color_for_severity helper with 4 severity levels plus neutral fallback
- build_block_text now emits left-anchored SOI BlockLabel for Complete blocks with soi_summary set

## Task Commits

Each task was committed atomically:

1. **Task 1: Add SoiSection config, Block SOI fields, and SoiReady raw_line_count** - `b8f2387` (feat)
2. **Task 2: Add SOI label rendering in build_block_text** - `fa10a87` (feat)

## Files Created/Modified

- `crates/glass_core/src/config.rs` - Added SoiSection struct, soi field on GlassConfig, 3 tests
- `crates/glass_core/src/event.rs` - Added raw_line_count: i64 to SoiReady, updated test
- `crates/glass_terminal/src/block_manager.rs` - Added soi_summary/soi_severity fields to Block
- `crates/glass_renderer/src/block_renderer.rs` - Added soi_color_for_severity, SOI label in build_block_text, 4 tests
- `src/main.rs` - Updated SoiReady construction to capture raw_line_count, updated destructuring

## Decisions Made

- `raw_line_count` is `i64` on `SoiReady` (not `usize`) to match HistoryDb row ID types and avoid casting in Plan 02 handler
- `SoiSection` uses `Option<SoiSection>` in `GlassConfig` — `None` when absent, matching `PipesSection` pattern
- SOI label at `x = cell_width * 1.0` left-anchored — right side is occupied by exit badge, duration, and undo labels
- `soi_color_for_severity` is a free function (not method) — pure severity-to-color mapping with no instance state

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## Next Phase Readiness

- Plan 02 can now populate `block.soi_summary` / `block.soi_severity` in the `SoiReady` event handler and the SOI label will render automatically
- `raw_line_count` is available on `SoiReady` for Plan 02's `min_lines` threshold check
- All 7 new tests pass; build and clippy clean

---
*Phase: 52-soi-display*
*Completed: 2026-03-13*
