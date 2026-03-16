# Orchestrator Overlay & Background Tabs Design

**Date:** 2026-03-16
**Status:** Approved
**Context:** The orchestrator works but lacks visibility into what the Glass Agent is doing. Agent text responses leak into the PTY, MCP-created tabs steal focus, and there's no way to monitor the orchestrator's activity without reading raw logs.

## Problem

Three gaps in the orchestrator UX:

1. **No visibility** — the Glass Agent thinks, calls tools, and makes decisions, but none of this is surfaced to the user. The only indicator is `[orchestrating | iter #N]` in the status bar.
2. **Tab disruption** — in audit mode, the agent creates/closes tabs via MCP tools to test features. Each tab creation steals focus and visually flashes, which is disruptive.
3. **No history** — after an overnight run, there's no way to review what the agent did iteration by iteration.

## Design

### 1. Orchestrator Event Buffer

A ring buffer on the `Processor` struct that stores the last 200 orchestrator events. Data comes from the existing agent reader thread, which already parses all JSON from the agent subprocess.

**New struct:**

```rust
/// A single event in the orchestrator transcript.
pub enum OrchestratorEvent {
    /// Agent's internal reasoning (extended thinking block).
    Thinking { text: String, token_estimate: usize },
    /// Agent called an MCP tool or Claude tool.
    ToolCall { name: String, params_summary: String },
    /// Result returned from a tool call.
    ToolResult { name: String, output_summary: String },
    /// Agent's final text response for the turn (after all tool calls).
    AgentText { text: String },
    /// Glass sent terminal context to the agent.
    ContextSent { line_count: usize, has_soi: bool, has_nudge: bool },
    /// Agent was respawned (checkpoint refresh or crash recovery).
    AgentRespawn { reason: String },
    /// Verification ran and produced results.
    VerifyResult { passed: Option<u32>, failed: Option<u32>, regressed: bool },
}

pub struct OrchestratorEventEntry {
    pub event: OrchestratorEvent,
    pub timestamp: std::time::Instant,
    pub iteration: u32,
    /// Monotonic ID for stable indexing (used by expanded_thinking set).
    pub id: u64,
}
```

**Token estimation:** `Thinking.token_estimate` is computed as `text.split_whitespace().count() * 4 / 3` (rough word-to-token ratio). This is a display hint, not an exact count.

**Location:** New file `src/orchestrator_events.rs` for the struct definitions and ring buffer. The `Processor` struct in `main.rs` gets these new fields:
- `orchestrator_events: VecDeque<OrchestratorEventEntry>` — ring buffer, capped at 1000 entries
- `orchestrator_event_counter: u64` — monotonic ID counter for stable indexing
- `expanded_thinking: HashSet<u64>` — set of event IDs whose thinking blocks are expanded
- `orchestrator_scroll_offset: usize` — separate scroll state for the orchestrator transcript
- `orchestrator_activated_at: Option<Instant>` — set when orchestrator transitions to active, used for relative timestamps

**Data flow:**

- **Reader thread** — already parses agent stdout JSON. Currently extracts `text` blocks and discards `thinking`/`tool_use`/`tool_result`. Change: emit new `AppEvent` variants (`OrchestratorThinking`, `OrchestratorToolCall`, `OrchestratorToolResult`) for non-text content blocks. These are pushed into the ring buffer on the main thread.
- **OrchestratorSilence handler** — pushes `ContextSent` when sending context to the agent.
- **OrchestratorResponse handler** — pushes `AgentText` with the final buffered response.
- **Respawn** — pushes `AgentRespawn` when `respawn_orchestrator_agent` is called.
- **VerifyComplete handler** — pushes `VerifyResult`.

The buffer is a `VecDeque` capped at 1000 entries (small structs, ~500KB-2MB total). When full, oldest entries are popped from the front. Expanded thinking entries whose IDs fall out of the buffer are cleaned from `expanded_thinking`. The buffer persists across agent respawns — an `AgentRespawn` separator event is inserted but the buffer is not cleared.

### 2. Orchestrator Tab in Activity Overlay

A new filter tab `Orchestrator` added to the existing `ActivityViewFilter` enum in `activity_overlay.rs`. The tab cycle becomes: All → Agents → Locks → Observations → Messages → Orchestrator → All.

When the Orchestrator tab is selected, the overlay renders a **single-column layout** (no agent cards column) with two sections:

**Dashboard Header (fixed, ~6-8 lines):**

```
ORCHESTRATOR                                      iter #12 (3 since checkpoint)
Mode: audit | Verify: floor (1088 passed) | Guard: 10 kept, 1 reverted
Status: active | Max iterations: 100
Last checkpoint: "SOI parsers complete" → Next: "History module audit"
```

