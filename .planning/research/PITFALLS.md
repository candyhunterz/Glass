# Domain Pitfalls: v1.1 Structured Scrollback + MCP Server

**Domain:** Adding SQLite history, FTS5 search, MCP server, search overlay UI, and CLI query to existing Rust GPU-accelerated terminal emulator
**Researched:** 2026-03-05
**Applies to:** Glass v1.1 milestone

---

## Critical Pitfalls

Mistakes that cause rewrites, data corruption, or fundamental architecture breakage.

### Pitfall 1: SQLite Connection Sharing Across Threads Causes Deadlocks or Corruption

**What goes wrong:** Sharing a single `rusqlite::Connection` across the PTY reader thread, winit main thread, MCP server thread, and CLI process. SQLite connections are not `Send` in rusqlite by default. Even with `unsafe` workarounds, concurrent writes from multiple threads on a single connection corrupt state.

**Why it happens:** Glass already has 3 threads (winit main, PTY reader, git query). Adding SQLite history writes (from PTY reader or main thread), MCP server reads (from a new thread/process), and CLI reads (from a separate process) creates 4-5 concurrent access points. Developers assume WAL mode "just works" for concurrent access without understanding the single-writer constraint.

**Consequences:** `SQLITE_BUSY` errors silently dropping command history. Deadlocks between the PTY reader thread waiting for a DB write lock and the main thread holding a read transaction. CLI queries timing out while the terminal is running. In the worst case, database corruption from unserialized writes.

**Prevention:**
- Use **one connection per thread**, never share a `Connection` across threads. The PTY reader thread gets a dedicated write connection. The winit main thread gets a read-only connection for search overlay queries. The MCP server and CLI each open their own read connections.
- Enable **WAL mode** on the database at creation time: `PRAGMA journal_mode=WAL;`. WAL allows unlimited concurrent readers while one writer proceeds without blocking readers.
- Set **`busy_timeout`** to at least 5000ms on all connections: `conn.busy_timeout(Duration::from_millis(5000))?;`. This prevents `SQLITE_BUSY` errors from killing writes when the CLI or MCP server holds a brief read lock.
- Use **`BEGIN IMMEDIATE`** for write transactions so the writer grabs the lock upfront rather than failing mid-transaction. rusqlite's default `transaction()` uses `DEFERRED`, which can deadlock if a read transaction later tries to write.
- The CLI binary should open its own connection to the same database file. WAL mode specifically supports this pattern -- multiple processes reading while one writes.

**Detection:** Unit tests that open 3+ connections concurrently and hammer reads/writes. If any test gets `SQLITE_BUSY` with a 5-second timeout, the architecture is wrong. Log every `SQLITE_BUSY` error at WARN level in production.

**Phase:** Must be addressed in the very first phase (SQLite schema + write path). Getting this wrong poisons everything built on top.

---

### Pitfall 2: Output Capture Killing PTY Throughput

**What goes wrong:** Intercepting PTY output for SQLite storage adds latency to the PTY read loop, causing visible lag during high-throughput commands like `cat large_file`, `cargo build` (verbose), or `find /`.

**Why it happens:** Glass's PTY reader thread (`glass_pty_loop` in `pty.rs`) already does OscScanner pre-scanning before feeding bytes to the VTE parser. Adding SQLite writes to this hot path -- even buffered -- introduces blocking I/O in a tight read loop that currently processes up to 1MB (`READ_BUFFER_SIZE = 0x10_0000`) per iteration.

**Consequences:** Terminal throughput drops from megabytes/second to kilobytes/second during bulk output. Users see visible pause/stutter during `git log`, build output, or any command producing >100KB of output. This is the most user-visible regression possible.

**Prevention:**
- **Never write to SQLite from the PTY reader thread directly.** The PTY reader thread must remain a pure read-parse-scan loop. Instead, send output chunks to a bounded async channel (`tokio::sync::mpsc`) that a dedicated writer task drains.
- **Buffer output aggressively.** Accumulate command output in memory (bounded at e.g. 1MB per command), and flush to SQLite only when a command completes (OSC 133;D received). This means the PTY reader only appends to an in-memory `Vec<u8>` -- zero syscalls, zero blocking.
- **Truncate captured output.** Set a per-command output capture limit (e.g. 512KB or 1MB). Commands producing more output than the limit get the first N bytes + a truncation marker. This prevents `cat /dev/urandom | head -c 1G` from consuming all RAM.
- **Benchmark the hot path.** Measure PTY throughput (`cat /dev/zero | head -c 100M`) before and after adding capture. Any regression >5% means the capture path is too coupled to the read loop.

