use crate::config::{PermissionKind, QuietRules};
use std::time::{Duration, Instant};

/// Controls which severity levels reach the agent subprocess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
pub enum AgentMode {
    /// Agent is disabled. No events are forwarded.
    #[default]
    Off,
    /// Only Error-severity events trigger the agent.
    Watch,
    /// Error and Warning-severity events trigger the agent.
    Assist,
    /// All events (Error, Warning, Info, Success) trigger the agent.
    Autonomous,
}

/// Runtime configuration for the agent subprocess.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AgentRuntimeConfig {
    /// Which severity levels are forwarded to the agent. Default: `Off`.
    pub mode: AgentMode,
    /// Maximum accumulated cost in USD before the budget gate stops events.
    /// Default: 1.0 USD.
    pub max_budget_usd: f64,
    /// Minimum seconds that must elapse between forwarded events.
    /// Default: 30 seconds.
    pub cooldown_secs: u64,
    /// Comma-separated list of MCP tools the agent is allowed to call.
    /// Default: "glass_query,glass_context,Bash,Read".
    pub allowed_tools: String,
    /// Orchestrator sub-config. None when orchestrator is not configured.
    pub orchestrator: Option<crate::config::OrchestratorSection>,
}

impl Default for AgentRuntimeConfig {
    fn default() -> Self {
        Self {
            mode: AgentMode::Off,
            max_budget_usd: 1.0,
            cooldown_secs: 30,
            allowed_tools:
                "glass_query,glass_query_trend,glass_query_drill,glass_context,Bash,Read"
                    .to_string(),
            orchestrator: None,
        }
    }
}

/// A proposal emitted by the agent subprocess in response to an activity event.
#[derive(Debug, Clone)]
pub struct AgentProposalData {
    /// Short description of what the agent proposes to do.
    pub description: String,
    /// The action the agent will take (e.g. a shell command).
    pub action: String,
    /// Severity of the triggering event.
    pub severity: String,
    /// History DB row id of the command that triggered this proposal.
    pub command_id: i64,
    /// The full raw text of the agent's response message.
    pub raw_response: String,
    /// File changes proposed by the agent: (relative_path, new_content).
    /// Empty if the proposal has no file edits (backward compatible).
    pub file_changes: Vec<(String, String)>,
}

/// Prevents the agent from receiving events faster than the configured window.
///
/// `check_and_update` returns `true` (allowed) when either no event has been
/// forwarded yet, or the cooldown window has elapsed since the last forward.
pub struct CooldownTracker {
    last_sent: Option<Instant>,
    window: Duration,
}

impl CooldownTracker {
    /// Create a new tracker with the given cooldown duration in seconds.
    pub fn new(secs: u64) -> Self {
        Self {
            last_sent: None,
            window: Duration::from_secs(secs),
        }
    }

    /// Returns `true` if this event is allowed (i.e. cooldown has elapsed).
    /// When allowed, updates the internal timestamp.
    pub fn check_and_update(&mut self) -> bool {
        let now = Instant::now();
        match self.last_sent {
            None => {
                self.last_sent = Some(now);
                true
            }
            Some(last) => {
                if now.duration_since(last) >= self.window {
                    self.last_sent = Some(now);
                    true
                } else {
                    false
                }
            }
        }
    }

    /// Reset the cooldown, allowing the next event immediately.
    pub fn reset(&mut self) {
        self.last_sent = None;
    }
}

/// Tracks accumulated agent cost against a configured USD budget.
pub struct BudgetTracker {
    accumulated: f64,
    max_budget: f64,
}

impl BudgetTracker {
    /// Create a new tracker with the given maximum budget in USD.
    pub fn new(max: f64) -> Self {
        Self {
            accumulated: 0.0,
            max_budget: max,
        }
    }

    /// Add a cost amount (in USD) to the running total.
    pub fn add_cost(&mut self, cost: f64) {
        self.accumulated += cost;
    }

    /// Returns `true` when accumulated cost is at or above the configured max.
    pub fn is_exceeded(&self) -> bool {
        self.accumulated >= self.max_budget
    }