Fields sourced from `OrchestratorState`:
- `iteration`, `iterations_since_checkpoint`, `max_iterations`
- `metric_baseline` (keep_count, revert_count, last test counts)
- `last_checkpoint_completed`, `last_checkpoint_next`
- `active`, `response_pending`, `checkpoint_phase`
- Orchestrator mode from config (`build`/`audit`)

**Live Transcript (scrollable, fills remaining space):**

Each entry is rendered as a compact line or group:

```
[#12 00:42]  Thinking...  (142 tokens)              [expandable]
[#12 00:42]  → glass_tab_create(cwd: "/apps/Glass")
[#12 00:42]  → glass_tab_send("cargo test -p glass_history")
[#12 00:43]  → glass_tab_output → "45 passed, 0 failed"
[#12 00:43]  → glass_tab_close(tab: 2)
[#12 00:44]  Agent: "History tests all pass. Moving to snapshot module."
[#12 00:44]  Context sent (80 lines, SOI)
             --- Agent respawned (checkpoint) ---
[#13 00:45]  ✓ Verify: 1110 passed, 0 failed
[#13 00:45]  Thinking...  (89 tokens)                [expandable]
```

**Rendering approach:**
- Reuse the existing `ActivityOverlayRenderer` text label system.
- Each event type gets a distinct color: thinking=dim gray, tool calls=blue, tool results=cyan, agent text=green, context sent=dim, respawn=amber separator, verify=green/red.
- Thinking blocks show a one-line summary by default. When the user presses Enter or a designated key on a selected thinking entry, it expands to show the full text. Track `expanded_thinking: HashSet<u64>` (keyed on monotonic event ID, not buffer index) on `Processor`.
- Scrolling uses a separate `orchestrator_scroll_offset` (not the shared `activity_scroll_offset`) since the fixed dashboard header reduces the scrollable area.
- Timestamps shown as relative to orchestrator activation (MM:SS) using `orchestrator_activated_at`, not absolute wall clock.

**Data passed to renderer:**

Extend `ActivityOverlayRenderData` with:

```rust
pub orchestrator_events: Vec<OrchestratorEventDisplay>,
pub orchestrator_dashboard: Option<OrchestratorDashboard>,
```

```rust
pub struct OrchestratorDashboard {
    pub iteration: u32,
    pub iterations_since_checkpoint: u32,
    pub max_iterations: Option<u32>,
    pub mode: String,
    pub verify_mode: String,
    pub tests_passed: Option<u32>,
    pub keep_count: u32,
    pub revert_count: u32,
    pub last_completed: String,
    pub next_item: String,
    pub active: bool,
    pub response_pending: bool,
    pub checkpoint_phase: String,
    pub paused_reason: Option<String>,
}

pub struct OrchestratorEventDisplay {
    pub iteration: u32,
    pub relative_time: String,
    pub kind: OrchestratorEventKind,
    pub text: String,
    pub expanded: bool,
    pub expandable: bool,
}

pub enum OrchestratorEventKind {
    Thinking,
    ToolCall,
    ToolResult,
    AgentText,
    ContextSent,
    Respawn,
    Verify,
}
```

**Tab visibility:** The Orchestrator tab always appears in the filter cycle (not gated on `orchestrator.active`). When the orchestrator hasn't been activated, the transcript is empty and the dashboard shows "Orchestrator inactive — press Ctrl+Shift+O to start".

### 3. Background Tabs

When the Glass Agent creates a tab via the `glass_tab_create` MCP tool, the tab should not steal focus.

**Change to `SessionMux::add_tab()`:**

Add a `background: bool` parameter. When `true`:
- Insert the tab at the **end** of the tab list (not at `active_tab + 1`) to avoid confusing insertion ordering when multiple background tabs are created rapidly
- Do NOT change `self.active_tab` (new behavior — keep current tab focused)
- The new tab still appears in the tab bar

All existing callers of `add_tab()` (Ctrl+Shift+T handler, session creation) must be updated to pass `background: false`.

**Change to `glass_tab_create` MCP handler:**

The MCP handler in `main.rs` currently calls `session_mux.add_tab(session)` and the new tab becomes active. Change: when the orchestrator is active (`self.orchestrator.active`), pass `background: true` to `add_tab()`.

**Tab bar visual indicator:**

Tabs created by the agent get a visual tag. Add an `agent_created: bool` field to `TabState` (or equivalent). The tab bar renderer dims the label or prepends a small indicator (e.g., `◆` prefix or reduced opacity) for agent-created tabs.

**Transcript logging:**

When a tab is created/closed by the agent, push `ToolCall` / `ToolResult` events into the orchestrator event buffer (this happens naturally since tab operations are MCP tool calls that flow through the reader thread).

### 4. Reader Thread Changes

