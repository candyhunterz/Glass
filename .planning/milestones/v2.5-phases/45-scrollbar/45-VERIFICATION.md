---
phase: 45-scrollbar
verified: 2026-03-11T03:00:00Z
status: human_needed
score: 13/15 must-haves verified
re_verification: false
human_verification:
  - test: "Run cargo run, generate scrollback history, verify scrollbar is visible on right edge"
    expected: "Subtle dark track with gray thumb visible on right edge of terminal pane"
    why_human: "Visual rendering requires GPU and running window"
  - test: "Verify terminal text does not render under the scrollbar gutter"
    expected: "Text stops 8px before right window edge, scrollbar occupies that 8px strip"
    why_human: "Requires visual inspection of running terminal"
  - test: "Hover mouse over scrollbar thumb, verify it brightens"
    expected: "Thumb changes from dim gray (alpha 0.4) to brighter gray (alpha 0.7)"
    why_human: "Requires mouse interaction with running app"
  - test: "Click and drag the scrollbar thumb up and down"
    expected: "Terminal scrolls smoothly through history without jitter"
    why_human: "Requires mouse drag interaction"
  - test: "Click in track above/below thumb"
    expected: "Terminal jumps one page up or down respectively"
    why_human: "Requires mouse click interaction"
  - test: "Create split panes (Ctrl+Shift+D), verify each pane has its own scrollbar"
    expected: "Each pane has independent scrollbar, scrolling one does not affect the other"
    why_human: "Requires multi-pane visual + interaction verification"
  - test: "Click a non-focused pane's scrollbar in multi-pane mode"
    expected: "That pane becomes focused"
    why_human: "Requires multi-pane interaction"
---

# Phase 45: Scrollbar Verification Report

**Phase Goal:** Always-visible interactive scrollbar on every terminal pane with drag, click, and hover support
**Verified:** 2026-03-11T03:00:00Z
**Status:** human_needed
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | ScrollbarRenderer produces track + thumb RectInstance quads for any pane | VERIFIED | `scrollbar.rs` lines 106-142: `build_scrollbar_rects` returns Vec with track + thumb RectInstance; 18 unit tests pass |
| 2 | Thumb height is proportional to visible/total line ratio with minimum enforced | VERIFIED | `scrollbar.rs` lines 78-79: `thumb_ratio = screen_lines / total_lines`, clamped to MIN_THUMB_HEIGHT (20px); tests `thumb_height_proportional_to_visible_ratio` and `min_thumb_height_enforced` pass |
| 3 | Thumb position correctly maps from display_offset (0=bottom, history_size=top) | VERIFIED | `scrollbar.rs` lines 85-90: `scroll_ratio = 1.0 - (offset/history)`, thumb_y = scrollable_track * ratio; tests for bottom/top/middle all pass |
| 4 | Empty history produces a full-track thumb | VERIFIED | `scrollbar.rs` lines 74-76 + test `empty_history_fills_entire_track` passes |
| 5 | Hit-test correctly distinguishes Thumb, TrackAbove, TrackBelow, and miss | VERIFIED | `scrollbar.rs` lines 148-183 + 4 hit_test unit tests pass |
| 6 | Hover/drag state changes thumb color from dim to bright | VERIFIED | `scrollbar.rs` lines 130-134; tests `hover_produces_active_color`, `drag_produces_active_color`, `rest_produces_rest_color` pass |
| 7 | Scrollbar rects appear in both single-pane and multi-pane draw pipelines | VERIFIED | `frame.rs` lines 263-282 (single-pane) and lines 924-939 (multi-pane) both call `build_scrollbar_rects` and extend `rect_instances` |
| 8 | Terminal grid width is reduced by 8px in all resize calculations | VERIFIED | 7 locations in `main.rs` (lines 333, 389, 591, 1041, 1062, 1120, 1140) all subtract SCROLLBAR_WIDTH before dividing by cell_w |
| 9 | PTY column count reflects the reduced grid width | VERIFIED | Same 7 subtraction sites feed directly into `resize_pty` calls with the reduced num_cols/pane_cols |
| 10 | Dragging the scrollbar thumb scrolls smoothly through history | VERIFIED (code) | `main.rs` lines 1653-1671: drag handler computes target_offset from mouse_y, calls `scroll_display(Scroll::Delta(delta))` -- needs human visual confirmation |
| 11 | Clicking above/below the thumb jumps one page | VERIFIED (code) | `main.rs` lines 1940-1947: TrackAbove calls `scroll_display(Scroll::PageUp)`, TrackBelow calls `scroll_display(Scroll::PageDown)` |
| 12 | Scrollbar drag does not trigger text selection | VERIFIED | `main.rs` line 1653: drag check runs before selection logic and returns early; mouse press sets `mouse_left_pressed = false` preventing selection start |
| 13 | Hovering over a scrollbar brightens the thumb | VERIFIED (code) | `main.rs` lines 1700-1767: hover detection updates `scrollbar_hovered_pane`, wired to draw calls at lines 847-848 (single) and 953-960 (multi) |
| 14 | Each pane's scrollbar operates independently in multi-pane mode | VERIFIED (code) | `main.rs` lines 1705-1730: iterates pane_layouts for per-pane hit-test; `scrollbar_state` built per-pane at lines 953-960 |
| 15 | Clicking a pane's scrollbar focuses that pane | VERIFIED (code) | `main.rs`: scrollbar click handler includes `set_focused_pane` call for multi-pane mode |