    /// Returns the accumulated cost formatted as `"$X.XXXX"`.
    pub fn cost_text(&self) -> String {
        format!("${:.4}", self.accumulated)
    }

    /// Returns the accumulated cost formatted as `"PAUSED $X.XX"`.
    pub fn paused_text(&self) -> String {
        format!("PAUSED ${:.2}", self.accumulated)
    }
}

/// Returns `true` if an event with the given `severity` should be forwarded
/// to the agent when the runtime is in `mode`.
///
/// | Mode       | Error | Warning | Info | Success |
/// |------------|-------|---------|------|---------|
/// | Off        | no    | no      | no   | no      |
/// | Watch      | yes   | no      | no   | no      |
/// | Assist     | yes   | yes     | no   | no      |
/// | Autonomous | yes   | yes     | yes  | yes     |
pub fn should_send_in_mode(mode: AgentMode, severity: &str) -> bool {
    match mode {
        AgentMode::Off => false,
        AgentMode::Watch => severity == "Error",
        AgentMode::Assist => severity == "Error" || severity == "Warning",
        AgentMode::Autonomous => true,
    }
}

/// Serialize an `ActivityEvent` as a Claude CLI stream-json user message.
///
/// Returns a JSON string of the form:
/// ```json
/// {"type":"user","message":{"role":"user","content":"[ACTIVITY] severity=X summary=Y command_id=Z collapsed=N"}}
/// ```
pub fn format_activity_as_user_message(event: &crate::activity_stream::ActivityEvent) -> String {
    let content = format!(
        "[ACTIVITY] severity={} summary={} command_id={} collapsed={}",
        event.severity, event.summary, event.command_id, event.collapsed_count
    );
    serde_json::json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": content
        }
    })
    .to_string()
}

/// Attempt to extract a cost value from a Claude CLI stream-json result line.
///
/// Returns `Some(cost_usd)` when the line is a valid JSON object with
/// `"type": "result"` and a numeric `"cost_usd"` field. Returns `None`
/// for all other lines.
pub fn parse_cost_from_result(line: &str) -> Option<f64> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v.get("type")?.as_str()? != "result" {
        return None;
    }
    v.get("cost_usd")?.as_f64()
}

/// Structured handoff data emitted by an agent at the end of a session.
///
/// Mirrors `HandoffData` in `glass_agent::types` but is defined here to keep
/// `glass_core` dependency-free of `glass_agent` (same pattern as
/// `AgentProposalData` vs glass_agent types).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct AgentHandoffData {
    /// Summary of work the agent completed in this session.
    pub work_completed: String,
    /// Summary of work that remains to be done.
    pub work_remaining: String,
    /// Key decisions or context for the next session.
    pub key_decisions: String,
    /// The `session_id` from the prior session, if this session was a continuation.
    #[serde(default)]
    pub previous_session_id: Option<String>,
}

/// Attempt to extract a structured handoff from the assistant's response text.
///
/// Searches for a `GLASS_HANDOFF:` prefix followed by a JSON object `{...}`.
/// Uses the same brace-depth walker as `extract_proposal`.
/// Returns `Some((handoff_data, raw_json_string))` on success, `None` on failure.
pub fn extract_handoff(assistant_text: &str) -> Option<(AgentHandoffData, String)> {
    let marker = "GLASS_HANDOFF:";
    let start = assistant_text.find(marker)?;
    let after_marker = assistant_text[start + marker.len()..].trim_start();

    let brace_start = after_marker.find('{')?;
    let json_slice = &after_marker[brace_start..];

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
    let json_str = &json_slice[..end?];
    let handoff: AgentHandoffData = serde_json::from_str(json_str).ok()?;
    Some((handoff, json_str.to_string()))
}

