---
phase: 35-mcp-command-channel
plan: 01
subsystem: infra
tags: [ipc, tokio, named-pipe, unix-socket, json-rpc, mcp]

requires:
  - phase: 34-agent-coordination
    provides: "coordination_poller spawn pattern, AppEvent enum"
provides:
  - "McpRequest/McpResponse types with serde"
  - "McpEventRequest with oneshot reply channel"
  - "Platform-abstracted IPC listener (Unix socket / Windows named pipe)"
  - "AppEvent::McpRequest variant in winit event loop"
  - "ping health check handler"
affects: [35-02, 36-mcp-live-tools, 37-mcp-smart-context]

tech-stack:
  added: [tokio (in glass_core), anyhow (in glass_core)]
  patterns: [IPC listener thread with own tokio runtime, oneshot reply channel through event loop, JSON-line protocol]

key-files:
  created:
    - crates/glass_core/src/ipc.rs
  modified:
    - crates/glass_core/src/event.rs
    - crates/glass_core/src/lib.rs
    - crates/glass_core/Cargo.toml
    - src/main.rs

key-decisions:
  - "Dedicated tokio runtime per IPC listener thread (avoids dependency on main thread async)"
  - "JSON-line protocol (newline-delimited JSON) for simplicity and debuggability"
  - "5-second timeout on oneshot response to prevent connection hangs"
  - "Removed Clone derive from AppEvent since McpEventRequest contains oneshot::Sender"
  - "McpResponse helper methods (ok/err) and ping_result() to avoid serde_json dep in main crate"

patterns-established:
  - "IPC request/response pattern: McpRequest -> AppEvent::McpRequest -> oneshot reply -> McpResponse"
  - "Method dispatch in user_event() match arm for MCP methods"

requirements-completed: [INFRA-01, INFRA-02]

duration: 8min
completed: 2026-03-09
---

# Phase 35 Plan 01: MCP Command Channel Summary

**Platform-abstracted IPC listener with JSON-line protocol, oneshot reply channels through winit event loop, and ping health check**

## Performance

- **Duration:** 8 min
- **Started:** 2026-03-10T02:33:35Z
- **Completed:** 2026-03-10T02:41:00Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments
- McpRequest/McpResponse types with serde serialization and helper constructors
- Platform-abstracted IPC listener: Unix domain socket at ~/.glass/glass.sock, Windows named pipe at \\.\pipe\glass-terminal
- Connection handler with JSON-line protocol, 5-second request timeout, and error handling for invalid JSON
- AppEvent::McpRequest variant with oneshot reply channel dispatched through winit event loop
- "ping" health check method wired end-to-end through IPC -> event loop -> response
- 8 unit/integration tests covering serialization, round-trip, platform paths, and error cases

## Task Commits

Each task was committed atomically:

1. **Task 1: IPC types, listener, and connection handler in glass_core** - `0274cd2` (feat)
2. **Task 2: Wire IPC listener into GUI event loop and handle McpRequest** - `6dddc1f` (feat)

## Files Created/Modified
- `crates/glass_core/src/ipc.rs` - McpRequest/McpResponse types, IPC listener, connection handler, platform socket helpers
- `crates/glass_core/src/event.rs` - AppEvent::McpRequest variant (removed Clone derive)
- `crates/glass_core/src/lib.rs` - pub mod ipc
- `crates/glass_core/Cargo.toml` - Added tokio and anyhow dependencies
- `src/main.rs` - IPC listener startup and McpRequest handler in user_event()

## Decisions Made
- Used dedicated tokio runtime per IPC listener thread to avoid coupling with main thread
- JSON-line (newline-delimited JSON) protocol chosen for simplicity and debuggability
- 5-second timeout on oneshot response prevents connection hangs if event loop is slow
- Removed Clone derive from AppEvent since oneshot::Sender is not Clone -- verified no existing code clones AppEvent
- Created McpResponse::ok/err helpers and ping_result() function to avoid adding serde_json as a main crate dependency

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Removed Clone derive from AppEvent**
- **Found during:** Task 1
- **Issue:** AppEvent derived Clone but McpEventRequest contains oneshot::Sender which is not Clone
- **Fix:** Removed Clone from AppEvent derive (verified no code clones AppEvent instances)
- **Files modified:** crates/glass_core/src/event.rs
- **Verification:** cargo build succeeds, all 531 workspace tests pass

**2. [Rule 3 - Blocking] Added Deserialize to McpResponse for test**
- **Found during:** Task 1
- **Issue:** Round-trip test needed to deserialize McpResponse but it only had Serialize
- **Fix:** Added Deserialize derive to McpResponse
- **Files modified:** crates/glass_core/src/ipc.rs
- **Verification:** ipc_round_trip_over_tcp test passes

**3. [Rule 3 - Blocking] Avoided serde_json dependency in main crate**
- **Found during:** Task 2
- **Issue:** serde_json is only a dev-dependency in the main crate, can't use json! macro
- **Fix:** Created McpResponse::ok/err helpers and ping_result() in glass_core::ipc
- **Files modified:** crates/glass_core/src/ipc.rs, src/main.rs
- **Verification:** cargo build succeeds without adding serde_json as main dep

---

**Total deviations:** 3 auto-fixed (1 bug, 2 blocking)
**Impact on plan:** All auto-fixes necessary for compilation. No scope creep.

## Issues Encountered
None beyond the auto-fixed deviations.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- IPC channel is operational with ping health check
- Ready for Plan 02 to add live MCP tool methods (tab_list, pane_read, etc.)
- Method dispatch in user_event() is extensible via match arms

---
*Phase: 35-mcp-command-channel*
*Completed: 2026-03-09*
