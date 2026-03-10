# Phase 35: MCP Command Channel - Research

**Researched:** 2026-03-09
**Domain:** Inter-process communication between MCP server process and GUI event loop
**Confidence:** HIGH

## Summary

Phase 35 establishes a communication bridge between the MCP server process (`glass mcp serve`, spawned by AI clients over stdio) and the running GUI process (winit event loop with full `SessionMux` access). Currently these are entirely separate OS processes with no shared memory -- the MCP server only accesses SQLite databases on disk.

The recommended approach is **IPC via local socket**: the GUI process listens on a well-known local socket path (`~/.glass/glass.sock` on Unix, `\\.\pipe\glass-terminal` on Windows), and the MCP process connects as a client for request/response exchanges. This requires no third-party IPC crate -- tokio provides `tokio::net::UnixListener` on Unix and `tokio::net::windows::named_pipe` on Windows, both already available through the `tokio = { features = ["full"] }` dependency.

The communication protocol is simple JSON-over-newline: the MCP process sends a JSON request line, the GUI reads it, processes it against `SessionMux`, and writes back a JSON response line. A `tokio::sync::oneshot` channel inside the GUI bridges the async IPC listener task to the synchronous winit `user_event()` handler.

**Primary recommendation:** Use tokio's built-in platform-specific IPC (UnixListener + named pipes) with `#[cfg]` abstraction, JSON-line protocol, and `AppEvent::McpRequest` variant for event loop integration. No new crate dependencies needed.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| INFRA-01 | MCP server can send requests to the main event loop and receive responses via async channel | IPC listener in GUI + JSON request/response protocol + AppEvent::McpRequest variant + oneshot reply channel |
| INFRA-02 | Main event loop processes MCP requests without blocking rendering or keyboard input | IPC listener runs in spawned tokio task, request handling in user_event() is non-blocking (same pattern as CoordinationUpdate), oneshot reply doesn't block the event loop |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| tokio | 1.50.0 (workspace) | Async IPC listener, oneshot channels | Already in use, "full" features include net + sync |
| serde_json | 1.0 (workspace) | JSON serialization for IPC protocol | Already in use for MCP tool responses |
| serde | workspace | Derive Serialize/Deserialize for request/response types | Already in use |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| winit EventLoopProxy | 0.30 (workspace) | Send AppEvent from IPC task to event loop | Already used by PTY threads, config watcher, coordination poller |
| tokio::sync::oneshot | (part of tokio) | Reply channel from event loop back to IPC task | Each MCP request gets a oneshot for its response |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Raw tokio IPC | `interprocess` crate | Unified API but adds dependency; tokio's built-ins are sufficient with ~20 lines of `#[cfg]` |
| Local socket IPC | Shared SQLite polling | Would work for some cases but adds latency (polling interval) and can't query live terminal grid state |
| Embedding MCP server in GUI process | Separate process with IPC | Embedding breaks the stdio transport model that AI clients (Claude Desktop, etc.) require |

**Installation:**
```bash
# No new dependencies needed -- all libraries already in workspace
```

## Architecture Patterns

### Recommended Project Structure
```
crates/glass_core/src/
    event.rs            # Add McpRequest enum + AppEvent::McpRequest variant
    ipc.rs              # NEW: IPC listener (platform-abstracted), request/response types
    lib.rs              # Add pub mod ipc

crates/glass_mcp/src/
    lib.rs              # run_mcp_server() gains IPC client connection for live tools
    tools.rs            # GlassServer gains Optional<IpcClient> for forwarding live requests

src/main.rs             # Spawn IPC listener task, handle AppEvent::McpRequest in user_event()
```

### Pattern 1: IPC Listener in GUI Process
**What:** The GUI spawns a tokio task that listens on a local socket. Each connection reads JSON request lines and sends them into the winit event loop via `EventLoopProxy::send_event(AppEvent::McpRequest(...))`.
**When to use:** Always -- this is the core infrastructure for Phase 35.
**Example:**
```rust
// crates/glass_core/src/ipc.rs

use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

/// Request sent from MCP process to GUI via IPC.
#[derive(Debug, Deserialize)]
pub struct McpRequest {
    pub id: u64,
    pub method: String,
    pub params: serde_json::Value,
}

/// Response sent from GUI back to MCP process via IPC.
#[derive(Debug, Serialize)]
pub struct McpResponse {
    pub id: u64,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

/// Internal message sent through the winit event loop.
/// Contains the parsed request + a oneshot sender for the response.
pub struct McpEventRequest {
    pub request: McpRequest,
    pub reply: oneshot::Sender<McpResponse>,
}
```

