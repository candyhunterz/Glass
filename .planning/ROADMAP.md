# Roadmap: Glass

## Milestones

- [x] **v1.0 MVP** -- Phases 1-4 (shipped 2026-03-05)
- [x] **v1.1 Structured Scrollback + MCP Server** -- Phases 5-9 (shipped 2026-03-05)
- [x] **v1.2 Command-Level Undo** -- Phases 10-14 (shipped 2026-03-06)
- [x] **v1.3 Pipe Visualization** -- Phases 15-20 (shipped 2026-03-06)
- [x] **v2.0 Cross-Platform & Tabs** -- Phases 21-25 (shipped 2026-03-07)
- [x] **v2.1 Packaging & Polish** -- Phases 26-30 (shipped 2026-03-07)
- [x] **v2.2 Multi-Agent Coordination** -- Phases 31-34 (shipped 2026-03-10)
- [ ] **v2.3 Agent MCP Features** -- Phases 35-39 (in progress)

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

### v2.3 Agent MCP Features (In Progress)

**Milestone Goal:** Make Glass the most token-efficient, capable terminal for AI agents by exposing multi-tab orchestration, structured error extraction, and token-saving tools through 12 new MCP tools.

- [x] **Phase 35: MCP Command Channel** - Async channel bridge between MCP server and GUI event loop with IPC listener (completed 2026-03-10)
- [ ] **Phase 36: Multi-Tab Orchestration** - Agent can create, list, command, read, and close tabs via MCP tools
- [ ] **Phase 37: Token-Saving Tools** - Filtered output, cached results, file diffs, and budget-aware context via MCP
- [ ] **Phase 38: Structured Error Extraction** - New glass_errors crate with language-aware parsers and MCP tool
- [ ] **Phase 39: Live Command Awareness** - Agent can check command status and cancel running commands via MCP

## Phase Details

### Phase 35: MCP Command Channel
**Goal**: MCP tools that need live session data can communicate with the running GUI process
**Depends on**: Nothing (first phase of v2.3)
**Requirements**: INFRA-01, INFRA-02
**Success Criteria** (what must be TRUE):
  1. MCP server can send a request to the GUI process and receive a structured response within 5 seconds
  2. GUI continues rendering frames and accepting keyboard input while processing MCP requests
  3. MCP tools that need live data gracefully return an error when no GUI process is running
**Plans**: 2 plans

Plans:
- [ ] 35-01-PLAN.md -- IPC infrastructure in glass_core + event loop integration
- [ ] 35-02-PLAN.md -- IPC client in glass_mcp + GlassServer wiring + glass_ping tool

### Phase 36: Multi-Tab Orchestration
**Goal**: Agent can orchestrate multiple terminal tabs as parallel workspaces through MCP
**Depends on**: Phase 35
**Requirements**: TAB-01, TAB-02, TAB-03, TAB-04, TAB-05, TAB-06
**Success Criteria** (what must be TRUE):
  1. Agent can create a new tab with a specified shell and working directory, and the tab appears in the GUI
  2. Agent can list all tabs and see each tab's name, cwd, and whether a command is running
  3. Agent can send a command string to a specific tab and read that tab's output (last N lines, optionally filtered by regex)
  4. Agent can close a tab, and the tool refuses to close the last remaining tab
  5. All tab tools accept both numeric tab index and stable session ID as identifiers
**Plans**: 2 plans

Plans:
- [ ] 36-01-PLAN.md -- GUI-side IPC handlers for all 5 tab methods + resolve_tab and extract_term_lines helpers
- [ ] 36-02-PLAN.md -- MCP tool handlers in tools.rs + param types + unit tests

### Phase 37: Token-Saving Tools
**Goal**: Agent can retrieve command results with minimal token overhead through filtering, caching, and budget-aware compression
**Depends on**: Phase 35
**Requirements**: TOKEN-01, TOKEN-02, TOKEN-03, TOKEN-04
**Success Criteria** (what must be TRUE):
  1. Agent can retrieve command output filtered by regex pattern, line count, or head/tail mode -- returning only relevant lines instead of full output
  2. Agent can check if a previous command's cached result is still valid based on whether files it touched have been modified since
  3. Agent can see which files a command modified along with unified diffs of the changes
  4. Agent can request a compressed context summary that respects a token budget and focuses on specified aspects (errors, files, history)
**Plans**: TBD

Plans:
- [ ] 37-01: TBD
- [ ] 37-02: TBD

### Phase 38: Structured Error Extraction
**Goal**: Agent can extract structured, machine-readable errors from raw command output
**Depends on**: Nothing (independent -- glass_errors is a pure library crate)
**Requirements**: ERR-01, ERR-02, ERR-03, ERR-04
**Success Criteria** (what must be TRUE):
  1. Agent can call glass_errors MCP tool and receive structured errors with file path, line, column, message, and severity
  2. Rust compiler output (both human-readable and --error-format=json) is parsed into structured errors
  3. Generic fallback parser extracts errors matching the file:line:col: message pattern from any compiler
  4. Parser auto-selects the appropriate language parser based on command text hint or output content patterns
**Plans**: TBD

Plans:
- [ ] 38-01: TBD
- [ ] 38-02: TBD

### Phase 39: Live Command Awareness
**Goal**: Agent can monitor and control running commands in real time
**Depends on**: Phase 35
**Requirements**: LIVE-01, LIVE-02
**Success Criteria** (what must be TRUE):
  1. Agent can query whether a command is currently running in a specific tab and see its elapsed time
  2. Agent can cancel a running command (equivalent to Ctrl+C) and receive confirmation that the signal was sent
**Plans**: TBD

Plans:
- [ ] 39-01: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 35 -> 36 -> 37 -> 38 -> 39
Note: Phases 37 and 38 are independent of Phase 36 (both only need Phase 35's channel or DB-only access). Phase 39 depends on Phase 35.

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
| 35. MCP Command Channel | 2/2 | Complete    | 2026-03-10 | - |
| 36. Multi-Tab Orchestration | 1/2 | In Progress|  | - |
| 37. Token-Saving Tools | v2.3 | 0/? | Not started | - |
| 38. Structured Error Extraction | v2.3 | 0/? | Not started | - |
| 39. Live Command Awareness | v2.3 | 0/? | Not started | - |
