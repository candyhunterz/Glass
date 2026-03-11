# Glass Agent MCP Features — Implementation Plan

## Goal

Make Glass the most token-efficient, capable terminal for AI agents by exposing three feature groups through new MCP tools:

1. **Multi-tab orchestration** — Agents can spawn tabs, run commands in them, and read output across tabs
2. **Structured error extraction** — Parse compiler/test output into structured data instead of raw text
3. **Token-saving tools** — Output filtering, file change tracking, cached results, compressed context

All features are exposed as MCP tools on `GlassServer` in `crates/glass_mcp/src/tools.rs`, following the existing `#[tool]` macro pattern with `Parameters<T>` structs.

---

## Architecture Challenge

The MCP server runs in a separate tokio task (spawned via `glass mcp serve` or per-session), but it currently only accesses SQLite databases (history, snapshots, coordination). It has **no access** to live session state — the `SessionMux`, running PTY processes, terminal grids, or `BlockManager`.

For multi-tab orchestration and live output features, we need a communication bridge between the MCP server and the main event loop.

### Solution: MCP Command Channel

Add a bounded async channel pair to `GlassServer`:

```
MCP Tool call
    → McpRequest sent over channel to main event loop
    → main.rs processes request (has full SessionMux access)
    → McpResponse sent back over oneshot channel
    → MCP tool returns result to agent
```

```rust
// New types in glass_mcp or glass_core:
pub enum McpRequest {
    // Tab orchestration
    TabCreate { shell: Option<String>, cwd: Option<String>, reply: oneshot::Sender<McpResponse> },
    TabList { reply: oneshot::Sender<McpResponse> },
    TabClose { tab_id: usize, reply: oneshot::Sender<McpResponse> },
    TabRun { tab_id: usize, command: String, reply: oneshot::Sender<McpResponse> },
    TabOutput { tab_id: usize, lines: usize, reply: oneshot::Sender<McpResponse> },

    // Live command awareness
    CommandStatus { session_id: Option<String>, reply: oneshot::Sender<McpResponse> },
    CommandCancel { session_id: Option<String>, reply: oneshot::Sender<McpResponse> },
}

pub enum McpResponse {
    Ok(serde_json::Value),
    Error(String),
}
```

The `GlassServer` holds the sender half. `main.rs` holds the receiver and processes requests inside `user_event()` (or a new `AppEvent::McpRequest` variant), where it has full access to `SessionMux`, sessions, PTY senders, and terminal grids.

---

## Feature 1: Multi-Tab Orchestration

Lets agents manage a full dev environment — server in tab 1, tests in tab 2, watcher in tab 3.

### New MCP Tools

#### `glass_tab_create`

Create a new tab with an optional shell and working directory.

```
Params:  { name?: string, shell?: string, cwd?: string }
Returns: { tab_id: number, session_id: string }
```

Implementation:
- Send `McpRequest::TabCreate` to main event loop
- main.rs calls `create_session()` (same flow as Ctrl+Shift+T) with optional shell/cwd overrides
- Calls `session_mux.add_tab(session)` and returns the new tab ID

#### `glass_tab_list`

List all open tabs with their state.

```
Params:  {}
Returns: { tabs: [{ tab_id, name, session_id, cwd, is_active, has_running_command }] }
```

Implementation:
- Send `McpRequest::TabList` to main event loop
- Iterate `session_mux.tabs`, read each session's block_manager for running command state
- Read CWD from the session's last known directory

#### `glass_tab_run`

Send a command string to a specific tab's PTY. Does not wait for completion.

```
Params:  { tab_id: number, command: string }
Returns: { ok: bool }
```

Implementation:
- Send `McpRequest::TabRun` to main event loop
- Resolve tab_id to session_id via `session_mux`
- Write command bytes + newline to `session.pty_sender.send(PtyMsg::Input(...))`
- Return immediately (non-blocking)

#### `glass_tab_output`

Read the last N lines of visible output from a tab's terminal grid.

```
Params:  { tab_id: number, lines?: number (default 50), pattern?: string }
Returns: { output: string, total_lines: number, has_running_command: bool }
```

Implementation:
- Send `McpRequest::TabOutput` to main event loop
- Lock `session.term` (FairMutex), read grid content from scrollback + visible area
- If `pattern` provided, filter lines to only those matching (regex)
- Strip ANSI escape sequences before returning
- Include whether a command is currently executing (from block_manager state)

#### `glass_tab_close`

Close a tab and its PTY process.

```
Params:  { tab_id: number }
Returns: { ok: bool }
```

