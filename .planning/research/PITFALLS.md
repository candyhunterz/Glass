# Domain Pitfalls

**Domain:** Adding async MCP channel, multi-tab orchestration, structured error parsers, token-saving tools, and live command awareness to an existing GPU-accelerated terminal emulator (Glass v2.3)
**Researched:** 2026-03-09

---

## Critical Pitfalls

Mistakes that cause deadlocks, rewrites, or data corruption.

### Pitfall 1: Process Boundary -- MCP Server Runs in a Separate Process

**What goes wrong:** The implementation plan describes an `mpsc` channel between `GlassServer` and the main event loop as if they share a process. They do not. `glass mcp serve` creates a separate tokio runtime in a separate OS process (see `McpAction::Serve` handler in `main.rs` line 2732). An `mpsc` channel cannot cross process boundaries. The developer builds the channel infrastructure, discovers it cannot be wired up, and must redesign the entire communication layer.

**Why it happens:** The design document says "add a bounded async channel pair to GlassServer" without addressing that `run_mcp_server()` runs in a completely separate process with its own tokio runtime. The current GlassServer only accesses SQLite databases on disk -- the only shared state between the processes is the filesystem.

**Consequences:** Complete redesign of the communication architecture. Every feature that requires live session access (tab orchestration, grid reads, command status, command cancel) is blocked until this is resolved.

**Prevention:**
Two viable architectures exist. Choose one before writing any code:

**Option A: Embed MCP server as a task within the terminal process.** The terminal process spawns an async task that serves MCP over a socket transport (not stdio, because `#![windows_subsystem = "windows"]` suppresses the console). Use a local TCP port or platform IPC (named pipe on Windows, Unix domain socket elsewhere). The `glass mcp serve` CLI entry point changes to connect to the running terminal's socket. Pros: in-process channels work, simplest data access. Cons: requires transport change, MCP server lifecycle tied to terminal.

**Option B: Keep MCP as a separate process, add IPC.** The terminal process listens on a platform IPC channel. The MCP server process connects to it for live state queries. Define a binary or JSON protocol for request/response. Pros: maintains process isolation, current CLI entry point works. Cons: significant IPC layer to build and maintain, serialization overhead.

Recommendation: **Option A** (embedded) is dramatically simpler. The MCP tools that only access SQLite (existing tools, `glass_cached_result`, `glass_changed_files`) work identically either way. Only the live-state tools need the channel, and those are trivial with in-process access.

If Option A is chosen, the `#![windows_subsystem = "windows"]` constraint must be addressed: the terminal process needs a way for AI agents to connect to the MCP server. The AI agent's MCP client (e.g., Claude Code) would connect via a local socket instead of spawning `glass mcp serve` as a child process.

**Detection:** If you find yourself passing an `mpsc::Sender` to `run_mcp_server()` and expecting it to work across `glass mcp serve`, you've hit this pitfall.

**Phase:** MCP Command Channel (Phase 1) -- this is the FIRST decision. Everything else depends on it.

---

### Pitfall 2: FairMutex Contention -- Three-Way Lock Fight on Terminal Grid

**What goes wrong:** The terminal grid (`Arc<FairMutex<Term<EventProxy>>>` in `Session`) is the single most contended resource. Currently two threads compete: the PTY reader thread (writes bytes continuously) and the renderer (reads via `snapshot_term()` every frame). Adding a third contender -- MCP grid reads for `glass_tab_output` -- creates visible rendering stalls, input lag, or MCP timeouts.

**Why it happens:** FairMutex guarantees fairness but not speed. `snapshot_term()` already iterates all cells via `renderable_content()`, allocating and copying per-cell color data. An MCP grid read that similarly iterates all visible cells (for text extraction) would hold the lock for comparable duration. During a `cargo build` flooding output, the PTY reader holds the lock almost continuously. The renderer and MCP reader compete for narrow gaps.