/// Serialize a handoff as a Claude CLI stream-json user message.
///
/// Returns a JSON string of the form:
/// ```json
/// {"type":"user","message":{"role":"user","content":"[PRIOR_SESSION_CONTEXT] session_id=... work_completed=... work_remaining=... key_decisions=..."}}
/// ```
pub fn format_handoff_as_user_message(session_id: &str, handoff: &AgentHandoffData) -> String {
    let content = format!(
        "[PRIOR_SESSION_CONTEXT] session_id={} work_completed={} work_remaining={} key_decisions={}",
        session_id, handoff.work_completed, handoff.work_remaining, handoff.key_decisions
    );
    serde_json::json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": content
        }
    })
    .to_string()
}

/// Attempt to extract an agent proposal from the assistant's response text.
///
/// Searches for a `GLASS_PROPOSAL:` prefix followed by a JSON object `{...}`.
/// Expects the object to contain `action`, `description`, `severity`, and
/// `command_id` fields. Returns `None` when no proposal marker is found or the
/// JSON is malformed.
pub fn extract_proposal(assistant_text: &str) -> Option<AgentProposalData> {
    let marker = "GLASS_PROPOSAL:";
    let start = assistant_text.find(marker)?;
    let after_marker = &assistant_text[start + marker.len()..].trim_start();

    // Find matching braces
    let brace_start = after_marker.find('{')?;
    let json_slice = &after_marker[brace_start..];

    // Walk through to find the matching closing brace
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
    let json_str = &json_slice[..end?];
    let v: serde_json::Value = serde_json::from_str(json_str).ok()?;

    // Parse optional files array: [{"path": "...", "content": "..."}]
    let file_changes = v
        .get("files")
        .and_then(|f| f.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let path = item.get("path")?.as_str()?.to_string();
                    let content = item.get("content")?.as_str()?.to_string();
                    Some((path, content))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Some(AgentProposalData {
        action: v.get("action")?.as_str()?.to_string(),
        description: v.get("description")?.as_str()?.to_string(),
        severity: v.get("severity")?.as_str()?.to_string(),
        command_id: v.get("command_id")?.as_i64()?,
        raw_response: assistant_text.to_string(),
        file_changes,
    })
}

/// Classify an agent proposal into the `PermissionKind` category it requires.
///
/// Decision logic:
/// - If `proposal.file_changes` is non-empty → `EditFiles`
/// - Else if `proposal.action` starts with `"git "` → `GitOperations`
/// - Else → `RunCommands`
pub fn classify_proposal(proposal: &AgentProposalData) -> PermissionKind {
    if !proposal.file_changes.is_empty() {
        PermissionKind::EditFiles
    } else if proposal.action.starts_with("git ") {
        PermissionKind::GitOperations
    } else {
        PermissionKind::RunCommands
    }
}

/// Returns `true` when the event described by `summary`/`severity` should be
/// suppressed according to the given `QuietRules`.
///
/// - If `quiet_rules.ignore_exit_zero` is true and `severity == "Success"` → `true`
/// - If any pattern in `quiet_rules.ignore_patterns` is a substring of `summary` → `true`
/// - Otherwise → `false`
pub fn should_quiet(quiet_rules: &QuietRules, summary: &str, severity: &str) -> bool {
    if quiet_rules.ignore_exit_zero && severity == "Success" {
        return true;
    }
    for pattern in &quiet_rules.ignore_patterns {
        if summary.contains(pattern.as_str()) {
            return true;
        }
    }
    false
}

/// Build the CLI argument list for invoking the Claude agent subprocess.
///
/// The `--mcp-config` flag is only included when `mcp_config_path` is non-empty.
/// This prevents a dangling flag when no MCP config file is available.
///
/// Returns:
/// ```text
/// ["-p", "--output-format", "stream-json", "--input-format", "stream-json",
///  "--system-prompt-file", prompt_path, [--mcp-config, mcp_config_path],
///  "--allowedTools", allowed_tools, "--dangerously-skip-permissions"]
/// ```
pub fn build_agent_command_args(
    config: &AgentRuntimeConfig,
    prompt_path: &str,
    mcp_config_path: &str,
) -> Vec<String> {
    let mut args = vec![
        "-p".to_string(),
        "--verbose".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--input-format".to_string(),
        "stream-json".to_string(),
        "--system-prompt-file".to_string(),
        prompt_path.to_string(),
    ];
    if !mcp_config_path.is_empty() {
        args.push("--mcp-config".to_string());
        args.push(mcp_config_path.to_string());
    }
    // In orchestrator mode, restrict to observation-only tools so the agent
    // writes instructions for Claude Code instead of doing the work itself.
    // With Bash/Read available, the agent bypasses Claude Code entirely.
    let orchestrator_active = config
        .orchestrator
        .as_ref()
        .map(|o| o.enabled)
        .unwrap_or(false);
    let tools = if orchestrator_active {
        "glass_query,glass_context".to_string()
    } else {
        config.allowed_tools.clone()
    };
    args.push("--allowedTools".to_string());
    args.push(tools);
    args.push("--dangerously-skip-permissions".to_string());
    // Disable skills/slash-commands to prevent SessionStart hooks
    // (e.g., Superpowers) from injecting instructions the agent can't follow
    args.push("--disable-slash-commands".to_string());
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity_stream::ActivityEvent;
    use crate::event::SessionId;

    fn make_event(summary: &str, severity: &str, command_id: i64) -> ActivityEvent {
        ActivityEvent {
            command_id,
            session_id: SessionId::new(1),
            summary: summary.to_string(),
            severity: severity.to_string(),
            timestamp_secs: 0,
            token_cost: 8,
            collapsed_count: 1,
        }
    }

    // --- AgentMode / should_send_in_mode ---

    #[test]
    fn mode_off_blocks_all() {
        assert!(!should_send_in_mode(AgentMode::Off, "Error"));
        assert!(!should_send_in_mode(AgentMode::Off, "Warning"));
        assert!(!should_send_in_mode(AgentMode::Off, "Info"));
        assert!(!should_send_in_mode(AgentMode::Off, "Success"));
    }

    #[test]
    fn mode_watch_only_error() {
        assert!(should_send_in_mode(AgentMode::Watch, "Error"));
        assert!(!should_send_in_mode(AgentMode::Watch, "Warning"));
        assert!(!should_send_in_mode(AgentMode::Watch, "Info"));
        assert!(!should_send_in_mode(AgentMode::Watch, "Success"));
    }

    #[test]
    fn mode_assist_error_and_warning() {
        assert!(should_send_in_mode(AgentMode::Assist, "Error"));
        assert!(should_send_in_mode(AgentMode::Assist, "Warning"));
        assert!(!should_send_in_mode(AgentMode::Assist, "Info"));
        assert!(!should_send_in_mode(AgentMode::Assist, "Success"));
    }

    #[test]
    fn mode_autonomous_all_severities() {
        assert!(should_send_in_mode(AgentMode::Autonomous, "Error"));
        assert!(should_send_in_mode(AgentMode::Autonomous, "Warning"));
        assert!(should_send_in_mode(AgentMode::Autonomous, "Info"));
        assert!(should_send_in_mode(AgentMode::Autonomous, "Success"));
    }

    // --- AgentRuntimeConfig defaults ---

    #[test]
    fn config_default_values() {
        let cfg = AgentRuntimeConfig::default();
        assert_eq!(cfg.max_budget_usd, 1.0);
        assert_eq!(cfg.cooldown_secs, 30);
        assert_eq!(cfg.mode, AgentMode::Off);
        assert_eq!(
            cfg.allowed_tools,
            "glass_query,glass_query_trend,glass_query_drill,glass_context,Bash,Read"
        );
    }

    // --- format_activity_as_user_message ---

    #[test]
    fn format_activity_produces_valid_json() {
        let event = make_event("3 errors in main.rs", "Error", 42);
        let json_str = format_activity_as_user_message(&event);
        let v: serde_json::Value = serde_json::from_str(&json_str).expect("must be valid JSON");
        assert_eq!(v["type"], "user");
        assert_eq!(v["message"]["role"], "user");
        let content = v["message"]["content"].as_str().unwrap();
        assert!(content.contains("severity=Error"));
        assert!(content.contains("summary=3 errors in main.rs"));
        assert!(content.contains("command_id=42"));
        assert!(content.contains("collapsed=1"));
    }

    // --- parse_cost_from_result ---

    #[test]
    fn parse_cost_extracts_from_result_line() {
        let line = r#"{"type":"result","cost_usd":0.0023,"duration_ms":1200}"#;
        let cost = parse_cost_from_result(line);
        assert_eq!(cost, Some(0.0023));
    }

    #[test]
    fn parse_cost_returns_none_for_non_result() {
        let line = r#"{"type":"assistant","message":{"role":"assistant","content":"hello"}}"#;
        assert_eq!(parse_cost_from_result(line), None);
    }

    #[test]
    fn parse_cost_returns_none_for_invalid_json() {
        assert_eq!(parse_cost_from_result("not json"), None);
    }

    // --- extract_handoff ---

    #[test]
    fn extract_handoff_parses_valid_marker() {
        let text = r#"Session complete.
GLASS_HANDOFF: {"work_completed":"Implemented auth module","work_remaining":"Write integration tests","key_decisions":"Used JWT with refresh rotation","previous_session_id":"sess-prev-42"}
Thanks for using Glass."#;
        let (data, raw) = extract_handoff(text).expect("should parse handoff");
        assert_eq!(data.work_completed, "Implemented auth module");
        assert_eq!(data.work_remaining, "Write integration tests");
        assert_eq!(data.key_decisions, "Used JWT with refresh rotation");
        assert_eq!(data.previous_session_id, Some("sess-prev-42".to_string()));
        assert!(raw.contains("work_completed"));
    }

    #[test]
    fn extract_handoff_returns_none_without_marker() {
        let text = "Here is some assistant output without any handoff.";
        assert!(extract_handoff(text).is_none());
    }

    #[test]
    fn extract_handoff_returns_none_for_malformed_json() {
        let text = "GLASS_HANDOFF: {broken json here";
        assert!(extract_handoff(text).is_none());
    }

    #[test]
    fn extract_handoff_handles_surrounding_text() {
        let text = r#"Prefix text before marker. GLASS_HANDOFF: {"work_completed":"Done","work_remaining":"Nothing","key_decisions":"Fast path"} Suffix text after marker."#;
        let (data, _raw) = extract_handoff(text).expect("should parse with surrounding text");
        assert_eq!(data.work_completed, "Done");
        assert_eq!(data.work_remaining, "Nothing");
        assert_eq!(data.key_decisions, "Fast path");
        assert_eq!(data.previous_session_id, None);
    }

    #[test]
    fn format_handoff_produces_valid_json() {
        let handoff = AgentHandoffData {
            work_completed: "Completed phase 1".to_string(),
            work_remaining: "Phase 2 pending".to_string(),
            key_decisions: "Use async throughout".to_string(),
            previous_session_id: Some("old-sess".to_string()),
        };
        let json_str = format_handoff_as_user_message("new-sess-123", &handoff);
        let v: serde_json::Value =
            serde_json::from_str(&json_str).expect("must produce valid JSON");
        assert_eq!(v["type"], "user");
        assert_eq!(v["message"]["role"], "user");
        let content = v["message"]["content"].as_str().unwrap();
        assert!(content.contains("[PRIOR_SESSION_CONTEXT]"));
        assert!(content.contains("session_id=new-sess-123"));
        assert!(content.contains("work_completed=Completed phase 1"));
        assert!(content.contains("work_remaining=Phase 2 pending"));
        assert!(content.contains("key_decisions=Use async throughout"));
    }

    // --- extract_proposal ---

    #[test]
    fn extract_proposal_parses_valid_marker() {
        let text = r#"I suggest the following fix.
GLASS_PROPOSAL: {"action":"cargo fix --lib -p myapp","description":"Fix unused import","severity":"Warning","command_id":7}
Let me know if you agree."#;
        let proposal = extract_proposal(text).expect("should parse proposal");
        assert_eq!(proposal.action, "cargo fix --lib -p myapp");
        assert_eq!(proposal.description, "Fix unused import");
        assert_eq!(proposal.severity, "Warning");
        assert_eq!(proposal.command_id, 7);
        assert!(proposal.raw_response.contains("GLASS_PROPOSAL:"));
        // No files key: file_changes should be empty (backward compatible).
        assert!(proposal.file_changes.is_empty());
    }

    #[test]
    fn extract_proposal_without_files_key_returns_empty_file_changes() {
        let text = r#"GLASS_PROPOSAL: {"action":"run","description":"No files","severity":"Info","command_id":2}"#;
        let p = extract_proposal(text).unwrap();
        assert!(p.file_changes.is_empty());
    }

    #[test]
    fn extract_proposal_with_empty_files_array_returns_empty_file_changes() {
        let text = r#"GLASS_PROPOSAL: {"action":"run","description":"Empty files","severity":"Info","command_id":3,"files":[]}"#;
        let p = extract_proposal(text).unwrap();
        assert!(p.file_changes.is_empty());
    }

    #[test]
    fn extract_proposal_with_file_changes() {
        let text = r#"GLASS_PROPOSAL: {"action":"fix","description":"Fix bug","severity":"Error","command_id":1,"files":[{"path":"src/main.rs","content":"fn main() {}"}]}"#;
        let p = extract_proposal(text).unwrap();
        assert_eq!(p.file_changes.len(), 1);
        assert_eq!(p.file_changes[0].0, "src/main.rs");
        assert_eq!(p.file_changes[0].1, "fn main() {}");
    }

    #[test]
    fn extract_proposal_returns_none_without_marker() {
        let text = "Here is some assistant output without any proposal.";
        assert!(extract_proposal(text).is_none());
    }

    // --- CooldownTracker ---

    #[test]
    fn cooldown_allows_first_event() {
        let mut tracker = CooldownTracker::new(30);
        assert!(tracker.check_and_update(), "first event must be allowed");
    }

    #[test]
    fn cooldown_blocks_within_window() {
        let mut tracker = CooldownTracker::new(30);
        tracker.check_and_update(); // first allowed
                                    // Immediately try again — within cooldown window
        assert!(
            !tracker.check_and_update(),
            "second event within window must be blocked"
        );
    }

    #[test]
    fn cooldown_allows_after_reset() {
        let mut tracker = CooldownTracker::new(30);
        tracker.check_and_update(); // first
        tracker.reset();
        assert!(
            tracker.check_and_update(),
            "after reset, next event must be allowed"
        );
    }

    #[test]
    fn cooldown_zero_window_always_allows() {
        let mut tracker = CooldownTracker::new(0);
        assert!(tracker.check_and_update());
        // With a 0-second window, the next call should also pass since no time
        // has elapsed but the window is 0 seconds (>=0 is always true).
        assert!(tracker.check_and_update());
    }

    // --- BudgetTracker ---

    #[test]
    fn budget_not_exceeded_initially() {
        let tracker = BudgetTracker::new(1.0);
        assert!(!tracker.is_exceeded());
    }

    #[test]
    fn budget_exceeded_when_at_max() {
        let mut tracker = BudgetTracker::new(1.0);
        tracker.add_cost(1.0);
        assert!(tracker.is_exceeded());
    }

    #[test]
    fn budget_not_exceeded_when_under() {
        let mut tracker = BudgetTracker::new(1.0);
        tracker.add_cost(0.5);
        assert!(!tracker.is_exceeded());
    }

    #[test]
    fn budget_cost_text_format() {
        let mut tracker = BudgetTracker::new(5.0);
        tracker.add_cost(0.0023);
        assert_eq!(tracker.cost_text(), "$0.0023");
    }

    #[test]
    fn budget_paused_text_format() {
        let mut tracker = BudgetTracker::new(1.0);
        tracker.add_cost(1.0);
        assert_eq!(tracker.paused_text(), "PAUSED $1.00");
    }

    #[test]
    fn budget_tracks_accumulated_costs() {
        let mut tracker = BudgetTracker::new(5.0);
        tracker.add_cost(0.10);
        tracker.add_cost(0.25);
        tracker.add_cost(0.05);
        assert!(!tracker.is_exceeded());
        assert_eq!(tracker.cost_text(), "$0.4000");
    }

    // --- classify_proposal ---

    fn make_proposal(action: &str, file_changes: Vec<(String, String)>) -> AgentProposalData {
        AgentProposalData {
            description: "test proposal".to_string(),
            action: action.to_string(),
            severity: "Info".to_string(),
            command_id: 1,
            raw_response: String::new(),
            file_changes,
        }
    }

    #[test]
    fn classify_proposal_non_empty_file_changes_is_edit_files() {
        let proposal = make_proposal(
            "npm install",
            vec![("src/main.rs".to_string(), "fn main() {}".to_string())],
        );
        assert_eq!(classify_proposal(&proposal), PermissionKind::EditFiles);
    }

    #[test]
    fn classify_proposal_empty_file_changes_git_action_is_git_operations() {
        let proposal = make_proposal("git commit -m \"fix\"", vec![]);
        assert_eq!(classify_proposal(&proposal), PermissionKind::GitOperations);
    }

    #[test]
    fn classify_proposal_empty_file_changes_non_git_is_run_commands() {
        let proposal = make_proposal("npm install", vec![]);
        assert_eq!(classify_proposal(&proposal), PermissionKind::RunCommands);
    }

    // --- should_quiet ---

    fn make_quiet_rules(ignore_exit_zero: bool, ignore_patterns: Vec<&str>) -> QuietRules {
        QuietRules {
            ignore_exit_zero,
            ignore_patterns: ignore_patterns.into_iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn should_quiet_ignore_exit_zero_true_suppresses_success() {
        let qr = make_quiet_rules(true, vec![]);
        assert!(should_quiet(&qr, "cargo check: ok", "Success"));
    }

    #[test]
    fn should_quiet_ignore_exit_zero_true_does_not_suppress_error() {
        let qr = make_quiet_rules(true, vec![]);
        assert!(!should_quiet(&qr, "cargo check: failed", "Error"));
    }

    #[test]
    fn should_quiet_pattern_match_returns_true() {
        let qr = make_quiet_rules(false, vec!["cargo check"]);
        assert!(should_quiet(&qr, "cargo check: 0 errors", "Info"));
    }

    #[test]
    fn should_quiet_pattern_no_match_returns_false() {
        let qr = make_quiet_rules(false, vec!["cargo check"]);
        assert!(!should_quiet(&qr, "npm install: 5 packages", "Info"));
    }

    #[test]
    fn should_quiet_empty_rules_returns_false() {
        let qr = QuietRules::default();
        assert!(!should_quiet(&qr, "cargo build: ok", "Success"));
        assert!(!should_quiet(&qr, "cargo build: failed", "Error"));
    }

    // --- build_agent_command_args ---

    #[test]
    fn build_args_includes_required_flags() {
        let config = AgentRuntimeConfig::default();
        let args = build_agent_command_args(&config, "/tmp/prompt.md", "/tmp/mcp.json");
        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"stream-json".to_string()));
        assert!(args.contains(&"--system-prompt-file".to_string()));
        assert!(args.contains(&"/tmp/prompt.md".to_string()));
        assert!(args.contains(&"--mcp-config".to_string()));
        assert!(args.contains(&"/tmp/mcp.json".to_string()));
        assert!(args.contains(&"--allowedTools".to_string()));
        assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
    }

    #[test]
    fn build_args_omits_mcp_when_empty() {
        let config = AgentRuntimeConfig::default();
        let args = build_agent_command_args(&config, "/tmp/prompt.md", "");
        assert!(
            !args.contains(&"--mcp-config".to_string()),
            "empty mcp_config_path must not produce --mcp-config flag"
        );
        // Other flags must still be present
        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"--system-prompt-file".to_string()));
        assert!(args.contains(&"--allowedTools".to_string()));
        assert!(args.contains(&"--dangerously-skip-permissions".to_string()));
    }
}
