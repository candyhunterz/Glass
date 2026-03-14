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
    /// All PRD items are complete; stop orchestration.
    Done {
        summary: String,
    },
}

/// Parse a raw Glass Agent response into a structured action.
pub fn parse_agent_response(raw: &str) -> AgentResponse {
    let trimmed = raw.trim();

    if trimmed == "GLASS_WAIT" {
        return AgentResponse::Wait;
    }

    if trimmed.starts_with("GLASS_DONE") {
        let summary = trimmed
            .strip_prefix("GLASS_DONE:")
            .or_else(|| trimmed.strip_prefix("GLASS_DONE"))
            .unwrap_or("")
            .trim()
            .to_string();
        return AgentResponse::Done { summary };
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

/// State of a checkpoint refresh cycle.
#[derive(Debug, Clone, PartialEq)]
pub enum CheckpointPhase {
    /// Not in a checkpoint cycle.
    Idle,
    /// Waiting for Claude Code to write checkpoint.md (polling mtime).
    WaitingForCheckpoint {
        started_at: std::time::Instant,
        last_mtime: Option<std::time::SystemTime>,
    },
    /// Checkpoint written; waiting for /clear to complete.
    ClearingSent,
}

/// How many iterations before forcing an automatic context refresh.
pub const AUTO_CHECKPOINT_INTERVAL: u32 = 15;

/// Grace period after the orchestrator types into the PTY.
/// PromptStart events within this window are ignored for crash recovery.
pub const CRASH_RECOVERY_GRACE_SECS: u64 = 10;

/// Maximum time to wait for Claude Code to write checkpoint.md before respawning anyway.
pub const CHECKPOINT_TIMEOUT_SECS: u64 = 180;

/// Orchestrator state, lives on Processor in main.rs.
pub struct OrchestratorState {
    /// Whether orchestration is active (toggled by Ctrl+Shift+O).
    pub active: bool,
    /// Iteration counter (for status bar display and logging).
    pub iteration: u32,
    /// Iteration count since the last checkpoint (resets on refresh).
    pub iterations_since_checkpoint: u32,
    /// Last N responses for stuck detection (ring buffer).
    pub recent_responses: Vec<String>,
    /// Max identical responses before stuck triggers.
    pub max_retries: u32,
    /// Current checkpoint refresh cycle state.
    pub checkpoint_phase: CheckpointPhase,
    /// Summary of what was completed (from GLASS_CHECKPOINT).
    pub last_checkpoint_completed: String,
    /// Next item to work on (from GLASS_CHECKPOINT).
    pub last_checkpoint_next: String,
    /// Timestamp of last PTY write by the orchestrator (for crash recovery grace period).
    pub last_pty_write: Option<std::time::Instant>,
    /// Whether we're waiting for the agent to respond to a context send.
    pub response_pending: bool,
}

impl OrchestratorState {
    pub fn new(max_retries: u32) -> Self {
        Self {
            active: false,
            iteration: 0,
            iterations_since_checkpoint: 0,
            max_retries,
            recent_responses: Vec::new(),
            checkpoint_phase: CheckpointPhase::Idle,
            last_checkpoint_completed: String::new(),
            last_checkpoint_next: String::new(),
            last_pty_write: None,
            response_pending: false,
        }
    }

    /// Check if we're within the crash recovery grace period.
    pub fn in_grace_period(&self) -> bool {
        self.last_pty_write
            .map(|t| t.elapsed().as_secs() < CRASH_RECOVERY_GRACE_SECS)
            .unwrap_or(false)
    }

    /// Record that the orchestrator just typed into the PTY.
    pub fn mark_pty_write(&mut self) {
        self.last_pty_write = Some(std::time::Instant::now());
    }

    /// Check if automatic checkpoint should trigger.
    pub fn should_auto_checkpoint(&self) -> bool {
        self.iterations_since_checkpoint >= AUTO_CHECKPOINT_INTERVAL
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

    /// Start a checkpoint refresh cycle.
    pub fn begin_checkpoint(
        &mut self,
        completed: &str,
        next: &str,
        checkpoint_mtime: Option<std::time::SystemTime>,
    ) {
        self.last_checkpoint_completed = completed.to_string();
        self.last_checkpoint_next = next.to_string();
        self.iterations_since_checkpoint = 0;
        self.response_pending = false;
        self.checkpoint_phase = CheckpointPhase::WaitingForCheckpoint {
            started_at: std::time::Instant::now(),
            last_mtime: checkpoint_mtime,
        };
    }
}

/// Append an iteration row to .glass/iterations.tsv.
///
/// Format: iteration\tcommit\tfeature\tmetric\tstatus\tdescription
pub fn append_iteration_log(
    project_root: &str,
    iteration: u32,
    feature: &str,
    status: &str,
    description: &str,
) {
    let glass_dir = std::path::Path::new(project_root).join(".glass");
    let _ = std::fs::create_dir_all(&glass_dir);
    let path = glass_dir.join("iterations.tsv");

    let needs_header = !path.exists();

    let mut file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("Failed to open iterations.tsv: {e}");
            return;
        }
    };

    use std::io::Write;
    if needs_header {
        let _ = writeln!(
            file,
            "iteration\tcommit\tfeature\tmetric\tstatus\tdescription"
        );
    }

    // Get current git commit hash (short)
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    let _ = writeln!(
        file,
        "{iteration}\t{commit}\t{feature}\t\t{status}\t{description}"
    );
}

/// Read the iterations.tsv file content for inclusion in the system prompt.
pub fn read_iterations_log(project_root: &str) -> String {
    let path = std::path::Path::new(project_root)
        .join(".glass")
        .join("iterations.tsv");
    std::fs::read_to_string(path).unwrap_or_default()
}

/// Read the last N lines of iterations.tsv (plus header) for the system prompt.
pub fn read_iterations_log_truncated(project_root: &str, max_entries: usize) -> String {
    let content = read_iterations_log(project_root);
    if content.is_empty() {
        return content;
    }
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= max_entries + 1 {
        // +1 for header
        return content;
    }
    // Keep header + last N entries
    let header = lines[0];
    let tail = &lines[lines.len() - max_entries..];
    let skipped = lines.len() - max_entries - 1;
    let mut result = String::from(header);
    result.push('\n');
    result.push_str(&format!("... ({skipped} earlier entries omitted)\n"));
    for line in tail {
        result.push_str(line);
        result.push('\n');
    }
    result
}

/// Resolve the checkpoint file path for a given project root.
pub fn checkpoint_path(project_root: &str, config: Option<&str>) -> std::path::PathBuf {
    let rel = config.unwrap_or(".glass/checkpoint.md");
    std::path::Path::new(project_root).join(rel)
}

/// Get the current mtime of a file, or None if it doesn't exist.
pub fn file_mtime(path: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

/// Check if a checkpoint file has been updated since a baseline mtime.
pub fn checkpoint_changed(
    path: &std::path::Path,
    baseline: Option<std::time::SystemTime>,
) -> bool {
    match (baseline, file_mtime(path)) {
        (None, Some(_)) => true,
        (Some(old), Some(new)) => new > old,
        _ => false,
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

    #[test]
    fn parse_done() {
        assert_eq!(
            parse_agent_response("GLASS_DONE: Built all 5 pages of DevPulse"),
            AgentResponse::Done {
                summary: "Built all 5 pages of DevPulse".to_string()
            }
        );
    }

    #[test]
    fn parse_done_no_summary() {
        assert_eq!(
            parse_agent_response("GLASS_DONE"),
            AgentResponse::Done {
                summary: String::new()
            }
        );
    }

    #[test]
    fn checkpoint_changed_detects_creation() {
        let path = std::path::Path::new("nonexistent_test_file_12345.md");
        assert!(!checkpoint_changed(path, None));
    }

    #[test]
    fn begin_checkpoint_stores_mtime() {
        let mut state = OrchestratorState::new(3);
        let fake_mtime = std::time::SystemTime::now();
        state.begin_checkpoint("feature-a", "feature-b", Some(fake_mtime));
        match state.checkpoint_phase {
            CheckpointPhase::WaitingForCheckpoint { last_mtime, .. } => {
                assert_eq!(last_mtime, Some(fake_mtime));
            }
            _ => panic!("Expected WaitingForCheckpoint"),
        }
        assert_eq!(state.last_checkpoint_completed, "feature-a");
        assert_eq!(state.last_checkpoint_next, "feature-b");
        assert_eq!(state.iterations_since_checkpoint, 0);
    }

    #[test]
    fn iterations_truncation_keeps_header_and_tail() {
        let input = "header\nline1\nline2\nline3\nline4\nline5\n";
        let lines: Vec<&str> = input.lines().collect();
        assert_eq!(lines.len(), 6); // header + 5 entries
        let max = 3;
        let header = lines[0];
        let tail = &lines[lines.len() - max..];
        assert_eq!(header, "header");
        assert_eq!(tail, &["line3", "line4", "line5"]);
    }

    #[test]
    fn response_pending_gates_context_sends() {
        let mut state = OrchestratorState::new(3);
        state.active = true;
        assert!(!state.response_pending);
        state.response_pending = true;
        assert!(state.response_pending);
        state.response_pending = false;
        assert!(!state.response_pending);
    }
}
