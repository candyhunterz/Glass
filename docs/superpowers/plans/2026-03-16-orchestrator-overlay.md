# Orchestrator Overlay & Background Tabs Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an Orchestrator tab to the activity overlay showing a live transcript of agent activity, plus background tabs that don't steal focus.

**Architecture:** Three independent subsystems wired together: (1) a 1000-entry ring buffer capturing orchestrator events from the reader thread and main event handlers, (2) an Orchestrator tab in the Ctrl+Shift+G overlay rendering a dashboard header + scrollable transcript, (3) background tab support in SessionMux so MCP-created tabs don't steal focus. Data flows from the reader thread → AppEvent → ring buffer → overlay render data → GPU text rendering.

**Tech Stack:** Rust, wgpu, glyphon, winit, serde_json, alacritty_terminal

---

## Chunk 1: Event Buffer Infrastructure

### Task 1: Create orchestrator_events.rs

**Files:**
- Create: `src/orchestrator_events.rs`

- [ ] **Step 1: Write the OrchestratorEvent enum and OrchestratorEventEntry struct**

```rust
// src/orchestrator_events.rs
//! Orchestrator event buffer for the activity overlay transcript.
//!
//! Stores the last N events from the Glass Agent (thinking, tool calls,
//! responses) for display in the Orchestrator tab of the activity overlay.

use std::collections::{HashSet, VecDeque};
use std::time::Instant;

/// Maximum events retained in the ring buffer.
pub const MAX_EVENTS: usize = 1000;

/// A single event in the orchestrator transcript.
#[derive(Debug, Clone)]
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
    ContextSent {
        line_count: usize,
        has_soi: bool,
        has_nudge: bool,
    },
    /// Agent was respawned (checkpoint refresh or crash recovery).
    AgentRespawn { reason: String },
    /// Verification ran and produced results.
    VerifyResult {
        passed: Option<u32>,
        failed: Option<u32>,
        regressed: bool,
    },
}

/// A timestamped event entry with monotonic ID.
#[derive(Debug, Clone)]
pub struct OrchestratorEventEntry {
    pub event: OrchestratorEvent,
    pub timestamp: Instant,
    pub iteration: u32,
    /// Monotonic ID for stable indexing (used by expanded_thinking set).
    pub id: u64,
}

/// Ring buffer of orchestrator events with monotonic ID generation.
pub struct OrchestratorEventBuffer {
    pub events: VecDeque<OrchestratorEventEntry>,
    next_id: u64,
    pub expanded_thinking: HashSet<u64>,
}

impl OrchestratorEventBuffer {
    pub fn new() -> Self {
        Self {
            events: VecDeque::with_capacity(MAX_EVENTS),
            next_id: 0,
            expanded_thinking: HashSet::new(),
        }
    }

    /// Push an event into the buffer, evicting the oldest if full.
    pub fn push(&mut self, event: OrchestratorEvent, iteration: u32) {
        let id = self.next_id;
        self.next_id += 1;

        // Evict oldest if full, cleaning up expanded_thinking
        if self.events.len() >= MAX_EVENTS {
            if let Some(old) = self.events.pop_front() {
                self.expanded_thinking.remove(&old.id);
            }
        }

        self.events.push_back(OrchestratorEventEntry {
            event,
            timestamp: Instant::now(),
            iteration,
            id,
        });
    }

    /// Toggle thinking block expansion for a given event ID.
    pub fn toggle_thinking(&mut self, id: u64) {
        if self.expanded_thinking.contains(&id) {
            self.expanded_thinking.remove(&id);
        } else {
            self.expanded_thinking.insert(id);
        }
    }

    /// Check if a thinking block is expanded.
    pub fn is_expanded(&self, id: u64) -> bool {
        self.expanded_thinking.contains(&id)
    }
}

/// Truncate a string to at most `max_chars` characters, appending "..." if truncated.
pub fn truncate_display(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count > max_chars {
        let truncated: String = s.chars().take(max_chars).collect();
        format!("{truncated}...")
    } else {
        s.to_string()
    }
}

/// Estimate token count from text (rough word-to-token ratio).
pub fn estimate_tokens(text: &str) -> usize {
    text.split_whitespace().count() * 4 / 3
}
```

- [ ] **Step 2: Add module declaration to main.rs**

In `src/main.rs`, find the existing `mod orchestrator;` declaration and add below it:

```rust
mod orchestrator_events;
```

- [ ] **Step 3: Write tests for the event buffer**