The reader thread currently only extracts `text` blocks from assistant messages. Extend it to also capture `thinking` and `tool_use` blocks, plus `tool_result` from user messages.

**Tool name mapping:** The reader thread maintains a `HashMap<String, String>` mapping `tool_use_id` → tool name. Populated when `tool_use` blocks are parsed, consumed when `tool_result` blocks arrive. This allows tool results to display the human-readable tool name instead of the UUID.

**UTF-8 safe truncation:** All string truncation uses `chars().take(N).collect::<String>()` instead of byte slicing (`&s[..N]`) to avoid panics on multi-byte characters.

```rust
/// Truncate a string to at most `max_chars` characters, appending "..." if truncated.
fn truncate_display(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count > max_chars {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{truncated}...")
    } else {
        s.to_string()
    }
}

// In the reader thread:
let mut tool_id_to_name: HashMap<String, String> = HashMap::new();

// ... inside the message loop:

Some("assistant") => {
    let mut full_text = String::new();
    if let Some(arr) = content.as_array() {
        for block in arr {
            match block.get("type").and_then(|t| t.as_str()) {
                Some("text") => { /* existing: append to full_text */ }
                Some("thinking") => {
                    if let Some(text) = block.get("thinking").and_then(|t| t.as_str()) {
                        let _ = proxy_reader.send_event(
                            AppEvent::OrchestratorThinking {
                                text: text.to_string(),
                            },
                        );
                    }
                }
                Some("tool_use") => {
                    let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("?");
                    let id = block.get("id").and_then(|i| i.as_str()).unwrap_or("");
                    let input = block.get("input").map(|i| i.to_string()).unwrap_or_default();
                    let summary = truncate_display(&input, 200);
                    // Map tool_use_id to tool name for later tool_result lookup
                    if !id.is_empty() {
                        tool_id_to_name.insert(id.to_string(), name.to_string());
                    }
                    let _ = proxy_reader.send_event(
                        AppEvent::OrchestratorToolCall {
                            name: name.to_string(),
                            params_summary: summary,
                        },
                    );
                }
                _ => {}
            }
        }
    }
}
Some("user") => {
    // Tool results appear as user messages with tool_result content
    if let Some(arr) = content.as_array() {
        for block in arr {
            if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                let tool_use_id = block.get("tool_use_id")
                    .and_then(|t| t.as_str()).unwrap_or("?");
                // Resolve tool name from ID mapping
                let tool_name = tool_id_to_name.remove(tool_use_id)
                    .unwrap_or_else(|| tool_use_id.to_string());
                // Handle content as either string or array of content blocks
                let content_text = match block.get("content") {
                    Some(c) if c.is_string() => c.as_str().unwrap_or("").to_string(),
                    Some(c) if c.is_array() => {
                        c.as_array().unwrap().iter()
                            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                            .collect::<Vec<_>>().join("\n")
                    }
                    _ => String::new(),
                };
                let summary = truncate_display(&content_text, 200);
                let _ = proxy_reader.send_event(
                    AppEvent::OrchestratorToolResult {
                        name: tool_name,
                        output_summary: summary,
                    },
                );
            }
        }
    }
}
```

### 5. New AppEvent Variants

Add to `glass_core/src/event.rs`:

```rust
OrchestratorThinking { text: String },
OrchestratorToolCall { name: String, params_summary: String },
OrchestratorToolResult { name: String, output_summary: String },
```

These are handled in `main.rs` by pushing into the orchestrator event ring buffer.

## Files Changed

| File | Change |
|------|--------|
| `src/orchestrator_events.rs` | **New.** Event enum, entry struct, ring buffer helpers |
| `src/main.rs` | Add event buffer to Processor, handle new AppEvents, populate overlay data, background tab logic |
| `crates/glass_core/src/event.rs` | Add 3 new AppEvent variants |
| `crates/glass_renderer/src/activity_overlay.rs` | Add Orchestrator filter, dashboard + transcript structs |
| `crates/glass_renderer/src/frame.rs` | Add `build_orchestrator_text()` method (separate from `build_overlay_text()`) for orchestrator tab rendering |
| `crates/glass_mux/src/session_mux.rs` | Add `background` param to `add_tab()` |
| `crates/glass_renderer/src/tab_bar.rs` | Dim/tag agent-created tabs |
| `crates/glass_mcp/src/tools.rs` | No change (MCP params unchanged) |

## Not In Scope

- **Tab bar icon/badge for agent tabs** — start with a simple text dimming, iterate later.
- **Filtering transcript by event type** — start with showing everything, add sub-filters if needed.
- **Exporting transcript** — can be added later if useful.
- **Real-time streaming** — the overlay renders on redraw, not on every event. Events accumulate and display on next frame. This is fine since the overlay is a review tool, not a live terminal.
