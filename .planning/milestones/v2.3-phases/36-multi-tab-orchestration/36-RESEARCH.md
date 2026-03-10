# Phase 36: Multi-Tab Orchestration - Research

**Researched:** 2026-03-10
**Domain:** MCP tool handlers for tab lifecycle management via IPC to the GUI event loop
**Confidence:** HIGH

## Summary

Phase 36 adds five MCP tools (`glass_tab_create`, `glass_tab_list`, `glass_tab_send`, `glass_tab_output`, `glass_tab_close`) that let AI agents orchestrate multiple terminal tabs as parallel workspaces. The IPC infrastructure built in Phase 35 is the transport: each tool sends a JSON-line request through the `IpcClient` to the GUI's event loop, which has direct access to `SessionMux` and all `Session` state.

The architecture is straightforward because all the building blocks exist. `SessionMux` already has `add_tab()`, `close_tab()`, `tabs()`, and per-session accessors. `Session` has `pty_sender` for writing commands and `term` (via `FairMutex<Term>`) for reading terminal grid content. `BlockManager` tracks `BlockState::Executing` for running-command detection. The `create_session()` function in `main.rs` handles all PTY spawning, shell integration, history DB, and snapshot store setup. The only new code needed is: (1) MCP tool parameter types and handlers in `glass_mcp/tools.rs`, (2) IPC method dispatch in `main.rs` `AppEvent::McpRequest` handler, and (3) a helper to extract text lines from the `Term` grid.

A key design question is TAB-06: tools must accept both numeric tab index and stable session ID. Since `TabId` and `SessionId` are both u64-based, the tools should accept an identifier parameter that tries both: first as a tab index (0-based), then as a session ID. Alternatively, accept `tab_index` and `session_id` as separate optional fields where exactly one must be provided. The latter is cleaner for JSON schema and avoids ambiguity.

**Primary recommendation:** Add five `#[tool]` handlers in `tools.rs` that delegate to IPC methods dispatched in `main.rs`. Each IPC method handler accesses `SessionMux` synchronously (fast, in-memory). Reading terminal output requires locking `FairMutex<Term>` briefly to extract text lines. Creating tabs requires calling `create_session()` which spawns a PTY. No new crate dependencies needed.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| TAB-01 | Agent can create a new tab with optional shell and working directory via MCP | `create_session()` in main.rs already handles full session setup (PTY, shell integration, history, snapshots); `SessionMux::add_tab()` inserts and activates |
| TAB-02 | Agent can list all open tabs with their state (name, cwd, running command) via MCP | `SessionMux::tabs()` returns `&[Tab]` with title; `Session::status.cwd` has working directory; `BlockManager` current block state = Executing means command is running |
| TAB-03 | Agent can send a command to a specific tab's PTY via MCP | `Session::pty_sender.send(PtyMsg::Input(...))` writes bytes to PTY stdin; append `\r` for Enter |
| TAB-04 | Agent can read last N lines from a specific tab with optional regex filtering via MCP | Lock `Session::term` (FairMutex), iterate grid rows from bottom, extract text; apply regex filter with `regex` crate |
| TAB-05 | Agent can close a tab via MCP (refuses to close last tab) | `SessionMux::tab_count()` check + `SessionMux::close_tab()` removes tab and all sessions; must send `PtyMsg::Shutdown` to each session's PTY first |
| TAB-06 | Tab tools accept both numeric tab_id and stable session_id as identifiers | Use two optional params (`tab_index: Option<u64>`, `session_id: Option<u64>`) where exactly one must be provided; resolve to tab index in the handler |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio | 1.50.0 (workspace) | IPC client async I/O | Already in use |
| serde_json | 1.0 (workspace) | JSON serialization for IPC + MCP tool results | Already in use |
| serde | workspace | Derive Serialize/Deserialize | Already in use |
| rmcp | workspace | MCP tool macros (`#[tool]`, `#[tool_handler]`) | Already in use |
| schemars | workspace | JSON Schema generation for MCP tool params | Already in use |
| regex | 1.x | Regex filtering for `glass_tab_output` (TAB-04) | Already in workspace (used by glass_snapshot) |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| alacritty_terminal | =0.25.1 | `Term` grid access for reading terminal content | Already pinned; `term.grid()` for row iteration |
| glass_terminal::PtySender | N/A | Writing commands to PTY via `PtyMsg::Input` | Already in `Session` |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Reading grid rows directly | Using OutputCapture buffer | OutputCapture only captures between CommandExecuted/Finished; grid has full scrollback |
| regex crate for filtering | Simple string contains | Regex is more powerful, already a dependency, and matches the requirement spec |
| Two optional params (tab_index/session_id) | Single polymorphic `target` string | Separate fields give clear JSON schema; polymorphic string requires parsing heuristics |

