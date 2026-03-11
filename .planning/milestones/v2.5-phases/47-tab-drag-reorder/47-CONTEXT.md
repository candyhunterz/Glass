# Phase 47: Tab Drag Reorder - Context

**Gathered:** 2026-03-10
**Status:** Ready for planning

<domain>
## Phase Boundary

Allow users to click and drag tabs to reorder them in the tab bar. Visual indicator shows the drop location during drag.

</domain>

<decisions>
## Implementation Decisions

### Drag behavior
- Click and drag a tab to change its position
- Visual indicator shows drop location during drag
- Standard browser-like tab reorder behavior

### Claude's Discretion
- Drag threshold before reorder initiates (avoid accidental drags)
- Visual indicator style (insertion line, ghost tab, etc.)
- Whether tab index keyboard shortcuts (Ctrl+1-9) update to match new order
- Animation during reorder (smooth slide vs instant swap)

</decisions>

<code_context>
## Existing Code Insights

### Reusable Assets
- `TabBarRenderer` in `tab_bar.rs`: Already has tab rect positions — drag needs source/target rect tracking
- `session_mux.rs`: Tab list is a `Vec<Tab>` — reorder is a vec swap/rotate

### Established Patterns
- Mouse press/move/release already tracked in `main.rs` for text selection
- Tab bar hit-testing in place from Phase 46

### Integration Points
- `main.rs` mouse handlers: Track drag state (source tab, current mouse x)
- `tab_bar.rs`: Render drag indicator / ghost tab during drag
- `session_mux.rs`: Add `reorder_tab(from, to)` method

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

*Phase: 47-tab-drag-reorder*
*Context gathered: 2026-03-10*
