# Phase 6: Output Capture + Writer Integration - Context

**Gathered:** 2026-03-05
**Status:** Ready for planning

<domain>
## Phase Boundary

Command output is captured from the PTY and stored alongside command metadata in the history database. Block decorations scroll correctly with display_offset. PTY throughput does not regress. Requirements: HIST-02, INFR-02.

</domain>

<decisions>
## Implementation Decisions

### Output truncation
- Head + tail split when output exceeds the configured max: keep first half and last half with a `[...truncated N bytes...]` marker in between
- Default max output capture: 50KB, configurable via `max_output_capture_kb` in `[history]` TOML config section
- Binary output detection: if high ratio of non-printable bytes, store `[binary output: N bytes]` placeholder instead of raw content
- ANSI escape sequences stripped before storage -- store plain text only for cleaner search and smaller storage

### Claude's Discretion
- Output capture point in the PTY pipeline (where to tap bytes -- OscScanner level, BlockManager level, or separate buffer)
- Alternate-screen detection approach (how to detect vim/less/top and skip capture)
- History writer thread architecture (channel-based, shared buffer, etc.)
- display_offset wiring through frame.rs and block_renderer.rs
- Schema migration strategy for adding output column to commands table
- Capture scope: stdout+stderr interleaving, per-command accumulation, and flush-on-completion timing
- What to store for alternate-screen applications (empty string, placeholder, or null)
- Graceful handling when Glass exits mid-command (partial output storage vs discard)

</decisions>

<specifics>
## Specific Ideas

- PRD already defines `max_output_capture_kb = 50` config key in the `[history]` section -- reuse that exact key name
- Head+tail split mirrors how `git diff` truncates large diffs -- familiar pattern for developers

</specifics>

<code_context>
## Existing Code Insights

### Reusable Assets
- `OscScanner` in `glass_terminal/src/osc_scanner.rs`: pre-scans PTY bytes for shell integration sequences -- output capture can tap into the same data flow
- `BlockManager` in `glass_terminal/src/block_manager.rs`: tracks command lifecycle (PromptActive -> InputActive -> Executing -> Complete) with line ranges and timing -- knows when a command starts/finishes
- `HistoryDb` in `glass_history/src/db.rs`: SQLite with WAL mode, insert/search/prune already working -- needs `output` column added to `commands` table and `CommandRecord` struct
- `GridSnapshot` in `glass_terminal/src/grid_snapshot.rs`: already captures `display_offset` from the terminal at line 196

### Established Patterns
- Dedicated PTY reader thread (std::thread, not Tokio) for blocking I/O -- history writer should follow similar threading pattern
- Lock-minimizing `GridSnapshot` pattern -- capture data under brief lock, process without holding it
- `AppEvent::Shell` messages sent from PTY thread to main thread via `EventLoopProxy` -- could use similar channel for output data

### Integration Points
- `frame.rs` lines 115 and 169: `display_offset = 0` hardcoded with TODO comments -- must wire `GridSnapshot.display_offset` through to `block_renderer` and `block_text` calls
- `CommandRecord` struct needs `output: Option<String>` field added
- `commands` table schema needs `output TEXT` column
- Config system in `glass_core/src/config.rs` needs `max_output_capture_kb` field in history section

</code_context>

<deferred>
## Deferred Ideas

None -- discussion stayed within phase scope

</deferred>

---

*Phase: 06-output-capture-writer-integration*
*Context gathered: 2026-03-05*
