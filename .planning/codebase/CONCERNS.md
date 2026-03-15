# Codebase Concerns

**Analysis Date:** 2026-03-15

## Tech Debt

### Unwrap/Expect Patterns in Main Event Loop

**Area:** Event handling and window context access
- **Issue:** 21+ `.unwrap()` and `.expect()` calls throughout `src/main.rs`, primarily for accessing session mux state (lines 1790, 1856, 2120, 2857, 3178, 3280, 4353, 4608, 5064, etc.)
- **Files:** `src/main.rs` (lines 193, 200, 990, 991, 1558, 1569, 1790, 1856, 2120, 2857, 3178, 3280, 4353, 4608, 5064, 7234, 7310, 7376)
- **Impact:** If a session becomes unavailable during event processing (e.g., due to concurrent tab removal or crash), panics kill the entire GUI. No graceful degradation.
- **Fix approach:** Wrap all `focused_session()` calls with error handling paths that either skip the operation or close the offending pane. Return `Option` from event handlers instead of panicking.

### Platform-Specific Test Gating Incomplete

**Area:** Cross-platform PTY handling
- **Issue:** ConPTY tests gated with `#[cfg(target_os = "windows")]` but Unix PTY code paths (fork, forkpty) lack integration test coverage on CI
- **Files:** `crates/glass_terminal/src/pty.rs`, CI configuration (not in repo, on remote)
- **Impact:** Unix-specific PTY issues (signal handling, child reaping, PTY resize) may not be caught until user deployment
- **Fix approach:** Add per-platform test suites in CI; ensure Unix tests exercise pty resize, SIGCHLD handling, and EOF scenarios

### Alacritty Terminal Pinned to Exact Version

**Area:** Dependency fragility
- **Issue:** `alacritty_terminal = "=0.25.1"` pinned exact. If a critical bug fix is released in 0.25.2+, Glass cannot use it
- **Files:** `Cargo.toml`
- **Impact:** VT parsing bugs or PTY handling regressions in the terminal emulator cannot be patched without a full PR to update the pin
- **Fix approach:** Document why exact pin is necessary (likely due to event loop integration); consider loose pin (0.25.*) after validation tests pass on newer versions

## Known Risks & Fragile Areas

### Orchestrator Checkpoint Timeout (3 minutes)

**Area:** Agent respawn loop
- **Issue:** If Claude Code hangs writing `checkpoint.md`, orchestrator waits 180 seconds before respawning (line 279 `CHECKPOINT_TIMEOUT_SECS`)
- **Files:** `src/orchestrator.rs` (line 279)
- **Impact:** During a hang, Glass GUI remains responsive but orchestration stalls silently. User may not know why agent isn't responding.
- **Safe modification:** Add a non-blocking progress indicator in status bar; display "Checkpoint timeout pending" after 30 seconds to give user visibility
- **Test coverage:** Need integration test covering timeout path (currently may be untested)

### Session Mux State Synchronization

**Area:** Multi-pane tab management
- **Issue:** SessionMux tracks split tree and focus state in memory. If a panic occurs while mutating tree (split/resize), state becomes inconsistent. No persistent state restore.
- **Files:** `crates/glass_mux/src/session_mux.rs` (718 lines), `crates/glass_mux/src/split_tree.rs` (725 lines)
- **Impact:** Tab state lost on crash; user returns to default single pane
- **Safe modification:** Consider snapshotting session layout to `~/.glass/session_state.json` on every tree mutation; restore on startup
- **Test coverage:** Gaps in split/unsplit edge cases (rapid splits, deeply nested panes, resize during drag)

### PTY Reader Thread Polling Token Mismatch

**Area:** Platform-specific event loop integration
- **Issue:** `PTY_READ_WRITE_TOKEN` differs per platform (Windows: 2, Unix: 0) — hardcoded at lines 34-36 in `crates/glass_terminal/src/pty.rs`
- **Files:** `crates/glass_terminal/src/pty.rs` (lines 29-39)
- **Impact:** If alacritty_terminal upstream changes these token values, Glass polling breaks silently (wrong event handler called)
- **Mitigation:** Tokens are re-exported by alacritty_terminal; sync comment documents dependency
- **Fix approach:** Extract magic constants to a module-level helper that validates tokens match upstream at compile time (via `const_assert!` or similar)

## Performance Bottlenecks

### Frame Renderer Per-Frame Allocations

**Area:** GPU rendering hot path
- **Issue:** `FrameRenderer` has `text_buffers`, `cell_positions`, `overlay_buffers`, `pipeline_buffers` as reusable storage (lines 48-54 in `crates/glass_renderer/src/frame.rs`), but no explicit capacity pre-allocation or debug stats
- **Files:** `crates/glass_renderer/src/frame.rs` (lines 38-55)
- **Impact:** On large terminal sizes (80+ cols), buffer reallocation per frame may cause frame drops; no visibility into peak memory usage
- **Improvement path:** Add runtime metrics (--features perf) to measure buffer sizes; implement `with_capacity()` for large expected grid sizes; log peak allocations

