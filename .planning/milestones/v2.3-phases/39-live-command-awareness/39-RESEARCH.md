# Phase 39: Live Command Awareness - Research

**Researched:** 2026-03-10
**Domain:** MCP tool integration, terminal session state inspection, PTY signal delivery
**Confidence:** HIGH

## Summary

Phase 39 adds two MCP tools: `glass_has_running_command` (LIVE-01) and `glass_cancel_command` (LIVE-02). Both follow the exact same architectural pattern as the existing 10+ IPC-proxied tools (glass_tab_list, glass_tab_send, etc.). The implementation is straightforward because all the building blocks already exist in the codebase.

For LIVE-01 (running command check): The `tab_list` handler in `main.rs` already queries `block_manager.current_block_index()` and checks `BlockState::Executing`. The new tool just needs to return richer per-tab data including `started_at` elapsed time (computed from `Instant::now() - block.started_at`).

For LIVE-02 (cancel command): The `tab_send` handler already writes arbitrary bytes to the PTY via `session.pty_sender.send(PtyMsg::Input(...))`. Sending Ctrl+C is simply sending `0x03` (ETX byte) instead of a command string with `\r`. This is the standard approach used by every terminal emulator.

**Primary recommendation:** Follow the exact same pattern as glass_tab_send for both tools -- MCP tool handler in tools.rs sends IPC request, main.rs handles it in the McpRequest match arm, accessing session state through session_mux.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| LIVE-01 | Agent can check whether a command is currently running in a tab via MCP | BlockState::Executing check + Instant elapsed time calculation already exist in block_manager; IPC proxy pattern established |
| LIVE-02 | Agent can cancel a running command (send SIGINT) in a tab via MCP | PtyMsg::Input with 0x03 byte; same pattern as tab_send; no platform-specific signal handling needed since PTY handles it |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| rmcp | (existing) | MCP tool registration via `#[tool]` macros | Already used for all 26 existing tools |
| schemars | (existing) | JSON Schema generation for tool params | Already used for all param structs |
| serde/serde_json | (existing) | Serialization for IPC JSON-line protocol | Already used throughout |

### Supporting
No new dependencies needed. All functionality uses existing crate APIs.

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Sending ETX (0x03) via PTY | Platform SIGINT APIs | ETX via PTY is simpler, cross-platform, and is what terminal emulators actually do -- the PTY driver translates it to SIGINT |

**Installation:**
```bash
# No new dependencies required
```

## Architecture Patterns

### Existing Pattern: IPC-Proxied MCP Tool

Every live GUI tool follows a 3-layer pattern. The new tools MUST follow this exactly:

**Layer 1: MCP Tool (glass_mcp/src/tools.rs)**
```rust
// 1. Define params struct with schemars
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HasRunningCommandParams {
    #[schemars(description = "0-based tab index (provide this OR session_id)")]
    pub tab_index: Option<u64>,
    #[schemars(description = "Stable session ID (provide this OR tab_index)")]
    pub session_id: Option<u64>,
}

// 2. Define tool handler that delegates to IPC
#[tool(description = "Check if a command is running in a tab...")]
async fn glass_has_running_command(
    &self,
    Parameters(input): Parameters<HasRunningCommandParams>,
) -> Result<CallToolResult, McpError> {
    let client = match self.ipc_client.as_ref() { ... };
    let mut params = serde_json::json!({});
    if let Some(idx) = input.tab_index { params["tab_index"] = json!(idx); }
    if let Some(sid) = input.session_id { params["session_id"] = json!(sid); }
    match client.send_request("has_running_command", params).await { ... }
}
```

**Layer 2: IPC Handler (src/main.rs, McpRequest match arm)**
```rust
"has_running_command" => {
    // Use resolve_tab_index() to find the tab
    // Access session.block_manager to check current block state
    // Return JSON with is_running, elapsed_seconds, etc.
}
```

**Layer 3: Terminal State (glass_terminal block_manager)**
```rust
// Already exists -- no changes needed:
// block.state == BlockState::Executing
// block.started_at: Option<Instant>
// Elapsed: Instant::now().duration_since(started_at)
```

### Key Code Locations

| What | Where | Lines |
|------|-------|-------|
| Existing `tab_list` handler (already reads running state) | `src/main.rs` | ~2444-2488 |
| Existing `tab_send` handler (sends bytes to PTY) | `src/main.rs` | ~2552-2589 |
| `resolve_tab_index()` helper | `src/main.rs` | ~467-495 |
| `BlockManager` + `BlockState` | `glass_terminal/src/block_manager.rs` | Full file |
| `PtyMsg::Input` for writing to PTY | `glass_terminal/src/pty.rs` | ~63-65 |
| MCP tool registration pattern | `glass_mcp/src/tools.rs` | Full file |
| IPC request/response types | `glass_core/src/ipc.rs` | Full file |

