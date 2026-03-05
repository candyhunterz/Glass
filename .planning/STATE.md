---
gsd_state_version: 1.0
milestone: v1.1
milestone_name: Structured Scrollback + MCP Server
status: in-progress
stopped_at: Completed 08-01 search overlay state and input
last_updated: "2026-03-05T19:04:49.384Z"
last_activity: 2026-03-05 -- Completed 08-01 search overlay state and input
progress:
  total_phases: 5
  completed_phases: 3
  total_plans: 10
  completed_plans: 9
  percent: 94
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-04)

**Core value:** A terminal that looks and feels normal but passively watches, indexes, and snapshots everything -- surfacing intelligence only when you need it.
**Current focus:** Phase 8 - Search Overlay

## Current Position

Milestone: v1.1 Structured Scrollback + MCP Server
Phase: 8 of 9 (Search Overlay)
Plan: 1 of 2 in current phase
Status: Plan 08-01 Complete
Last activity: 2026-03-05 -- Completed 08-01 search overlay state and input

Progress: [=========+] 94% (v1.1: 3/5 phases, 1/2 plans in phase 8)

## Performance Metrics

**Velocity (from v1.0):**
- Total plans completed: 12
- Total execution time: ~4 hours
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

### Pending Todos

None.

### Blockers/Concerns

None.

## Session Continuity

Last session: 2026-03-05T19:04:49.382Z
Stopped at: Completed 08-01-PLAN.md
Resume file: None
