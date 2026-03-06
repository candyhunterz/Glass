# Roadmap: Glass

## Milestones

- [x] **v1.0 MVP** -- Phases 1-4 (shipped 2026-03-05)
- [x] **v1.1 Structured Scrollback + MCP Server** -- Phases 5-9 (shipped 2026-03-05)
- [x] **v1.2 Command-Level Undo** -- Phases 10-14 (shipped 2026-03-06)
- [ ] **v1.3 Pipe Visualization** -- Phases 15-19 (in progress)

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

### v1.3 Pipe Visualization (In Progress)

- [x] **Phase 15: Pipe Parsing Core** - glass_pipes crate with pipe splitter, TTY detection, buffer policies, and data types (completed 2026-03-06)
- [x] **Phase 16: Shell Capture + Terminal Transport** - Shell integration rewriting (bash tee, PowerShell Tee-Object), OSC 133;S/P protocol, OscScanner extension, and event wiring (completed 2026-03-06)
- [x] **Phase 17: Pipeline UI** - Multi-row pipeline blocks with expand/collapse, stage output display, and auto-expand logic (completed 2026-03-06)
- [x] **Phase 18: Storage + Retention** - pipe_stages DB table, schema migration, and retention policy integration (completed 2026-03-06)
- [x] **Phase 19: MCP + Config + Polish** - GlassPipeInspect tool, GlassContext pipeline stats, and [pipes] config section (completed 2026-03-06)

## Phase Details

### Phase 15: Pipe Parsing Core
**Goal**: Users' piped commands are correctly detected, parsed, and classified with appropriate exclusions
**Depends on**: Phase 14 (v1.2 complete)
**Requirements**: PIPE-01, PIPE-02, PIPE-03, CAPT-03, CAPT-04
**Success Criteria** (what must be TRUE):
  1. A command like `cat file | grep foo | wc -l` is parsed into 3 distinct stages with correct command text per stage
  2. Pipe characters inside quotes or escaped are not treated as pipe boundaries
  3. Commands containing TTY-sensitive programs (less, vim, fzf, git log) are flagged for exclusion from capture
  4. A `--no-glass` flag on any command opts it out of pipe interception
  5. Stage buffers exceeding 10MB are sampled (head + tail) rather than truncated, and binary data is detected and labeled
**Plans:** 2/2 plans complete
Plans:
- [ ] 15-01-PLAN.md -- Crate foundation: types + pipe-splitting parser
- [ ] 15-02-PLAN.md -- Pipeline classification (TTY/opt-out) + StageBuffer with sampling

### Phase 16: Shell Capture + Terminal Transport
**Goal**: Pipe stage intermediate output is captured by the shell and delivered to the terminal via OSC sequences
**Depends on**: Phase 15
**Requirements**: CAPT-01, CAPT-02
**Success Criteria** (what must be TRUE):
  1. In bash/zsh, running a piped command produces tee-captured intermediate output for each stage, stored in temp files and emitted as OSC 133;P sequences
  2. In PowerShell, running a piped command captures per-stage text representation via Tee-Object and emits OSC 133;P sequences
  3. Exit codes are preserved correctly through tee-rewritten pipelines (PIPESTATUS captured)
  4. The terminal's OscScanner parses OSC 133;S and 133;P sequences into ShellEvent variants, and Block structs gain pipeline_stages fields
**Plans:** 3/3 plans complete
Plans:
- [x] 16-01-PLAN.md -- Types, OscScanner parsing, and event wiring for pipeline OSC sequences
- [ ] 16-02-PLAN.md -- Block pipeline_stages field and main event loop temp file reading
- [ ] 16-03-PLAN.md -- Bash tee rewriting and PowerShell Tee-Object capture in shell scripts

### Phase 17: Pipeline UI
**Goal**: Users see piped commands rendered as multi-row pipeline blocks with inspectable stage output
**Depends on**: Phase 16
**Requirements**: UI-01, UI-02, UI-03, UI-04
**Success Criteria** (what must be TRUE):
  1. A piped command renders as a multi-row block showing each stage with its command text, line count, and byte count
  2. Pipeline blocks with >2 stages or a failed exit code auto-expand; simple successful pipelines auto-collapse
  3. User can click or use keyboard to expand any individual stage and view its full intermediate output
  4. User can collapse/expand the entire pipeline block
**Plans:** 2/2 plans complete
Plans:
- [ ] 17-01-PLAN.md -- Block data model extensions, auto-expand logic, and pipeline stage rendering
- [ ] 17-02-PLAN.md -- Command text wiring, mouse click handling, keyboard shortcut, and visual verification

### Phase 18: Storage + Retention
**Goal**: Pipeline stage data persists in the history database with proper lifecycle management
**Depends on**: Phase 16
**Requirements**: STOR-01, STOR-02
**Success Criteria** (what must be TRUE):
  1. Pipe stage output is stored in a `pipe_stages` table linked to the parent command_id in history.db
  2. Schema migration from v1 to v2 creates the pipe_stages table without data loss
  3. Retention pruning (age and count policies) cascades to pipe_stages when parent commands are pruned
**Plans:** 1/1 plans complete
Plans:
- [ ] 18-01-PLAN.md -- Schema migration, DB methods, retention cascade, and main.rs wiring

### Phase 19: MCP + Config + Polish
**Goal**: AI assistants can inspect pipeline stages, and users can configure pipe visualization behavior
**Depends on**: Phase 17, Phase 18
**Requirements**: MCP-01, MCP-02, CONF-01
**Success Criteria** (what must be TRUE):
  1. `GlassPipeInspect(command_id, stage)` returns the intermediate output for a specific pipeline stage via MCP
  2. `GlassContext` activity summaries include pipeline statistics (pipe count, avg stages, failure rate)
  3. `[pipes]` section in config.toml controls enabled/disabled, max_capture_mb, and auto_expand behavior
**Plans:** 1/1 plans complete
Plans:
- [ ] 19-01-PLAN.md -- GlassPipeInspect MCP tool, GlassContext pipeline stats, and [pipes] config section with main.rs wiring

## Progress

**Execution Order:**
Phases execute in numeric order: 15 -> 16 -> 17 -> 18 -> 19
Note: Phase 17 and Phase 18 both depend on Phase 16 and could execute in parallel. Phase 19 depends on both.

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
| 19. MCP + Config + Polish | 1/1 | Complete    | 2026-03-06 | - |