**Installation:**
```bash
# No new dependencies needed -- all libraries already in workspace
# regex may need adding to glass_mcp/Cargo.toml if not already there
```

## Architecture Patterns

### Recommended Project Structure
```
crates/glass_mcp/src/
    tools.rs            # Add 5 new #[tool] handlers + param types
    ipc_client.rs       # Already exists, no changes needed

crates/glass_core/src/
    ipc.rs              # No changes needed (McpRequest/McpResponse already generic)

src/main.rs             # Expand AppEvent::McpRequest match to handle 5 new IPC methods
                        # Add helper functions for tab operations
```

### Pattern 1: Tab Identifier Resolution
**What:** Every tab tool accepts either `tab_index` or `session_id`. The handler resolves either to the concrete tab for operations.
**When to use:** All five tab tools.
**Example:**
```rust
// In main.rs, helper function
fn resolve_tab<'a>(
    mux: &'a SessionMux,
    tab_index: Option<u64>,
    session_id: Option<u64>,
) -> Result<(usize, &'a Tab), String> {
    match (tab_index, session_id) {
        (Some(idx), None) => {
            let idx = idx as usize;
            mux.tabs().get(idx)
                .map(|t| (idx, t))
                .ok_or_else(|| format!("Tab index {} out of range (0..{})", idx, mux.tab_count()))
        }
        (None, Some(sid)) => {
            let target = SessionId::new(sid);
            mux.tabs().iter().enumerate()
                .find(|(_, tab)| tab.session_ids().contains(&target))
                .map(|(i, t)| (i, t))
                .ok_or_else(|| format!("No tab contains session {}", sid))
        }
        (Some(_), Some(_)) => Err("Provide either tab_index or session_id, not both".into()),
        (None, None) => Err("Provide tab_index or session_id".into()),
    }
}
```

### Pattern 2: MCP Tool -> IPC -> Event Loop -> Response
**What:** Each MCP tool handler sends a JSON request via `IpcClient`, the GUI event loop processes it synchronously, and returns a JSON response via oneshot.
**When to use:** All tab tools (same pattern as `glass_ping`).
**Example:**
```rust
// In tools.rs
#[tool(description = "List all open tabs with their state")]
async fn glass_tab_list(&self) -> Result<CallToolResult, McpError> {
    let client = match self.ipc_client.as_ref() {
        Some(c) => c,
        None => return Ok(CallToolResult::error(vec![Content::text(
            "Glass GUI is not running. Tab tools require a running Glass window."
        )])),
    };
    match client.send_request("tab_list", serde_json::json!({})).await {
        Ok(resp) => Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&resp).unwrap_or_default()
        )])),
        Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
            "Failed to communicate with Glass GUI: {}", e
        ))])),
    }
}
```

### Pattern 3: Reading Terminal Grid as Text Lines
**What:** Lock the `FairMutex<Term>`, iterate grid rows, extract character content as strings.
**When to use:** `tab_output` IPC handler in main.rs.
**Example:**
```rust
// In main.rs, helper for extracting text from terminal grid
fn extract_term_lines(
    term: &Arc<FairMutex<Term<EventProxy>>>,
    last_n: usize,
) -> Vec<String> {
    let term = term.lock();
    let grid = term.grid();
    let total_lines = grid.screen_lines();
    let history_len = grid.history_size();

    let mut lines = Vec::new();
    // Iterate visible lines + scrollback
    for line_idx in 0..total_lines {
        let row = &grid[alacritty_terminal::index::Line(line_idx as i32)];
        let text: String = (0..grid.columns())
            .map(|col| row[alacritty_terminal::index::Column(col)].c)
            .collect::<String>()
            .trim_end()
            .to_string();
        lines.push(text);
    }

    // Return last N lines (trim empty trailing lines)
    while lines.last().map_or(false, |l| l.is_empty()) {
        lines.pop();
    }
    let start = lines.len().saturating_sub(last_n);
    lines[start..].to_vec()
}
```