### Pattern 2: Platform-Abstracted Socket Path
**What:** Use `#[cfg]` to select the appropriate socket mechanism per platform.
**When to use:** All IPC listener and client code.
**Example:**
```rust
// Socket path resolution
#[cfg(unix)]
pub fn ipc_socket_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".glass")
        .join("glass.sock")
}

#[cfg(windows)]
pub fn ipc_pipe_name() -> String {
    r"\\.\pipe\glass-terminal".to_string()
}
```

### Pattern 3: Non-Blocking Event Loop Integration
**What:** The `user_event()` handler processes MCP requests synchronously (reading SessionMux state) then sends the response via the oneshot channel. No blocking, no awaiting.
**When to use:** Inside `Processor::user_event()` in main.rs.
**Example:**
```rust
// In main.rs user_event() handler:
AppEvent::McpRequest(mcp_req) => {
    let McpEventRequest { request, reply } = mcp_req;
    let response = match request.method.as_str() {
        "tab_list" => self.handle_tab_list(&request),
        "tab_output" => self.handle_tab_output(&request),
        // ... other methods
        _ => McpResponse {
            id: request.id,
            result: None,
            error: Some(format!("Unknown method: {}", request.method)),
        },
    };
    let _ = reply.send(response); // oneshot send, never blocks
}
```

### Pattern 4: Graceful Degradation When GUI Not Running
**What:** MCP tools that need live data attempt IPC connection; if it fails (no listener), return a clear error message.
**When to use:** In GlassServer tool handlers that need live session data.
**Example:**
```rust
// In glass_mcp tools.rs:
async fn glass_tab_list(&self, ...) -> Result<CallToolResult, McpError> {
    let client = match self.ipc_client.as_ref() {
        Some(c) => c,
        None => return Ok(CallToolResult::error(vec![Content::text(
            "Glass GUI is not running. Tab tools require a running Glass window."
        )])),
    };
    match client.send_request("tab_list", json!({})).await {
        Ok(resp) => Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&resp)?
        )])),
        Err(e) => Ok(CallToolResult::error(vec![Content::text(
            format!("Failed to communicate with Glass GUI: {}", e)
        )])),
    }
}
```

### Anti-Patterns to Avoid
- **Embedding the MCP server in the GUI process:** Breaks the stdio transport model. AI clients like Claude Desktop spawn `glass mcp serve` as a child process and communicate via stdin/stdout. The MCP server must remain a separate process.
- **Polling SQLite for live data:** Adds unacceptable latency (seconds) and can't access terminal grid content, which lives only in memory.
- **Blocking the event loop while waiting for IPC response:** The event loop processes MCP requests, it should never wait on IPC. The flow is: IPC task -> event loop (via proxy) -> response (via oneshot) -> IPC task sends back to MCP process.
- **Using TCP sockets for IPC:** Exposes a network port unnecessarily. Local sockets are file-permission protected and don't appear on the network.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Cross-platform local socket | Custom socket abstraction crate | tokio UnixListener + named_pipe with `#[cfg]` | Only ~20 lines of platform code, no need for a dependency |
| JSON framing protocol | Custom binary protocol | Newline-delimited JSON (same as MCP stdio uses) | Simple, debuggable, serde_json already available |
| Request/response correlation | Custom ID tracking | Request ID in JSON + tokio::sync::oneshot per request | Oneshot guarantees exactly one response per request |
| Socket path discovery | Environment variable negotiation | Well-known path `~/.glass/glass.sock` / `\\.\pipe\glass-terminal` | Predictable, no configuration needed |

**Key insight:** The IPC layer is thin glue between two existing systems (MCP tool handlers and winit event loop). Over-engineering it with a generic RPC framework would add complexity without value.

## Common Pitfalls

### Pitfall 1: Stale Socket File on Unix
**What goes wrong:** If the GUI crashes without cleanup, `~/.glass/glass.sock` remains on disk. The next GUI instance can't bind to it.
**Why it happens:** Unix domain sockets leave filesystem entries that must be explicitly removed.
**How to avoid:** On startup, attempt to connect to the existing socket. If connection fails (no listener), delete the stale file and create a new listener. If connection succeeds, another GUI is already running -- log a warning and skip IPC listener.
**Warning signs:** "Address already in use" error on bind.

### Pitfall 2: Named Pipe Security on Windows
**What goes wrong:** Default named pipe ACLs may allow any user to connect.
**Why it happens:** Windows named pipes are accessible system-wide by default.
**How to avoid:** This is acceptable for a local development tool. The named pipe path includes no secrets and the worst case is another user on the same machine sending commands to your terminal -- same as if they had keyboard access.
**Warning signs:** Not a real concern for a local terminal emulator.

