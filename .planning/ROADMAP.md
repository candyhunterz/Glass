# Roadmap: Glass

## Milestones

- [x] **v1.0 MVP** -- Phases 1-4 (shipped 2026-03-05)
- [ ] **v1.1 Structured Scrollback + MCP Server** -- Phases 5-9 (in progress)

## Phases

<details>
<summary>v1.0 MVP (Phases 1-4) -- SHIPPED 2026-03-05</summary>

- [x] Phase 1: Scaffold (3/3 plans) -- completed 2026-03-05
- [x] Phase 2: Terminal Core (3/3 plans) -- completed 2026-03-05
- [x] Phase 3: Shell Integration and Block UI (4/4 plans) -- completed 2026-03-05
- [x] Phase 4: Configuration and Performance (2/2 plans) -- completed 2026-03-05

</details>

### v1.1 Structured Scrollback + MCP Server

- [x] **Phase 5: History Database Foundation** - Standalone glass_history crate with SQLite schema, FTS5 search, retention, and subcommand routing
- [ ] **Phase 6: Output Capture + Writer Integration** - PTY output capture pipeline, history writer thread, and display_offset fix
- [x] **Phase 7: CLI Query Interface** - `glass history` subcommands with filters and formatted output
- [ ] **Phase 8: Search Overlay** - Ctrl+Shift+F modal overlay with live incremental search and block navigation
- [ ] **Phase 9: MCP Server** - glass_mcp crate with stdio JSON-RPC server exposing GlassHistory and GlassContext tools

## Phase Details

### Phase 5: History Database Foundation
**Goal**: Commands executed in the terminal are persisted to a structured, searchable SQLite database with project-aware storage
**Depends on**: Phase 4 (v1.0 complete)
**Requirements**: HIST-01, HIST-03, HIST-04, HIST-05, INFR-01
**Success Criteria** (what must be TRUE):
  1. Running a command in Glass creates a row in the SQLite database with command text, cwd, exit code, timestamps, and duration
  2. Searching the database with FTS5 MATCH syntax returns relevant commands ranked by relevance
  3. Running Glass from a directory with `.glass/history.db` uses the project database; otherwise uses `~/.glass/global-history.db`
  4. Records older than the configured max age are automatically pruned, and database size stays within the configured limit
  5. Running `glass history` or `glass mcp serve` routes to the correct subcommand instead of launching the terminal
**Plans:** 2 plans
Plans:
- [x] 05-01-PLAN.md -- glass_history crate: schema, insert, FTS5 search, path resolution, retention
- [x] 05-02-PLAN.md -- Clap subcommand routing in glass binary

### Phase 6: Output Capture + Writer Integration
**Goal**: Command output is captured from the PTY and stored alongside command metadata, and block decorations scroll correctly
**Depends on**: Phase 5
**Requirements**: HIST-02, INFR-02
**Success Criteria** (what must be TRUE):
  1. After a command completes, its stdout/stderr output (up to the configured max, default 50KB) is stored in the history database
  2. Output from alternate-screen applications (vim, less, top) is not captured
  3. Block decorations (separator lines, exit code badges) render at correct positions during scrollback navigation
  4. PTY throughput does not regress measurably compared to v1.0 baseline (output capture is non-blocking)
**Plans:** 4 plans (3 executed + 1 gap closure)
Plans:
- [x] 06-01-PLAN.md -- Output processing module + schema migration + config (glass_history)
- [x] 06-02-PLAN.md -- OutputBuffer in PTY thread + AppEvent wiring (DB write deferred)
- [x] 06-03-PLAN.md -- display_offset fix in frame.rs
- [ ] 06-04-PLAN.md -- Gap closure: wire HistoryDb into terminal runtime (insert + output update)

### Phase 7: CLI Query Interface
**Goal**: Users can query their command history from the terminal using `glass history` with flexible filters
**Depends on**: Phase 6
**Requirements**: CLI-01, CLI-02, CLI-03
**Success Criteria** (what must be TRUE):
  1. Running `glass history search <term>` returns matching commands from the database
  2. Filters work in combination: `--exit 1 --after "1 hour ago" --cwd /project --limit 10` narrows results correctly
  3. Results display as structured terminal output showing command text, exit code, timestamp, duration, and cwd
**Plans:** 2 plans
Plans:
- [x] 07-01-PLAN.md -- QueryFilter + filtered_query + parse_time in glass_history query module
- [x] 07-02-PLAN.md -- CLI subcommand expansion + display formatting in glass binary

### Phase 8: Search Overlay
**Goal**: Users can search their entire command history from within the running terminal via a modal overlay
**Depends on**: Phase 7
**Requirements**: SRCH-01, SRCH-02, SRCH-03, SRCH-04
**Success Criteria** (what must be TRUE):
  1. Pressing Ctrl+Shift+F opens a search overlay on top of the terminal content; pressing Escape dismisses it
  2. Typing in the search box shows matching results immediately (live/incremental, debounced)
  3. Arrow keys navigate through results; pressing Enter jumps to the selected command block in scrollback
  4. Each result shows command text, exit code, timestamp, and an output preview as a structured block
**Plans**: TBD

### Phase 9: MCP Server
**Goal**: AI assistants can query terminal history and context through a standards-compliant MCP server
**Depends on**: Phase 6
**Requirements**: MCP-01, MCP-02, MCP-03
**Success Criteria** (what must be TRUE):
  1. Running `glass mcp serve` starts a JSON-RPC 2.0 server over stdio that completes the MCP initialize handshake
  2. The GlassHistory tool returns filtered command history (by text, timeframe, exit status, cwd, limit) as structured JSON
  3. The GlassContext tool returns an activity summary (command count, failure count, recent directories, time range) as structured JSON
  4. All logging goes to stderr; stdout carries only JSON-RPC messages (no corruption)
**Plans**: TBD

## Progress

**Execution Order:** 5 -> 6 -> 7 -> 8 -> 9
(Phase 9 depends on Phase 6, not Phase 8 -- it can theoretically run after Phase 6, but is sequenced last to reduce cognitive load)

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1. Scaffold | v1.0 | 3/3 | Complete | 2026-03-05 |
| 2. Terminal Core | v1.0 | 3/3 | Complete | 2026-03-05 |
| 3. Shell Integration and Block UI | v1.0 | 4/4 | Complete | 2026-03-05 |
| 4. Configuration and Performance | v1.0 | 2/2 | Complete | 2026-03-05 |
| 5. History Database Foundation | v1.1 | 2/2 | Complete | 2026-03-05 |
| 6. Output Capture + Writer Integration | v1.1 | 3/4 | In Progress | - |
| 7. CLI Query Interface | v1.1 | 2/2 | Complete | 2026-03-05 |
| 8. Search Overlay | v1.1 | 0/? | Not started | - |
| 9. MCP Server | v1.1 | 0/? | Not started | - |