**Detection:** Run `vtebench` or a simple throughput test (`time cat large_file`) and compare against v1.0 baseline. The numbers should be indistinguishable.

**Phase:** Must be designed correctly in the output capture phase, before FTS5 indexing is added. The buffering strategy is the foundation.

---

### Pitfall 3: FTS5 External Content Table Sync Corruption

**What goes wrong:** Using FTS5 with an external content table (to avoid duplicating output text) but getting the trigger-based sync wrong, leading to stale or corrupted search indices where searches miss results or return phantom matches.

**Why it happens:** FTS5 external content tables require precise trigger definitions. The `DELETE` operation must use the special `INSERT INTO fts_table(fts_table, rowid, ...) VALUES('delete', old.rowid, old.content)` syntax. Standard `DELETE FROM fts_table WHERE rowid = old.rowid` does not work and silently corrupts the index. Additionally, `UPDATE` triggers must perform a delete-then-insert, not just an insert.

**Consequences:** Search returns results for deleted commands. Search misses recently added commands. `PRAGMA integrity_check` passes but `INSERT INTO fts_table(fts_table) VALUES('integrity-check')` fails. Requires full rebuild of the FTS index.

**Prevention:**
- **Use a content table (not external content) for the first implementation.** Let FTS5 manage its own copy of the indexed text. The storage overhead is acceptable for terminal output (typically <100MB even after months of use). External content tables are an optimization to add later if storage becomes a concern.
- If using external content tables: write the triggers exactly per the SQLite FTS5 documentation, including the `VALUES('delete', ...)` syntax. Test with a sequence of INSERT, UPDATE, DELETE on the content table and verify the FTS index matches with `INSERT INTO fts_table(fts_table) VALUES('integrity-check')`.
- **Run FTS integrity check on startup.** If it fails, rebuild the index (`INSERT INTO fts_table(fts_table) VALUES('rebuild')`). This self-heals from any corruption transparently.

**Detection:** Integration test that inserts 100 commands, deletes 50, updates 25, then runs the FTS5 integrity check. If the check fails, the trigger definitions are wrong.

**Phase:** Addressed when FTS5 indexing is implemented, after the base schema.

---

### Pitfall 4: MCP Server stdout Corruption from Logging

**What goes wrong:** The MCP server process writes log messages, debug output, or panic traces to stdout, corrupting the JSON-RPC message stream. The MCP client (Claude, Cursor, etc.) receives malformed JSON and disconnects.

**Why it happens:** Rust's default panic handler writes to stderr (safe), but `println!()` writes to stdout. The `tracing` crate defaults to stdout. Any dependency that uses `println!()` or `print!()` will corrupt the MCP protocol stream. The MCP protocol specification is strict: "the server MUST NOT write anything to stdout that is not a valid MCP message."

**Consequences:** MCP client immediately disconnects on receiving non-JSON data. The error is intermittent (only when a log line or panic happens), making it extremely hard to debug. Users see "MCP server disconnected" with no useful error message.

**Prevention:**
- **Route ALL logging to stderr in the MCP server binary.** Configure tracing-subscriber with `fmt().with_writer(std::io::stderr)`. This is the single most important line of code in the MCP server.
- **Set a custom panic hook** that writes to stderr: `std::panic::set_hook(Box::new(|info| eprintln!("{info}")));`
- **Never use `println!()` in the MCP server crate.** Add a clippy lint: `#![deny(clippy::print_stdout)]` at the crate level.
- **Run the MCP server as a separate binary**, not embedded in the terminal process. This isolates the stdout concern -- the terminal's stdout is the PTY, while the MCP server's stdout is the JSON-RPC channel. Mixing these in a single process is asking for trouble.
- **Test with a mock MCP client** that validates every line on stdout is valid JSON-RPC. Any non-JSON line should fail the test.

**Detection:** CI test that starts the MCP server, sends a tool call, and asserts every byte on stdout parses as JSON. Fuzzing: feed malformed requests and verify stdout remains clean JSON-RPC.

**Phase:** Must be enforced from the first line of MCP server code. Add the clippy lint and tracing config before writing any tool handlers.

