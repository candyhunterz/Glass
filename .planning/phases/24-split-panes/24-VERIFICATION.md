---
phase: 24-split-panes
verified: 2026-03-07T04:00:00Z
status: human_needed
score: 16/16 must-haves verified (automated)
gaps: []
human_verification:
  - test: "Create horizontal split with Ctrl+Shift+D, verify two panes render side-by-side"
    expected: "Window splits into two panes with correct terminal content, no bleed"
    why_human: "Visual rendering correctness cannot be verified via static analysis"
  - test: "Create vertical split with Ctrl+Shift+E, verify panes stack top/bottom"
    expected: "Focused pane splits vertically, 3 panes visible in correct arrangement"
    why_human: "Visual layout correctness requires human inspection"
  - test: "Alt+Arrow focus navigation, verify accent border moves"
    expected: "Cornflower blue border moves to the target pane"
    why_human: "Visual border rendering cannot be verified programmatically"
  - test: "Alt+Shift+Arrow resize, verify divider position changes"
    expected: "Pane ratio adjusts visually, PTY columns update (verify with tput cols)"
    why_human: "Visual resize and PTY dimension sync requires interactive testing"
  - test: "Mouse click in different pane, verify focus changes"
    expected: "Clicking a non-focused pane moves the accent border to that pane"
    why_human: "Mouse input routing requires interactive testing"
  - test: "Close pane with Ctrl+Shift+W, verify remaining panes expand"
    expected: "Pane closes, remaining panes fill the space, no visual artifacts"
    why_human: "Visual layout reflow cannot be verified programmatically"
  - test: "Close last pane in tab, verify tab closes"
    expected: "Closing the only pane closes the tab, next tab activates"
    why_human: "Tab lifecycle integration requires interactive testing"
  - test: "Verify no zombie PTY processes after pane close"
    expected: "No orphaned shell processes in task manager after closing panes"
    why_human: "Process cleanup requires interactive system inspection"
---

# Phase 24: Split Panes Verification Report

**Phase Goal:** Split Panes -- binary tree layout engine, per-pane rendering with scissor clipping, keyboard/mouse interaction
**Verified:** 2026-03-07T04:00:00Z
**Status:** human_needed
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | SplitNode tree can be constructed as Leaf or nested Split | VERIFIED | `split_tree.rs` lines 10-24: SplitNode enum with Leaf(SessionId) and Split variants, 26 unit tests |
| 2 | compute_layout returns pixel rects for all leaf panes | VERIFIED | `split_tree.rs` lines 28-38: recursive compute_layout with container splitting, 7 tests covering nested splits |
| 3 | Horizontal split divides width by ratio with divider gap | VERIFIED | `layout.rs` lines 26-34: usable = width - DIVIDER_GAP, left_w = usable * ratio. Tests confirm left+right+gap = container |
| 4 | Vertical split divides height by ratio with divider gap | VERIFIED | `layout.rs` lines 36-44: usable = height - DIVIDER_GAP, top_h = usable * ratio. Tests confirm top+bottom+gap = container |
| 5 | remove_leaf collapses parent Split to surviving sibling | VERIFIED | `split_tree.rs` lines 42-61: pattern matches (None, Some(surviving)) to collapse. 5 tests for all cases |
| 6 | find_neighbor returns correct pane in each cardinal direction | VERIFIED | `split_tree.rs` lines 66-96: spatial search using layout computation + Manhattan distance. 4 tests including cross-nested splits |
| 7 | Resize ratio adjustment clamps to 0.1..0.9 range | VERIFIED | `split_tree.rs` lines 100-116: `(*ratio + delta).clamp(0.1, 0.9)`. Tests for adjust, clamp max, clamp min, noop on leaf, wrong direction |
| 8 | Tab holds SplitNode tree root and focused_pane SessionId | VERIFIED | `tab.rs` lines 11-19: Tab struct with `root: SplitNode`, `focused_pane: SessionId`. session_ids() and pane_count() helpers |
| 9 | SessionMux tracks focused session via active tab's focused_pane | VERIFIED | `session_mux.rs` lines 53-61: focused_session/focused_session_mut use `tab.focused_pane`. 3+ tests confirm |
| 10 | FrameRenderer can render panes within viewport sub-regions | VERIFIED | `frame.rs` lines 599-894: draw_multi_pane_frame with PaneViewport, DividerRect, viewport offsets, TextBounds clipping |
| 11 | Render loop iterates all panes with scissor clipping | VERIFIED | `main.rs` lines 651-737: multi-pane path computes pane_layouts, snapshots all panes, calls draw_multi_pane_frame |
| 12 | Ctrl+Shift+D/E create horizontal/vertical splits | VERIFIED | `main.rs` lines 967-1000: D creates Horizontal split, E creates Vertical split, both call split_pane + resize_all_panes |
| 13 | Alt+Arrow moves focus between panes | VERIFIED | `main.rs` lines 1155-1203: Alt+Arrow mapped to FocusDirection, calls find_neighbor + set_focused_pane |
| 14 | Alt+Shift+Arrow resizes split ratio | VERIFIED | `main.rs` lines 1167-1180: Alt+Shift+Arrow calls resize_focused_split with +/-0.05 delta |
| 15 | Mouse click in pane changes focus | VERIFIED | `main.rs` lines 1243-1273: viewport hit-test checks click position against compute_layout, calls set_focused_pane |
| 16 | PTY resize sends correct per-pane cell dimensions | VERIFIED | `main.rs` lines 256-299: resize_all_panes computes per-pane cols/lines from viewport / cell_size, sends PtyMsg::Resize |