### History Database FTS5 Query Cost

**Area:** History search overlay
- **Issue:** FTS5 queries in `glass_history::query` module execute on main thread during typing (debounced 150ms but still sync). No query cost estimation.
- **Files:** `crates/glass_history/src/db.rs` (1464 lines)
- **Impact:** On projects with >100k command records, FTS5 full-text search may block rendering for 50-100ms per keystroke
- **Improvement path:** Move FTS5 queries to background thread; debounce at 300ms; show "searching..." placeholder; add query timeout (5s max)

### Snapshot Pruning Synchronous at Startup

**Area:** Background disk I/O at launch
- **Issue:** `Pruner::prune()` runs in a spawned thread, but scans all snapshots and blobs — no progress indicator or early exit
- **Files:** `src/main.rs` (lines 1618-1649)
- **Impact:** First launch may block shell rendering while pruning hundreds of old blobs (observable as slow startup)
- **Improvement path:** Implement incremental pruning (batch of 10 snapshots per poll); add pruning progress event; skip pruning if Glass was opened recently

### OSC 133 Parsing Without Bounds

**Area:** Command boundary detection
- **Issue:** `OscScanner` in `crates/glass_terminal/src/osc_scanner.rs` accumulates OSC sequences in memory without documented size limit
- **Files:** `crates/glass_terminal/src/osc_scanner.rs`
- **Impact:** Pathological case: if shell emits very long OSC sequences (e.g., binary data), scanner buffer may grow unbounded
- **Improvement path:** Document max OSC payload size (e.g., 64KB); log warning and truncate if exceeded

## Concurrency & State Management

### Activity Stream Channel Capacity Risk

**Area:** Agent event ingestion (lines 1698-1701 in `src/main.rs`)
- **Issue:** `create_channel(&activity_config)` creates bounded channel with default capacity. When agent is `Off`, rx is stored but tx is dropped periodically, causing channel fill-up
- **Files:** `src/main.rs` (lines 1698-1701), `crates/glass_core/src/activity_stream.rs`
- **Impact:** When channel is full, `try_send()` fails silently — activity events are discarded. If agent mode later changes to `Assist`, buffered activity is lost.
- **Safe modification:** Make channel capacity configurable; emit trace logs when channel fills; consider persisting unbuffered activity to disk
- **Test coverage:** Need test for mode-off → mode-assist transition under load

### Database Connection Per-Query Pattern

**Area:** SQLite multi-agent access
- **Issue:** Each MCP tool opens a new `CoordinationDb` connection (e.g., lines 1005-1006 in `crates/glass_mcp/src/tools.rs`). With WAL mode and 5000ms busy_timeout, contention possible but not quantified
- **Files:** `crates/glass_coordination/src/db.rs` (lines 26-40), `crates/glass_mcp/src/tools.rs` (1000+ lines)
- **Mitigation:** `BEGIN IMMEDIATE` transactions reduce SQLITE_BUSY risk; WAL mode allows concurrent readers
- **Test coverage:** No load test for 10+ concurrent agents all querying agents.db simultaneously
- **Improvement path:** Add stress test for 5+ concurrent agents; measure query latency under contention; consider connection pooling if needed

### Usage Tracker OAuth Token File Access

**Area:** Polling loop credential read
- **Issue:** `read_oauth_token()` in `src/usage_tracker.rs` reads `.credentials.json` every 60 seconds without caching. If file is deleted or permissions change, polling silently degrades
- **Files:** `src/usage_tracker.rs` (lines 36-47)
- **Impact:** User disables orchestrator via config, but usage_tracker thread keeps polling (wasting cycles, logging errors)
- **Fix approach:** Cache token + expiry time; re-read only if cache expires or file mtime changes; log warnings when read fails (not just silently)

## Missing Critical Features

### No Snapshot Integrity Verification

**Area:** Undo engine trust boundary
- **Issue:** `UndoEngine::restore_file()` in `crates/glass_snapshot/src/undo.rs` restores files from snapshots without cryptographic verification. If blob store is corrupted, bad data is silently written to disk.
- **Files:** `crates/glass_snapshot/src/undo.rs` (528 lines)
- **Impact:** Silent data loss if blake3 hash doesn't match (error is logged but file is not restored)
- **Recommendation:** Verify blake3 hash before restore; fail with clear error message; add recovery path (keep corrupted blob for forensics)

### Command Parser Read-Only Classification Incomplete

**Area:** Snapshot trust boundary
- **Issue:** `glass_snapshot::command_parser` marks many commands as `Confidence::Low` when parsing fails (e.g., complex pipes, aliases). Reads from stdin or network are not snapshot-protected even if file writes might occur downstream.
- **Files:** `crates/glass_snapshot/src/command_parser.rs` (1031 lines)
- **Impact:** Destructive command may execute without pre-exec snapshot if parser marks it Low confidence
- **Recommendation:** Add allowlist mode to whitelist-only commands for snapshot (default: Conservative); document limitations

### No Graceful Degradation for MCP Tool Errors

