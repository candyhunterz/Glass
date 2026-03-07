---
phase: 27-config-validation-hot-reload
plan: 02
subsystem: ui
tags: [notify, hot-reload, config, file-watcher, font, overlay]

# Dependency graph
requires:
  - phase: 27-config-validation-hot-reload/01
    provides: ConfigError, load_validated(), font_changed() on GlassConfig
provides:
  - Config file watcher using notify crate watching ~/.glass/ directory
  - AppEvent::ConfigReloaded variant for event-driven config propagation
  - FrameRenderer::update_font() for live font metric rebuild
  - ConfigErrorOverlay renderer for inline error display
  - End-to-end hot-reload wired through Processor event loop
affects: [28-packaging]

# Tech tracking
tech-stack:
  added: []
  patterns: [file-watcher-via-parent-directory, error-overlay-renderer-pattern, box-enum-variant]

key-files:
  created:
    - crates/glass_core/src/config_watcher.rs
    - crates/glass_renderer/src/config_error_overlay.rs
  modified:
    - crates/glass_core/src/event.rs
    - crates/glass_core/src/lib.rs
    - crates/glass_core/src/config.rs
    - crates/glass_renderer/src/frame.rs
    - crates/glass_renderer/src/lib.rs
    - src/main.rs

key-decisions:
  - "Box<GlassConfig> in ConfigReloaded variant to keep AppEvent size reasonable"
  - "Watch parent directory (not config file directly) to handle atomic saves from vim/VSCode"
  - "Error overlay follows SearchOverlayRenderer pattern for consistency"

patterns-established:
  - "File watcher pattern: spawn thread, watch parent dir, filter by filename, send event via proxy"
  - "Error overlay pattern: ConfigErrorOverlay with build_error_rects() and build_error_text() mirroring SearchOverlayRenderer"

requirements-completed: [CONF-02, CONF-03]

# Metrics
duration: 8min
completed: 2026-03-07
---

# Phase 27 Plan 02: Config Hot-Reload Summary

**Live config hot-reload via notify file watcher with font rebuild, PTY resize propagation, and red error overlay banner**

## Performance

- **Duration:** ~8 min
- **Started:** 2026-03-07T18:07:00Z
- **Completed:** 2026-03-07T18:15:38Z
- **Tasks:** 3
- **Files modified:** 9

## Accomplishments
- Config watcher spawns background thread watching ~/.glass/ directory for config.toml changes, handling atomic saves
- Font changes trigger FrameRenderer::update_font() which rebuilds GridRenderer and all dependent sub-renderers, then resizes PTY for all sessions
- Config parse errors display a red banner overlay at viewport top with line/column info without blocking input
- Error banner auto-dismisses on next successful config save
- Non-visual config changes (history thresholds, snapshots) apply without triggering font rebuild

## Task Commits

Each task was committed atomically:

1. **Task 1: Config watcher, AppEvent variant, and error overlay renderer** - `a57f7a4` (feat)
2. **Task 2: FrameRenderer::update_font() and Processor ConfigReloaded handler** - `5a0ae22` (feat)
3. **Task 3: Verify config hot-reload end-to-end** - human-verify checkpoint (approved)

## Files Created/Modified
- `crates/glass_core/src/config_watcher.rs` - spawn_config_watcher() using notify to watch parent directory
- `crates/glass_core/src/event.rs` - ConfigReloaded variant on AppEvent with Box<GlassConfig> and optional ConfigError
- `crates/glass_core/src/lib.rs` - Export config_watcher module
- `crates/glass_core/src/config.rs` - Additional helper for config comparison
- `crates/glass_renderer/src/config_error_overlay.rs` - ConfigErrorOverlay with rect and text label builders
- `crates/glass_renderer/src/frame.rs` - update_font() method rebuilding grid and sub-renderers
- `crates/glass_renderer/src/lib.rs` - Export config_error_overlay module
- `src/main.rs` - Watcher spawn, ConfigReloaded handler, error overlay rendering

## Decisions Made
- Used Box<GlassConfig> in ConfigReloaded enum variant to keep AppEvent size small (GlassConfig has multiple String fields)
- Watch parent directory instead of config file directly to survive atomic saves (vim write-tmp-then-rename)
- Error overlay follows established SearchOverlayRenderer pattern for architectural consistency
- Direct f32 comparison in font_changed() since values come from TOML parsing, not floating-point computation

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Config validation (Plan 01) and hot-reload (Plan 02) complete -- Phase 27 done
- Ready for Phase 28 (packaging) with all config infrastructure in place

## Self-Check: PASSED

- All key files exist on disk
- Both task commits (a57f7a4, 5a0ae22) verified in git history
- Task 3 human-verify checkpoint approved by user

---
*Phase: 27-config-validation-hot-reload*
*Completed: 2026-03-07*