Append to `src/orchestrator_events.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_retrieve() {
        let mut buf = OrchestratorEventBuffer::new();
        buf.push(OrchestratorEvent::AgentText { text: "hello".into() }, 1);
        assert_eq!(buf.events.len(), 1);
        assert_eq!(buf.events[0].id, 0);
        assert_eq!(buf.events[0].iteration, 1);
    }

    #[test]
    fn monotonic_ids() {
        let mut buf = OrchestratorEventBuffer::new();
        buf.push(OrchestratorEvent::AgentText { text: "a".into() }, 1);
        buf.push(OrchestratorEvent::AgentText { text: "b".into() }, 1);
        assert_eq!(buf.events[0].id, 0);
        assert_eq!(buf.events[1].id, 1);
    }

    #[test]
    fn eviction_at_capacity() {
        let mut buf = OrchestratorEventBuffer::new();
        for i in 0..MAX_EVENTS + 5 {
            buf.push(
                OrchestratorEvent::AgentText {
                    text: format!("event {i}"),
                },
                1,
            );
        }
        assert_eq!(buf.events.len(), MAX_EVENTS);
        // Oldest should be event 5 (0-4 evicted)
        assert_eq!(buf.events[0].id, 5);
    }

    #[test]
    fn expanded_thinking_cleaned_on_eviction() {
        let mut buf = OrchestratorEventBuffer::new();
        buf.push(
            OrchestratorEvent::Thinking {
                text: "reasoning".into(),
                token_estimate: 10,
            },
            1,
        );
        buf.toggle_thinking(0);
        assert!(buf.is_expanded(0));
        // Fill to evict
        for i in 1..=MAX_EVENTS {
            buf.push(
                OrchestratorEvent::AgentText {
                    text: format!("event {i}"),
                },
                1,
            );
        }
        assert!(!buf.is_expanded(0));
    }

    #[test]
    fn toggle_thinking() {
        let mut buf = OrchestratorEventBuffer::new();
        buf.push(
            OrchestratorEvent::Thinking {
                text: "t".into(),
                token_estimate: 1,
            },
            1,
        );
        assert!(!buf.is_expanded(0));
        buf.toggle_thinking(0);
        assert!(buf.is_expanded(0));
        buf.toggle_thinking(0);
        assert!(!buf.is_expanded(0));
    }

    #[test]
    fn truncate_display_short() {
        assert_eq!(truncate_display("hello", 10), "hello");
    }

    #[test]
    fn truncate_display_long() {
        let long = "a".repeat(300);
        let result = truncate_display(&long, 200);
        assert_eq!(result.chars().count(), 203); // 200 + "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_display_multibyte() {
        let s = "日本語テスト"; // 6 chars, but many bytes
        let result = truncate_display(s, 3);
        assert_eq!(result, "日本語...");
    }

    #[test]
    fn estimate_tokens_basic() {
        assert_eq!(estimate_tokens("hello world foo bar"), 5); // 4 words * 4/3 = 5
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p glass orchestrator_events`
Expected: All 8 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/orchestrator_events.rs src/main.rs
git commit -m "feat(orchestrator): add event buffer for overlay transcript

Ring buffer with 1000-entry capacity, monotonic IDs for stable
expanded_thinking tracking, and UTF-8 safe truncation helper."
```

### Task 2: Add AppEvent variants

**Files:**
- Modify: `crates/glass_core/src/event.rs:67-168`

- [ ] **Step 1: Add three new AppEvent variants**

In `crates/glass_core/src/event.rs`, find the `UsageResume` variant (line ~167) and add after it:

```rust
    /// Agent thinking block for orchestrator transcript.
    OrchestratorThinking { text: String },
    /// Agent tool call for orchestrator transcript.
    OrchestratorToolCall {
        name: String,
        params_summary: String,
    },
    /// Agent tool result for orchestrator transcript.
    OrchestratorToolResult {
        name: String,
        output_summary: String,
    },
```

- [ ] **Step 2: Verify compilation**

Run: `cargo build -p glass_core`
Expected: Compiles cleanly (new variants are unused but that's fine — no warnings for enum variants)

- [ ] **Step 3: Commit**

```bash
git add crates/glass_core/src/event.rs
git commit -m "feat(core): add OrchestratorThinking/ToolCall/ToolResult AppEvent variants"
```

### Task 3: Add event buffer fields to Processor

**Files:**
- Modify: `src/main.rs:254-343` (Processor struct)
- Modify: `src/main.rs` (Processor::new or initialization)

- [ ] **Step 1: Add fields to Processor struct**

In `src/main.rs`, find the `Processor` struct (line ~254). Add these fields after `activity_verbose` (line ~313):

```rust
    /// Orchestrator event ring buffer for the overlay transcript.
    orchestrator_event_buffer: orchestrator_events::OrchestratorEventBuffer,
    /// Separate scroll offset for orchestrator transcript (independent of activity overlay).
    orchestrator_scroll_offset: usize,
    /// When orchestrator was activated (for relative timestamps in transcript).
    orchestrator_activated_at: Option<std::time::Instant>,
```

- [ ] **Step 2: Initialize fields in Processor construction**

Find where `Processor` is constructed (search for `activity_verbose:` in the struct literal initialization). Add after it:

```rust
            orchestrator_event_buffer: orchestrator_events::OrchestratorEventBuffer::new(),
            orchestrator_scroll_offset: 0,
            orchestrator_activated_at: None,
```

- [ ] **Step 3: Set orchestrator_activated_at when orchestrator activates**

In the Ctrl+Shift+O handler (line ~3337), after `self.orchestrator.active = !self.orchestrator.active;`, inside the `if self.orchestrator.active` block, add:

```rust
                                    self.orchestrator_activated_at = Some(std::time::Instant::now());
```

And in the `else` (deactivation) block, do NOT clear it — we want timestamps to persist for review after deactivation.

- [ ] **Step 4: Also set it in the settings overlay activation path**

Find the config reload handler where `orch_enabled && !was_active` (the settings overlay sync we added). After `self.orchestrator.active = true;`, add:

```rust
                            self.orchestrator_activated_at = Some(std::time::Instant::now());
```

- [ ] **Step 5: Verify compilation**

Run: `cargo build`
Expected: Compiles cleanly

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "feat(orchestrator): add event buffer and activation timestamp to Processor"
```

## Chunk 2: Reader Thread & Event Population

### Task 4: Extend reader thread to emit orchestrator events

**Files:**
- Modify: `src/main.rs:1082-1163` (reader thread)

- [ ] **Step 1: Add tool_id_to_name HashMap and truncate helper import**

In the reader thread spawn closure (line ~1085), after `let mut buffered_response: Option<String> = None;`, add:

```rust
            let mut tool_id_to_name: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
```

- [ ] **Step 2: Extend the assistant message handler to capture thinking and tool_use blocks**

In the `Some("assistant")` branch, inside the `for block in arr` loop, change the block type matching from:

```rust
                                    if block.get("type").and_then(|t| t.as_str()) == Some("text") {
```

To a `match` that also captures thinking and tool_use:

```rust
                                    match block.get("type").and_then(|t| t.as_str()) {
                                        Some("text") => {
                                            if let Some(text) =
                                                block.get("text").and_then(|t| t.as_str())
                                            {
                                                full_text.push_str(text);
                                            }
                                        }
                                        Some("thinking") => {
                                            if let Some(text) =
                                                block.get("thinking").and_then(|t| t.as_str())
                                            {
                                                let _ = proxy_reader.send_event(
                                                    glass_core::event::AppEvent::OrchestratorThinking {
                                                        text: text.to_string(),
                                                    },
                                                );
                                            }
                                        }
                                        Some("tool_use") => {
                                            let name = block
                                                .get("name")
                                                .and_then(|n| n.as_str())
                                                .unwrap_or("?");
                                            let id = block
                                                .get("id")
                                                .and_then(|i| i.as_str())
                                                .unwrap_or("");
                                            let input = block
                                                .get("input")
                                                .map(|i| i.to_string())
                                                .unwrap_or_default();
                                            let summary =
                                                orchestrator_events::truncate_display(&input, 200);
                                            if !id.is_empty() {
                                                tool_id_to_name
                                                    .insert(id.to_string(), name.to_string());
                                            }
                                            let _ = proxy_reader.send_event(
                                                glass_core::event::AppEvent::OrchestratorToolCall {
                                                    name: name.to_string(),
                                                    params_summary: summary,
                                                },
                                            );
                                        }
                                        _ => {}
                                    }
```

- [ ] **Step 3: Add user message handler for tool_result blocks**

In the `match` on message type, after the `Some("assistant")` branch and before `_ => {}`, add a new branch:

```rust
                    Some("user") => {
                        // Tool results for orchestrator transcript
                        if let Some(content) = val.get("message").and_then(|m| m.get("content")) {
                            if let Some(arr) = content.as_array() {
                                for block in arr {
                                    if block.get("type").and_then(|t| t.as_str())
                                        == Some("tool_result")
                                    {
                                        let tool_use_id = block
                                            .get("tool_use_id")
                                            .and_then(|t| t.as_str())
                                            .unwrap_or("?");
                                        let tool_name = tool_id_to_name
                                            .remove(tool_use_id)
                                            .unwrap_or_else(|| tool_use_id.to_string());
                                        let content_text = match block.get("content") {
                                            Some(c) if c.is_string() => {
                                                c.as_str().unwrap_or("").to_string()
                                            }
                                            Some(c) if c.is_array() => c
                                                .as_array()
                                                .unwrap()
                                                .iter()
                                                .filter_map(|b| {
                                                    b.get("text").and_then(|t| t.as_str())
                                                })
                                                .collect::<Vec<_>>()
                                                .join("\n"),
                                            _ => String::new(),
                                        };
                                        let summary = orchestrator_events::truncate_display(
                                            &content_text,
                                            200,
                                        );
                                        let _ = proxy_reader.send_event(
                                            glass_core::event::AppEvent::OrchestratorToolResult {
                                                name: tool_name,
                                                output_summary: summary,
                                            },
                                        );
                                    }
                                }
                            }
                        }
                    }
```

- [ ] **Step 4: Verify compilation**

Run: `cargo build`
Expected: Compiles (may have warnings about unused imports — address if needed)

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat(orchestrator): emit thinking/tool_call/tool_result events from reader thread

Captures agent thinking blocks, tool_use with id-to-name mapping,
and tool_result with UTF-8 safe truncation for the orchestrator transcript."
```

### Task 5: Handle new AppEvents and populate event buffer

**Files:**
- Modify: `src/main.rs` (AppEvent handlers)

- [ ] **Step 1: Add handlers for the three new AppEvent variants**

Find the `AppEvent::UsageResume` handler block in `main.rs`. After it, add:

```rust
            AppEvent::OrchestratorThinking { text } => {
                let token_estimate = orchestrator_events::estimate_tokens(&text);
                self.orchestrator_event_buffer.push(
                    orchestrator_events::OrchestratorEvent::Thinking {
                        text,
                        token_estimate,
                    },
                    self.orchestrator.iteration,
                );
                for ctx in self.windows.values() {
                    ctx.window.request_redraw();
                }
            }
            AppEvent::OrchestratorToolCall {
                name,
                params_summary,
            } => {
                self.orchestrator_event_buffer.push(
                    orchestrator_events::OrchestratorEvent::ToolCall {
                        name,
                        params_summary,
                    },
                    self.orchestrator.iteration,
                );
                for ctx in self.windows.values() {
                    ctx.window.request_redraw();
                }
            }
            AppEvent::OrchestratorToolResult {
                name,
                output_summary,
            } => {
                self.orchestrator_event_buffer.push(
                    orchestrator_events::OrchestratorEvent::ToolResult {
                        name,
                        output_summary,
                    },
                    self.orchestrator.iteration,
                );
                for ctx in self.windows.values() {
                    ctx.window.request_redraw();
                }
            }
```

- [ ] **Step 2: Push AgentText event in OrchestratorResponse handler**

In the `AppEvent::OrchestratorResponse { response }` handler (line ~5829), after `self.orchestrator.response_pending = false;`, add:

```rust
                self.orchestrator_event_buffer.push(
                    orchestrator_events::OrchestratorEvent::AgentText {
                        text: response.clone(),
                    },
                    self.orchestrator.iteration,
                );
```

- [ ] **Step 3: Push ContextSent event in OrchestratorSilence handler**

In the `AppEvent::OrchestratorSilence` handler, after the context is sent to the agent (after `self.orchestrator.response_pending = true;` in the normal context send path), add:

```rust
                                        self.orchestrator_event_buffer.push(
                                            orchestrator_events::OrchestratorEvent::ContextSent {
                                                line_count: lines.len(),
                                                has_soi: soi_summary.is_some(),
                                                has_nudge: nudge.is_some(),
                                            },
                                            self.orchestrator.iteration,
                                        );
```

- [ ] **Step 4: Push AgentRespawn event in respawn_orchestrator_agent**

In `respawn_orchestrator_agent()` (line ~1519), at the start of the method, add:

```rust
        self.orchestrator_event_buffer.push(
            orchestrator_events::OrchestratorEvent::AgentRespawn {
                reason: "checkpoint".to_string(),
            },
            self.orchestrator.iteration,
        );