### Pattern 4: Creating a Tab from IPC (Requires Window Context)
**What:** Tab creation requires access to `WindowContext` (renderer dimensions, event loop proxy, window ID) plus config. The IPC handler in `user_event()` has access to all of these via `self`.
**When to use:** `tab_create` IPC handler.
**Example:**
```rust
// In main.rs McpRequest handler
"tab_create" => {
    // Extract params
    let shell = request.params.get("shell").and_then(|v| v.as_str());
    let cwd = request.params.get("cwd").and_then(|v| v.as_str());

    // Use the first window's context (single-window app)
    if let Some(ctx) = self.windows.values().first() {
        let session_id = ctx.session_mux.next_session_id();
        // create_session needs cell dimensions from renderer
        let session = create_session(
            &self.proxy, window_id, session_id, &self.config,
            cwd.map(std::path::Path::new),
            cell_w, cell_h, width, height, tab_bar_lines,
        );
        let tab_id = ctx.session_mux.add_tab(session);
        McpResponse::ok(request.id, json!({
            "tab_index": ctx.session_mux.active_tab_index(),
            "session_id": session_id.val(),
            "tab_id": tab_id.val(),
        }))
    } else {
        McpResponse::err(request.id, "No window available".into())
    }
}
```

### Anti-Patterns to Avoid
- **Sending PtyMsg::Input without \r:** Agents send command text but forget the carriage return. The tool handler MUST append `\r` (or `\n` on Unix) to actually execute the command.
- **Blocking on PTY output after sending a command:** `tab_send` should fire-and-forget. The agent uses `tab_output` separately to poll for results. Trying to wait for output would block the event loop.
- **Reading grid content outside the FairMutex lock:** The `Term` grid must be accessed under lock. Copy all needed data, release the lock, then serialize.
- **Using tab_index as a stable identifier:** Tab indices shift when tabs are closed. Session IDs are stable for the lifetime of a session. The tool documentation should recommend session_id for long-lived workflows.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Tab identifier resolution | Custom lookup per tool | Shared `resolve_tab()` helper | DRY; all 5 tools need the same logic |
| Terminal text extraction | Custom VTE parser | `Term::grid()` row iteration + cell.c | alacritty_terminal already parses VTE; grid is the rendered result |
| Regex filtering | Custom pattern matching | `regex::Regex` | Standard, fast, already a dependency |
| PTY command execution | Raw byte writing + newline handling | `PtyMsg::Input(Cow::Owned(format!("{}\r", cmd).into_bytes()))` | PtySender handles write + poller wakeup |
| Session lifecycle (PTY, shell integration, history, snapshots) | Manual setup code | `create_session()` in main.rs | Already encapsulates all setup; reuse it |

**Key insight:** Phase 36 is glue code. All the hard work (PTY management, terminal grid, session lifecycle, IPC transport) is already implemented. The new code is parameter types, dispatch, and thin handler logic.

## Common Pitfalls

### Pitfall 1: SessionMux Requires Mutable Borrow for Tab Creation/Closure
**What goes wrong:** The `user_event()` handler needs `&mut self` to modify `SessionMux` (add/close tabs), but may also need shared references to other fields.
**Why it happens:** Rust borrow checker prevents simultaneous mutable and immutable borrows.
**How to avoid:** Extract needed immutable data (cell dimensions, window_id, config) into local variables before taking `&mut session_mux`.
**Warning signs:** Compilation errors about "cannot borrow `self` as mutable because it is also borrowed as immutable".

### Pitfall 2: Tab Close Requires PTY Shutdown
**What goes wrong:** Closing a tab via `SessionMux::close_tab()` removes the session from the HashMap, but the PTY thread continues running, leaking OS resources.
**Why it happens:** `close_tab()` only manages SessionMux state; PTY shutdown is separate.
**How to avoid:** Before calling `close_tab()`, iterate the tab's `session_ids()`, get each session's `pty_sender`, and send `PtyMsg::Shutdown`. The existing keyboard Ctrl+Shift+W handler already does this -- follow that pattern.
**Warning signs:** Orphaned PTY processes, leaked ConPTY handles on Windows.

### Pitfall 3: Grid Row Indexing in alacritty_terminal
**What goes wrong:** Confusing `Line` index semantics -- `Line(0)` is the first visible line (top of screen), negative lines go into scrollback, positive lines go down.
**Why it happens:** alacritty_terminal uses signed line indices for scrollback.
**How to avoid:** Use `grid.display_iter()` or iterate `Line(0)..Line(screen_lines)` for visible content. For scrollback, use negative indices. Test with real terminal output.
**Warning signs:** Empty or garbled output from `tab_output`.

### Pitfall 4: CWD Not Always Available
**What goes wrong:** `Session::status.cwd` may be `None` if the shell hasn't sent an OSC 7 / OSC 9;9 sequence yet (e.g., shell integration not loaded).
**Why it happens:** CWD is reported asynchronously via shell integration.
**How to avoid:** Return `null` for cwd in the tab list when unavailable. Don't treat it as an error.
**Warning signs:** Tab list shows null cwd for newly created tabs.