**Score:** 13/15 truths verified programmatically; 2 need human visual/interaction confirmation

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `crates/glass_renderer/src/scrollbar.rs` | ScrollbarRenderer, build_scrollbar_rects, hit_test, ScrollbarHit, SCROLLBAR_WIDTH | VERIFIED | 351 lines, all expected APIs present, 18 unit tests, no TODOs/placeholders |
| `crates/glass_renderer/src/lib.rs` | pub mod scrollbar, re-exports | VERIFIED | Line 10: `pub mod scrollbar;`, Line 23: `pub use scrollbar::{ScrollbarHit, ScrollbarRenderer, SCROLLBAR_WIDTH};` |
| `crates/glass_renderer/src/frame.rs` | ScrollbarRenderer field, scrollbar rects in both draw paths | VERIFIED | Line 41: field, lines 101/115: init, lines 151-154: accessor, lines 263-282: single-pane draw, lines 924-939: multi-pane draw |
| `src/main.rs` | ScrollbarDragInfo, scrollbar mouse handling, grid width reduction | VERIFIED | ScrollbarDragInfo struct at line 132, drag/hover/click handlers wired, 7 SCROLLBAR_WIDTH subtraction sites |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| scrollbar.rs | rect_renderer.rs | `use crate::rect_renderer::RectInstance` | WIRED | Line 7: import present, RectInstance used in build_scrollbar_rects return type |
| frame.rs | scrollbar.rs | FrameRenderer owns ScrollbarRenderer, calls build_scrollbar_rects | WIRED | Line 20: import, line 41: field, lines 271+928: method calls |
| main.rs | scrollbar.rs | imports ScrollbarHit, SCROLLBAR_WIDTH | WIRED | Line 29-30: `use glass_renderer::{..., ScrollbarHit, SCROLLBAR_WIDTH}` |
| main.rs | alacritty_terminal scroll_display | Scrollbar click/drag calls scroll_display | WIRED | Lines 1670, 1941, 1946: `scroll_display(Scroll::Delta/PageUp/PageDown)` |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| SB-01 | 45-01 | ScrollbarRenderer produces track + thumb rects | SATISFIED | scrollbar.rs build_scrollbar_rects, 18 tests pass |
| SB-02 | 45-01 | Thumb height proportional to visible/total ratio | SATISFIED | compute_thumb_geometry with ratio math, unit tested |
| SB-03 | 45-01 | Thumb position maps from display_offset | SATISFIED | scroll_ratio formula, tests for top/middle/bottom |
| SB-04 | 45-01 | Minimum thumb height enforced | SATISFIED | MIN_THUMB_HEIGHT=20.0, .max() clamp, unit tested |
| SB-05 | 45-01 | Empty history produces full-track thumb | SATISFIED | history_size=0 check returns full track_height, tested |
| SB-06 | 45-01 | Hit-test identifies Thumb/TrackAbove/TrackBelow | SATISFIED | hit_test method with geometry comparison, 4 tests |
| SB-07 | 45-01 | Hit-test returns None outside scrollbar | SATISFIED | x-range and y-range boundary checks, 2 tests |
| SB-08 | 45-01 | Hover state changes thumb color | SATISFIED | THUMB_COLOR_REST vs THUMB_COLOR_ACTIVE, tested |
| SB-09 | 45-02 | Grid width subtracted in resize calculations | SATISFIED | 7 SCROLLBAR_WIDTH subtraction sites in main.rs |
| SB-10 | 45-02 | Scrollbar drag updates display_offset | SATISFIED | drag handler computes target_offset, calls scroll_display |
| SB-11 | 45-02 | Multi-pane: each pane has independent scrollbar | SATISFIED | Per-pane hit-test, per-pane scrollbar_state, per-pane draw |

All 11 requirement IDs (SB-01 through SB-11) accounted for. No orphaned requirements.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (none) | - | - | - | No TODOs, FIXMEs, placeholders, or empty implementations found |

No anti-patterns detected. Zero TODOs/FIXMEs in scrollbar.rs or scrollbar-related main.rs code. No stub implementations.

### Human Verification Required

### 1. Scrollbar Visual Appearance

**Test:** Run `cargo run`, generate scrollback history (e.g., `ls -la` several times, `cat` a long file). Verify scrollbar track and thumb are visible on right edge.
**Expected:** Subtle dark track with gray thumb, text does not render under the scrollbar gutter.
**Why human:** Requires GPU rendering and visual inspection.

### 2. Hover Feedback

**Test:** Move mouse over the scrollbar thumb.
**Expected:** Thumb brightens from dim gray to slightly brighter gray.
**Why human:** Requires mouse interaction with running application.

### 3. Drag-to-Scroll

**Test:** Click and drag the scrollbar thumb up and down through history.
**Expected:** Terminal content scrolls smoothly without jitter, thumb follows mouse.
**Why human:** Requires mouse drag interaction and visual feedback assessment.

### 4. Track Click Page Jump

**Test:** Click in the track area above the thumb, then below the thumb.
**Expected:** Content jumps one page up or down respectively.
**Why human:** Requires click interaction.

### 5. Multi-Pane Independence

**Test:** Create split panes (Ctrl+Shift+D), generate history in each, scroll one pane's scrollbar.
**Expected:** Each pane has its own scrollbar, scrolling one does not affect the other, clicking a non-focused pane's scrollbar focuses that pane.
**Why human:** Requires multi-pane UI with interaction testing.

### Build Verification

- `cargo clippy --workspace -- -D warnings`: CLEAN (no warnings)
- `cargo test --workspace`: ALL PASS (646 tests, 0 failures)
- `cargo test -p glass_renderer scrollbar`: ALL PASS (18 scrollbar-specific tests)

### Gaps Summary

No gaps found. All artifacts exist, are substantive (not stubs), and are fully wired. All 11 requirements (SB-01 through SB-11) are satisfied at the code level. The only remaining verification is human testing of visual appearance and mouse interactions, which cannot be verified programmatically.

---

_Verified: 2026-03-11T03:00:00Z_
_Verifier: Claude (gsd-verifier)_