---

### Pitfall 5: Search Overlay Blocking the Render Loop

**What goes wrong:** The search overlay UI (Ctrl+Shift+F) performs SQLite FTS5 queries synchronously in the `window_event` handler or `RedrawRequested` handler, blocking the winit event loop. During a slow query (large history, complex pattern), the terminal freezes -- no input, no output, no redraw.

**Why it happens:** The winit event loop is single-threaded and synchronous (`ApplicationHandler` methods are not async). The natural impulse is to query SQLite directly when the user types a search term. But FTS5 queries on a large corpus (100K+ commands) can take 50-200ms, and the query runs on every keystroke.

**Consequences:** Terminal becomes unresponsive during search. If the user types quickly, queries queue up and the UI freezes for seconds. This is especially bad because the terminal must continue rendering PTY output (commands may still be running) while the search overlay is open.

**Prevention:**
- **Run search queries on a background thread/task.** When the user types a search term, debounce for 150ms, then spawn a query on a background thread. Send results back via `EventLoopProxy<AppEvent>` (the same pattern used for git status queries).
- **Never query SQLite on the winit main thread.** The main thread should only read from an in-memory results cache that the background query thread populates.
- **Debounce search input.** Don't query on every keystroke. Wait until the user stops typing for 150ms before issuing a query. Cancel in-flight queries when new input arrives.
- **Limit result count.** `SELECT ... LIMIT 50`. The overlay can only display ~20 results at once; fetching 10,000 matches is wasteful.
- **Keep a read-only SQLite connection on the search thread** (separate from the write connection on the PTY reader thread). WAL mode ensures this doesn't block writes.

**Detection:** Open search overlay, type rapidly while a long-running command is producing output. If the terminal output stutters or freezes, the search is blocking the event loop.

**Phase:** Addressed when the search overlay UI is implemented. The async query pattern should be designed alongside the overlay rendering.

---

## Moderate Pitfalls

### Pitfall 6: MCP Server as Embedded Thread vs. Separate Process -- Wrong Choice

**What goes wrong:** Embedding the MCP server as a thread inside the Glass terminal process. The MCP client launches the server via `stdio`, expecting to own stdin/stdout of the process. But Glass's main process already uses stdin/stdout for the winit event loop and PTY. There is no clean way to share stdin/stdout between winit and the MCP server in a single process.

**Prevention:**
- **Run the MCP server as a separate binary** (`glass-mcp` or `glass mcp serve`). The MCP client config points to this binary. The binary opens its own SQLite read connection to the Glass history database. It communicates with the terminal (if needed for live context) via a lightweight IPC mechanism (Unix domain socket on Linux, named pipe on Windows) or simply reads from the shared SQLite database.
- The separate binary is simpler: it only needs `rusqlite` + `rmcp` (the official Rust MCP SDK) + `tokio`. No wgpu, no winit, no alacritty_terminal dependencies. This keeps compile times low and the binary small.
- Add the `glass-mcp` binary to the workspace `Cargo.toml` as a separate `[[bin]]` target or a dedicated crate.

**Detection:** If you find yourself trying to redirect stdin/stdout inside the terminal process to split between MCP and PTY, stop -- you've chosen the wrong architecture.

**Phase:** Architecture decision needed before any MCP code is written.

---

### Pitfall 7: display_offset Hardcoded to 0 Breaks Scrollback Search Navigation

**What goes wrong:** The existing `display_offset` is hardcoded to 0 (noted as tech debt). When search finds a result in scrollback history, the UI needs to scroll to that line. But if `display_offset` doesn't work correctly, search results in scrollback are unreachable -- the user sees "3 results" but can't navigate to any of them.

**Why it happens:** This is pre-existing tech debt from v1.0. The `GridSnapshot` captures `display_offset` from `term.renderable_content().display_offset`, and `BlockManager.visible_blocks()` uses it. But the actual scroll navigation (Shift+PageUp/Down) works through `term.scroll_display()` which does update the offset internally. The "hardcoded to 0" note likely refers to block visibility calculations not accounting for scrollback properly.

**Prevention:**
- **Fix `display_offset` integration before building the search overlay.** The search overlay's "jump to result" feature depends entirely on being able to programmatically scroll the terminal to a specific line in history.
- Test: scroll up 100 lines with Shift+PageUp, verify `snapshot.display_offset` is non-zero, verify `visible_blocks()` returns blocks from scrollback (not current viewport).
- The fix likely involves `term.lock().scroll_display(Scroll::Delta(target_offset))` and verifying the snapshot reflects the new offset.