### Pitfall 3: Blocking the Winit Event Loop
**What goes wrong:** If MCP request handling performs I/O or computation in `user_event()`, frame rendering stutters.
**Why it happens:** `user_event()` runs on the main thread. Any blocking call delays the next frame.
**How to avoid:** All MCP request handling in `user_event()` must be synchronous and fast. Reading `SessionMux` state is fast (in-memory). Reading terminal grid requires locking `FairMutex<Term>` but this lock is also held briefly by the PTY reader thread -- contention is minimal. Never do file I/O or network calls in the handler.
**Warning signs:** Frame drops during MCP tool calls.

### Pitfall 4: Multiple GUI Instances
**What goes wrong:** Two Glass windows try to bind the same socket path.
**Why it happens:** User opens multiple Glass instances.
**How to avoid:** Only the first instance binds the IPC listener. Subsequent instances detect the existing listener and skip IPC setup. MCP tools connect to whichever instance is listening. For MVP, this is fine -- advanced multi-window routing can be added later.
**Warning signs:** Second GUI fails to start IPC listener.

### Pitfall 5: MCP Process Outlives GUI
**What goes wrong:** The MCP process holds an IPC connection to a GUI that has exited. Subsequent requests fail with broken pipe.
**Why it happens:** AI client keeps the MCP process running between tool calls. GUI closes normally.
**How to avoid:** IPC client in the MCP process should detect connection failure and attempt reconnection on each request. If reconnection fails, return the "Glass GUI is not running" error.
**Warning signs:** Intermittent "connection reset" errors in MCP tools.

## Code Examples

### IPC Listener (GUI Side) - Unix
```rust
// Source: tokio::net::UnixListener docs
#[cfg(unix)]
async fn spawn_ipc_listener(
    proxy: EventLoopProxy<AppEvent>,
    socket_path: std::path::PathBuf,
) -> anyhow::Result<()> {
    // Clean up stale socket
    if socket_path.exists() {
        let _ = std::fs::remove_file(&socket_path);
    }

    let listener = tokio::net::UnixListener::bind(&socket_path)?;
    tracing::info!("IPC listener started on {}", socket_path.display());

    loop {
        let (stream, _) = listener.accept().await?;
        let proxy = proxy.clone();
        tokio::spawn(handle_ipc_connection(stream, proxy));
    }
}
```

### IPC Listener (GUI Side) - Windows
```rust
// Source: tokio::net::windows::named_pipe docs
#[cfg(windows)]
async fn spawn_ipc_listener(
    proxy: EventLoopProxy<AppEvent>,
    pipe_name: &str,
) -> anyhow::Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;

    loop {
        let server = ServerOptions::new()
            .first_pipe_instance(false)
            .create(pipe_name)?;

        server.connect().await?;  // Wait for client connection
        let proxy = proxy.clone();
        tokio::spawn(handle_ipc_connection(server, proxy));
    }
}
```

### IPC Connection Handler (Shared)
```rust
// Works with any AsyncRead + AsyncWrite stream
async fn handle_ipc_connection<S>(stream: S, proxy: EventLoopProxy<AppEvent>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = tokio::io::BufReader::new(reader).lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let request: McpRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let err_resp = serde_json::json!({"error": format!("Invalid JSON: {}", e)});
                let _ = writer.write_all(format!("{}\n", err_resp).as_bytes()).await;
                continue;
            }
        };

        let (tx, rx) = tokio::sync::oneshot::channel();
        let event = McpEventRequest { request, reply: tx };

        if proxy.send_event(AppEvent::McpRequest(event)).is_err() {
            break; // Event loop closed
        }

        match rx.await {
            Ok(response) => {
                let json = serde_json::to_string(&response).unwrap_or_default();
                let _ = writer.write_all(format!("{}\n", json).as_bytes()).await;
            }
            Err(_) => break, // Event loop dropped the sender
        }
    }
}
```

