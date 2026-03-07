# Phase 24: Split Panes

## Goal

Implement horizontal and vertical split panes within tabs using a binary tree layout (SplitTree). Each pane is an independent terminal session rendered into a scissor-clipped viewport sub-region.

## Key Deliverables

- Horizontal split (Ctrl+Shift+D / Cmd+D) and vertical split (Ctrl+Shift+E / Cmd+Shift+D)
- SplitTree binary tree layout engine computing pixel rects
- draw_frame_viewport() in FrameRenderer with wgpu scissor rect clipping
- Alt+Arrow focus navigation between panes
- Alt+Shift+Arrow keyboard resize of pane ratios
- Visual pane dividers (1-2px lines between panes)
- Focused pane border highlight (accent color)
- PTY resize on split (correct cell dimensions per pane)
- Pane close with parent split collapse
- Mouse click to change pane focus
- Independent PTY + state per pane (blocks, history, snapshots)

## Test Gate

Nested splits in both directions, correct resize cascading, no viewport gaps/overlaps, mouse click to focus, zero zombie processes on pane close.

## Dependencies

Phase 23 (Tabs) -- panes live within tabs. Tab infrastructure must exist.

## Research Notes

- Viewport scissor-rect rendering with glyphon text is not well-documented. May need to prototype draw_frame_viewport early.
- May benefit from research-phase to validate scissor clipping with wgpu text rendering.