**Score:** 16/16 truths verified (automated)

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `crates/glass_mux/src/split_tree.rs` | SplitNode tree with layout, navigation, removal, resize methods | VERIFIED | 185 lines, SplitNode enum + compute_layout, remove_leaf, find_neighbor, resize_ratio, session_ids, first_leaf, split_leaf, contains, leaf_count. 26 tests. |
| `crates/glass_mux/src/layout.rs` | ViewportLayout with split/center helpers | VERIFIED | 87 lines, ViewportLayout with Clone/Debug/PartialEq/Eq, split() method with DIVIDER_GAP, center(). 3 tests. |
| `crates/glass_mux/src/tab.rs` | Tab with SplitNode root and focused_pane | VERIFIED | 32 lines, Tab struct with id, root: SplitNode, focused_pane, title. session_ids() and pane_count() helpers. |
| `crates/glass_mux/src/session_mux.rs` | SessionMux with split_pane, close_pane, focus management | VERIFIED | 615 lines, split_pane, close_pane, active_tab_root, set_focused_pane, resize_focused_split, active_tab_pane_count. 22 tests. |
| `crates/glass_renderer/src/frame.rs` | draw_multi_pane_frame with per-pane viewport offsets | VERIFIED | 917 lines, draw_multi_pane_frame method, PaneViewport/DividerRect types, accent border rendering. |
| `crates/glass_renderer/src/grid_renderer.rs` | build_rects_offset, build_text_areas_offset | VERIFIED | Offset methods confirmed at lines 264, 271, 290. |
| `src/main.rs` | Keyboard shortcuts, mouse routing, PTY resize, render loop | VERIFIED | 2008 lines, all split pane shortcuts wired, resize_all_panes helper, compute_dividers, single/multi-pane render branching. |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| split_tree.rs | layout.rs | ViewportLayout used in compute_layout | WIRED | `use crate::layout::ViewportLayout` at line 3, used in compute_layout signature and body |
| split_tree.rs | types.rs | SessionId, SplitDirection, FocusDirection | WIRED | `use crate::types::{FocusDirection, SessionId, SplitDirection}` at line 4 |
| main.rs | split_tree.rs | compute_layout in render loop | WIRED | Lines 275, 667, 1255: compute_layout called in resize_all_panes, multi-pane render, mouse hit-test |
| main.rs | frame.rs | draw_multi_pane_frame per pane | WIRED | Line 726: draw_multi_pane_frame called with panes, dividers, status, tab_bar |
| session_mux.rs | tab.rs | focused_session uses tab.focused_pane | WIRED | Lines 54, 60: focused_session/mut uses tab.focused_pane |
| main.rs | session_mux.rs | split_pane and close_pane calls | WIRED | Lines 978, 995: split_pane; line 937: close_pane |
| main.rs | split_tree.rs | find_neighbor for focus, compute_layout for resize | WIRED | Line 1194: find_neighbor; lines 275, 667: compute_layout |
| glass_mux lib.rs | split_tree.rs, layout.rs | Re-exports | WIRED | lib.rs exports SplitNode, ViewportLayout, FocusDirection, SplitDirection |
| glass_renderer lib.rs | frame.rs | Re-exports | WIRED | lib.rs exports PaneViewport, DividerRect, FrameRenderer |

### Requirements Coverage