Implementation:
- Send `McpRequest::TabClose` to main event loop
- Same flow as Ctrl+Shift+W: drop session, remove tab from mux
- Refuse to close the last remaining tab (return error)

### Data Flow

```
Agent calls glass_tab_run({ tab_id: 2, command: "cargo test" })
    → GlassServer sends McpRequest::TabRun over channel
    → main.rs receives in user_event(AppEvent::Mcp(request))
    → Resolves tab 2 → session_id
    → session.pty_sender.send(PtyMsg::Input("cargo test\n"))
    → Sends McpResponse::Ok back via oneshot
    → Agent gets { ok: true }

Agent calls glass_tab_output({ tab_id: 2, lines: 20 })
    → GlassServer sends McpRequest::TabOutput over channel
    → main.rs locks session.term, reads last 20 grid lines
    → Strips ANSI, sends back as McpResponse::Ok
    → Agent gets { output: "...", has_running_command: true }
```

### Key Considerations

- **Tab naming**: Agents need stable identifiers. Use numeric tab_id (index in mux tab list). If tabs are reordered, IDs shift — consider using session_id as stable reference instead.
- **Security**: `glass_tab_run` executes arbitrary commands. No different from the agent typing in a terminal, but worth noting.
- **Output size**: `glass_tab_output` should cap returned content (e.g., 100KB max) to avoid blowing up MCP message size.
- **Resize handling**: New tabs created via MCP should inherit the window's current size. main.rs already handles this in `resize_all_panes()`.

---

## Feature 2: Structured Error Extraction

Parse compiler and test output into structured `{file, line, column, message, severity}` records so agents skip raw output parsing.

### New MCP Tool

#### `glass_errors`

Extract structured errors/warnings from a command's output.

```
Params:  { command_id?: number, tab_id?: number, format?: string }
Returns: {
    errors: [{
        file: string,
        line: number,
        column?: number,
        message: string,
        severity: "error" | "warning" | "note",
        source_line?: string
    }],
    summary: { errors: number, warnings: number },
    raw_snippet?: string  // first few lines of unparsed output for context
}
```

If `command_id` is provided, parse from stored history output. If `tab_id` is provided, parse from the last completed command's output in that tab. If neither, use the most recent command.

### New Crate: `glass_errors`

A pure library crate for error format parsing. No async, no IO — just `&str` in, `Vec<ParsedError>` out.

```
crates/glass_errors/
    src/
        lib.rs          - Public API: parse(output, hint) -> Vec<ParsedError>
        rust.rs         - Rust/cargo error parser
        python.rs       - Python traceback parser
        node.rs         - Node.js/TypeScript error parser
        go.rs           - Go compiler error parser
        gcc.rs          - GCC/Clang error parser
        generic.rs      - Generic "file:line: message" fallback
```

#### Parser Architecture

```rust
pub struct ParsedError {
    pub file: String,
    pub line: u32,
    pub column: Option<u32>,
    pub message: String,
    pub severity: Severity,
    pub source_line: Option<String>,
}

pub enum Severity { Error, Warning, Note }

/// Parse command output into structured errors.
/// `hint` can be the command text (e.g., "cargo build") to select the right parser.
pub fn parse(output: &str, hint: Option<&str>) -> Vec<ParsedError> {
    // 1. Try to detect format from hint (command name)
    // 2. Fall back to content-based detection
    // 3. Apply matched parser
    // 4. Deduplicate
}
```

#### Parser Detection Strategy

| Hint / Pattern | Parser |
|---|---|
| Command starts with `cargo`, `rustc` | `rust.rs` |
| Command starts with `python`, `pytest`, `pip` | `python.rs` |
| Command starts with `node`, `npm`, `npx`, `tsx`, `tsc` | `node.rs` |
| Command starts with `go build`, `go test`, `go vet` | `go.rs` |
| Command starts with `gcc`, `g++`, `clang`, `make` | `gcc.rs` |
| Output contains `error[E` | `rust.rs` |
| Output contains `Traceback (most recent call last)` | `python.rs` |
| Output contains `SyntaxError:` or `TypeError:` with `.js`/`.ts` paths | `node.rs` |
| Fallback: lines matching `filepath:line:col: message` | `generic.rs` |

#### Rust Parser Detail (most complex)

Cargo/rustc output format:
```
error[E0308]: mismatched types
 --> src/main.rs:42:5
  |
42 |     let x: i32 = "hello";
  |                   ^^^^^^^ expected `i32`, found `&str`
```

Parse strategy:
1. Match `^(error|warning)\[E\d+\]: (.+)$` for severity + message
2. Match `^\s*--> (.+):(\d+):(\d+)$` for file, line, column
3. Capture subsequent `|` lines as source context
4. Also handle `warning:` lines from clippy (same format)

