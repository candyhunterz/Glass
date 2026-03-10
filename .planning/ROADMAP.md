# Roadmap: Glass

## Milestones

- [x] **v1.0 MVP** -- Phases 1-4 (shipped 2026-03-05)
- [x] **v1.1 Structured Scrollback + MCP Server** -- Phases 5-9 (shipped 2026-03-05)
- [x] **v1.2 Command-Level Undo** -- Phases 10-14 (shipped 2026-03-06)
- [x] **v1.3 Pipe Visualization** -- Phases 15-20 (shipped 2026-03-06)
- [x] **v2.0 Cross-Platform & Tabs** -- Phases 21-25 (shipped 2026-03-07)
- [x] **v2.1 Packaging & Polish** -- Phases 26-30 (shipped 2026-03-07)
- [x] **v2.2 Multi-Agent Coordination** -- Phases 31-34 (shipped 2026-03-10)
- [x] **v2.3 Agent MCP Features** -- Phases 35-39 (shipped 2026-03-10)
- [ ] **v2.4 Rendering Correctness** -- Phases 40-44 (in progress)

## Phases

<details>
<summary>v1.0 MVP (Phases 1-4) -- SHIPPED 2026-03-05</summary>

- [x] Phase 1: Scaffold (3/3 plans) -- completed 2026-03-05
- [x] Phase 2: Terminal Core (3/3 plans) -- completed 2026-03-05
- [x] Phase 3: Shell Integration and Block UI (4/4 plans) -- completed 2026-03-05
- [x] Phase 4: Configuration and Performance (2/2 plans) -- completed 2026-03-05

</details>

<details>
<summary>v1.1 Structured Scrollback + MCP Server (Phases 5-9) -- SHIPPED 2026-03-05</summary>

- [x] Phase 5: History Database Foundation (2/2 plans) -- completed 2026-03-05
- [x] Phase 6: Output Capture + Writer Integration (4/4 plans) -- completed 2026-03-05
- [x] Phase 7: CLI Query Interface (2/2 plans) -- completed 2026-03-05
- [x] Phase 8: Search Overlay (2/2 plans) -- completed 2026-03-05
- [x] Phase 9: MCP Server (2/2 plans) -- completed 2026-03-05

</details>

<details>
<summary>v1.2 Command-Level Undo (Phases 10-14) -- SHIPPED 2026-03-06</summary>

- [x] Phase 10: Content Store + DB Schema (2/2 plans) -- completed 2026-03-05
- [x] Phase 11: Command Parser (2/2 plans) -- completed 2026-03-05
- [x] Phase 12: FS Watcher Engine (2/2 plans) -- completed 2026-03-06
- [x] Phase 13: Integration + Undo Engine (4/4 plans) -- completed 2026-03-06
- [x] Phase 14: UI + CLI + MCP + Pruning (3/3 plans) -- completed 2026-03-06

</details>

<details>
<summary>v1.3 Pipe Visualization (Phases 15-20) -- SHIPPED 2026-03-06</summary>

- [x] Phase 15: Pipe Parsing Core (2/2 plans) -- completed 2026-03-06
- [x] Phase 16: Shell Capture + Terminal Transport (3/3 plans) -- completed 2026-03-06
- [x] Phase 17: Pipeline UI (2/2 plans) -- completed 2026-03-06
- [x] Phase 18: Storage + Retention (1/1 plan) -- completed 2026-03-06
- [x] Phase 19: MCP + Config + Polish (1/1 plan) -- completed 2026-03-06
- [x] Phase 20: Config Gate + Dead Code Cleanup (2/2 plans) -- completed 2026-03-06

</details>

<details>
<summary>v2.0 Cross-Platform & Tabs (Phases 21-25) -- SHIPPED 2026-03-07</summary>

- [x] Phase 21: Session Extraction & Platform Foundation (3/3 plans) -- completed 2026-03-06
- [x] Phase 22: Cross-Platform Validation (2/2 plans) -- completed 2026-03-07
- [x] Phase 23: Tabs (3/3 plans) -- completed 2026-03-07
- [x] Phase 24: Split Panes (3/3 plans) -- completed 2026-03-07
- [x] Phase 25: TerminalExit Multi-Pane Fix (1/1 plan) -- completed 2026-03-07

</details>

<details>
<summary>v2.1 Packaging & Polish (Phases 26-30) -- SHIPPED 2026-03-07</summary>

- [x] Phase 26: Performance Profiling & Optimization (2/2 plans) -- completed 2026-03-07
- [x] Phase 27: Config Validation & Hot-Reload (2/2 plans) -- completed 2026-03-07
- [x] Phase 28: Platform Packaging & CI Release (2/2 plans) -- completed 2026-03-07
- [x] Phase 29: Auto-Update (2/2 plans) -- completed 2026-03-07
- [x] Phase 30: Documentation & Distribution (3/3 plans) -- completed 2026-03-07

</details>

