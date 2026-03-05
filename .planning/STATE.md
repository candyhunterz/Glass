---
gsd_state_version: 1.0
milestone: v1.1
milestone_name: Structured Scrollback + MCP Server
status: executing
stopped_at: Completed 07-01 QueryFilter module
last_updated: "2026-03-05T18:35:31Z"
last_activity: 2026-03-05 -- Completed 07-01 QueryFilter module
progress:
  total_phases: 5
  completed_phases: 2
  total_plans: 8
  completed_plans: 7
  percent: 88
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-04)

**Core value:** A terminal that looks and feels normal but passively watches, indexes, and snapshots everything -- surfacing intelligence only when you need it.
**Current focus:** Phase 7 - CLI Query Interface

## Current Position

Milestone: v1.1 Structured Scrollback + MCP Server
Phase: 7 of 9 (CLI Query Interface)
Plan: 1 of 2 in current phase
Status: Executing
Last activity: 2026-03-05 -- Completed 07-01 QueryFilter module

Progress: [=========-] 88% (v1.1: 2/5 phases, 1/2 plans in phase 7)

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

### Pending Todos

None.

### Blockers/Concerns

None.

## Session Continuity

Last session: 2026-03-05T18:35:31Z
Stopped at: Completed 07-01 QueryFilter module
Resume file: .planning/phases/07-cli-query-interface/07-01-SUMMARY.md
