# Phase 46: Tab Bar Controls - Context

**Gathered:** 2026-03-10
**Status:** Ready for planning

<domain>
## Phase Boundary

Add clickable "+" new tab button and "x" close tab buttons to the tab bar. Add tab overflow handling when too many tabs to fit. Keyboard shortcuts (Ctrl+Shift+T/W) already work — this phase adds mouse-driven controls.

</domain>

<decisions>
## Implementation Decisions

### New tab button placement
- "+" button appears immediately after the last tab
- Moves rightward as tabs are added
- Like Chrome / VS Code tab bar pattern

### Close button behavior
- "x" close button appears on the right side of a tab only on hover
- Replaces some of the title space when visible
- Keeps tabs clean when not interacting
- Middle-click to close continues to work as existing fallback

### Button visual style
- Use "+" and "x" text glyphs (not custom icons)
- On hover: show a subtle circular/rounded background highlight behind the glyph
- Minimal style consistent with VS Code approach

### Tab overflow handling
- Tabs compress to a minimum width as more are added
- Titles truncate with ellipsis ("...")
- Simple and predictable — no scroll arrows or dropdown menus

### Claude's Discretion
- Minimum tab width before truncation kicks in
- Exact hover highlight size and opacity
- Close button glyph positioning within tab rect
- "+" button sizing relative to tab height

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `TabBarRenderer` in `tab_bar.rs`: Already renders tab rects, text labels, and does hit-testing — extend with button rects
- `build_tab_rects()` (tab_bar.rs:74-108): Currently equal-width tabs — needs new layout logic for "+" button and variable widths
- `hit_test()` (tab_bar.rs:163-179): Currently returns tab index — needs to distinguish tab click vs close button click
- Existing tab management: `add_tab()`, `close_tab()`, `activate_tab()` in `session_mux.rs` — button clicks wire to these

### Established Patterns
- Tab bar uses `cell_height` for bar height — buttons fit within this
- Text rendering via glyphon — "+" and "x" glyphs use same pipeline
- Colors: BAR_BG (30,30,30), ACTIVE_TAB (50,50,50), INACTIVE_TAB (35,35,35) — hover highlight should be in same range

### Integration Points
- `tab_bar.rs`: Main changes — layout, rendering, hit-testing
- `main.rs:1653-1667`: Left-click handler needs to check for "+" button and "x" button hits
- `main.rs:1606-1643`: CursorMoved needs hover state tracking for tab bar region
- `frame.rs:247-250`: Tab bar rect rendering pipeline — add button rects

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

*Phase: 46-tab-bar-controls*
*Context gathered: 2026-03-10*