**Detection:** Manual test: run 200 lines of output, scroll up, check if block separators render correctly in scrollback. If they don't, `display_offset` is broken.

**Phase:** Must be fixed before or during the search overlay phase. This is a prerequisite.

---

### Pitfall 8: CLI Binary Database Locking Conflicts

**What goes wrong:** The `glass history` CLI binary opens the SQLite database while the terminal is running, and either: (a) the CLI gets `SQLITE_BUSY` and fails, or (b) the CLI's read transaction blocks the terminal's write transactions, causing dropped history entries.

**Why it happens:** Without WAL mode, SQLite uses rollback journaling where readers block writers and vice versa. Even with WAL mode, if the CLI opens a long-running read transaction (e.g., streaming results to a pager), it prevents the WAL from being checkpointed, causing the WAL file to grow unboundedly.

**Prevention:**
- **WAL mode is non-negotiable.** Set `PRAGMA journal_mode=WAL` in both the terminal and the CLI.
- **Set `busy_timeout(5000)` in the CLI** to gracefully wait for any in-progress writes.
- **Don't hold read transactions open.** The CLI should execute its query, collect results into memory, close the transaction, then format and output. Never pipe a live SQLite cursor through a pager.
- **Use `PRAGMA wal_checkpoint(PASSIVE)` periodically** in the terminal's write connection to prevent unbounded WAL growth. Passive checkpointing never blocks readers.
- **Test concurrent access explicitly.** Integration test: start a write loop in one thread, start a read loop in another thread, verify zero `SQLITE_BUSY` errors over 10,000 operations.

**Detection:** Run `glass history search "foo"` while the terminal is actively writing history. If it fails with a database locked error, the concurrency setup is wrong.

**Phase:** Addressed when the CLI binary is implemented, but WAL mode must be set from the initial schema creation.

---

### Pitfall 9: FTS5 Indexing Every Byte of Output Explodes Database Size

**What goes wrong:** Indexing the full raw output of every command (including ANSI escape sequences, progress bars, binary data) creates a massive FTS5 index that's mostly noise. A single `cargo build` can produce 50KB of output; indexing every byte means the FTS index grows faster than the raw data.

**Prevention:**
- **Strip ANSI escape sequences before indexing.** Store raw output for display, but index only the plain text content. Use a simple state machine to strip CSI/OSC sequences (the OscScanner pattern already exists in the codebase).
- **Don't index output from commands with exit code != 0 by default**, or index only the last N lines (error summary). Failed commands often produce stack traces that pollute search results.
- **Set a maximum indexable output size per command** (e.g., 64KB of plain text after stripping). Commands producing more output get partial indexing.
- **Use FTS5 `detail='none'` or `detail='column'`** if you only need to know which commands match, not the exact position within the output. This dramatically reduces index size.
- **Provide a config option** to disable output indexing entirely (index only command text, cwd, timestamps). Some users want search but don't need full-text output search.

**Detection:** Monitor database size growth. If the database exceeds 100MB after 1,000 commands, output indexing is too aggressive.

**Phase:** Addressed during FTS5 indexing implementation.

---

### Pitfall 10: Search Overlay Rendering Z-Order Conflicts with GPU Pipeline

**What goes wrong:** The search overlay (text input, results list, highlighting) doesn't render correctly on top of the terminal grid. Results appear behind the terminal text, or the terminal grid bleeds through the overlay background.

**Why it happens:** Glass uses a specific render pass order: background rects -> text (glyphon) -> block decorations -> status bar. Adding a search overlay requires rendering opaque background rects over the terminal content and then rendering overlay text on top. If the overlay is added in the wrong position in the render pass, it either gets overwritten by later passes or fails to cover the terminal content.

**Prevention:**
- **Render the search overlay as the last element in the render pass** -- after the status bar. The overlay should render: (1) a semi-transparent or opaque background rect covering the overlay area, (2) the search input text, (3) the results list text. All using the existing `RectRenderer` and `glyphon` text pipeline.
- **Use a separate `TextArea` for overlay text**, distinct from the terminal grid text. Glyphon supports multiple `TextArea` entries in a single draw call, but they need different positions and potentially different font sizes.
- **Don't create a separate render pass for the overlay.** Adding a second render pass causes a GPU round-trip and adds latency. Instead, append overlay draw calls to the existing single render pass after all terminal content.
- **Handle the overlay in `FrameRenderer::draw_frame()`** with a conditional overlay rendering step at the end.