### Pitfall 5: Regex Compilation on Every Call
**What goes wrong:** Compiling a regex for each `tab_output` call with a pattern is wasteful but probably fine for typical usage.
**Why it happens:** MCP tools are called intermittently, not in tight loops.
**How to avoid:** Compile the regex once per call. If the pattern is invalid, return a clear error message rather than panicking.
**Warning signs:** Not a real concern for MCP tool latency.

### Pitfall 6: Window Context Access in McpRequest Handler
**What goes wrong:** The McpRequest handler needs to find the right WindowContext. Currently Glass has a single window, but `self.windows` is a HashMap.
**Why it happens:** Multi-window support exists in data structures but only one window is created.
**How to avoid:** Use `self.windows.values_mut().next()` to get the (single) window context. If no windows exist, return an error.
**Warning signs:** `None` from `windows.values().next()` when the window hasn't been created yet.

## Code Examples

### MCP Tool Parameter Types
```rust
// In tools.rs -- parameter types for tab tools

/// Common tab targeting parameters.
/// Provide exactly one of tab_index or session_id.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TabTarget {
    /// 0-based tab index.
    #[schemars(description = "0-based tab index")]
    pub tab_index: Option<u64>,
    /// Stable session ID (from glass_tab_create or glass_tab_list).
    #[schemars(description = "Stable session ID")]
    pub session_id: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TabCreateParams {
    /// Shell to use (e.g. 'bash', 'pwsh'). Uses default if omitted.
    #[schemars(description = "Shell to use. Uses default if omitted")]
    pub shell: Option<String>,
    /// Working directory for the new tab.
    #[schemars(description = "Working directory for the new tab")]
    pub cwd: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TabSendParams {
    #[serde(flatten)]
    pub target: TabTarget,
    /// Command string to send to the tab's PTY.
    #[schemars(description = "Command string to send (Enter is appended automatically)")]
    pub command: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TabOutputParams {
    #[serde(flatten)]
    pub target: TabTarget,
    /// Number of lines to return from the end of output.
    #[schemars(description = "Number of lines to return from the end (default 50)")]
    pub lines: Option<usize>,
    /// Regex pattern to filter lines.
    #[schemars(description = "Regex pattern to filter output lines")]
    pub pattern: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TabCloseParams {
    #[serde(flatten)]
    pub target: TabTarget,
}
```

### IPC Method Dispatch in main.rs
```rust
// Extending the existing McpRequest match in user_event()
AppEvent::McpRequest(mcp_req) => {
    let McpEventRequest { request, reply } = mcp_req;
    let response = match request.method.as_str() {
        "ping" => McpResponse::ok(request.id, ping_result()),
        "tab_list" => self.handle_tab_list(&request),
        "tab_create" => self.handle_tab_create(&request),
        "tab_send" => self.handle_tab_send(&request),
        "tab_output" => self.handle_tab_output(&request),
        "tab_close" => self.handle_tab_close(&request),
        _ => McpResponse::err(request.id, format!("Unknown method: {}", request.method)),
    };
    let _ = reply.send(response);
}
```

### Tab List Response Shape
```json
{
    "tabs": [
        {
            "index": 0,
            "title": "~/projects/glass",
            "session_id": 0,
            "cwd": "/home/user/projects/glass",
            "is_active": true,
            "has_running_command": false,
            "pane_count": 1
        },
        {
            "index": 1,
            "title": "~/projects/other",
            "session_id": 3,
            "cwd": "/home/user/projects/other",
            "is_active": false,
            "has_running_command": true,
            "pane_count": 1
        }
    ]
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| MCP only queries SQLite databases | MCP can query live GUI state via IPC (Phase 35) | Phase 35 | Enables live tab tools |
| Tabs managed only by keyboard shortcuts | Tabs manageable programmatically via MCP | Phase 36 (now) | Agents can create parallel workspaces |

**Existing patterns leveraged:**
- `glass_ping` tool handler in `tools.rs` -- exact pattern for IPC tools
- `AppEvent::McpRequest` dispatch in `main.rs` -- extend the match
- `create_session()` -- reuse for tab creation
- Ctrl+Shift+T/W keyboard handlers -- reference for tab create/close cleanup

## Open Questions

1. **Serde flatten for TabTarget in MCP tool params**
   - What we know: rmcp uses schemars for JSON schema generation. `#[serde(flatten)]` should work to embed `TabTarget` fields into parent param types.
   - What's unclear: Whether rmcp/schemars handles flatten correctly for tool parameter schema generation.
   - Recommendation: If flatten doesn't work with schemars, inline `tab_index` and `session_id` directly into each param struct. Test compilation early.