**Consequences:** Visible terminal lag (missed frames), MCP tool response times of 100ms+ during heavy output, potential UI freeze if grid read takes too long while PTY reader is queued.

**Prevention:**
- Do NOT use `snapshot_term()` for MCP text extraction. That function resolves colors for every cell, which MCP doesn't need. Write a minimal `extract_text_lines()` function that only reads character content and line boundaries.
- Lock the grid, copy raw text out, release lock, THEN process (filter, strip escapes, serialize JSON). Never do string processing while holding the lock.
- Set a hard timeout: try to acquire the lock for at most 50ms. If the terminal is flooding output, return `{ error: "terminal busy, try again" }` to the MCP caller rather than blocking the event loop.
- Limit the number of lines read per lock acquisition. For 200+ lines, consider releasing and re-acquiring the lock in chunks to let the renderer and PTY reader interleave.
- For `glass_tab_output` with a `pattern` filter: read ALL lines under lock, release, THEN apply regex. Never run regex while holding the lock.

**Detection:** Add tracing spans around lock acquisition in the MCP handler. If lock hold time exceeds 5ms, investigate. If it exceeds 16ms (one frame at 60fps), the renderer is stuttering.

**Phase:** MCP Command Channel (Phase 1) for the text extraction function, Multi-Tab Orchestration (Phase 2) for the tools that use it.

---

### Pitfall 3: Event Loop Starvation from MCP Request Processing

**What goes wrong:** The winit event loop processes MCP requests synchronously in `user_event()`. Each request (grid read, tab create, command status check) takes 1-10ms. An agent sends 5 requests in rapid succession (one per tab). The event loop is blocked for 5-50ms total, causing visible typing lag and dropped frames.

**Why it happens:** `user_event()` in winit's `ApplicationHandler` is called in-line during event dispatch. While processing MCP requests, no keyboard events, mouse events, or redraw requests are handled. The existing AppEvent variants (TerminalDirty, Shell, GitInfo, etc.) are all fast (<1ms). MCP requests involving grid reads or session creation are significantly heavier.

**Consequences:** Terminal becomes unresponsive during bursts of MCP activity. The user types keystrokes that are delayed by 50-100ms. The renderer skips frames.

**Prevention:**
- Process AT MOST one MCP request per event loop iteration. If more are queued, request a redraw and process the next one in the next frame cycle.
- Use a bounded channel (capacity 16-32). Backpressure naturally limits concurrency.
- Categorize requests by cost:
  - **Fast** (< 1ms): `TabList`, `CommandStatus` -- process inline
  - **Medium** (1-5ms): `TabOutput`, `TabCreate` -- process one per frame
  - **Heavy** (5ms+): never. Redesign if any request takes this long.
- For `TabCreate`, do the PTY spawn asynchronously: send the request, return a session ID immediately, let the PTY reader thread start in the background. The agent can poll `glass_tab_list` to know when the tab is ready.

**Detection:** Track time spent in `user_event()` for each AppEvent variant. Any MCP request consistently over 5ms needs optimization.

**Phase:** MCP Command Channel (Phase 1).

---

### Pitfall 4: Tab ID Instability Causing Wrong-Tab Command Execution

**What goes wrong:** The design uses `tab_id: usize` (vector index) as the identifier for tab orchestration. But tab indices shift when tabs are closed or reordered. An agent creates tab 3, stores the ID, another tab closes, and tab 3 is now a different session. The agent then runs a command in the wrong tab.

**Why it happens:** `SessionMux.tabs` is a `Vec<Tab>`. Removing tab at index 1 shifts all subsequent indices. `tabs[2]` after deletion refers to what was previously `tabs[3]`.

**Consequences:** Commands sent to wrong sessions. In the best case, a build runs in the wrong directory. In the worst case, destructive commands (`rm -rf`, `git clean`) in the wrong directory.

