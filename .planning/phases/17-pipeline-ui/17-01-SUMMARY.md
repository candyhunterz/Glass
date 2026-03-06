---
phase: 17-pipeline-ui
plan: 01
subsystem: ui
tags: [pipeline, block-renderer, overlay, wgpu, glyphon]

# Dependency graph
requires:
  - phase: 16-shell-capture-terminal-transport
    provides: CapturedStage, FinalizedBuffer, pipeline_stages on Block
provides:
  - Block pipeline_expanded, pipeline_stage_commands, expanded_stage_index fields
  - Auto-expand logic (failure or >2 stages)
  - BlockRenderer pipeline rect and text overlay generation
  - FrameRenderer pipeline overlay integration
affects: [17-02-PLAN, pipeline-interaction, mouse-click-handling]

# Tech tracking
tech-stack:
  added: [glass_pipes dependency in glass_renderer]
  patterns: [pipeline overlay rendering via build_pipeline_rects/build_pipeline_text]

key-files:
  created: []
  modified:
    - crates/glass_terminal/src/block_manager.rs
    - crates/glass_renderer/src/block_renderer.rs
    - crates/glass_renderer/src/frame.rs
    - crates/glass_renderer/Cargo.toml

key-decisions:
  - "Pipeline stage rows rendered as overlays (not inserted grid rows) for consistency with block decorations"
  - "Expanded stage output capped at 50 lines to avoid frame rate drops"
  - "FinalizedBuffer::Sampled shows 25 head + 25 tail lines with omission indicator"

patterns-established:
  - "build_pipeline_rects/build_pipeline_text follow same signature pattern as build_block_rects/build_block_text"
  - "Pipeline labels wired into FrameRenderer Phase A/B overlay buffer pattern"

requirements-completed: [UI-01, UI-02, UI-03]

# Metrics
duration: 4min
completed: 2026-03-06
---

# Phase 17 Plan 01: Pipeline UI Core Summary

**Block pipeline expand/collapse state with auto-expand logic and stage row overlay rendering showing command text, line count, and byte count**

## Performance

- **Duration:** 4 min
- **Started:** 2026-03-06T07:29:25Z
- **Completed:** 2026-03-06T07:33:01Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- Block struct extended with pipeline_expanded, pipeline_stage_commands, expanded_stage_index fields plus toggle/set methods
- Auto-expand logic fires on CommandFinished for failed pipelines or >2 stages
- BlockRenderer generates pipeline stage overlay rects and text labels with command, line count, byte count, expand indicators
- Expanded stage content rendering supports Complete, Sampled (head+tail), and Binary FinalizedBuffer variants
- FrameRenderer integrates pipeline overlays in draw_frame render path (both rects and text)

## Task Commits

Each task was committed atomically:

1. **Task 1: Extend Block with pipeline UI state and auto-expand logic** - `702546e` (feat, TDD)
2. **Task 2: Add pipeline stage rendering to BlockRenderer and wire into FrameRenderer** - `08ef97a` (feat)

## Files Created/Modified
- `crates/glass_terminal/src/block_manager.rs` - Added pipeline_expanded, pipeline_stage_commands, expanded_stage_index fields; toggle/set methods; auto-expand logic in CommandFinished handler; 10 new tests
- `crates/glass_renderer/src/block_renderer.rs` - Added line_count(), format_bytes() helpers; build_pipeline_rects(), build_pipeline_text(), build_stage_output_labels() methods
- `crates/glass_renderer/src/frame.rs` - Wired pipeline rects and labels into draw_frame overlay pipeline
- `crates/glass_renderer/Cargo.toml` - Added glass_pipes dependency for FinalizedBuffer access

## Decisions Made
- Pipeline stage rows rendered as overlays (not inserted grid rows) -- consistent with existing block decoration pattern, avoids grid content shifting
- Expanded stage output capped at 50 lines -- prevents frame rate drops on large captures, virtual scrolling deferred
- Sampled output shows 25 head + 25 tail lines with byte omission indicator between

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Pipeline blocks render with stage rows when expanded
- Ready for Plan 02: mouse click handling and keyboard shortcuts for toggling expansion
- Auto-expand/collapse logic complete, user interaction needed for manual toggle

---
*Phase: 17-pipeline-ui*
*Completed: 2026-03-06*
