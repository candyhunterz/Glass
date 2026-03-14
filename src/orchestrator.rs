//! Orchestrator: drives Claude Code sessions autonomously via the Glass Agent.
//!
//! Owns the silence-triggered loop that captures terminal context, sends it
//! to the Glass Agent, and routes the response (type into PTY, wait, or checkpoint).

/// Parsed response from the Glass Agent.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentResponse {
    /// Type this text into the terminal.
    TypeText(String),
    /// Claude Code is still working; reset silence timer and check again later.
    Wait,
    /// Feature complete; trigger context refresh cycle.
    Checkpoint {
        completed: String,
        next: String,
    },
}

/// Parse a raw Glass Agent response into a structured action.
pub fn parse_agent_response(raw: &str) -> AgentResponse {
    let trimmed = raw.trim();

    if trimmed == "GLASS_WAIT" {
        return AgentResponse::Wait;
    }

    let checkpoint_marker = "GLASS_CHECKPOINT:";
    if let Some(start) = trimmed.find(checkpoint_marker) {
        let after = trimmed[start + checkpoint_marker.len()..].trim();
        if let Some(json_start) = after.find('{') {
            let json_slice = &after[json_start..];
            // Find matching closing brace
            let mut depth = 0usize;
            let mut end = None;
            for (i, ch) in json_slice.char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            end = Some(i + 1);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(end_idx) = end {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_slice[..end_idx]) {
                    let completed = val
                        .get("completed")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let next = val
                        .get("next")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    return AgentResponse::Checkpoint { completed, next };
                }
            }
        }
    }

    // Default: type the text into the terminal
    AgentResponse::TypeText(trimmed.to_string())
}

/// Orchestrator state, lives on Processor in main.rs.
pub struct OrchestratorState {
    /// Whether orchestration is active (toggled by Ctrl+Shift+O).
    pub active: bool,
    /// Iteration counter (for status bar display and logging).
    pub iteration: u32,
    /// Last N responses for stuck detection (ring buffer).
    pub recent_responses: Vec<String>,
    /// Max identical responses before stuck triggers.
    pub max_retries: u32,
}

impl OrchestratorState {
    pub fn new(max_retries: u32) -> Self {
        Self {
            active: false,
            iteration: 0,
            max_retries,
            recent_responses: Vec::new(),
        }
    }

    /// Record a response and check if we're stuck (N identical consecutive responses).
    /// Returns true if stuck.
    pub fn record_response(&mut self, response: &str) -> bool {
        self.recent_responses.push(response.to_string());
        if self.recent_responses.len() > self.max_retries as usize {
            self.recent_responses
                .drain(..self.recent_responses.len() - self.max_retries as usize);
        }
        if self.recent_responses.len() >= self.max_retries as usize {
            self.recent_responses
                .iter()
                .all(|r| r == &self.recent_responses[0])
        } else {
            false
        }
    }

    /// Reset stuck detection (e.g., after a successful verification).
    pub fn reset_stuck(&mut self) {
        self.recent_responses.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plain_text() {
        let resp = parse_agent_response("continue with the next feature");
        assert_eq!(
            resp,
            AgentResponse::TypeText("continue with the next feature".to_string())
        );
    }

    #[test]
    fn parse_wait() {
        assert_eq!(parse_agent_response("GLASS_WAIT"), AgentResponse::Wait);
        assert_eq!(parse_agent_response("  GLASS_WAIT  "), AgentResponse::Wait);
    }

    #[test]
    fn parse_checkpoint() {
        let raw = r#"GLASS_CHECKPOINT: {"completed": "auth module", "next": "database layer"}"#;
        match parse_agent_response(raw) {
            AgentResponse::Checkpoint { completed, next } => {
                assert_eq!(completed, "auth module");
                assert_eq!(next, "database layer");
            }
            other => panic!("Expected Checkpoint, got {:?}", other),
        }
    }

    #[test]
    fn parse_checkpoint_with_extra_text() {
        let raw = r#"Some preamble GLASS_CHECKPOINT: {"completed": "x", "next": "y"} trailing"#;
        match parse_agent_response(raw) {
            AgentResponse::Checkpoint { completed, next } => {
                assert_eq!(completed, "x");
                assert_eq!(next, "y");
            }
            other => panic!("Expected Checkpoint, got {:?}", other),
        }
    }

    #[test]
    fn parse_malformed_checkpoint_falls_back_to_text() {
        let raw = "GLASS_CHECKPOINT: not json";
        match parse_agent_response(raw) {
            AgentResponse::TypeText(_) => {} // expected fallback
            other => panic!("Expected TypeText fallback, got {:?}", other),
        }
    }

    #[test]
    fn stuck_detection_triggers_after_n_identical() {
        let mut state = OrchestratorState::new(3);
        assert!(!state.record_response("fix the test"));
        assert!(!state.record_response("fix the test"));
        assert!(state.record_response("fix the test")); // 3rd identical
    }

    #[test]
    fn stuck_detection_resets_on_different_response() {
        let mut state = OrchestratorState::new(3);
        state.record_response("fix the test");
        state.record_response("fix the test");
        assert!(!state.record_response("try a different approach")); // different
    }

    #[test]
    fn stuck_detection_reset_clears() {
        let mut state = OrchestratorState::new(3);
        state.record_response("fix the test");
        state.record_response("fix the test");
        state.reset_stuck();
        assert!(!state.record_response("fix the test")); // reset, only 1 now
    }
}