**Prevention:**
- Use `SessionId` (the monotonically increasing u64 counter) as the stable identifier, NOT tab index. SessionId is never reused and never shifts.
- MCP tools should accept `session_id: string` (e.g., "session-3"), not `tab_id: number`.
- `glass_tab_list` should return `session_id` as the primary identifier, with tab index as a display-only hint.
- Add `session_by_id()` lookup that returns `Option` and surface clear errors when a session no longer exists: `{ error: "session-3 no longer exists (tab was closed)" }`.
- The `SessionMux` already has `fn session(&self, id: SessionId) -> Option<&Session>` which is exactly the right lookup.

**Detection:** If any MCP tool parameter is named `tab_id: usize`, this pitfall is present.

**Phase:** Multi-Tab Orchestration (Phase 2).

---

### Pitfall 5: Unbounded Output Blowing Up MCP Message Size

**What goes wrong:** `glass_tab_output` reads terminal grid content and returns it as JSON. The alacritty_terminal scrollback buffer can hold 10,000+ lines at 200+ columns. A naive read returns 2-4MB of text. MCP transports may have message size limits, and even without limits, this wastes agent tokens.

**Why it happens:** The terminal grid pads every line to the full column width with spaces. 200 columns * 10,000 lines = 2M characters, most of which are trailing whitespace. Add JSON encoding overhead and escape sequences, and the response can hit 4-5MB.

**Consequences:** MCP transport errors, agent token budget exhaustion, slow response times, potential OOM in JSON serialization.

**Prevention:**
- Hard cap: 100KB maximum response size, always. Truncate from the top (keep most recent output).
- Default `lines` to 50, not "all". Force the caller to ask for more explicitly.
- Strip trailing whitespace from every line. This alone typically reduces output size by 60-80%.
- Count bytes during extraction and stop early once the limit is hit.
- Return metadata: `{ truncated: true, total_lines: 8743, returned_lines: 50 }` so the caller knows they have a partial view.
- For `glass_output` reading from history DB: the `output` column is already capped at 50KB during capture (see `output_capture.rs`), so this is only a concern for live grid reads.

**Detection:** Test with `find / -name "*.rs" 2>/dev/null` (thousands of lines) or `cargo build 2>&1` (hundreds of lines with color codes). If the MCP response exceeds 100KB, the cap is missing.

**Phase:** Multi-Tab Orchestration (Phase 2) and Token-Saving Tools (Phase 3).

---

### Pitfall 6: Oneshot Reply Channel Dropped Without Response

**What goes wrong:** An MCP tool sends `McpRequest::TabOutput { session_id, reply: oneshot::Sender }`. The `user_event()` handler looks up the session, it doesn't exist, and returns early without sending a response on the oneshot channel. The MCP tool awaits the oneshot receiver forever, leaking the tokio task and blocking the agent.

**Why it happens:** Error paths in match arms are easy to miss. The happy path sends a reply, but `None` from session lookup, panics in grid reading, or early returns from validation all silently drop the oneshot sender.

**Consequences:** MCP tool hangs indefinitely. The agent's MCP client may time out after 30+ seconds, reporting a generic error. The leaked task consumes memory.

**Prevention:**
- Structure every MCP request handler as a function that always returns a response:
  ```rust
  let response = handle_tab_output(&session_mux, &params);
  let _ = reply.send(response); // ignore send error = receiver already dropped (MCP tool timed out)
  ```
- Add a timeout on the MCP tool's oneshot `recv` (5 seconds). If the main thread panics or is stuck, the tool returns a timeout error rather than hanging.
- Write a test that sends a request for a non-existent session and verifies the oneshot completes with an error response.
- Consider a `Drop` guard on the oneshot sender that logs if it's dropped without being used.

**Detection:** If any `user_event()` match arm for MCP requests has an early `return` or `if let Some(...)` without an `else` that sends an error reply, this pitfall is present.

