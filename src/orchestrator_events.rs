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
    ToolCall {
        name: String,
        params_summary: String,
    },
    /// Result returned from a tool call.
    ToolResult {
        name: String,
        output_summary: String,
    },
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
    /// Orchestrator gathered context files (PRD, instructions, etc.).
    // Emitted by orchestrator startup — suppressed until Task 5 wires emission.
    #[allow(dead_code)]
    ContextGathered { files: String, size_bytes: usize },
    /// Agent process was spawned (initial activation only, not checkpoints).
    // Emitted by orchestrator startup — suppressed until Task 5 wires emission.
    #[allow(dead_code)]
    AgentSpawned,
    /// Agent sent its first response after spawn.
    // Emitted by orchestrator startup — suppressed until Task 5 wires emission.
    #[allow(dead_code)]
    AgentResponded { elapsed_secs: u64 },
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
    #[allow(dead_code)] // Used by future Enter key handler in activity overlay
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_retrieve() {
        let mut buf = OrchestratorEventBuffer::new();
        buf.push(
            OrchestratorEvent::AgentText {
                text: "hello".into(),
            },
            1,
        );
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