**Detection:** Open the search overlay on top of a terminal full of colored text. If any terminal text is visible through the overlay background, the z-order is wrong.

**Phase:** Addressed when the search overlay UI is built.

---

## Minor Pitfalls

### Pitfall 11: Forgetting to Enable `bundled` Feature in rusqlite (FTS5 Dependency)

**What goes wrong:** The `bundled` feature of rusqlite compiles SQLite with FTS5 enabled, but if you later switch to system SQLite or a different feature flag, FTS5 may not be available, causing runtime SQL errors.

**Prevention:** Pin to `rusqlite = { version = "0.38.0", features = ["bundled"] }`. The `bundled` feature automatically enables FTS5. Add an integration test that creates an FTS5 table and runs a `MATCH` query. If this test fails, the feature flag is wrong. Never use system SQLite on Windows (it may not exist or may lack FTS5).

**Phase:** Set up correctly in Cargo.toml from the start.

---

### Pitfall 12: MCP Tool Schema Drift from Implementation

**What goes wrong:** The MCP tool definitions (JSON Schema for `GlassHistory`, `GlassContext`) get out of sync with the actual query capabilities. The AI assistant requests fields that don't exist, or the tool returns fields the schema doesn't describe.

**Prevention:** Derive tool schemas from Rust types using serde + `schemars` or the `rmcp` crate's `#[tool]` macro. This ensures the schema always matches the implementation. Write tests that serialize a tool response and validate it against the declared schema.

**Phase:** Addressed when MCP tool definitions are implemented.

---

### Pitfall 13: Retention Policy Deleting Data While FTS5 Index References It

**What goes wrong:** A retention policy job deletes old rows from the commands table, but the FTS5 index still references those rows. Subsequent searches return phantom results (row exists in FTS index but not in the content table), or worse, the FTS index becomes corrupted.

**Prevention:**
- If using a content FTS5 table (recommended), deleting from the FTS5 virtual table automatically removes the index entry. Delete from the FTS5 table, which cascades.
- If using external content FTS5, ensure the delete trigger fires before or uses the `VALUES('delete', ...)` syntax.
- Retention should be a single transaction: `DELETE FROM commands WHERE timestamp < ? RETURNING rowid` -> `DELETE FROM commands_fts WHERE rowid IN (...)`.
- Run FTS5 integrity check after retention: `INSERT INTO commands_fts(commands_fts) VALUES('integrity-check')`.

**Phase:** Addressed when retention policies are implemented.

---

### Pitfall 14: Command Text Capture Missing Due to OSC 133 Gaps

**What goes wrong:** The history database stores command text by capturing bytes between OSC 133;B (command start) and OSC 133;C (command executed). But some shells or shell configurations don't emit these sequences reliably, leaving the command text field empty.

**Prevention:**
- **Fall back to Get-History (PowerShell) or HISTFILE (Bash)** when OSC 133;B/C are missing. Query the shell's own history as a secondary source.
- **Handle multi-line commands** -- the text between B and C may span multiple terminal lines. Use `\n` joining.
- **Test with both pwsh and bash** (Glass supports both). PowerShell's PSReadLine and Bash's PS0/PROMPT_COMMAND have different timing characteristics.
- **Accept that command text capture is best-effort.** Some commands (e.g., piped through `ssh`) will never emit OSC sequences. Log the command as "[unknown]" rather than dropping the entire history entry.

**Phase:** Addressed during output capture implementation.

---

### Pitfall 15: tokio Runtime Conflicts with winit Event Loop

**What goes wrong:** Adding tokio for the MCP server or async SQLite operations, then accidentally blocking the tokio runtime from the winit event loop or vice versa. `pollster::block_on()` inside a tokio context panics ("cannot block on a future inside a runtime").

**Prevention:**
- **Keep the winit event loop on the main thread without tokio.** The PTY reader thread is `std::thread` (already correct in v1.0). The MCP server binary runs its own tokio runtime -- it's a separate process, so no conflict.
- **If any async work is needed in the terminal process** (e.g., async channel draining for SQLite writes), spawn a dedicated `std::thread` that runs `tokio::runtime::Runtime::new().block_on(...)`. Do not use `#[tokio::main]` in the terminal binary.
- **The MCP server binary can use `#[tokio::main]`** freely since it's a separate process.