<details>
<summary>v2.2 Multi-Agent Coordination (Phases 31-34) -- SHIPPED 2026-03-10</summary>

- [x] Phase 31: Coordination Crate (3/3 plans) -- completed 2026-03-09
- [x] Phase 32: MCP Tools (2/2 plans) -- completed 2026-03-09
- [x] Phase 33: Integration and Testing (1/1 plan) -- completed 2026-03-09
- [x] Phase 34: GUI Integration (2/2 plans) -- completed 2026-03-10

</details>

<details>
<summary>v2.3 Agent MCP Features (Phases 35-39) -- SHIPPED 2026-03-10</summary>

- [x] Phase 35: MCP Command Channel (2/2 plans) -- completed 2026-03-10
- [x] Phase 36: Multi-Tab Orchestration (2/2 plans) -- completed 2026-03-10
- [x] Phase 37: Token-Saving Tools (2/2 plans) -- completed 2026-03-10
- [x] Phase 38: Structured Error Extraction (2/2 plans) -- completed 2026-03-10
- [x] Phase 39: Live Command Awareness (1/1 plan) -- completed 2026-03-10

</details>

### v2.4 Rendering Correctness (In Progress)

**Milestone Goal:** Fix grid-aligned rendering so TUI apps (vim, htop, tmux, Claude Code) render correctly, and add missing text rendering features (wide chars, decorations, font fallback, DPI).

- [x] **Phase 40: Grid Alignment** - Per-cell glyph positioning and font-metric line height for correct TUI rendering (completed 2026-03-10)
- [x] **Phase 41: Wide Character Support** - CJK and double-width characters render at correct 2-cell width (completed 2026-03-10)
- [ ] **Phase 42: Text Decorations** - Underline and strikethrough GPU rendering via rect instances
- [ ] **Phase 43: Font Fallback** - Missing glyphs resolved via cosmic-text system font fallback
- [ ] **Phase 44: Dynamic DPI** - ScaleFactorChanged triggers full font and surface rebuild

## Phase Details

### Phase 40: Grid Alignment
**Goal**: TUI applications render with pixel-perfect grid alignment -- no horizontal drift, no vertical gaps
**Depends on**: Nothing (first phase of v2.4)
**Requirements**: GRID-01, GRID-02
**Success Criteria** (what must be TRUE):
  1. Running `vim` or `htop` shows box-drawing borders that connect seamlessly with no vertical gaps between lines
  2. Long lines of text in TUI apps (tmux status bar, vim line numbers) show no horizontal drift -- characters stay aligned to their grid columns
  3. The terminal grid renders identically to Alacritty or Windows Terminal for the same font and size
**Plans:** 2/2 plans complete
Plans:
- [x] 40-01-PLAN.md — Rewrite GridRenderer core with per-cell Buffers and font-metric cell height
- [ ] 40-02-PLAN.md — Migrate frame.rs call sites and visual verification

### Phase 41: Wide Character Support
**Goal**: CJK text and other double-width characters render correctly spanning two cell widths
**Depends on**: Phase 40
**Requirements**: WIDE-01, WIDE-02
**Success Criteria** (what must be TRUE):
  1. CJK characters (Chinese, Japanese, Korean) render at double cell width without overlapping adjacent characters
  2. Cell backgrounds, cursor highlighting, and text selection correctly span 2 cells for wide characters
  3. Mixed ASCII and CJK text on the same line maintains correct column alignment
**Plans:** 2/2 plans complete
Plans:
- [ ] 41-01-PLAN.md — Wide char Buffer creation with double-width and spacer skip (WIDE-01)
- [ ] 41-02-PLAN.md — Double-width background rects, cursor, and visual verification (WIDE-02)

### Phase 42: Text Decorations
**Goal**: Underlined and struck-through text renders with visible decoration lines
**Depends on**: Phase 40
**Requirements**: DECO-01, DECO-02
**Success Criteria** (what must be TRUE):
  1. Text with SGR 4 (underline) shows a visible line below the baseline
  2. Text with SGR 9 (strikethrough) shows a visible line through the middle of the text
  3. Decorations render at correct vertical positions within the cell regardless of font size
**Plans:** 1 plan
Plans:
- [ ] 42-01-PLAN.md — Add build_decoration_rects and integrate into frame rendering (DECO-01, DECO-02)

### Phase 43: Font Fallback
**Goal**: Characters missing from the primary font render via system font fallback
**Depends on**: Phase 40
**Requirements**: FONT-01, FONT-02
**Success Criteria** (what must be TRUE):
  1. Characters not in the configured font (e.g., CJK glyphs when using a Latin-only font) render instead of showing tofu/missing glyph boxes
  2. Fallback glyphs render at the correct size and position within the cell grid, not overflowing or misaligned
**Plans**: TBD

