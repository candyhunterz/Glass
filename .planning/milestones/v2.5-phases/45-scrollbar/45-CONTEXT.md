# Phase 45: Scrollbar - Context

**Gathered:** 2026-03-10
**Status:** Ready for planning

<domain>
## Phase Boundary

Add a visible, interactive scrollbar to every terminal pane. Users can see their position in the scrollback buffer and use the mouse to scroll by dragging the thumb or clicking the track. The scrollbar reserves its own space and does not overlay terminal content.

</domain>

<decisions>
## Implementation Decisions

### Scrollbar visibility
- Always visible — permanent thin scrollbar on the right edge of every pane
- Not auto-hide, not hover-only — always present so users always know their position

### Scrollbar dimensions
- 8px wide narrow gutter
- Reserves its own space (terminal grid shrinks by 8px on the right) — never overlays text

### Mouse interactions
- Drag thumb to scroll smoothly through history
- Click above/below thumb to jump by one page
- Both interactions supported (full-featured like most apps)

### Thumb appearance
- Resting: subtle dim gray (~rgba(100,100,100,0.4))
- On hover/drag: brighter (~rgba(150,150,150,0.7))
- Smooth visual transition between states

### Track appearance
- Slightly visible track background (~rgba(255,255,255,0.03))
- Barely noticeable darker stripe so users know the scrollbar area exists

### Multi-pane behavior
- Every pane gets its own scrollbar on its right edge
- Scrollbar drawn inside pane viewport bounds (doesn't touch pane dividers)
- Clicking/dragging a pane's scrollbar also focuses that pane

### Claude's Discretion
- Minimum thumb height (ensure thumb is always grabbable)
- Exact scroll-to-position mapping math
- Animation/transition timing for hover effects
- Behavior when scrollback buffer is empty (thumb fills entire track)

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `TabBarRenderer` in `tab_bar.rs`: Pattern for rendering rects + hit-testing — scrollbar follows same approach
- `StatusBarRenderer` in `status_bar.rs`: Another pinned UI element reference
- `GridSnapshot::display_offset` / `history_size` in `grid_snapshot.rs:65-74`: Already tracks scroll position ratio for thumb placement

### Established Patterns
- wgpu rect rendering: All UI elements (tab bar, status bar, blocks) use `RectInstance` quads
- Hit-testing: Tab bar uses simple coordinate range checks — scrollbar can follow same pattern
- Mouse events in `main.rs:1818-1834`: Mouse wheel already calls `term.lock().scroll_display(Scroll::Delta(lines))`

### Integration Points
- `frame.rs:draw_frame()`: Grid content positioning needs to account for 8px scrollbar gutter
- `frame.rs:draw_frame_multipane()`: Per-pane viewport rectangles need scrollbar width subtracted
- `main.rs` mouse event handlers: New hit-test region for scrollbar drag/click
- `ViewportLayout` in `layout.rs`: Pane bounds calculation may need adjustment for scrollbar width
- PTY resize: Terminal column count must reflect the reduced grid width

</code_context>

<specifics>
## Specific Ideas

No specific requirements — open to standard approaches

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 45-scrollbar*
*Context gathered: 2026-03-10*
