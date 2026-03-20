//! Claude CLI backend.
//!
//! Implements parsing of Claude CLI's `--output-format stream-json` line-by-line
//! output format and normalization into the provider-agnostic [`AgentEvent`] enum.
//!
//! Each line emitted by the Claude CLI is a JSON object with a `"type"` field.
//! This module maps those objects to zero or more [`AgentEvent`]s.

use crate::AgentEvent;

/// Placeholder for the Claude CLI backend implementation.
///
/// The full [`crate::AgentBackend`] trait implementation is added in Task 3.
pub struct ClaudeCliBackend;

/// Parse a single JSON line from Claude CLI's `stream-json` output into
/// zero or more [`AgentEvent`]s.
///
/// Returns an empty `Vec` for:
/// - empty / whitespace-only lines
/// - lines that are not valid JSON
/// - lines whose `"type"` value is not handled
///
/// A single line can produce multiple events (e.g. a `"thinking"` block
/// followed by a `"text"` block in the same `"assistant"` message).
pub(crate) fn parse_stream_json_line(line: &str) -> Vec<AgentEvent> {
    if line.trim().is_empty() {
        return vec![];
    }
    let val: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    match val.get("type").and_then(|t| t.as_str()) {
        // ── system ────────────────────────────────────────────────────────────
        Some("system") => {
            if val.get("subtype").and_then(|s| s.as_str()) == Some("init") {
                let session_id = val
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                vec![AgentEvent::Init { session_id }]
            } else {
                vec![]
            }
        }

        // ── assistant ─────────────────────────────────────────────────────────
        Some("assistant") => {
            let mut events: Vec<AgentEvent> = Vec::new();
            let mut accumulated_text = String::new();

            if let Some(arr) = val
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for block in arr {
                    match block.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                accumulated_text.push_str(text);
                            }
                        }
                        Some("thinking") => {
                            if let Some(text) = block.get("thinking").and_then(|t| t.as_str()) {
                                events.push(AgentEvent::Thinking {
                                    text: text.to_string(),
                                });
                            }
                        }
                        Some("tool_use") => {
                            let name = block
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("?")
                                .to_string();
                            let id = block
                                .get("id")
                                .and_then(|i| i.as_str())
                                .unwrap_or("")
                                .to_string();
                            let input = block
                                .get("input")
                                .map(|i| i.to_string())
                                .unwrap_or_default();
                            events.push(AgentEvent::ToolCall { name, id, input });
                        }
                        _ => {}
                    }
                }
            }

            if !accumulated_text.is_empty() {
                events.push(AgentEvent::AssistantText {
                    text: accumulated_text,
                });
            }

            events
        }

        // ── result ────────────────────────────────────────────────────────────
        Some("result") => {
            let cost_usd =
                glass_core::agent_runtime::parse_cost_from_result(line).unwrap_or(0.0);
            vec![AgentEvent::TurnComplete { cost_usd }]
        }

        // ── user (tool results) ───────────────────────────────────────────────
        Some("user") => {
            let mut events: Vec<AgentEvent> = Vec::new();

            if let Some(arr) = val
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for block in arr {
                    if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                        let tool_use_id = block
                            .get("tool_use_id")
                            .and_then(|t| t.as_str())
                            .unwrap_or("?")
                            .to_string();
                        let content = match block.get("content") {
                            Some(c) if c.is_string() => {
                                c.as_str().unwrap_or("").to_string()
                            }
                            Some(c) if c.is_array() => c
                                .as_array()
                                .unwrap()
                                .iter()
                                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                                .collect::<Vec<_>>()
                                .join("\n"),
                            _ => String::new(),
                        };
                        events.push(AgentEvent::ToolResult {
                            tool_use_id,
                            content,
                        });
                    }
                }
            }

            events
        }

        _ => vec![],
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: assert exactly one event is returned and return it.
    fn single(line: &str) -> AgentEvent {
        let mut events = parse_stream_json_line(line);
        assert_eq!(events.len(), 1, "expected exactly 1 event, got {:?}", events);
        events.remove(0)
    }

    // ── system ────────────────────────────────────────────────────────────────

    #[test]
    fn parse_system_init() {
        let line = r#"{"type":"system","subtype":"init","session_id":"sess-123"}"#;
        match single(line) {
            AgentEvent::Init { session_id } => assert_eq!(session_id, "sess-123"),
            other => panic!("expected Init, got {:?}", other),
        }
    }

    #[test]
    fn parse_system_non_init_ignored() {
        let line = r#"{"type":"system","subtype":"heartbeat"}"#;
        assert!(parse_stream_json_line(line).is_empty());
    }

    // ── assistant ─────────────────────────────────────────────────────────────

    #[test]
    fn parse_assistant_text() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hello!"}]}}"#;
        match single(line) {
            AgentEvent::AssistantText { text } => assert_eq!(text, "Hello!"),
            other => panic!("expected AssistantText, got {:?}", other),
        }
    }

    #[test]
    fn parse_assistant_thinking() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"Let me think..."}]}}"#;
        match single(line) {
            AgentEvent::Thinking { text } => assert_eq!(text, "Let me think..."),
            other => panic!("expected Thinking, got {:?}", other),
        }
    }

    #[test]
    fn parse_assistant_tool_use() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash","id":"tool-abc","input":{"command":"ls"}}]}}"#;
        match single(line) {
            AgentEvent::ToolCall { name, id, input } => {
                assert_eq!(name, "Bash");
                assert_eq!(id, "tool-abc");
                // input is the JSON-serialised representation
                assert!(input.contains("ls"), "input should contain 'ls', got: {input}");
            }
            other => panic!("expected ToolCall, got {:?}", other),
        }
    }

    #[test]
    fn parse_assistant_multiple_text_blocks_concatenates() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"foo"},{"type":"text","text":"bar"}]}}"#;
        match single(line) {
            AgentEvent::AssistantText { text } => assert_eq!(text, "foobar"),
            other => panic!("expected AssistantText, got {:?}", other),
        }
    }

    #[test]
    fn parse_assistant_mixed_blocks() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"done"}]}}"#;
        let events = parse_stream_json_line(line);
        assert_eq!(events.len(), 2, "expected 2 events, got {:?}", events);
        match &events[0] {
            AgentEvent::Thinking { text } => assert_eq!(text, "hmm"),
            other => panic!("expected Thinking first, got {:?}", other),
        }
        match &events[1] {
            AgentEvent::AssistantText { text } => assert_eq!(text, "done"),
            other => panic!("expected AssistantText second, got {:?}", other),
        }
    }

    // ── result ────────────────────────────────────────────────────────────────

    #[test]
    fn parse_result_with_cost() {
        let line = r#"{"type":"result","cost_usd":0.0042}"#;
        match single(line) {
            AgentEvent::TurnComplete { cost_usd } => {
                assert!((cost_usd - 0.0042).abs() < 1e-9, "cost mismatch: {cost_usd}")
            }
            other => panic!("expected TurnComplete, got {:?}", other),
        }
    }

    #[test]
    fn parse_result_without_cost() {
        let line = r#"{"type":"result"}"#;
        match single(line) {
            AgentEvent::TurnComplete { cost_usd } => {
                assert_eq!(cost_usd, 0.0)
            }
            other => panic!("expected TurnComplete, got {:?}", other),
        }
    }

    // ── user / tool_result ────────────────────────────────────────────────────

    #[test]
    fn parse_user_tool_result_string() {
        let line = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tid-1","content":"output text"}]}}"#;
        match single(line) {
            AgentEvent::ToolResult {
                tool_use_id,
                content,
            } => {
                assert_eq!(tool_use_id, "tid-1");
                assert_eq!(content, "output text");
            }
            other => panic!("expected ToolResult, got {:?}", other),
        }
    }

    #[test]
    fn parse_user_tool_result_array() {
        let line = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tid-2","content":[{"type":"text","text":"line1"},{"type":"text","text":"line2"}]}]}}"#;
        match single(line) {
            AgentEvent::ToolResult {
                tool_use_id,
                content,
            } => {
                assert_eq!(tool_use_id, "tid-2");
                assert_eq!(content, "line1\nline2");
            }
            other => panic!("expected ToolResult, got {:?}", other),
        }
    }

    // ── edge cases ────────────────────────────────────────────────────────────

    #[test]
    fn parse_empty_line_returns_empty() {
        assert!(parse_stream_json_line("").is_empty());
        assert!(parse_stream_json_line("   ").is_empty());
    }

    #[test]
    fn parse_invalid_json_returns_empty() {
        assert!(parse_stream_json_line("not json").is_empty());
    }

    #[test]
    fn parse_unknown_type_returns_empty() {
        let line = r#"{"type":"unknown"}"#;
        assert!(parse_stream_json_line(line).is_empty());
    }
}