**Area:** Agent tool availability
- **Issue:** If a single MCP tool handler panics (e.g., IPC timeout in `glass_ping`), error is caught as `McpError` but no fallback tool list is provided to agent
- **Files:** `crates/glass_mcp/src/tools.rs` (3111 lines)
- **Impact:** Agent loses a capability mid-task; no built-in retry or alternative provided
- **Recommendation:** Implement circuit-breaker pattern for slow/failing tools (disable after 3 consecutive timeouts); emit advisory message to agent

## Test Coverage Gaps

### Event Loop Panic Recovery

**Area:** Main application stability
- **Issue:** No integration test for panic in event handler (e.g., in window_event callback)
- **Files:** `src/main.rs` (ApplicationHandler impl, lines 1537+)
- **Risk:** Panic propagates to winit event loop, crashes app without error logging
- **Priority:** High
- **Safe test approach:** Unit test individual event handlers in isolation; add panic hook in tests to verify error is logged

### Orchestrator Stuck Detection False Positives

**Area:** State fingerprinting (lines 282-327 in `src/orchestrator.rs`)
- **Issue:** `StateFingerprint::compute()` uses DefaultHasher over terminal lines. If output is slow (e.g., 100 lines/sec), fingerprint may match even though terminal advanced
- **Files:** `src/orchestrator.rs` (lines 282-327)
- **Risk:** Stuck detection triggers when agent is still making progress
- **Priority:** Medium
- **Test approach:** Unit test with real terminal output (e.g., `cargo build` output); verify fingerprint changes

### Database Schema Migration Path

**Area:** Schema evolution safety
- **Issue:** `HistoryDb` tracks `SCHEMA_VERSION` but migration logic is incomplete. If user upgrades Glass with new schema, old code running new schema may fail silently.
- **Files:** `crates/glass_history/src/db.rs` (line 8)
- **Risk:** Data loss or corruption during concurrent access by old + new Glass versions
- **Priority:** Medium
- **Safe migration:** Add detailed migration documentation in code; version each migration with date; add pre-flight check that prevents running if schema > code version

## Security Considerations

### Command Parser Shell Injection via Whitelist Bypass

**Area:** Snapshot coverage
- **Issue:** If a new shell or command alias becomes popular (e.g., `trash` instead of `rm`), command parser doesn't know it's destructive. Pre-exec snapshot is skipped.
- **Files:** `crates/glass_snapshot/src/command_parser.rs` (1031 lines)
- **Mitigation:** Parser maintains comprehensive whitelist of dangerous commands (rm, mv, dd, git checkout, etc.)
- **Recommendation:** Document user-extension mechanism for adding custom dangerous commands; add config option to require snapshots for all commands

### OAuth Token Exposure in Logs

**Area:** Credential handling
- **Issue:** Usage tracker logs API errors that may contain truncated oauth token in debug builds
- **Files:** `src/usage_tracker.rs` (lines 50-65)
- **Mitigation:** Errors are logged via tracing, which respects RUST_LOG level (debug-only by default)
- **Recommendation:** Explicit token redaction in error strings (replace with `...REDACTED...`); add audit log of token access

### Coordination Database Stale Agent Cleanup

**Area:** Agent registration
- **Issue:** `CoordinationDb` tracks `last_heartbeat` but has no automatic cleanup of stale agents. If agent crashes without calling `glass_agent_deregister`, file locks remain held indefinitely.
- **Files:** `crates/glass_coordination/src/db.rs` (49-100)
- **Mitigation:** File locks on deleted agent records cascade due to foreign key
- **Recommendation:** Add cleanup task that deletes agents with `last_heartbeat > 24h` old; emit event when lock holder is stale

## Scaling Limits

### Terminal Grid Allocation for Large Displays

**Area:** High-resolution rendering
- **Issue:** Terminal grid stored as 2D array in memory. With 300+ column displays (e.g., 4K at high DPI), grid scales O(n²) memory
- **Files:** `crates/glass_terminal/src/grid_snapshot.rs` (483 lines)
- **Current limit:** Untested beyond 200 columns
- **Improvement path:** Profile memory usage at 300+ cols; consider sparse grid representation for mostly-empty lines; add runtime warning if grid exceeds 250K cells

### Snapshot Blob Store Linear Scan

**Area:** File storage lookup
- **Issue:** `glass_snapshot` blob store uses blake3 hash as filename; queries iterate all blobs to find matches. No index.
- **Files:** `crates/glass_snapshot/src/db.rs` (537 lines)
- **Current limit:** Untested beyond 10K blobs (~500MB)
- **Improvement path:** Add SQLite index on (hash) in metadata DB; benchmark query time at 100K blobs

### History Database FTS5 Index Memory

**Area:** Search performance
- **Issue:** FTS5 virtual table builds in-memory index. Large history (>1M commands) may consume 500MB+ RAM for index
- **Files:** `crates/glass_history/src/db.rs` (1464 lines)
- **Current limit:** Untested above 500K records
- **Improvement path:** Implement retention auto-pruning based on index size (not just age/count); document FTS5 memory overhead in config guide

---

*Concerns audit: 2026-03-15*