**Phase:** MCP Command Channel (Phase 1).

---

## Moderate Pitfalls

### Pitfall 7: ANSI Escape Sequence Contamination in All Output

**What goes wrong:** Terminal grid content and stored history output contain ANSI escape sequences (colors, cursor movement, SGR codes). If these leak into MCP tool responses, the AI agent receives `\x1b[31merror\x1b[0m: mismatched types` instead of `error: mismatched types`. Error parsers fail because regex patterns don't match with embedded escape sequences.

**Why it happens:** The terminal grid stores styled content. Reading cells gives you the character plus escape sequences from the original output stream. History DB output may also contain escapes if the command produced colored output and stripping was incomplete.

**Prevention:**
- Strip ANSI escapes at the MCP boundary: every tool that returns text content must strip escapes before JSON serialization.
- Reuse the existing `OutputBuffer::strip_ansi()` in `glass_terminal/src/output_capture.rs`. Do NOT write a new stripping function.
- Error parsers (glass_errors crate) must operate on pre-stripped text only. Include a debug assertion that input contains no escape sequences.
- Test with `CARGO_TERM_COLOR=always cargo build` to ensure color-coded output is properly stripped.
- When reading from the terminal grid (not history DB), extract only the character content of each cell, not the raw byte stream. Grid cells already have escape sequences decoded into flags (bold, color, etc.), so reading `cell.c` gives clean characters.

**Detection:** Search MCP tool responses for `\x1b` or `\u001b`. Any match means stripping is missing.

**Phase:** All phases -- must be correct from Phase 1.

---

### Pitfall 8: Error Parser Scope Creep -- 6 Parsers Becomes 30+ Format Variations

**What goes wrong:** The plan lists 6 error parsers (Rust, Python, Node, Go, GCC, Generic). In practice, each tool has 3-5 output format variations. Rust alone has: cargo errors, clippy lints, `cargo test` failures (with `---- test_name stdout ----` blocks), proc-macro panics, and `cargo bench` output. Python has tracebacks, pytest, mypy, ruff, flake8. The "6 parsers" become 30+ format handlers, each with edge cases.

**Why it happens:** Error output formats are not standardized. Even within a single tool, different error types produce different patterns. The Rust parser regex for `error[E0308]` doesn't match `cargo test` failure output. Each new variation discovered during testing requires its own pattern.

**Prevention:**
- Start with exactly TWO parsers: **Rust (cargo/clippy)** and **Generic fallback** (`file:line:col: message`). These cover 80%+ of use cases for a Rust project, and the generic fallback catches GCC, Go, and many other tools.
- Each parser should return a `confidence: f32` so the MCP tool can pick the best match when multiple parsers produce results.
- Auto-detection from output content alone is unreliable. Use the command text hint as the PRIMARY signal (command starts with `cargo` = Rust parser).
- Add Python and Node parsers only after the first two are battle-tested with real-world output samples.
- Collect real compiler output as test fixtures from actual builds, not hand-written examples. Hand-written examples miss edge cases (multi-line errors, errors within errors, warnings mixed with errors, etc.).

**Detection:** If the error parser has more than 3 modules before the first release, scope creep has occurred.

**Phase:** Structured Error Extraction (Phase 4).

---

### Pitfall 9: Stale Cache Causing False Confidence in `glass_cached_result`

**What goes wrong:** `glass_cached_result` returns old `cargo test` output saying "all tests pass" even though files have changed. The `files_changed_since` counter only checks the snapshot DB, but most file changes happen outside of Glass-tracked commands (e.g., editor saves, `git checkout`). The agent trusts the cached result, skips re-running tests, and pushes broken code.

**Why it happens:** The snapshot DB only records files changed by commands that Glass's command parser identified as file-modifying. Editor saves, IDE auto-format, git operations, and direct file writes from other terminals are invisible to the snapshot DB.