Cargo test failures:
```
failures:
    tests::my_test

test result: FAILED. 5 passed; 1 failed;
```
Parse `---- test_name stdout ----` blocks for assertion details.

#### Python Parser Detail

```
Traceback (most recent call last):
  File "app.py", line 15, in main
    result = process(data)
  File "lib.py", line 42, in process
    return data["missing_key"]
KeyError: 'missing_key'
```

Parse strategy:
1. Detect `Traceback (most recent call last):`
2. Match `File "(.+)", line (\d+), in (.+)` for stack frames
3. Capture final exception line as message
4. Deepest frame = primary error location

#### Generic Fallback Parser

Match common patterns:
- `file:line:col: severity: message` (GCC-style)
- `file:line: message` (minimal)
- `file(line,col): severity message` (MSVC-style)

### Integration with MCP

In `glass_mcp/src/tools.rs`:

```rust
#[tool(description = "Extract structured errors from command output.")]
async fn glass_errors(
    &self,
    Parameters(params): Parameters<ErrorsParams>,
) -> Result<CallToolResult, McpError> {
    // 1. Get output text:
    //    - From history DB (command_id) OR
    //    - From live grid (tab_id via MCP channel) OR
    //    - Most recent command from history
    // 2. Get command text (for parser hint)
    // 3. Call glass_errors::parse(output, Some(command_text))
    // 4. Format as JSON response
}
```

---

## Feature 3: Token-Saving Tools

### Tool 3a: `glass_output`

Filtered access to command output. Replaces reading raw output dumps.

```
Params: {
    command_id?: number,   // specific command (from history)
    tab_id?: number,       // latest output from tab (live grid)
    lines?: number,        // last N lines only (default: all)
    pattern?: string,      // regex filter — return only matching lines
    head?: number,         // first N lines only
    context?: number       // lines of context around pattern matches
}
Returns: {
    output: string,
    matched_lines: number,
    total_lines: number,
    truncated: bool
}
```

**Token savings:** Agent asks for `pattern: "FAIL|error"` and gets 5 lines instead of 400. Typical 80-95% reduction for build/test output.

Implementation:
- If `command_id`: query `output` column from `commands` table in HistoryDb
- If `tab_id`: read grid via MCP channel (same as `glass_tab_output`)
- Apply `lines` (tail), `head`, and `pattern` filters
- `context` adds surrounding lines around matches (like `grep -C`)
- Cap total response at 100KB

### Tool 3b: `glass_changed_files`

Show which files a command modified and their diffs.

```
Params: {
    command_id?: number,    // specific command (default: last file-modifying command)
    diff?: bool             // include unified diffs (default: true)
}
Returns: {
    command: string,
    files: [{
        path: string,
        action: "modified" | "created" | "deleted",
        diff?: string       // unified diff if diff=true
    }]
}
```

**Token savings:** Agent gets a 20-line diff instead of re-reading 3 entire files (hundreds of lines). Eliminates the "read file to see what changed" pattern.

Implementation:
- Query `snapshot_files` table for the given command_id
- For each file:
  - `blob_hash` NULL + file exists now → `created`
  - `blob_hash` exists + file gone → `deleted`
  - `blob_hash` exists + file exists → `modified`
- For `modified`: read blob content from `BlobStore`, read current file, generate unified diff (use `similar` crate)
- For `created`/`deleted`: optionally include content snippet

### Tool 3c: `glass_cached_result`

Return the output of a previous command if nothing has changed, so the agent skips re-running it.

```
Params: {
    command: string,        // command text to look up (fuzzy match)
    max_age_seconds?: number // how old is acceptable (default: 300)
}
Returns: {
    hit: bool,
    command_id?: number,
    command?: string,       // exact command that matched
    exit_code?: number,
    output?: string,
    ran_at?: string,        // ISO timestamp
    age_seconds?: number,
    files_changed_since?: number  // files modified in CWD since this command ran
}
```

**Token savings:** After a context reset, agent calls `glass_cached_result({ command: "cargo test" })` and gets the output from 2 minutes ago instead of re-running a 4-minute test suite. Saves both tokens and wall-clock time.

Implementation:
- Search `commands` table for most recent match (LIKE or FTS) within `max_age_seconds`
- If found, check if any snapshots in the same CWD have been created after the command's `finished_at` timestamp — this indicates files changed
- Return `files_changed_since` count so the agent can decide if the cached result is still valid
- If `files_changed_since > 0`, agent knows to re-run; if 0, cached result is reliable

### Tool 3d: `glass_context` Enhancement