```

- [ ] **Step 5: Push VerifyResult event in VerifyComplete handler**

In the `AppEvent::VerifyComplete` handler, after the metric guard processing (after the `if regressed` / `else` block), add:

```rust
                // Push to orchestrator transcript
                if let Some(first) = verify_results.first() {
                    self.orchestrator_event_buffer.push(
                        orchestrator_events::OrchestratorEvent::VerifyResult {
                            passed: first.tests_passed,
                            failed: first.tests_failed,
                            regressed: orchestrator::MetricBaseline::check_regression(
                                &self
                                    .orchestrator
                                    .metric_baseline
                                    .as_ref()
                                    .map(|b| b.baseline_results.clone())
                                    .unwrap_or_default(),
                                &verify_results,
                            ),
                        },
                        self.orchestrator.iteration,
                    );
                }
```

- [ ] **Step 6: Verify compilation**

Run: `cargo build`
Expected: Compiles cleanly

- [ ] **Step 7: Commit**

```bash
git add src/main.rs
git commit -m "feat(orchestrator): populate event buffer from all orchestrator handlers

Events pushed from: OrchestratorResponse (AgentText), OrchestratorSilence
(ContextSent), respawn (AgentRespawn), VerifyComplete (VerifyResult),
and new thinking/tool_call/tool_result AppEvent handlers."
```

## Chunk 3: Activity Overlay Data Structures

### Task 6: Add Orchestrator filter and display structs

**Files:**
- Modify: `crates/glass_renderer/src/activity_overlay.rs:10-82`

- [ ] **Step 1: Add Orchestrator variant to ActivityViewFilter**

In `crates/glass_renderer/src/activity_overlay.rs`, add `Orchestrator` to the enum (after `Messages`):

```rust
pub enum ActivityViewFilter {
    #[default]
    All,
    Agents,
    Locks,
    Observations,
    Messages,
    Orchestrator,
}
```

Update `next()`:
```rust
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Agents,
            Self::Agents => Self::Locks,
            Self::Locks => Self::Observations,
            Self::Observations => Self::Messages,
            Self::Messages => Self::Orchestrator,
            Self::Orchestrator => Self::All,
        }
    }
```

Update `prev()`:
```rust
    pub fn prev(self) -> Self {
        match self {
            Self::All => Self::Orchestrator,
            Self::Agents => Self::All,
            Self::Locks => Self::Agents,
            Self::Observations => Self::Locks,
            Self::Messages => Self::Observations,
            Self::Orchestrator => Self::Messages,
        }
    }
```

Update `category()`:
```rust
    pub fn category(&self) -> Option<&str> {
        match self {
            Self::All => None,
            Self::Agents => Some("agent"),
            Self::Locks => Some("lock"),
            Self::Observations => Some("observe"),
            Self::Messages => Some("message"),
            Self::Orchestrator => None, // Not category-based
        }
    }
```

Update `label()`:
```rust
    pub fn label(&self) -> &str {
        match self {
            Self::All => "All",
            Self::Agents => "Agents",
            Self::Locks => "Locks",
            Self::Observations => "Observations",
            Self::Messages => "Messages",
            Self::Orchestrator => "Orchestrator",
        }
    }
```

- [ ] **Step 2: Add OrchestratorDashboard and OrchestratorEventDisplay structs**

After `ActivityPinnedAlert` (line ~112), add:

```rust
/// Orchestrator dashboard data for the header section.
#[derive(Debug)]
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

/// Kind of orchestrator event for rendering color/icon selection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OrchestratorEventKind {
    Thinking,
    ToolCall,
    ToolResult,
    AgentText,
    ContextSent,
    Respawn,
    Verify,
}

/// Display-ready orchestrator event for the transcript.
#[derive(Debug)]
pub struct OrchestratorEventDisplay {
    pub id: u64,
    pub iteration: u32,
    pub relative_time: String,
    pub kind: OrchestratorEventKind,
    pub text: String,
    pub expanded: bool,
    pub expandable: bool,
}
```

- [ ] **Step 3: Add fields to ActivityOverlayRenderData**

In the `ActivityOverlayRenderData` struct (line ~68), add after `usage_text`:

```rust
    /// Orchestrator dashboard data (None if never activated).
    pub orchestrator_dashboard: Option<OrchestratorDashboard>,
    /// Orchestrator transcript events for display.
    pub orchestrator_events: Vec<OrchestratorEventDisplay>,
    /// Scroll offset for orchestrator transcript.
    pub orchestrator_scroll_offset: usize,
```

- [ ] **Step 4: Verify compilation**

Run: `cargo build -p glass_renderer`
Expected: May fail due to missing fields in struct construction in main.rs and frame.rs — that's fine, we'll fix in the next task.

- [ ] **Step 5: Update ActivityOverlayRenderData construction in main.rs**

Find where `ActivityOverlayRenderData` is constructed (line ~2489). Add the new fields:

```rust
                            orchestrator_dashboard: None, // Will be populated in Task 8
                            orchestrator_events: Vec::new(),
                            orchestrator_scroll_offset: self.orchestrator_scroll_offset,
```

- [ ] **Step 6: Verify compilation**

Run: `cargo build`
Expected: Compiles cleanly

- [ ] **Step 7: Commit**

```bash
git add crates/glass_renderer/src/activity_overlay.rs src/main.rs
git commit -m "feat(renderer): add Orchestrator filter tab and display data structures

