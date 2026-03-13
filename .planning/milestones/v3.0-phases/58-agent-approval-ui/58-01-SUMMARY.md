---
phase: 58-agent-approval-ui
plan: 01
subsystem: glass_renderer
tags: [renderer, agent-ui, proposal-toast, proposal-overlay, status-bar]
dependency_graph:
  requires: []
  provides: [ProposalToastRenderer, ProposalToastRenderData, ProposalToastTextLabel, ProposalOverlayRenderer, ProposalOverlayRenderData, ProposalOverlayTextLabel, StatusLabel.agent_mode_text, StatusLabel.proposal_count_text, draw_frame.proposal_toast, draw_frame.proposal_overlay]
  affects: [crates/glass_renderer, src/main.rs]
tech_stack:
  added: []
  patterns: [ConflictOverlay pattern for stateless renderer helpers, right-to-left status bar stacking chain]
key_files:
  created:
    - crates/glass_renderer/src/proposal_toast_renderer.rs
    - crates/glass_renderer/src/proposal_overlay_renderer.rs
  modified:
    - crates/glass_renderer/src/lib.rs
    - crates/glass_renderer/src/status_bar.rs
    - crates/glass_renderer/src/frame.rs
    - src/main.rs
decisions:
  - "ProposalToastRenderer/ProposalOverlayRenderer are stateless pure-computation helpers following ConflictOverlay pattern -- no GPU state, unit-testable without wgpu"
  - "Proposal UI rects added before bg_rect_count marker in draw_frame so they render in first pass with background rects"
  - "draw_multi_pane_frame renders proposal toast/overlay window-global (after all panes) -- per-plan spec"
  - "build_status_text gains 2 optional params with None defaults -- fully backward compatible"
  - "All main.rs call sites pass None, None for proposal params -- Plan 02 will wire actual data"
metrics:
  duration: ~15 min
  completed: 2026-03-13T17:20:42Z
  tasks_completed: 2
  files_created: 2
  files_modified: 4
---

# Phase 58 Plan 01: Agent Approval UI Renderers Summary

**One-liner:** Proposal toast renderer (dark teal, bottom-right, auto-dismiss countdown) and overlay renderer (backdrop + panel + diff preview, max 50 lines) with extended StatusLabel for agent mode and proposal count.

## Tasks Completed

| Task | Name | Commit | Files |
| ---- | ---- | ------ | ----- |
| 1 | Create ProposalToastRenderer and ProposalOverlayRenderer | a5d8d07 | proposal_toast_renderer.rs, proposal_overlay_renderer.rs, lib.rs |
| 2 | Extend StatusLabel and draw_frame for proposal UI | 5d12725 | status_bar.rs, frame.rs, main.rs |

## What Was Built

### ProposalToastRenderer (`crates/glass_renderer/src/proposal_toast_renderer.rs`)
- `ProposalToastRenderData { description: String, remaining_secs: u64 }` -- data transfer struct
- `ProposalToastTextLabel { text, x, y, color }` -- text label struct
- `build_toast_rects(viewport_w, viewport_h) -> Vec<RectInstance>`: 1 rect, right-aligned, 60% viewport width, 2.5 cell heights, dark teal `[0.05, 0.25, 0.35, 0.92]`, positioned above status bar
- `build_toast_text(data, viewport_w, viewport_h) -> Vec<ProposalToastTextLabel>`: 2 labels -- description (truncated to 60 chars) and hint with `[Ctrl+Shift+A: review] [auto-dismiss in Xs]`
- 9 unit tests: rect count, color, position bounds, text content, truncation at 60 chars

### ProposalOverlayRenderer (`crates/glass_renderer/src/proposal_overlay_renderer.rs`)
- `ProposalOverlayRenderData { proposals: Vec<(String, String)>, selected: usize, diff_preview: String }` -- data transfer struct
- `ProposalOverlayTextLabel { text, x, y, color }` -- text label struct
- `build_overlay_rects(viewport_w, viewport_h, data) -> Vec<RectInstance>`: backdrop (full viewport, `[0.03, 0.03, 0.03, 0.88]`) + panel (80% width centered, `[0.08, 0.12, 0.15, 1.0]`) + selected row highlight
- `build_overlay_text(viewport_w, viewport_h, data) -> Vec<ProposalOverlayTextLabel>`: header, proposal list with `>` marker for selected, diff preview (max 50 lines, colored by `+`/`-`), footer hint with `[Ctrl+Shift+Y/N/A]`
- 10 unit tests: backdrop presence, panel sizing/centering, selected highlight, empty proposals, diff truncation at 50 lines

### StatusLabel Extension (`crates/glass_renderer/src/status_bar.rs`)
- Added `agent_mode_text: Option<String>`, `proposal_count_text: Option<String>` fields
- Added `agent_mode_color: Rgb` (soft cyan `{100, 180, 200}`), `proposal_count_color: Rgb` (soft yellow `{220, 200, 100}`) fields
- Extended `build_status_text` with 2 new `Option<&str>` params -- fully backward compatible (all existing callers pass `None, None`)
- Positioned in right-to-left stacking: git_info > coord > agent_cost > agent_mode > proposal_count
- 7 new unit tests in `status_bar.rs`

### draw_frame / draw_multi_pane_frame Extension (`crates/glass_renderer/src/frame.rs`)
- Added `proposal_toast: Option<&ProposalToastRenderData>` and `proposal_overlay: Option<&ProposalOverlayRenderData>` params
- Rect rendering: overlay/toast rects added to rect_instances before `bg_rect_count` marker (first pass)
- Text rendering: overlay and toast text buffers added to `overlay_buffers`/`overlay_metas` after existing overlays
- `draw_multi_pane_frame` handles proposal UI as window-global (after all panes)
- main.rs call sites updated with `None, None` placeholders

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Floating point comparison in toast position test**
- **Found during:** Task 1 test run
- **Issue:** `assert_eq!(pos[0], 310.0)` failed due to float accumulation in `800.0 * 0.6 = 479.99997` instead of exact `480.0`
- **Fix:** Changed test assertion to use `.abs() < 0.01` approximate comparison
- **Files modified:** `crates/glass_renderer/src/proposal_toast_renderer.rs`
- **Commit:** a5d8d07 (fixed inline)

## Verification Results

- `cargo test --package glass_renderer`: 138 tests pass (19 new proposal tests)
- `cargo clippy --workspace -- -D warnings`: clean
- `cargo build --workspace`: builds cleanly

## Self-Check: PASSED
- `crates/glass_renderer/src/proposal_toast_renderer.rs`: FOUND
- `crates/glass_renderer/src/proposal_overlay_renderer.rs`: FOUND
- commit a5d8d07: FOUND
- commit 5d12725: FOUND
