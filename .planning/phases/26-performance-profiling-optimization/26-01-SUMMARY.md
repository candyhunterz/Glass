---
phase: 26-performance-profiling-optimization
plan: 01
subsystem: infra
tags: [criterion, tracing, tracing-chrome, benchmarks, profiling, perfetto]

# Dependency graph
requires: []
provides:
  - "Criterion benchmark harness with resolve_color, osc_scan, and cold_start benchmark groups"
  - "Feature-gated perf tracing instrumentation on hot-path functions"
  - "tracing-chrome subscriber producing glass-trace.json for Perfetto visualization"
affects: [26-02-PLAN]

# Tech tracking
tech-stack:
  added: [criterion 0.5, tracing-chrome 0.7]
  patterns: [cfg_attr feature-gated instrumentation, optional tracing-chrome subscriber]

key-files:
  created:
    - benches/perf_benchmarks.rs
  modified:
    - Cargo.toml
    - crates/glass_terminal/Cargo.toml
    - crates/glass_renderer/Cargo.toml
    - crates/glass_terminal/src/lib.rs
    - src/main.rs
    - crates/glass_terminal/src/pty.rs
    - crates/glass_terminal/src/grid_snapshot.rs
    - crates/glass_terminal/src/osc_scanner.rs
    - crates/glass_renderer/src/frame.rs
    - crates/glass_renderer/src/grid_renderer.rs

key-decisions:
  - "Export resolve_color from glass_terminal lib.rs for benchmark access (was previously internal)"
  - "Use cfg_attr with feature=perf for zero-overhead instrumentation when perf feature disabled"
  - "Use trace level for OscScanner::scan since it is called frequently per PTY read"
  - "Only instrument outer functions (not resolve_color or per-cell) to avoid tracing overhead in tight loops"

patterns-established:
  - "Feature-gated instrumentation: #[cfg_attr(feature = \"perf\", tracing::instrument(skip_all))]"
  - "Perf feature propagation: root perf -> glass_terminal/perf + glass_renderer/perf + dep:tracing-chrome"

requirements-completed: [PERF-01, PERF-02]

# Metrics
duration: 4min
completed: 2026-03-07
---

# Phase 26 Plan 01: Benchmark Infrastructure & Tracing Summary

**Criterion benchmark harness with resolve_color/osc_scan/cold_start groups and feature-gated tracing-chrome instrumentation on 7 hot-path functions**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-07T17:18:03Z
- **Completed:** 2026-03-07T17:22:16Z
- **Tasks:** 2
- **Files modified:** 11

## Accomplishments
- Criterion benchmarks for resolve_color (3 variants), osc_scan, and cold_start process startup
- Feature-gated tracing::instrument on 7 hot-path functions with zero overhead when perf feature disabled
- tracing-chrome subscriber in main.rs producing glass-trace.json viewable in Perfetto/chrome://tracing
- All 436 workspace tests pass with no regressions

## Task Commits

Each task was committed atomically:

1. **Task 1: Add criterion benchmarks and cargo feature configuration** - `21bedc7` (feat)
2. **Task 2: Add feature-gated tracing instrumentation and tracing-chrome subscriber** - `b85af0d` (feat)

## Files Created/Modified
- `benches/perf_benchmarks.rs` - Criterion benchmark harness with resolve_color, osc_scan, cold_start groups
- `Cargo.toml` - Added perf feature, tracing-chrome dep, criterion dev-dep, [[bench]] section
- `Cargo.lock` - Updated with new dependencies
- `crates/glass_terminal/Cargo.toml` - Added perf = [] feature flag
- `crates/glass_renderer/Cargo.toml` - Added perf = [] feature flag
- `crates/glass_terminal/src/lib.rs` - Export resolve_color for benchmark access
- `src/main.rs` - Feature-gated tracing-chrome subscriber for terminal mode
- `crates/glass_terminal/src/pty.rs` - Instrumented glass_pty_loop and pty_read_with_scan
- `crates/glass_terminal/src/grid_snapshot.rs` - Instrumented snapshot_term
- `crates/glass_terminal/src/osc_scanner.rs` - Instrumented OscScanner::scan at trace level
- `crates/glass_renderer/src/frame.rs` - Instrumented draw_frame and draw_multi_pane_frame
- `crates/glass_renderer/src/grid_renderer.rs` - Instrumented build_rects and build_text_buffers

## Decisions Made
- Exported resolve_color from glass_terminal lib.rs (was previously only used internally) to enable benchmarking
- Used cfg_attr feature gating for zero-overhead when perf is not enabled
- OscScanner::scan uses trace level (not default info) since it fires per PTY read
- Only instrumented outer functions -- resolve_color and per-cell functions skipped to avoid tracing overhead dominating tight loops

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Exported resolve_color from glass_terminal lib.rs**
- **Found during:** Task 1 (benchmark creation)
- **Issue:** resolve_color was pub in grid_snapshot.rs but not re-exported from lib.rs, so benches/perf_benchmarks.rs could not import it
- **Fix:** Added resolve_color to the pub use grid_snapshot::{...} line in lib.rs
- **Files modified:** crates/glass_terminal/src/lib.rs
- **Verification:** cargo bench --no-run compiles successfully
- **Committed in:** 21bedc7 (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Essential for benchmark compilation. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Benchmark infrastructure ready for Plan 02 optimization work
- `cargo bench` will produce statistical reports for baseline measurements
- `cargo run --release --features perf` will produce glass-trace.json for profiling

---
*Phase: 26-performance-profiling-optimization*
*Completed: 2026-03-07*