**Prevention:**
- Check actual filesystem modification times, not just the snapshot DB. For the cached command's CWD, stat common source directories (`src/`, `lib/`, `tests/`, `Cargo.toml`, etc.) and compare mtimes against the command's `finished_at` timestamp.
- Add `files_possibly_stale: true` flag when mtime check shows changes, regardless of snapshot DB state. The agent can decide whether to re-run.
- Default `max_age_seconds` to 120 (not 300). Shorter window reduces risk.
- Use git as a staleness signal when available: `git diff --stat HEAD` showing changes since the cached command's commit hash is a strong signal of staleness.
- Always include a disclaimer in the response: `"note": "cache validity is approximate; re-run for certainty"`.

**Detection:** Change a source file via an editor (not a Glass command), then call `glass_cached_result`. If it returns `files_changed_since: 0`, the mtime check is missing.

**Phase:** Token-Saving Tools (Phase 3).

---

### Pitfall 10: Command Cancel Race Condition

**What goes wrong:** Agent checks `glass_command_status` (sees "executing"), then calls `glass_command_cancel`. Between the check and the cancel, the command finishes naturally. The Ctrl+C byte (0x03) is sent to an idle shell prompt. On most shells this is harmless, but on some configurations it can abort an interactive prompt, cancel a partially-typed command, or trigger unexpected behavior.

**Why it happens:** TOCTOU (time-of-check-time-of-use) race between status check and cancel action. The command state transitions from `Executing` to `Complete` asynchronously (driven by PTY output and OSC 133;D detection).

**Prevention:**
- In the cancel handler itself, re-check `BlockManager` state. If the current block is `Complete` or `PromptActive`, do NOT send Ctrl+C. Return `{ cancelled: false, reason: "already_complete" }`.
- Even with the check, there's still a tiny TOCTOU window. This is inherent and acceptable. Sending Ctrl+C to an idle prompt is harmless in bash/zsh/fish/pwsh (it just prints a new prompt).
- Never send SIGKILL or SIGTERM through this mechanism. Only send the Ctrl+C byte (0x03) via `pty_sender.send(PtyMsg::Input(...))`.
- Document the inherent race in the MCP tool description so agents know to handle `already_complete` responses gracefully.

**Detection:** Write an integration test that cancels a command immediately after it finishes. Verify the response includes `already_complete` and no unexpected behavior occurs.

**Phase:** Live Command Awareness (Phase 5).

---

### Pitfall 11: `similar` Crate Quadratic Diff for Large Files

**What goes wrong:** `glass_changed_files` uses the `similar` crate for unified diffs. The diff algorithm is O(n*m) where n and m are file lengths. For large generated files (1000+ lines), diff computation takes seconds. If the snapshotted file is a 10,000-line `Cargo.lock` or a compiled artifact, the diff is both slow to compute and useless to the agent.

**Why it happens:** Content-addressed blob store doesn't distinguish between source files (small, diffable) and generated/config files (large, not useful to diff). Any modified file gets diffed regardless of size.

**Prevention:**
- Cap diffable file size at 50KB. For larger files, return `{ action: "modified", diff: null, reason: "file_too_large (128KB)" }`.
- Skip binary files entirely (check for null bytes in the first 8KB).
- Consider skipping known-generated files (`Cargo.lock`, `package-lock.json`, `yarn.lock`) or at least noting them as `"type": "lockfile"`.
- The agent can request specific file diffs via `glass_file_diff` if it needs the content of a large file.

**Phase:** Token-Saving Tools (Phase 3).

---

### Pitfall 12: MCP Embedding Breaks `#![windows_subsystem = "windows"]` stdio

**What goes wrong:** If the MCP server is embedded in the terminal process (per Pitfall 1 recommendation), the terminal process has `#![windows_subsystem = "windows"]` which suppresses the console. This means stdin/stdout are not connected to anything. The existing MCP stdio transport (`rmcp::transport::stdio()`) will fail or produce no output. AI agents that spawn `glass mcp serve` as a child process and communicate via stdio will get nothing.