Adds Orchestrator variant to ActivityViewFilter cycle, OrchestratorDashboard,
OrchestratorEventKind, OrchestratorEventDisplay structs, and extends
ActivityOverlayRenderData with orchestrator fields."
```

### Task 7: Populate orchestrator dashboard and events in main.rs

**Files:**
- Modify: `src/main.rs` (activity overlay data population, line ~2446)

- [ ] **Step 1: Build OrchestratorDashboard from state**

In the activity overlay rendering block (line ~2446), before the `ActivityOverlayRenderData` construction, add:

```rust
                        // Build orchestrator dashboard data
                        let orch_dashboard = if self.orchestrator_activated_at.is_some() {
                            let mode = self
                                .config
                                .agent
                                .as_ref()
                                .and_then(|a| a.orchestrator.as_ref())
                                .map(|o| o.orchestrator_mode.clone())
                                .unwrap_or_else(|| "build".to_string());
                            let verify_mode = self
                                .config
                                .agent
                                .as_ref()
                                .and_then(|a| a.orchestrator.as_ref())
                                .map(|o| o.verify_mode.clone())
                                .unwrap_or_else(|| "floor".to_string());
                            let (tests_passed, keep_count, revert_count) =
                                if let Some(ref baseline) = self.orchestrator.metric_baseline {
                                    (
                                        baseline
                                            .last_results
                                            .first()
                                            .and_then(|r| r.tests_passed),
                                        baseline.keep_count,
                                        baseline.revert_count,
                                    )
                                } else {
                                    (None, 0, 0)
                                };
                            let checkpoint_phase = match &self.orchestrator.checkpoint_phase {
                                orchestrator::CheckpointPhase::Idle => "idle".to_string(),
                                orchestrator::CheckpointPhase::WaitingForCheckpoint { .. } => {
                                    "waiting for checkpoint".to_string()
                                }
                                orchestrator::CheckpointPhase::ClearingSent => {
                                    "clearing sent".to_string()
                                }
                            };
                            let paused_reason = if let Ok(st) = self.usage_state.lock() {
                                if st.paused {
                                    Some("Usage limit".to_string())
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                            Some(glass_renderer::OrchestratorDashboard {
                                iteration: self.orchestrator.iteration,
                                iterations_since_checkpoint: self
                                    .orchestrator
                                    .iterations_since_checkpoint,
                                max_iterations: self.orchestrator.max_iterations,
                                mode,
                                verify_mode,
                                tests_passed,
                                keep_count,
                                revert_count,
                                last_completed: self
                                    .orchestrator
                                    .last_checkpoint_completed
                                    .clone(),
                                next_item: self.orchestrator.last_checkpoint_next.clone(),
                                active: self.orchestrator.active,
                                response_pending: self.orchestrator.response_pending,
                                checkpoint_phase,
                                paused_reason,
                            })
                        } else {
                            None
                        };
```

- [ ] **Step 2: Build OrchestratorEventDisplay vec from buffer**

Below the dashboard construction, add:

```rust
                        // Build orchestrator event displays
                        let activated_at = self.orchestrator_activated_at;
                        let orch_events: Vec<glass_renderer::OrchestratorEventDisplay> = self
                            .orchestrator_event_buffer
                            .events
                            .iter()
                            .map(|entry| {
                                let relative_time = activated_at
                                    .map(|at| {
                                        let elapsed = entry
                                            .timestamp
                                            .duration_since(at);
                                        let total_secs = elapsed.as_secs();
                                        format!("{:02}:{:02}", total_secs / 60, total_secs % 60)
                                    })
                                    .unwrap_or_else(|| "--:--".to_string());

                                let (kind, text, expandable) = match &entry.event {
                                    orchestrator_events::OrchestratorEvent::Thinking {
                                        token_estimate,
                                        text,
                                    } => (
                                        glass_renderer::OrchestratorEventKind::Thinking,
                                        format!("Thinking...  ({token_estimate} tokens)"),
                                        true,
                                    ),
                                    orchestrator_events::OrchestratorEvent::ToolCall {
                                        name,
                                        params_summary,
                                    } => (
                                        glass_renderer::OrchestratorEventKind::ToolCall,
                                        format!("-> {name}({params_summary})"),
                                        false,
                                    ),
                                    orchestrator_events::OrchestratorEvent::ToolResult {
                                        name,
                                        output_summary,
                                    } => (
                                        glass_renderer::OrchestratorEventKind::ToolResult,
                                        format!("-> {name} -> {output_summary}"),
                                        false,
                                    ),
                                    orchestrator_events::OrchestratorEvent::AgentText {
                                        text,
                                    } => (
                                        glass_renderer::OrchestratorEventKind::AgentText,
                                        format!(
                                            "Agent: \"{}\"",
                                            orchestrator_events::truncate_display(text, 120)
                                        ),
                                        false,
                                    ),
                                    orchestrator_events::OrchestratorEvent::ContextSent {
                                        line_count,
                                        has_soi,
                                        has_nudge,
                                    } => {
                                        let mut details = format!("{line_count} lines");
                                        if *has_soi {
                                            details.push_str(", SOI");
                                        }
                                        if *has_nudge {
                                            details.push_str(", nudge");
                                        }
                                        (
                                            glass_renderer::OrchestratorEventKind::ContextSent,
                                            format!("Context sent ({details})"),
                                            false,
                                        )
                                    }
                                    orchestrator_events::OrchestratorEvent::AgentRespawn {
                                        reason,
                                    } => (
                                        glass_renderer::OrchestratorEventKind::Respawn,
                                        format!("--- Agent respawned ({reason}) ---"),
                                        false,
                                    ),
                                    orchestrator_events::OrchestratorEvent::VerifyResult {
                                        passed,
                                        failed,
                                        regressed,
                                    } => {
                                        let icon = if *regressed { "X" } else { "ok" };
                                        let p = passed
                                            .map(|v| v.to_string())
                                            .unwrap_or_else(|| "?".into());
                                        let f = failed
                                            .map(|v| v.to_string())
                                            .unwrap_or_else(|| "?".into());
                                        (
                                            glass_renderer::OrchestratorEventKind::Verify,
                                            format!("{icon} Verify: {p} passed, {f} failed"),
                                            false,
                                        )
                                    }
                                };

                                let expanded = if expandable {
                                    self.orchestrator_event_buffer.is_expanded(entry.id)
                                } else {
                                    false
                                };

                                // If expanded, replace summary text with full thinking text
                                let display_text = if expanded {
                                    if let orchestrator_events::OrchestratorEvent::Thinking {
                                        text,
                                        ..
                                    } = &entry.event
                                    {
                                        text.clone()
                                    } else {
                                        text
                                    }
                                } else {
                                    text
                                };

                                glass_renderer::OrchestratorEventDisplay {
                                    id: entry.id,
                                    iteration: entry.iteration,
                                    relative_time,
                                    kind,
                                    text: display_text,
                                    expanded,
                                    expandable,
                                }
                            })
                            .collect();
```

- [ ] **Step 3: Wire into ActivityOverlayRenderData**

Replace the placeholder `orchestrator_dashboard: None` and `orchestrator_events: Vec::new()` with:

```rust
                            orchestrator_dashboard: orch_dashboard,
                            orchestrator_events: orch_events,
```

- [ ] **Step 4: Verify compilation**

Run: `cargo build`
Expected: Compiles cleanly

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat(orchestrator): populate dashboard and transcript data for overlay rendering"
```

## Chunk 4: Overlay Rendering & Keyboard

### Task 8: Build orchestrator tab rendering

**Files:**
- Modify: `crates/glass_renderer/src/activity_overlay.rs`
- Modify: `crates/glass_renderer/src/frame.rs`

- [ ] **Step 1: Add build_orchestrator_text method to ActivityOverlayRenderer**

In `crates/glass_renderer/src/activity_overlay.rs`, after the existing `build_overlay_text` method, add a new method:

```rust
    /// Build text labels for the Orchestrator tab (dashboard header + transcript).
    pub fn build_orchestrator_text(
        &self,
        data: &ActivityOverlayRenderData,
        width: f32,
        height: f32,
    ) -> Vec<ActivityOverlayTextLabel> {
        let mut labels = Vec::new();
        let margin = 20.0;
        let line_h = self.cell_height;
        let mut y = margin;

        // Title + filter tabs (same as other tabs)
        let tab_labels: Vec<&str> = [
            ActivityViewFilter::All,
            ActivityViewFilter::Agents,
            ActivityViewFilter::Locks,
            ActivityViewFilter::Observations,
            ActivityViewFilter::Messages,
            ActivityViewFilter::Orchestrator,
        ]
        .iter()
        .map(|f| f.label())
        .collect();

        let mut tab_x = margin;
        for (i, label) in tab_labels.iter().enumerate() {
            let filter = match i {
                0 => ActivityViewFilter::All,
                1 => ActivityViewFilter::Agents,
                2 => ActivityViewFilter::Locks,
                3 => ActivityViewFilter::Observations,
                4 => ActivityViewFilter::Messages,
                5 => ActivityViewFilter::Orchestrator,
                _ => ActivityViewFilter::All,
            };
            let color = if filter == data.filter {
                Rgb {
                    r: 100,
                    g: 200,
                    b: 255,
                }
            } else {
                Rgb {
                    r: 120,
                    g: 120,
                    b: 120,
                }
            };
            labels.push(ActivityOverlayTextLabel {
                text: format!("[{label}]"),
                x: tab_x,
                y,
                color,
            });
            tab_x += (label.len() as f32 + 3.0) * self.cell_width;
        }
        y += line_h * 1.5;

        // Dashboard header
        if let Some(ref dash) = data.orchestrator_dashboard {
            let header_color = if dash.active {
                Rgb {
                    r: 0,
                    g: 200,
                    b: 120,
                }
            } else {
                Rgb {
                    r: 120,
                    g: 120,
                    b: 120,
                }
            };

            // Line 1: Title + iteration
            let iter_text = if let Some(max) = dash.max_iterations {
                format!(
                    "ORCHESTRATOR                iter #{} ({} since checkpoint, max {})",
                    dash.iteration, dash.iterations_since_checkpoint, max
                )
            } else {
                format!(
                    "ORCHESTRATOR                iter #{} ({} since checkpoint)",
                    dash.iteration, dash.iterations_since_checkpoint
                )
            };
            labels.push(ActivityOverlayTextLabel {
                text: iter_text,
                x: margin,
                y,
                color: header_color,
            });
            y += line_h;

            // Line 2: Mode + verify + guard
            let verify_text = if let Some(passed) = dash.tests_passed {
                format!("{} ({} passed)", dash.verify_mode, passed)
            } else {
                dash.verify_mode.clone()
            };
            labels.push(ActivityOverlayTextLabel {
                text: format!(
                    "Mode: {} | Verify: {} | Guard: {} kept, {} reverted",
                    dash.mode, verify_text, dash.keep_count, dash.revert_count
                ),
                x: margin,
                y,
                color: Rgb {
                    r: 180,
                    g: 180,
                    b: 180,
                },
            });
            y += line_h;

            // Line 3: Status
            let status = if let Some(ref reason) = dash.paused_reason {
                format!("Status: PAUSED ({reason})")
            } else if dash.response_pending {
                "Status: waiting for agent response".to_string()
            } else if dash.active {
                "Status: active".to_string()
            } else {
                "Status: inactive".to_string()
            };
            labels.push(ActivityOverlayTextLabel {
                text: status,
                x: margin,
                y,
                color: Rgb {
                    r: 180,
                    g: 180,
                    b: 180,
                },
            });
            y += line_h;

            // Line 4: Checkpoint info
            if !dash.last_completed.is_empty() || !dash.next_item.is_empty() {
                labels.push(ActivityOverlayTextLabel {
                    text: format!(
                        "Last: \"{}\" -> Next: \"{}\"",
                        dash.last_completed, dash.next_item
                    ),
                    x: margin,
                    y,
                    color: Rgb {
                        r: 150,
                        g: 150,
                        b: 150,
                    },
                });
                y += line_h;
            }

            // Separator
            y += line_h * 0.5;
            labels.push(ActivityOverlayTextLabel {
                text: "-".repeat((width / self.cell_width) as usize - 4),
                x: margin,
                y,
                color: Rgb {
                    r: 60,
                    g: 60,
                    b: 60,
                },
            });
            y += line_h;
        } else {
            labels.push(ActivityOverlayTextLabel {
                text: "Orchestrator inactive -- press Ctrl+Shift+O to start".to_string(),
                x: margin,
                y,
                color: Rgb {
                    r: 120,
                    g: 120,
                    b: 120,
                },
            });
            y += line_h * 2.0;
        }

        // Transcript events
        let available_lines = ((height - y - margin) / line_h).max(0.0) as usize;
        let total_events = data.orchestrator_events.len();
        let start = if total_events > available_lines + data.orchestrator_scroll_offset {
            total_events - available_lines - data.orchestrator_scroll_offset
        } else {
            0
        };
        let end = if total_events > data.orchestrator_scroll_offset {
            total_events - data.orchestrator_scroll_offset
        } else {
            0
        };

        for event in &data.orchestrator_events[start..end] {
            let color = match event.kind {
                OrchestratorEventKind::Thinking => Rgb {
                    r: 100,
                    g: 100,
                    b: 100,
                },
                OrchestratorEventKind::ToolCall => Rgb {
                    r: 100,
                    g: 150,
                    b: 255,
                },
                OrchestratorEventKind::ToolResult => Rgb {
                    r: 80,
                    g: 200,
                    b: 200,
                },
                OrchestratorEventKind::AgentText => Rgb {
                    r: 80,
                    g: 220,
                    b: 120,
                },
                OrchestratorEventKind::ContextSent => Rgb {
                    r: 80,
                    g: 80,
                    b: 80,
                },
                OrchestratorEventKind::Respawn => Rgb {
                    r: 220,
                    g: 180,
                    b: 60,
                },
                OrchestratorEventKind::Verify => Rgb {
                    r: 80,
                    g: 220,
                    b: 120,
                },
            };

            let prefix = format!("[#{} {}]  ", event.iteration, event.relative_time);
            labels.push(ActivityOverlayTextLabel {
                text: format!("{prefix}{}", event.text),
                x: margin,
                y,
                color,
            });

            if event.expanded {
                // Expanded thinking takes multiple lines
                let text_lines: Vec<&str> = event.text.lines().collect();
                for line in text_lines.iter().skip(1) {
                    y += line_h;
                    if y > height - margin {
                        break;
                    }
                    labels.push(ActivityOverlayTextLabel {
                        text: format!("    {line}"),
                        x: margin,
                        y,
                        color: Rgb {
                            r: 120,
                            g: 120,
                            b: 120,
                        },
                    });
                }
            }

            y += line_h;
            if y > height - margin {
                break;
            }
        }

        labels
    }
```

- [ ] **Step 2: Branch on filter in draw_activity_overlay (frame.rs)**

In `crates/glass_renderer/src/frame.rs`, in the `draw_activity_overlay` method (line ~2333), find where `build_overlay_text` is called (line ~2352). Wrap it in a filter check:

```rust
        let labels = if data.filter == glass_renderer::ActivityViewFilter::Orchestrator {
            overlay_renderer.build_orchestrator_text(&data, width as f32, height as f32)
        } else {
            overlay_renderer.build_overlay_text(&data, width as f32, height as f32)
        };
```

Note: You may need to add `use crate::activity_overlay::ActivityViewFilter;` or reference it via the full path. Check existing import patterns in frame.rs.

- [ ] **Step 3: Export new types from glass_renderer lib.rs**

In `crates/glass_renderer/src/lib.rs`, find where `ActivityOverlayRenderData` is re-exported. Add the new types:

```rust
pub use activity_overlay::OrchestratorDashboard;
pub use activity_overlay::OrchestratorEventDisplay;
pub use activity_overlay::OrchestratorEventKind;
```

- [ ] **Step 4: Verify compilation**

Run: `cargo build`
Expected: Compiles cleanly

- [ ] **Step 5: Commit**

```bash
git add crates/glass_renderer/src/activity_overlay.rs crates/glass_renderer/src/frame.rs crates/glass_renderer/src/lib.rs
git commit -m "feat(renderer): render orchestrator dashboard and transcript in overlay

Single-column layout with dashboard header (iteration, mode, verify stats,
checkpoint info) and color-coded scrollable transcript."
```

### Task 9: Add keyboard handlers for orchestrator transcript

**Files:**
- Modify: `src/main.rs` (activity overlay keyboard handler, line ~3800)

- [ ] **Step 1: Add orchestrator-specific scroll handling**

In the activity overlay keyboard handler (line ~3800), add a check for the Orchestrator filter. Before the existing ArrowUp/ArrowDown handlers, add:

```rust
                        // Orchestrator tab uses separate scroll offset
                        if self.activity_view_filter
                            == glass_renderer::ActivityViewFilter::Orchestrator
                        {
                            match event.logical_key {
                                Key::Named(NamedKey::ArrowUp) | Key::Named(NamedKey::PageUp) => {
                                    self.orchestrator_scroll_offset =
                                        self.orchestrator_scroll_offset.saturating_add(
                                            if matches!(
                                                event.logical_key,
                                                Key::Named(NamedKey::PageUp)
                                            ) {
                                                20
                                            } else {
                                                1
                                            },
                                        );
                                    for ctx in self.windows.values() {
                                        ctx.window.request_redraw();
                                    }
                                    return;
                                }
                                Key::Named(NamedKey::ArrowDown)
                                | Key::Named(NamedKey::PageDown) => {
                                    self.orchestrator_scroll_offset =
                                        self.orchestrator_scroll_offset.saturating_sub(
                                            if matches!(
                                                event.logical_key,
                                                Key::Named(NamedKey::PageDown)
                                            ) {
                                                20
                                            } else {
                                                1
                                            },
                                        );
                                    for ctx in self.windows.values() {
                                        ctx.window.request_redraw();
                                    }
                                    return;
                                }
                                _ => {}
                            }
                        }
```

- [ ] **Step 2: Reset orchestrator scroll when closing overlay**

In the Ctrl+Shift+G handler (line ~3120), in the closing block (when `!self.activity_overlay_visible`), add:

```rust
                                    self.orchestrator_scroll_offset = 0;
```

- [ ] **Step 3: Verify compilation**

Run: `cargo build`
Expected: Compiles cleanly

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat(orchestrator): add keyboard scroll handling for orchestrator transcript

Separate scroll offset for orchestrator tab, PageUp/Down for 20-line jumps,
resets on overlay close."
```

## Chunk 5: Background Tabs

### Task 10: Add background parameter to add_tab

**Files:**
- Modify: `crates/glass_mux/src/session_mux.rs:89-114`

- [ ] **Step 1: Add background parameter to add_tab**

Change the signature of `add_tab` from:

```rust
pub fn add_tab(&mut self, session: Session) -> TabId {
```

To:

```rust
pub fn add_tab(&mut self, session: Session, background: bool) -> TabId {
```

In the method body, change the tab activation logic. Find where `self.active_tab` is set (line ~111). Change from unconditionally setting it to:

```rust
        if background {
            // Background tabs go at the end, don't change focus
            self.tabs.push(tab);
        } else {
            // Foreground tabs insert after active and become active
            let insert_pos = (self.active_tab + 1).min(self.tabs.len());
            self.tabs.insert(insert_pos, tab);
            self.active_tab = insert_pos;
        }
```

Note: You'll need to adjust the existing code that inserts and sets active_tab. Read the current implementation carefully to get the exact replacement right.

- [ ] **Step 2: Update all callers to pass background: false**

Search for `.add_tab(` across the codebase. Update each caller:

In `src/main.rs`, find all calls to `session_mux.add_tab(session)` (lines ~2993, ~4153, ~6791) and change to `session_mux.add_tab(session, false)`.

The MCP tab_create handler (line ~6791) is special — we'll make it conditional in the next step.

- [ ] **Step 3: Make MCP tab_create use background when orchestrating**

In the MCP tab_create handler (line ~6791), change from:

```rust
session_mux.add_tab(session, false)
```

To:

```rust
session_mux.add_tab(session, self.orchestrator.active)
```

- [ ] **Step 4: Verify compilation**

Run: `cargo build`
Expected: Compiles cleanly

- [ ] **Step 5: Add test for background tab behavior**

In `crates/glass_mux/src/session_mux.rs`, in the test module, add:

```rust
    #[test]
    fn background_tab_does_not_change_active() {
        // Setup would require creating a SessionMux with mock sessions
        // This is a structural test — verify the active_tab doesn't change
        // when add_tab is called with background=true
    }
```

Note: If `SessionMux` requires complex setup (PTY, terminal), this test may need to be integration-level or verified manually. Check existing tests in the file for patterns.

- [ ] **Step 6: Commit**

```bash
git add crates/glass_mux/src/session_mux.rs src/main.rs
git commit -m "feat(mux): add background tab support for MCP-created tabs

add_tab() takes a background parameter. Background tabs append to the end
of the tab list without changing focus. MCP tab_create uses background
mode when the orchestrator is active."
```

### Task 11: Visual indicator for agent-created tabs

**Files:**
- Modify: `crates/glass_renderer/src/tab_bar.rs:14-21`
- Modify: `src/main.rs` (where TabDisplayInfo is built)

- [ ] **Step 1: Add agent_created field to TabDisplayInfo**

In `crates/glass_renderer/src/tab_bar.rs`, add to `TabDisplayInfo`:

```rust
    /// Whether this tab was created by the orchestrator agent.
    pub agent_created: bool,
```

- [ ] **Step 2: Dim agent-created tabs in rendering**

In the tab bar rendering code (find where tab label colors are set based on `is_active`), add a dimming adjustment for agent-created tabs. If the tab has `agent_created: true` and is not active, use a dimmer color (e.g., reduce RGB values by 40%).

- [ ] **Step 3: Update TabDisplayInfo construction in main.rs**

Find where `TabDisplayInfo` is constructed (search for `TabDisplayInfo {`). Add `agent_created: false` for all existing constructions. For MCP-created tabs, we'll need to track this on the session or tab.

Since tracking which tabs were agent-created requires adding state to `Tab` or `Session`, and the spec says to start simple, use `agent_created: false` for now. This can be enhanced later by adding an `agent_created: bool` to the `Tab` struct in `glass_mux`.

- [ ] **Step 4: Verify compilation**

Run: `cargo build`
Expected: Compiles cleanly

- [ ] **Step 5: Commit**

```bash
git add crates/glass_renderer/src/tab_bar.rs src/main.rs
git commit -m "feat(tab-bar): add agent_created field for future tab dimming

TabDisplayInfo gains agent_created bool. Currently always false —
will be wired to actual agent tab tracking in a follow-up."
```

---

## Summary

| Task | Component | Files |
|------|-----------|-------|
| 1 | Event buffer structs + tests | `src/orchestrator_events.rs` (new) |
| 2 | AppEvent variants | `crates/glass_core/src/event.rs` |
| 3 | Processor fields | `src/main.rs` |
| 4 | Reader thread extensions | `src/main.rs` |
| 5 | Event buffer population | `src/main.rs` |
| 6 | Overlay data structures | `crates/glass_renderer/src/activity_overlay.rs` |
| 7 | Dashboard + events data | `src/main.rs` |
| 8 | Overlay rendering | `activity_overlay.rs`, `frame.rs`, `lib.rs` |
| 9 | Keyboard handlers | `src/main.rs` |
| 10 | Background tabs | `session_mux.rs`, `src/main.rs` |
| 11 | Tab visual indicator | `tab_bar.rs`, `src/main.rs` |