**Phase:** Architecture decision, affects MCP server and async write path design.

---

## Phase-Specific Warnings

| Phase Topic | Likely Pitfall | Mitigation | Severity |
|-------------|---------------|------------|----------|
| SQLite schema + write path | Pitfall 1 (connection sharing), Pitfall 2 (throughput) | One connection per thread, buffer writes, WAL mode | Critical |
| Output capture | Pitfall 2 (throughput), Pitfall 14 (OSC gaps) | In-memory buffer, flush on command complete, fallback capture | Critical |
| FTS5 indexing | Pitfall 3 (sync corruption), Pitfall 9 (size explosion) | Content table (not external), strip ANSI, size limits | Moderate |
| Search overlay UI | Pitfall 5 (blocking render), Pitfall 7 (display_offset), Pitfall 10 (z-order) | Async queries, fix display_offset first, render last | Moderate |
| CLI binary | Pitfall 8 (DB locking) | WAL mode, busy_timeout, short transactions | Moderate |
| MCP server | Pitfall 4 (stdout corruption), Pitfall 6 (embedded vs. separate) | Separate binary, stderr logging, clippy lint | Critical |
| Retention policies | Pitfall 13 (FTS5 orphans) | Single transaction delete, integrity check | Minor |
| Tool schemas | Pitfall 12 (schema drift) | Derive from types, test serialization | Minor |

---

## Pre-Existing Tech Debt Impact

| Debt Item | Impact on v1.1 | Required Action |
|-----------|----------------|-----------------|
| `display_offset` hardcoded to 0 | Search "jump to result" will not work | Fix before search overlay phase |
| Nyquist validation partial (phases 2-4) | Performance baselines may be inaccurate for regression testing | Re-baseline PTY throughput before output capture |
| `BlockManager` uses line numbers from cursor position | Line numbers may not match database row IDs | Use stable identifiers (timestamps + command index) for DB, not terminal line numbers |

---

## Sources

- [SQLite WAL documentation](https://sqlite.org/wal.html) -- concurrent reader/writer guarantees
- [SQLite threading modes](https://sqlite.org/threadsafe.html) -- serialized vs. multi-thread
- [rusqlite multi-thread discussion](https://github.com/rusqlite/rusqlite/issues/405) -- connection-per-thread pattern
- [SQLITE_BUSY despite timeout](https://berthub.eu/articles/posts/a-brief-post-on-sqlite3-database-locked-despite-timeout/) -- BEGIN IMMEDIATE requirement
- [SQLite connection pool write performance](https://emschwartz.me/psa-your-sqlite-connection-pool-might-be-ruining-your-write-performance/) -- single writer best practices
- [SQLite FTS5 documentation](https://sqlite.org/fts5.html) -- external content tables, integrity check, detail options
- [FTS5 index structure](https://darksi.de/13.sqlite-fts5-structure/) -- merge behavior, segment management
- [FTS5 trigger corruption](https://sqlite.org/forum/info/da59bf102d7a7951740bd01c4942b1119512a86bfa1b11d4f762056c8eb7fc4e) -- incorrect trigger syntax causes corruption
- [MCP specification - stdio transport](https://modelcontextprotocol.io/docs/develop/build-server) -- stdout purity requirement
- [rmcp official Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk) -- tool macro, stdio transport
- [Shuttle MCP server guide](https://www.shuttle.dev/blog/2025/07/18/how-to-build-a-stdio-mcp-server-in-rust) -- stderr logging pattern
- [rusqlite feature flags](https://docs.rs/crate/rusqlite/latest/features) -- bundled enables FTS5
- [Alacritty vtebench](https://github.com/alacritty/vtebench) -- PTY throughput benchmarking
- [kitty performance docs](https://sw.kovidgoyal.net/kitty/performance/) -- throughput vs. latency tradeoffs
- [FTS5 performance tuning](https://www.slingacademy.com/article/full-text-search-performance-tuning-avoiding-pitfalls-in-sqlite/) -- indexing size management

---
*Pitfalls research for: Glass v1.1 -- Structured Scrollback + MCP Server*
*Researched: 2026-03-05*