**Why it happens:** `#![windows_subsystem = "windows"]` tells Windows not to allocate a console for the process. Without a console, there are no standard I/O handles. The current `glass mcp serve` works because it's a separate invocation that IS launched from a terminal (so it inherits the terminal's console).

**Prevention:**
- If embedding MCP, use a socket-based transport instead of stdio. Options:
  - TCP on localhost with a random port (write port to a known file like `~/.glass/mcp-port`)
  - Windows named pipe (`\\.\pipe\glass-mcp-{pid}`)
  - Unix domain socket (`/tmp/glass-mcp-{pid}.sock`)
- The AI agent's MCP client configuration changes from `command: "glass mcp serve"` to `url: "tcp://localhost:{port}"` or equivalent.
- Keep the `glass mcp serve` CLI entry point working for backward compatibility: it can either start a standalone server (current behavior) or connect to a running terminal's socket and proxy stdio-to-socket.
- Verify rmcp supports custom transports. The `serve()` method currently takes `rmcp::transport::stdio()` but likely supports other `AsyncRead + AsyncWrite` implementations.

**Detection:** If the embedded MCP server starts but no AI agent can connect, this pitfall is the cause.

**Phase:** MCP Command Channel (Phase 1) -- blocking decision.

---

## Minor Pitfalls

### Pitfall 13: Regex ReDoS in Output Pattern Filtering

**What goes wrong:** The `pattern` parameter in `glass_output` and `glass_tab_output` accepts user-provided regex. A poorly crafted pattern could cause catastrophic backtracking.

**Prevention:** Use Rust's `regex` crate, which uses finite automata and is immune to ReDoS by design. Set `RegexBuilder::size_limit(1 << 20)` to reject absurdly complex patterns. Test with `(a+)+$` to verify no hang occurs.

**Phase:** Token-Saving Tools (Phase 3).

---

### Pitfall 14: Token Budget Approximation Drift

**What goes wrong:** `glass_context` with `budget` parameter approximates tokens as `chars / 4`. This ratio varies: code is closer to 1:3, English text closer to 1:4.5. The actual token count can overshoot the budget by 30-50%.

**Prevention:** Use `chars / 3` as the conservative estimate. Undershoot is better than overshoot. Document that `budget` is approximate ("may return up to 30% fewer tokens than requested"). Do not import a tokenizer.

**Phase:** Token-Saving Tools (Phase 3).

---

### Pitfall 15: Multiple Concurrent MCP Connections to Same Terminal

**What goes wrong:** Two AI agents both connect to the same terminal's MCP server. Both call `glass_tab_run` on the same tab simultaneously. Two commands are typed into the same PTY interleaved, producing garbled input like `carggo o tesbuilt`.

**Prevention:**
- Serialize PTY writes per session: only one `glass_tab_run` call per session can be active at a time. Queue or reject concurrent writes.
- Alternatively, require agents to use the coordination system (glass_agent_lock) to claim a tab before writing to it. If agent A holds the lock on session-2, agent B's `glass_tab_run` to session-2 returns a conflict error.
- For read-only tools (`glass_tab_output`, `glass_tab_list`, `glass_command_status`), concurrent access is safe and should not be serialized.

**Phase:** Multi-Tab Orchestration (Phase 2).

---

### Pitfall 16: History DB Output Column is Only 50KB

**What goes wrong:** `glass_output` and `glass_errors` can read from the history DB's `output` column. But output capture truncates at 50KB (see `OutputBuffer` in `output_capture.rs`). A `cargo build` with 200 errors easily produces 100KB+ of output. The agent asks for errors, but the stored output is truncated and the last 50 errors are missing.