No REQUIREMENTS.md file exists in this project. Requirements are tracked via SPLIT-XX IDs in plan frontmatter:

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-----------|-------------|--------|----------|
| SPLIT-01 | 24-01 | Tree construction (Leaf and Split) | VERIFIED | SplitNode enum + 3 construction tests |
| SPLIT-02 | 24-01 | compute_layout returns pixel rects | VERIFIED | Method + 4 layout tests |
| SPLIT-03 | 24-01 | Horizontal split gap accounting | VERIFIED | 2 tests with exact pixel math |
| SPLIT-04 | 24-01 | Vertical split gap accounting | VERIFIED | Test with 800px container |
| SPLIT-05 | 24-01 | remove_leaf collapses parent | VERIFIED | 5 tests covering all cases |
| SPLIT-06 | 24-01 | find_neighbor cardinal directions | VERIFIED | 4 tests including nested splits |
| SPLIT-07 | 24-01 | resize_ratio clamps 0.1..0.9 | VERIFIED | 5 tests including nested specific split |
| SPLIT-08 | 24-02 | Tab holds SplitNode, tracks focused_pane | VERIFIED | Tab struct + 4 tests |
| SPLIT-09 | 24-03 | PTY resize per-pane cell dimensions | VERIFIED | resize_all_panes helper with viewport/cell_size |
| SPLIT-10 | 24-02 | Per-pane scissor-clipped rendering | VERIFIED | draw_multi_pane_frame with viewport offsets + TextBounds |
| SPLIT-11 | 24-03 | Last-pane-close closes tab | VERIFIED | close_pane method + 2 tests |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (none) | - | - | - | No anti-patterns detected in any modified files |

Zero TODO/FIXME/PLACEHOLDER/stub patterns found across all 7 modified files.

### Human Verification Required

### 1. Split Pane Visual Rendering

**Test:** Launch Glass (`cargo run`). Press Ctrl+Shift+D to create a horizontal split. Type in both panes. Press Ctrl+Shift+E to create a vertical split.
**Expected:** Panes render side-by-side (horizontal) and stacked (vertical) with 2px gray dividers. Text stays within its pane boundaries. No bleed between panes.
**Why human:** Visual rendering correctness (text clipping, divider alignment, no artifacts) cannot be verified via static analysis.

### 2. Focus Navigation and Border

**Test:** With 3+ panes, use Alt+Arrow to navigate between panes. Observe the accent border.
**Expected:** Cornflower blue 1px border moves to the focused pane. Focus changes are reflected in which pane receives keyboard input.
**Why human:** Visual border rendering and input routing require interactive testing.

### 3. Pane Resize

**Test:** Press Alt+Shift+Right/Left to resize horizontal splits. Run `tput cols` in each pane before and after.
**Expected:** Divider position moves. Column count reported by `tput cols` changes to match the new pane width.
**Why human:** PTY dimension sync and visual resize require interactive verification.

### 4. Mouse Click Focus

**Test:** Click on a non-focused pane.
**Expected:** Focus (accent border) moves to clicked pane. Subsequent keyboard input goes to the clicked pane.
**Why human:** Mouse input routing requires interactive testing.

### 5. Pane Close Lifecycle

**Test:** With 3 panes, press Ctrl+Shift+W multiple times until the tab closes.
**Expected:** Each close removes one pane, remaining panes expand. Closing the last pane closes the tab. No zombie shell processes in task manager.
**Why human:** Process cleanup and visual reflow require interactive system inspection.

### 6. Single-Pane Regression

**Test:** Open Glass normally (single pane). Verify all existing functionality (typing, scrolling, blocks, status bar, tab bar) works identically to before Phase 24.
**Expected:** Zero visual or behavioral regression in single-pane mode.
**Why human:** Regression testing requires human comparison.

### Gaps Summary

No automated gaps found. All 16 observable truths verified through static code analysis:

- **Plan 01 (SplitTree Layout Engine):** All 7 SPLIT requirements (01-07) implemented with 26 unit tests. Binary tree data structure is complete with compute_layout, remove_leaf, find_neighbor, resize_ratio.

- **Plan 02 (Tab Restructure + Rendering):** Tab holds SplitNode root + focused_pane. SessionMux gains split_pane/close_pane. FrameRenderer has draw_multi_pane_frame with viewport offsets, TextBounds clipping, divider rects, and accent border. Single-pane path preserved for regression safety.

- **Plan 03 (Interaction Wiring):** All keyboard shortcuts (Ctrl+Shift+D/E/W, Alt+Arrow, Alt+Shift+Arrow) wired. Mouse click focus routing implemented. resize_all_panes sends per-pane PTY dimensions. Last-pane-close-closes-tab lifecycle complete.

All artifacts exist, are substantive (not stubs), and are properly wired through imports and call chains. The phase requires human verification for visual rendering correctness, input routing behavior, and process cleanup -- items that cannot be validated through static analysis alone.

---

_Verified: 2026-03-07T04:00:00Z_
_Verifier: Claude (gsd-verifier)_