### Phase 44: Dynamic DPI
**Goal**: Terminal renders correctly after moving between displays with different DPI/scale factors
**Depends on**: Phase 40
**Requirements**: DPI-01, DPI-02
**Success Criteria** (what must be TRUE):
  1. Moving the Glass window from a 1x display to a 2x HiDPI display triggers automatic font and surface rebuild with no user action required
  2. After a DPI change, the terminal grid remains correctly aligned with no rendering artifacts, clipping, or blurry text
  3. The PTY is notified of the new terminal dimensions after a DPI change so running programs reflow correctly
**Plans**: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 40 -> 41 -> 42 -> 43 -> 44
Note: Phases 41, 42, 43 all depend only on Phase 40 and could theoretically run in parallel, but sequential execution is safer since all modify GridRenderer.

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1. Scaffold | v1.0 | 3/3 | Complete | 2026-03-05 |
| 2. Terminal Core | v1.0 | 3/3 | Complete | 2026-03-05 |
| 3. Shell Integration and Block UI | v1.0 | 4/4 | Complete | 2026-03-05 |
| 4. Configuration and Performance | v1.0 | 2/2 | Complete | 2026-03-05 |
| 5. History Database Foundation | v1.1 | 2/2 | Complete | 2026-03-05 |
| 6. Output Capture + Writer Integration | v1.1 | 4/4 | Complete | 2026-03-05 |
| 7. CLI Query Interface | v1.1 | 2/2 | Complete | 2026-03-05 |
| 8. Search Overlay | v1.1 | 2/2 | Complete | 2026-03-05 |
| 9. MCP Server | v1.1 | 2/2 | Complete | 2026-03-05 |
| 10. Content Store + DB Schema | v1.2 | 2/2 | Complete | 2026-03-05 |
| 11. Command Parser | v1.2 | 2/2 | Complete | 2026-03-05 |
| 12. FS Watcher Engine | v1.2 | 2/2 | Complete | 2026-03-06 |
| 13. Integration + Undo Engine | v1.2 | 4/4 | Complete | 2026-03-06 |
| 14. UI + CLI + MCP + Pruning | v1.2 | 3/3 | Complete | 2026-03-06 |
| 15. Pipe Parsing Core | v1.3 | 2/2 | Complete | 2026-03-06 |
| 16. Shell Capture + Terminal Transport | v1.3 | 3/3 | Complete | 2026-03-06 |
| 17. Pipeline UI | v1.3 | 2/2 | Complete | 2026-03-06 |
| 18. Storage + Retention | v1.3 | 1/1 | Complete | 2026-03-06 |
| 19. MCP + Config + Polish | v1.3 | 1/1 | Complete | 2026-03-06 |
| 20. Config Gate + Dead Code Cleanup | v1.3 | 2/2 | Complete | 2026-03-06 |
| 21. Session Extraction & Platform Foundation | v2.0 | 3/3 | Complete | 2026-03-06 |
| 22. Cross-Platform Validation | v2.0 | 2/2 | Complete | 2026-03-07 |
| 23. Tabs | v2.0 | 3/3 | Complete | 2026-03-07 |
| 24. Split Panes | v2.0 | 3/3 | Complete | 2026-03-07 |
| 25. TerminalExit Multi-Pane Fix | v2.0 | 1/1 | Complete | 2026-03-07 |
| 26. Performance Profiling & Optimization | v2.1 | 2/2 | Complete | 2026-03-07 |
| 27. Config Validation & Hot-Reload | v2.1 | 2/2 | Complete | 2026-03-07 |
| 28. Platform Packaging & CI Release | v2.1 | 2/2 | Complete | 2026-03-07 |
| 29. Auto-Update | v2.1 | 2/2 | Complete | 2026-03-07 |
| 30. Documentation & Distribution | v2.1 | 3/3 | Complete | 2026-03-07 |
| 31. Coordination Crate | v2.2 | 3/3 | Complete | 2026-03-09 |
| 32. MCP Tools | v2.2 | 2/2 | Complete | 2026-03-09 |
| 33. Integration and Testing | v2.2 | 1/1 | Complete | 2026-03-09 |
| 34. GUI Integration | v2.2 | 2/2 | Complete | 2026-03-10 |
| 35. MCP Command Channel | v2.3 | 2/2 | Complete | 2026-03-10 |
| 36. Multi-Tab Orchestration | v2.3 | 2/2 | Complete | 2026-03-10 |
| 37. Token-Saving Tools | v2.3 | 2/2 | Complete | 2026-03-10 |
| 38. Structured Error Extraction | v2.3 | 2/2 | Complete | 2026-03-10 |
| 39. Live Command Awareness | v2.3 | 1/1 | Complete | 2026-03-10 |
| 40. Grid Alignment | 2/2 | Complete    | 2026-03-10 | - |
| 41. Wide Character Support | 2/2 | Complete    | 2026-03-10 | - |
| 42. Text Decorations | v2.4 | 0/1 | In progress | - |
| 43. Font Fallback | v2.4 | 0/0 | Not started | - |
| 44. Dynamic DPI | v2.4 | 0/0 | Not started | - |