### IPC Client (MCP Side)
```rust
// In glass_mcp -- connects to the GUI's IPC listener
pub struct IpcClient {
    // Lazily connects on first use, reconnects on failure
}

impl IpcClient {
    pub async fn send_request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let request = serde_json::json!({
            "id": self.next_id(),
            "method": method,
            "params": params,
        });
        let line = serde_json::to_string(&request).map_err(|e| e.to_string())?;

        let mut stream = self.connect().await.map_err(|e| {
            format!("Glass GUI is not running ({})", e)
        })?;

        stream.write_all(format!("{}\n", line).as_bytes()).await
            .map_err(|e| e.to_string())?;

        let mut reader = BufReader::new(&mut stream);
        let mut response_line = String::new();
        reader.read_line(&mut response_line).await
            .map_err(|e| e.to_string())?;

        let response: McpResponse = serde_json::from_str(&response_line)
            .map_err(|e| e.to_string())?;

        match response.error {
            Some(err) => Err(err),
            None => Ok(response.result.unwrap_or(serde_json::Value::Null)),
        }
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| MCP accesses SQLite only | MCP needs live session data via IPC | Phase 35 (now) | Enables all v2.3 live tools (tabs, output, status) |
| GUI has no inbound communication | GUI listens for IPC requests | Phase 35 (now) | GUI becomes a server for MCP tool requests |

**Existing patterns leveraged:**
- `EventLoopProxy::send_event()` -- already used by PTY threads, config watcher, coordination poller
- `AppEvent` enum dispatching in `user_event()` -- well-established pattern in main.rs
- `SessionMux` access in event handlers -- already done for shell events, terminal exit, etc.

## Open Questions

1. **Multiple GUI windows connecting to one socket**
   - What we know: Only one GUI can bind the IPC socket. Multiple windows within the same process share the listener.
   - What's unclear: If user opens a second Glass process, should it try to share the socket? Or should each have its own?
   - Recommendation: First process wins. Second process logs a warning and skips IPC. This is fine for MVP.

2. **Timeout for IPC requests**
   - What we know: Success criteria says "within 5 seconds". The oneshot channel itself has no timeout.
   - What's unclear: Where to enforce the timeout -- IPC task or MCP client?
   - Recommendation: MCP client enforces a 5-second timeout on the IPC round-trip. If the GUI is unresponsive (e.g., frozen), the MCP tool returns an error.

3. **Tokio runtime in GUI process**
   - What we know: The GUI process uses winit's synchronous event loop. The MCP process uses `tokio::runtime::Runtime::new()`. The GUI currently has no tokio runtime.
   - What's unclear: How to run the async IPC listener inside the GUI process.
   - Recommendation: Spawn a background `std::thread` with its own `tokio::runtime::Runtime` for the IPC listener. The listener thread communicates with the winit event loop via `EventLoopProxy`. This is the same pattern used by the coordination poller (background thread + proxy), just with tokio instead of `thread::sleep` loops.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` + `#[cfg(test)]` modules |
| Config file | None (Cargo.toml test configuration) |
| Quick run command | `cargo test -p glass_core ipc --no-fail-fast` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| INFRA-01 | MCP request reaches GUI and gets response via IPC | integration | `cargo test -p glass_core ipc_roundtrip` | No - Wave 0 |
| INFRA-01 | Request/response JSON serialization | unit | `cargo test -p glass_core ipc_serde` | No - Wave 0 |
| INFRA-02 | Event loop doesn't block on MCP processing | unit | Verify oneshot::send is non-blocking (by design) | No - Wave 0 |
| INFRA-02 | GUI returns error when no session matches | unit | `cargo test -p glass_core ipc_unknown_method` | No - Wave 0 |
| N/A | Graceful error when GUI not running | unit | `cargo test -p glass_mcp ipc_client_no_gui` | No - Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_core -p glass_mcp --no-fail-fast`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_core/src/ipc.rs` -- IPC types, listener, connection handler (new file)
- [ ] Unit tests for McpRequest/McpResponse serialization
- [ ] Integration test: IPC round-trip (spawn listener, connect client, exchange JSON)
- [ ] Test: IPC client gets error when no listener is running

## Sources

### Primary (HIGH confidence)
- Project codebase: `crates/glass_mcp/src/tools.rs`, `crates/glass_core/src/event.rs`, `src/main.rs` -- current MCP architecture, event loop, AppEvent dispatching
- `AGENT_MCP_FEATURES.md` -- project research doc with McpRequest/McpResponse design
- tokio docs: `tokio::net::UnixListener`, `tokio::net::windows::named_pipe` -- built-in IPC primitives
- tokio docs: `tokio::sync::oneshot` -- single-use reply channel

### Secondary (MEDIUM confidence)
- [interprocess crate](https://github.com/kotauskas/interprocess) -- considered but not needed; tokio built-ins sufficient
- [tokio named pipe docs](https://docs.rs/tokio/latest/tokio/net/windows/named_pipe/index.html) -- Windows named pipe API

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - no new dependencies, all existing workspace crates
- Architecture: HIGH - follows established patterns (EventLoopProxy, AppEvent, background thread)
- Pitfalls: HIGH - common IPC issues well-documented, mitigations are standard practice

**Research date:** 2026-03-09
**Valid until:** 2026-04-09 (stable domain, no fast-moving dependencies)