Enhance the existing `glass_context` tool with a `budget` parameter for compressed context.

```
New Params (added to existing):
    budget?: number,        // target token count (approximate)
    focus?: string          // "errors" | "recent" | "files" | "all"
```

**Behavior with `budget`:**
1. Prioritize information by importance:
   - Failed commands (highest) — include command + error summary
   - File modifications — which files changed
   - Recent commands (last 5) — command text + exit code, no output
   - Successful commands — just count ("47 commands succeeded")
2. Trim to fit budget (1 token ≈ 4 chars)
3. Include a `"truncated": true` flag if content was cut

**`focus` modes:**
- `errors`: Only failed commands with their output
- `recent`: Last N commands with exit codes, no output
- `files`: Files modified in session with latest diffs
- `all`: Default balanced mix

**Token savings:** `glass_context --budget 500` returns a 500-token summary instead of a 5000-token raw dump. 90% reduction for context restoration.

---

## Implementation Order

### Phase 1: Foundation (MCP Command Channel)

**Files:** `glass_core/event.rs`, `glass_mcp/src/tools.rs`, `src/main.rs`

1. Define `McpRequest` / `McpResponse` enums in `glass_core`
2. Add `tokio::sync::mpsc` channel to `GlassServer`
3. Add `AppEvent::Mcp(McpRequest)` variant
4. Wire receiver in `main.rs` `user_event()` handler
5. Implement request routing to `SessionMux` methods

This unblocks all features that need live session access.

### Phase 2: Multi-Tab Orchestration

**Files:** `glass_mcp/src/tools.rs`, `src/main.rs`

1. `glass_tab_list` — read-only, simplest to implement
2. `glass_tab_output` — read grid content, validates the channel works
3. `glass_tab_create` — reuse `create_session()` flow
4. `glass_tab_run` — write to PTY sender
5. `glass_tab_close` — cleanup session

### Phase 3: Token-Saving Tools (DB-only)

**Files:** `glass_mcp/src/tools.rs`, `glass_history/src/db.rs`

These don't need the MCP channel — they read from SQLite:

1. `glass_output` — filtered output from history
2. `glass_cached_result` — query + staleness check
3. `glass_changed_files` — snapshot DB + diff generation (add `similar` crate)
4. `glass_context` budget enhancement — modify existing tool

### Phase 4: Structured Error Extraction

**Files:** new `crates/glass_errors/`, `glass_mcp/src/tools.rs`

1. Create `glass_errors` crate with `ParsedError` types
2. Implement Rust parser (most relevant for this project)
3. Implement generic fallback parser
4. Implement Python, Node, Go, GCC parsers
5. Wire `glass_errors` MCP tool
6. Add tests with real compiler output samples

### Phase 5: Live Command Awareness (Stretch)

**Files:** `glass_mcp/src/tools.rs`, `src/main.rs`

1. `glass_command_status` — check block_manager for executing state
2. `glass_command_cancel` — send SIGINT via PTY

---

## New Dependencies

| Crate | Purpose | Used in |
|---|---|---|
| `similar` | Unified diff generation | `glass_changed_files` |
| `regex` | Error pattern matching, output filtering | `glass_errors`, `glass_output` |

Both are well-maintained, widely-used crates with no heavy dependency trees.

## Testing Strategy

- **Unit tests** in each parser module with real compiler output fixtures
- **Integration tests** for MCP tools using the existing `tests/mcp_integration.rs` pattern
- **Channel tests** for McpRequest/McpResponse round-trip
- Parser tests should cover: single error, multiple errors, warnings mixed with errors, no errors (clean output), malformed output

---

## Summary

| Tool | Category | Token Savings | Needs Channel? |
|---|---|---|---|
| `glass_tab_create` | Orchestration | — | Yes |
| `glass_tab_list` | Orchestration | — | Yes |
| `glass_tab_run` | Orchestration | — | Yes |
| `glass_tab_output` | Orchestration | High (filtered grid read) | Yes |
| `glass_tab_close` | Orchestration | — | Yes |
| `glass_errors` | Error extraction | Very high (structured vs raw) | Optional |
| `glass_output` | Token saving | Very high (filtered output) | Optional |
| `glass_changed_files` | Token saving | High (diff vs full file read) | No |
| `glass_cached_result` | Token saving | Very high (skip re-execution) | No |
| `glass_context` budget | Token saving | High (compressed summary) | No |
| `glass_command_status` | Live awareness | Medium | Yes |
| `glass_command_cancel` | Live awareness | Medium | Yes |

Total: 12 new MCP tools across 4 phases, plus 1 enhanced existing tool.
