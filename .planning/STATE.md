---
gsd_state_version: 1.0
milestone: v2.1
milestone_name: Packaging & Polish
status: completed
stopped_at: Completed 28-02-PLAN.md
last_updated: "2026-03-07T18:42:17.457Z"
last_activity: 2026-03-07 -- Phase 28 complete (CI release workflow)
progress:
  total_phases: 5
  completed_phases: 3
  total_plans: 6
  completed_plans: 6
  percent: 100
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-07)

**Core value:** A terminal that looks and feels normal but passively watches, indexes, and snapshots everything -- surfacing intelligence only when you need it.
**Current focus:** Phase 28 - Platform Packaging & CI Release

## Current Position

Phase: 28 of 30 (Platform Packaging & CI Release)
Plan: 2 of 2 in current phase
Status: Phase 28 Complete
Last activity: 2026-03-07 -- Phase 28 complete (CI release workflow)

Progress: [██████████] Phase 28: 2/2 plans complete (100%)

## Performance Metrics

**Velocity (cumulative):**
- v1.0: 12 plans in ~1.8 hours (~9 min/plan)
- v1.1: 12 plans in ~4.5 hours (~20 min/plan)
- v1.2: 13 plans in ~6 hours (~28 min/plan)
- v1.3: 11 plans in ~2 hours (~11 min/plan)
- v2.0: 6 plans in ~23 min (~4 min/plan)
- v2.1: 6 plans in ~20 min (~3 min/plan)
- Total: 61 plans across 28 phases in 3 days

## Accumulated Context

### Decisions

See PROJECT.md Key Decisions table for full history.

Recent decisions affecting v2.1:
- UpgradeCode GUID D5F79758-7183-4EBE-9B63-DADD19B1D42C is permanent for Windows MSI upgrades
- macOS minimum version 11.0 (Big Sur), bundle ID com.glass.terminal
- packaging/ directory structure for platform-specific files
- SessionMux multi-session architecture means config hot-reload must propagate to ALL sessions/panes
- Cross-platform CI matrix already exists (Windows/macOS/Linux) -- extend for release builds
- notify 8.2 already in workspace -- reuse for config file watching
- Feature-gated perf instrumentation: cfg_attr(feature = "perf") for zero-overhead when disabled
- Only instrument outer functions (not resolve_color/per-cell) to avoid tracing overhead in tight loops
- OscScanner::scan uses trace level since it fires per PTY read
- Record cold start honestly at 522ms (4.4% over 500ms target) -- transparency over vanity metrics
- PERFORMANCE.md as single source of truth for performance baselines and measurement methodology
- Used toml span() API for byte-offset-to-line/col conversion in ConfigError
- Direct f32 comparison in font_changed() since values are parsed from TOML, not computed
- Box<GlassConfig> in ConfigReloaded variant to keep AppEvent size reasonable
- Watch parent directory (not config file) to handle atomic saves from vim/VSCode
- Error overlay follows SearchOverlayRenderer pattern for architectural consistency
- Release workflow: three parallel jobs with no inter-job dependencies; softprops/action-gh-release handles race condition
- Version verification in all CI release jobs prevents Cargo.toml/tag mismatch

### Pending Todos

None.

### Blockers/Concerns

- ScaleFactorChanged is log-only (tech debt) -- Phase 27 config hot-reload should address font recalculation
- macOS code signing deferred -- unsigned DMG triggers Gatekeeper; document xattr workaround
- pruner.rs max_size_mb not enforced (minor, count/age pruning sufficient)

## Session Continuity

Last session: 2026-03-07T18:38:52Z
Stopped at: Completed 28-02-PLAN.md
Resume file: None