### Anti-Patterns to Avoid
- **Don't use platform-specific signal APIs:** The PTY master file descriptor handles SIGINT translation. Sending `0x03` (ETX) to the PTY is what Ctrl+C does in every terminal emulator. Using `kill()` or `TerminateProcess()` would bypass the PTY and potentially orphan processes.
- **Don't duplicate block state logic:** The `tab_list` handler already checks `BlockState::Executing`. The new `has_running_command` handler should follow the same pattern, not re-implement it differently.
- **Don't add new IPC methods if an existing one suffices:** Consider whether `tab_list` already provides enough data for LIVE-01, but since LIVE-01 needs elapsed time (which `tab_list` doesn't currently return), a dedicated method is justified.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Tab resolution | Custom tab lookup | `resolve_tab_index()` in main.rs | Already handles tab_index, session_id, and both-provided errors |
| PTY byte writing | Direct PTY access | `session.pty_sender.send(PtyMsg::Input(...))` | Cross-platform, thread-safe, already battle-tested |
| IPC communication | Custom protocol | `IpcClient::send_request()` | JSON-line protocol already working for all tools |
| Command running detection | Custom state tracking | `block_manager.current_block_index()` + `BlockState::Executing` | Shell integration OSC 133 sequences already track this |

**Key insight:** Every building block for this phase already exists. The implementation is purely wiring -- connecting existing APIs through the established IPC proxy pattern.

## Common Pitfalls

