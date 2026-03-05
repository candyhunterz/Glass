---
gsd_state_version: 1.0
milestone: v1.1
milestone_name: Structured Scrollback + MCP Server
status: in-progress
stopped_at: Completed 09-01-PLAN.md
last_updated: "2026-03-05T20:20:00.000Z"
last_activity: 2026-03-05 -- Completed 09-01 MCP server core implementation
progress:
  total_phases: 5
  completed_phases: 4
  total_plans: 12
  completed_plans: 11
  percent: 92
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-04)

**Core value:** A terminal that looks and feels normal but passively watches, indexes, and snapshots everything -- surfacing intelligence only when you need it.
**Current focus:** Phase 9 - MCP Server

## Current Position

Milestone: v1.1 Structured Scrollback + MCP Server
Phase: 9 of 9 (MCP Server)
Plan: 1 of 2 in current phase
Status: Phase 09 in progress
Last activity: 2026-03-05 -- Completed 09-01 MCP server core implementation

Progress: [#########-] 92% (v1.1: 4/5 phases, 1/2 plans in phase 9)

## Performance Metrics

**Velocity (from v1.0):**
- Total plans completed: 13
- Total execution time: ~4.5 hours
- Average: ~20 min/plan

## Accumulated Context

### Decisions

See PROJECT.md Key Decisions table for full history.
Recent decisions affecting v1.1:

- Option<Commands> clap pattern for default-to-terminal subcommand routing
- Clap parse before EventLoop creation to avoid window flash
- Use rmcp (official Rust MCP SDK) over hand-rolled JSON-RPC
- Use content FTS5 tables (not external content) for safety
- FTS5 on command text only for v1.1; defer output indexing to v1.2
- MCP server as separate process (`glass mcp serve`), not embedded
- Standard FTS5 table delete via DELETE FROM fts WHERE rowid=? (not INSERT 'delete' command)
- FTS5 delete before commands delete in same transaction for pruning
- Binary detection runs on raw bytes before ANSI stripping to preserve accurate non-printable ratio
- PRAGMA user_version migration pattern for schema evolution (v0->v1 adds output column)
- serde default function for backward-compatible TOML config parsing
- Absolute line numbers from PTY for block tracking (not viewport-relative)
- history_size in GridSnapshot for absolute-to-viewport coordinate conversion
- Raw bytes via AppEvent to main thread for output processing (avoids glass_terminal -> glass_history dep)
- Alt-screen detection via raw byte scanning (ESC[?1049h/l) instead of locking terminal TermMode
- HistorySection as Option in GlassConfig for backward-compatible config parsing
- [Phase 06]: Command text left empty -- metadata (cwd, exit_code, timestamps, output) is the high-value data; grid extraction deferred
- [Phase 06]: HistoryDb::open failure is non-fatal -- history never crashes the terminal
- [Phase 07]: Dynamic SQL with params_from_iter and Vec<Value> for QueryFilter
- [Phase 07]: FTS5 special chars escaped via double-quoting search terms
- [Phase 07]: CWD prefix matching via SQL LIKE with trailing %
- [Phase 07]: HistoryFilters clap::Args with flatten for shared filter args between Search/List
- [Phase 07]: Default limit 25 via clap default_value_t, not Default trait
- [Phase 07]: Relative timestamps (Nh ago) for recent entries, full datetime for older
- [Phase 08]: Overlay input interception placed BEFORE Ctrl+Shift check to fully swallow keys
- [Phase 08]: Ctrl+Shift+F toggle works both to open and close overlay
- [Phase 08]: Debounce polling via continuous request_redraw while search_pending
- [Phase 08]: 150ms debounce timer for search execution
- [Phase 08]: Epoch timestamp matching for scroll-to-block instead of index-position heuristic
- [Phase 08]: Command text extracted from terminal grid using block line ranges at finish time
- [Phase 08]: started_epoch on Block struct for wall-clock matching with DB records
- [Phase 09]: rmcp 1.1.0 (latest stable) instead of 0.11 from research -- API differs significantly
- [Phase 09]: Per-branch tracing init in main.rs to prevent double-init panic (MCP needs stderr writer)
- [Phase 09]: HistoryEntry response type separate from CommandRecord for controlled serialization
- [Phase 09]: internal_err() helper for concise McpError conversion in tool handlers

### Pending Todos

None.

### Blockers/Concerns

None.

## Session Continuity

Last session: 2026-03-05T20:20:00Z
Stopped at: Completed 09-01-PLAN.md
Resume file: None
