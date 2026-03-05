# Requirements: Glass

**Defined:** 2026-03-05
**Core Value:** A terminal that looks and feels normal but passively watches, indexes, and snapshots everything -- surfacing intelligence only when you need it.

## v1.1 Requirements

Requirements for Structured Scrollback + MCP Server. Each maps to roadmap phases.

### History Database

- [x] **HIST-01**: Every command execution is logged to a local SQLite database with metadata (command text, cwd, exit code, start/end timestamps, duration)
- [ ] **HIST-02**: Command output is captured and stored (truncated to configurable max, default 50KB)
- [x] **HIST-03**: FTS5 full-text search index on command text and output
- [x] **HIST-04**: Per-project database (`.glass/history.db`) with global fallback (`~/.glass/global-history.db`)
- [x] **HIST-05**: Retention policies: configurable max age (default 30 days) and max size (default 1GB), automatic pruning

### Search Overlay

- [ ] **SRCH-01**: User can open search overlay with Ctrl+Shift+F
- [ ] **SRCH-02**: Incremental/live search results as user types
- [ ] **SRCH-03**: Arrow key navigation through results with enter to select
- [ ] **SRCH-04**: Results displayed as structured blocks (command text, exit code, timestamp, preview)

### CLI Query

- [ ] **CLI-01**: `glass history` subcommand queries the history database
- [ ] **CLI-02**: Filter by exit code, time range, cwd, and text content
- [ ] **CLI-03**: Results formatted as structured terminal output

### MCP Server

- [ ] **MCP-01**: `glass mcp serve` runs an MCP server over stdio (JSON-RPC 2.0)
- [ ] **MCP-02**: GlassHistory tool: query commands with filters (text, timeframe, status, cwd, limit)
- [ ] **MCP-03**: GlassContext tool: returns high-level activity summary (command count, failures, files modified, time range)

### Infrastructure

- [ ] **INFR-01**: Subcommand routing via clap (default = terminal, `history` = CLI, `mcp serve` = MCP server)
- [ ] **INFR-02**: Fix display_offset tech debt so block decorations scroll correctly (prerequisite for search navigation)

## v2 Requirements

Deferred to future milestones. Tracked but not in current roadmap.

### History Enhancements

- **HIST-06**: Natural language query parsing ("failed commands last hour")
- **HIST-07**: Cross-session history linking
- **HIST-08**: History export (JSON, CSV)

### MCP Enhancements

- **MCP-04**: GlassUndo tool (depends on v1.2 filesystem snapshots)
- **MCP-05**: GlassPipeInspect tool (depends on v1.3 pipe visualization)
- **MCP-06**: GlassFileDiff tool (depends on v1.2 filesystem snapshots)

## Out of Scope

| Feature | Reason |
|---------|--------|
| FTS5 on output content | Defer until storage impact of output capture is measured in practice |
| Custom FTS5 tokenizer | unicode61 default is sufficient for v1.1; revisit if search quality is poor |
| MCP over network transport | stdio is sufficient for local AI assistants; network adds security concerns |
| Search result highlighting | Visual enhancement, not core functionality; defer to polish |
| History sync across machines | Cloud sync explicitly out of scope per PRD |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| HIST-01 | Phase 5 | Complete |
| HIST-02 | Phase 6 | Pending |
| HIST-03 | Phase 5 | Complete |
| HIST-04 | Phase 5 | Complete |
| HIST-05 | Phase 5 | Complete |
| SRCH-01 | Phase 8 | Pending |
| SRCH-02 | Phase 8 | Pending |
| SRCH-03 | Phase 8 | Pending |
| SRCH-04 | Phase 8 | Pending |
| CLI-01 | Phase 7 | Pending |
| CLI-02 | Phase 7 | Pending |
| CLI-03 | Phase 7 | Pending |
| MCP-01 | Phase 9 | Pending |
| MCP-02 | Phase 9 | Pending |
| MCP-03 | Phase 9 | Pending |
| INFR-01 | Phase 5 | Pending |
| INFR-02 | Phase 6 | Pending |

**Coverage:**
- v1.1 requirements: 17 total
- Mapped to phases: 17
- Unmapped: 0

---
*Requirements defined: 2026-03-05*
*Last updated: 2026-03-05 after roadmap creation*