**Prevention:**
- When reading from history DB, include `{ truncated: true, note: "output was truncated during capture at 50KB" }` if the stored output is exactly at the cap.
- For `glass_errors`, recommend using `tab_id` (live grid) instead of `command_id` (history DB) when the command just finished, as the grid has the full output in scrollback.
- Consider increasing the capture limit for failed commands (exit code != 0) to 200KB, since error output is the most valuable for analysis.

**Phase:** Structured Error Extraction (Phase 4) and Token-Saving Tools (Phase 3).

---

## Phase-Specific Warnings

| Phase Topic | Likely Pitfall | Mitigation |
|-------------|---------------|------------|
| MCP Command Channel | Process boundary (Pitfall 1) + Windows subsystem (Pitfall 12) | Resolve embed-vs-IPC and transport mechanism before writing any code. Prototype connection first. |
| MCP Command Channel | Oneshot reply drops (Pitfall 6) | Every request handler path must send a reply. Add timeout on recv side. |
| MCP Command Channel | Event loop starvation (Pitfall 3) | Max 1 heavy MCP request per frame. Bounded channel(16). |
| Multi-Tab Orchestration | Tab ID instability (Pitfall 4) | Use SessionId, not tab index, from day one. |
| Multi-Tab Orchestration | Grid lock contention (Pitfall 2) | Write a text-only grid reader, minimize lock hold time, never process under lock. |
| Multi-Tab Orchestration | Unbounded output (Pitfall 5) | 100KB hard cap, default 50 lines, strip trailing whitespace. |
| Multi-Tab Orchestration | Concurrent PTY writes (Pitfall 15) | Serialize writes per session, or integrate with coordination locks. |
| Token-Saving Tools (DB) | Stale cache (Pitfall 9) | Check filesystem mtimes, not just snapshot DB. Default max_age 120s. |
| Token-Saving Tools (DB) | Large file diffs (Pitfall 11) | 50KB cap on diffable files. Skip binary files. |
| Token-Saving Tools (DB) | History truncation (Pitfall 16) | Indicate truncation in response. Prefer live grid for recent commands. |
| Structured Error Extraction | Parser scope creep (Pitfall 8) | Ship Rust + Generic only. Add others post-launch. |
| Structured Error Extraction | ANSI contamination (Pitfall 7) | Strip escapes before parsing. Reuse existing stripper. |
| Live Command Awareness | Cancel race condition (Pitfall 10) | Re-check state in cancel handler. Document TOCTOU. Only send 0x03 byte. |

---

## Sources

- Direct codebase analysis of: `src/main.rs` (event loop, MCP serve entry point, WindowContext/Processor structs), `crates/glass_mcp/src/lib.rs` (separate process architecture), `crates/glass_mcp/src/tools.rs` (GlassServer struct, spawn_blocking pattern), `crates/glass_core/src/event.rs` (AppEvent enum, SessionId), `crates/glass_mux/src/session.rs` (Session struct with Arc<FairMutex<Term>>), `crates/glass_mux/src/session_mux.rs` (tab vector, session lookup), `crates/glass_terminal/src/pty.rs` (PtyMsg, PtySender), `crates/glass_terminal/src/block_manager.rs` (BlockState lifecycle), `crates/glass_terminal/src/grid_snapshot.rs` (snapshot_term function)
- `AGENT_MCP_FEATURES.md` -- project-local implementation design document
- `.planning/PROJECT.md` -- project context, key decisions, constraints
- alacritty_terminal FairMutex semantics (HIGH confidence -- core design property of alacritty)
- Rust `regex` crate ReDoS immunity (HIGH confidence -- fundamental design property using Thompson NFA)
- Windows subsystem behavior with stdio (HIGH confidence -- well-known Win32 API behavior)
- rmcp transport flexibility (MEDIUM confidence -- verify custom transport support in rmcp 1.1.0 before committing to socket transport)

---
*Pitfalls research for: Agent MCP Features (Glass v2.3)*
*Researched: 2026-03-09*