### Pitfall 1: Sending Ctrl+C When No Command is Running
**What goes wrong:** If Ctrl+C is sent to a shell with no running command, it just prints a new prompt line. Not harmful but confusing.
**Why it happens:** Agent doesn't check running state before cancelling.
**How to avoid:** The `cancel_command` IPC handler should check `BlockState::Executing` first and return `was_running: false` if nothing was running (still send the signal -- it's harmless). Let the MCP tool response indicate whether a command was actually interrupted.
**Warning signs:** Tool returns success but command wasn't actually running.

### Pitfall 2: Elapsed Time Precision with Instant
**What goes wrong:** `Instant` cannot be serialized, and computing elapsed on the wrong thread could give stale results.
**Why it happens:** Wanting to serialize timestamps instead of computing elapsed inline.
**How to avoid:** Compute `Instant::now().duration_since(started_at)` in the main event loop handler (where block_manager is accessed) and return `elapsed_seconds` as f64 in the JSON response. Never try to serialize `Instant` or `SystemTime` across the IPC boundary.
**Warning signs:** Trying to pass `started_at` to the MCP tool.

### Pitfall 3: Race Condition Between Check and Cancel
**What goes wrong:** Agent checks `has_running_command`, gets true, but by the time it sends `cancel_command`, the command has already finished.
**Why it happens:** Time gap between the two IPC requests.
**How to avoid:** Make `cancel_command` idempotent -- it should succeed (and send 0x03) regardless of whether a command is currently running. Return `was_running` flag so the agent knows the state at cancel time.
**Warning signs:** Agent logic that branches on running state without handling the race.

### Pitfall 4: Tool Count in Module Doc Comment
**What goes wrong:** The `tools.rs` module doc comment says "twenty-six tools" -- adding two new ones means updating this to "twenty-eight".
**Why it happens:** Easy to forget the doc comment.
**How to avoid:** Update the module doc comment and the tool list at the top of tools.rs.

## Code Examples

### Checking Running State (from existing tab_list handler)
```rust
// Source: src/main.rs ~line 2454-2465
let (cwd, has_running_command) = if let Some(session) =
    ctx.session_mux.session(primary_sid)
{
    let cwd = session.status.cwd().to_string();
    let running = session
        .block_manager
        .current_block_index()
        .and_then(|idx| session.block_manager.blocks().get(idx))
        .map(|b| b.state == glass_terminal::BlockState::Executing)
        .unwrap_or(false);
    (cwd, running)
} else {
    (String::new(), false)
};
```

### Computing Elapsed Time
```rust
// Source: glass_terminal/src/block_manager.rs ~line 81-86
// Block already has started_at: Option<Instant>
// To get elapsed seconds for a running command:
let elapsed_secs = block.started_at
    .map(|start| start.elapsed().as_secs_f64());
```

### Sending Ctrl+C (ETX byte) to PTY
```rust
// Source: adapted from src/main.rs tab_send handler ~line 2563-2566
// Ctrl+C = ASCII ETX = 0x03
let input = vec![0x03u8];
let _ = session.pty_sender.send(PtyMsg::Input(Cow::Owned(input)));
```

### IPC Tool Handler Pattern (from glass_tab_send)
```rust
// Source: glass_mcp/src/tools.rs ~line 1105-1133
async fn glass_tab_send(
    &self,
    Parameters(input): Parameters<TabSendParams>,
) -> Result<CallToolResult, McpError> {
    let client = match self.ipc_client.as_ref() {
        Some(c) => c,
        None => {
            return Ok(CallToolResult::error(vec![Content::text(
                "Glass GUI is not running. Live tools require a running Glass window.",
            )]));
        }
    };
    let mut params = serde_json::json!({ "command": input.command });
    if let Some(idx) = input.tab_index {
        params["tab_index"] = serde_json::json!(idx);
    }
    if let Some(sid) = input.session_id {
        params["session_id"] = serde_json::json!(sid);
    }
    match client.send_request("tab_send", params).await {
        Ok(resp) => Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&resp).unwrap_or_default(),
        )])),
        Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
            "Failed to communicate with Glass GUI: {}", e
        ))])),
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Polling tab_list for running state | Dedicated has_running_command tool | Phase 39 | More targeted query, includes elapsed time |
| No cancel capability | Send ETX byte via MCP | Phase 39 | Agents can abort hung commands |

**Note:** `tab_list` already returns `has_running_command` boolean per tab. LIVE-01 adds elapsed time and a targeted single-tab query. An alternative approach would be to enrich `tab_list` response with elapsed time, but a dedicated tool is cleaner for the MCP API surface.

## Open Questions

1. **Should `has_running_command` be a separate tool or an enhancement to `tab_list`?**
   - What we know: `tab_list` already returns `has_running_command` per tab. Adding `elapsed_seconds` to `tab_list` would technically satisfy LIVE-01.
   - What's unclear: Whether a separate tool or enhanced `tab_list` is better UX for agents.
   - Recommendation: Create a dedicated `glass_has_running_command` tool for targeted single-tab queries (with elapsed time). This keeps `tab_list` lightweight and gives agents a clear "check this specific tab" operation. The requirement says "query whether a command is currently running in a specific tab" which implies a per-tab query, not a list-all.

2. **Should cancel_command also return the last few lines of output?**
   - What we know: After sending Ctrl+C, the command may produce a few more lines before stopping.
   - What's unclear: Whether agents benefit from immediate output post-cancel.
   - Recommendation: Keep it simple -- return `{signal_sent: true, was_running: true/false, session_id: N}`. Agent can use `tab_output` separately to check post-cancel state.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in Rust test framework) |
| Config file | Cargo.toml workspace |
| Quick run command | `cargo test -p glass_mcp --lib` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| LIVE-01 | HasRunningCommandParams deserializes correctly | unit | `cargo test -p glass_mcp --lib test_has_running_command_params` | Wave 0 |
| LIVE-01 | Tool returns error when no GUI running | unit | `cargo test -p glass_mcp --lib test_has_running_command_no_gui` | Wave 0 |
| LIVE-02 | CancelCommandParams deserializes correctly | unit | `cargo test -p glass_mcp --lib test_cancel_command_params` | Wave 0 |
| LIVE-02 | Tool returns error when no GUI running | unit | `cargo test -p glass_mcp --lib test_cancel_command_no_gui` | Wave 0 |
| LIVE-01 | BlockState::Executing detected with elapsed time | unit | `cargo test -p glass_terminal --lib test_block_elapsed` | Existing (duration tests exist) |
| LIVE-02 | ETX byte is 0x03 | unit | `cargo test -p glass_mcp --lib test_etx_byte` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_mcp --lib && cargo test -p glass_terminal --lib`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `HasRunningCommandParams` deserialization test in tools.rs
- [ ] `CancelCommandParams` deserialization test in tools.rs
- [ ] No-GUI error tests for both new tools in tools.rs
- [ ] IPC handler integration is tested by existing IPC round-trip test pattern

## Sources

### Primary (HIGH confidence)
- Direct codebase analysis of `src/main.rs` lines 2438-2706 (McpRequest handler)
- Direct codebase analysis of `glass_mcp/src/tools.rs` (all existing tool patterns)
- Direct codebase analysis of `glass_terminal/src/block_manager.rs` (BlockState, started_at, elapsed)
- Direct codebase analysis of `glass_terminal/src/pty.rs` (PtyMsg::Input for writing bytes)
- Direct codebase analysis of `glass_core/src/ipc.rs` (IPC protocol types)

### Secondary (MEDIUM confidence)
- ETX byte (0x03) as Ctrl+C equivalent is terminal standard (POSIX, ConPTY)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - no new dependencies, all existing crate APIs
- Architecture: HIGH - exact same 3-layer IPC proxy pattern used by 10+ existing tools
- Pitfalls: HIGH - straightforward implementation with well-understood edge cases

**Research date:** 2026-03-10
**Valid until:** 2026-04-10 (stable domain, no external dependencies changing)