2. **Scrollback depth for tab_output**
   - What we know: `Term::grid()` holds visible lines + scrollback history. History size is configurable in alacritty_terminal.
   - What's unclear: Default scrollback size and whether requesting very large `lines` values causes performance issues.
   - Recommendation: Cap at a reasonable maximum (e.g., 10000 lines). Document the cap in the tool description.

3. **Tab creation cell dimensions**
   - What we know: `create_session()` needs cell_w, cell_h, window width, window height, and tab_bar_lines. These are properties of the renderer/window.
   - What's unclear: Whether these are easily accessible in the McpRequest handler.
   - Recommendation: They should be stored in or computable from `WindowContext` fields (renderer has font metrics, window has size). Extract them before creating the session.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` + `#[cfg(test)]` modules |
| Config file | None (Cargo.toml test configuration) |
| Quick run command | `cargo test -p glass_mcp tab --no-fail-fast` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| TAB-01 | tab_create sends IPC request with shell/cwd params | unit | `cargo test -p glass_mcp tab_create` | No - Wave 0 |
| TAB-02 | tab_list returns structured tab info | unit | `cargo test -p glass_mcp tab_list` | No - Wave 0 |
| TAB-03 | tab_send forwards command text via IPC | unit | `cargo test -p glass_mcp tab_send` | No - Wave 0 |
| TAB-04 | tab_output returns lines with optional regex filter | unit | `cargo test -p glass_mcp tab_output` | No - Wave 0 |
| TAB-05 | tab_close refuses to close last tab | unit | `cargo test -p glass_mcp tab_close_last` | No - Wave 0 |
| TAB-06 | Tools accept tab_index and session_id | unit | `cargo test -p glass_mcp tab_target` | No - Wave 0 |
| TAB-06 | Providing both tab_index and session_id returns error | unit | `cargo test -p glass_mcp tab_target_both` | No - Wave 0 |
| TAB-06 | Providing neither tab_index nor session_id returns error | unit | `cargo test -p glass_mcp tab_target_neither` | No - Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_mcp -p glass_core --no-fail-fast`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] Tab tool parameter struct serde/schemars tests (verify JSON deserialization)
- [ ] TabTarget resolution unit tests (index lookup, session_id lookup, error cases)
- [ ] IPC method round-trip tests (extend existing TCP-based test pattern from ipc.rs)
- [ ] Regex compilation error handling test for tab_output

## Sources

### Primary (HIGH confidence)
- Project codebase: `crates/glass_mux/src/session_mux.rs` -- SessionMux API (add_tab, close_tab, tabs, session accessors)
- Project codebase: `crates/glass_mux/src/session.rs` -- Session fields (pty_sender, term, status, block_manager)
- Project codebase: `crates/glass_mux/src/tab.rs` -- Tab struct (id, title, session_ids, pane_count)
- Project codebase: `crates/glass_mux/src/types.rs` -- TabId, SessionId types
- Project codebase: `crates/glass_mcp/src/tools.rs` -- Existing tool handler pattern (glass_ping as template)
- Project codebase: `crates/glass_mcp/src/ipc_client.rs` -- IPC client send_request pattern
- Project codebase: `crates/glass_core/src/ipc.rs` -- IPC listener, McpRequest/McpResponse types
- Project codebase: `src/main.rs` -- create_session(), AppEvent::McpRequest handler, WindowContext
- Project codebase: `crates/glass_terminal/src/pty.rs` -- PtySender, PtyMsg::Input/Shutdown
- Project codebase: `crates/glass_terminal/src/block_manager.rs` -- BlockState::Executing for running-command detection

### Secondary (MEDIUM confidence)
- alacritty_terminal grid API -- row/cell access patterns (training knowledge, verified by grid_snapshot.rs usage)

### Tertiary (LOW confidence)
- schemars `#[serde(flatten)]` support -- needs compilation verification

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - no new dependencies, all existing workspace crates
- Architecture: HIGH - follows established Phase 35 IPC pattern exactly; all SessionMux/Session APIs already exist
- Pitfalls: HIGH - well-understood domain; PTY shutdown, borrow checker, grid indexing are known issues with known solutions

**Research date:** 2026-03-10
**Valid until:** 2026-04-10 (stable domain, no fast-moving dependencies)
