//! Orchestrator: drives Claude Code sessions autonomously via the Glass Agent.
//!
//! Owns the silence-triggered loop that captures terminal context, sends it
//! to the Glass Agent, and routes the response (type into PTY, wait, or checkpoint).

use std::hash::{Hash, Hasher};

/// A verification command with its name and command string.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifyCommand {
    pub name: String,
    pub cmd: String,
}

/// Result of running a verification command.
#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub command_name: String,
    pub exit_code: i32,
    pub tests_passed: Option<u32>,
    pub tests_failed: Option<u32>,
    pub errors: Vec<String>,
}

/// Tracks verification baseline and results across iterations.
#[derive(Debug, Clone)]
pub struct MetricBaseline {
    pub commands: Vec<VerifyCommand>,
    pub baseline_results: Vec<VerifyResult>,
    pub last_results: Vec<VerifyResult>,
    pub keep_count: u32,
    pub revert_count: u32,
}

impl MetricBaseline {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            baseline_results: Vec::new(),
            last_results: Vec::new(),
            keep_count: 0,
            revert_count: 0,
        }
    }

    /// Check if current results represent a regression from baseline.
    /// Regression = pass count dropped, fail count increased, or exit code went from 0 to non-zero.
    pub fn check_regression(baseline: &[VerifyResult], current: &[VerifyResult]) -> bool {
        for (b, c) in baseline.iter().zip(current.iter()) {
            // Build broke (exit code regressed)
            if b.exit_code == 0 && c.exit_code != 0 {
                return true;
            }
            // Test pass count dropped
            if let (Some(bp), Some(cp)) = (b.tests_passed, c.tests_passed) {
                if cp < bp {
                    return true;
                }
            }
            // Test fail count increased
            if let (Some(bf), Some(cf)) = (b.tests_failed, c.tests_failed) {
                if cf > bf {
                    return true;
                }
            }
        }
        false
    }

    /// Update baseline when tests are added (floor rises).
    pub fn update_baseline_if_improved(&mut self, current: &[VerifyResult]) {
        for (b, c) in self.baseline_results.iter_mut().zip(current.iter()) {
            if let (Some(bp), Some(cp)) = (b.tests_passed, c.tests_passed) {
                if let (Some(bf), Some(cf)) = (b.tests_failed, c.tests_failed) {
                    if cp > bp && cf <= bf {
                        b.tests_passed = Some(cp);
                    }
                }
            }
        }
    }
}

/// Auto-detect verification commands based on project marker files.
pub fn auto_detect_verify_commands(project_root: &str) -> Vec<VerifyCommand> {
    let root = std::path::Path::new(project_root);

    if root.join("Cargo.toml").exists() {
        return vec![VerifyCommand {
            name: "cargo test".to_string(),
            cmd: "cargo test".to_string(),
        }];
    }

    if root.join("package.json").exists() {
        if let Ok(content) = std::fs::read_to_string(root.join("package.json")) {
            if content.contains("\"test\"") {
                return vec![VerifyCommand {
                    name: "npm test".to_string(),
                    cmd: "npm test".to_string(),
                }];
            }
        }
    }

    if root.join("pyproject.toml").exists() || root.join("setup.py").exists() {
        return vec![VerifyCommand {
            name: "pytest".to_string(),
            cmd: "pytest".to_string(),
        }];
    }

    if root.join("go.mod").exists() {
        return vec![VerifyCommand {
            name: "go test".to_string(),
            cmd: "go test ./...".to_string(),
        }];
    }

    if root.join("tsconfig.json").exists() {
        return vec![VerifyCommand {
            name: "tsc".to_string(),
            cmd: "npx tsc --noEmit".to_string(),
        }];
    }

    if root.join("Makefile").exists() {
        if let Ok(content) = std::fs::read_to_string(root.join("Makefile")) {
            if content.contains("test:") {
                return vec![VerifyCommand {
                    name: "make test".to_string(),
                    cmd: "make test".to_string(),
                }];
            }
        }
    }

    Vec::new()
}

/// Parsed response from the Glass Agent.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentResponse {
    /// Type this text into the terminal.
    TypeText(String),
    /// Claude Code is still working; reset silence timer and check again later.
    Wait,
    /// Feature complete; trigger context refresh cycle.
    Checkpoint { completed: String, next: String },
    /// All PRD items are complete; stop orchestration.
    Done { summary: String },
    /// Agent discovered additional verification commands.
    Verify { commands: Vec<VerifyCommand> },
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

    let verify_marker = "GLASS_VERIFY:";
    if let Some(start) = trimmed.find(verify_marker) {
        let after = trimmed[start + verify_marker.len()..].trim();
        if let Some(json_start) = after.find('{') {
            let json_slice = &after[json_start..];
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
                if let Ok(val) =
                    serde_json::from_str::<serde_json::Value>(&json_slice[..end_idx])
                {
                    if let Some(cmds) = val.get("commands").and_then(|c| c.as_array()) {
                        let commands: Vec<VerifyCommand> = cmds
                            .iter()
                            .filter_map(|c| {
                                let name = c.get("name")?.as_str()?.to_string();
                                let cmd = c.get("cmd")?.as_str()?.to_string();
                                Some(VerifyCommand { name, cmd })
                            })
                            .collect();
                        if !commands.is_empty() {
                            return AgentResponse::Verify { commands };
                        }
                    }
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

/// Composite environment state fingerprint for semantic stuck detection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateFingerprint {
    /// Hash of recent terminal lines.
    pub terminal_hash: u64,
    /// Hash of SOI error records (if a command failed with SOI data).
    pub soi_error_hash: Option<u64>,
    /// Hash of `git diff --stat` output (if in a git repo).
    pub git_diff_hash: Option<u64>,
}

impl StateFingerprint {
    /// Compute a fingerprint from available signals.
    pub fn compute(
        terminal_lines: &[String],
        soi_errors: Option<&[String]>,
        git_diff_stat: Option<&str>,
    ) -> Self {
        let terminal_hash = {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            for line in terminal_lines {
                line.hash(&mut hasher);
            }
            hasher.finish()
        };

        let soi_error_hash = soi_errors.map(|errors| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            for error in errors {
                error.hash(&mut hasher);
            }
            hasher.finish()
        });

        let git_diff_hash = git_diff_stat.map(|diff| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            diff.hash(&mut hasher);
            hasher.finish()
        });

        Self {
            terminal_hash,
            soi_error_hash,
            git_diff_hash,
        }
    }
}

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
    /// Last N environment fingerprints for semantic stuck detection.
    pub recent_fingerprints: Vec<StateFingerprint>,
    /// Whether the last fingerprint check detected stuck (consumed by response handler).
    pub fingerprint_stuck: bool,
    /// Metric guard: verification baseline and results tracking.
    pub metric_baseline: Option<MetricBaseline>,
    /// Git commit SHA at the start of the current iteration (for revert).
    pub last_good_commit: Option<String>,
    /// Maximum iterations before checkpoint-stop. None or Some(0) = unlimited.
    pub max_iterations: Option<u32>,
    /// Whether bounded stop has been triggered (deactivate after checkpoint completes).
    pub bounded_stop_pending: bool,
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
            recent_fingerprints: Vec::new(),
            fingerprint_stuck: false,
            metric_baseline: None,
            last_good_commit: None,
            max_iterations: None,
            bounded_stop_pending: false,
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

    /// Record an environment fingerprint and check if stuck (N identical consecutive).
    /// Returns true if stuck.
    pub fn record_fingerprint(&mut self, fp: StateFingerprint) -> bool {
        self.recent_fingerprints.push(fp);
        if self.recent_fingerprints.len() > self.max_retries as usize {
            self.recent_fingerprints
                .drain(..self.recent_fingerprints.len() - self.max_retries as usize);
        }
        if self.recent_fingerprints.len() >= self.max_retries as usize {
            self.recent_fingerprints
                .iter()
                .all(|f| f == &self.recent_fingerprints[0])
        } else {
            false
        }
    }

    /// Reset stuck detection (e.g., after a successful verification).
    pub fn reset_stuck(&mut self) {
        self.recent_responses.clear();
        self.recent_fingerprints.clear();
        self.fingerprint_stuck = false;
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

    /// Check if the bounded iteration limit has been reached.
    /// Returns false when max_iterations is None or Some(0) (unlimited).
    pub fn should_stop_bounded(&self) -> bool {
        self.max_iterations
            .filter(|&max| max > 0)
            .map(|max| self.iteration >= max)
            .unwrap_or(false)
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
                String::from_utf8(o.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
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

/// Line counts for SOI-driven context windowing.
const CONTEXT_LINES_ON_ERROR: usize = 30;
const CONTEXT_LINES_ON_SUCCESS: usize = 20;
const CONTEXT_LINES_FALLBACK: usize = 80;

/// Build context string for the Glass Agent based on command outcome and SOI data.
///
/// Uses severity-based selection:
/// - Failed command + SOI: structured errors + 30 terminal lines
/// - Succeeded command + SOI: one-line summary + 20 terminal lines
/// - No SOI: 80 terminal lines (generous fallback)
pub fn build_orchestrator_context(
    terminal_lines: &[String],
    last_exit_code: Option<i32>,
    soi_summary: Option<&str>,
    soi_error_records: &[String],
) -> String {
    let mut context = String::new();

    let has_soi = soi_summary.is_some();
    let failed = last_exit_code.is_some_and(|c| c != 0);

    if failed && has_soi {
        // Branch 1: Command failed with SOI data
        context.push_str(&format!(
            "[COMMAND_FAILED] exit code: {}\n",
            last_exit_code.unwrap_or(-1)
        ));
        if let Some(summary) = soi_summary {
            context.push_str(&format!("[SOI_SUMMARY] {summary}\n"));
        }
        if !soi_error_records.is_empty() {
            context.push_str("[SOI_ERRORS]\n");
            for record in soi_error_records {
                context.push_str(&format!("  {record}\n"));
            }
        }
        let n = CONTEXT_LINES_ON_ERROR;
        let start = terminal_lines.len().saturating_sub(n);
        context.push_str(&format!("[RECENT_OUTPUT] (last {n} lines)\n"));
        for line in &terminal_lines[start..] {
            context.push_str(line);
            context.push('\n');
        }
    } else if !failed && has_soi {
        // Branch 2: Command succeeded with SOI data
        context.push_str("[COMMAND_OK]\n");
        if let Some(summary) = soi_summary {
            context.push_str(&format!("[SOI_SUMMARY] {summary}\n"));
        }
        let n = CONTEXT_LINES_ON_SUCCESS;
        let start = terminal_lines.len().saturating_sub(n);
        context.push_str(&format!("[RECENT_OUTPUT] (last {n} lines)\n"));
        for line in &terminal_lines[start..] {
            context.push_str(line);
            context.push('\n');
        }
    } else {
        // Branch 3: No SOI data
        if failed {
            context.push_str(&format!(
                "[COMMAND_FAILED] exit code: {}\n",
                last_exit_code.unwrap_or(-1)
            ));
        }
        let n = CONTEXT_LINES_FALLBACK;
        let start = terminal_lines.len().saturating_sub(n);
        context.push_str(&format!("[RECENT_OUTPUT] (last {n} lines)\n"));
        for line in &terminal_lines[start..] {
            context.push_str(line);
            context.push('\n');
        }
    }

    context
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
pub fn checkpoint_changed(path: &std::path::Path, baseline: Option<std::time::SystemTime>) -> bool {
    match (baseline, file_mtime(path)) {
        (None, Some(_)) => true,
        (Some(old), Some(new)) => new > old,
        _ => false,
    }
}

/// Build the summary string for a bounded run completion.
pub fn build_bounded_summary(
    iterations: u32,
    metric_baseline: Option<&MetricBaseline>,
    checkpoint_path: &str,
) -> String {
    let mut summary = format!(
        "[GLASS_ORCHESTRATOR] Bounded run complete ({iterations}/{iterations} iterations)\n"
    );

    if let Some(baseline) = metric_baseline {
        if !baseline.commands.is_empty() {
            summary.push_str(&format!(
                "  Metric guard: {} kept, {} reverted\n",
                baseline.keep_count, baseline.revert_count
            ));
            // Show test counts from first command if available
            if let (Some(b), Some(c)) = (
                baseline.baseline_results.first(),
                baseline.last_results.first(),
            ) {
                if let (Some(bp), Some(cp)) = (b.tests_passed, c.tests_passed) {
                    summary.push_str(&format!(
                        "  Baseline: {} tests \u{2192} Current: {} tests\n",
                        bp, cp
                    ));
                }
            }
        }
    }

    summary.push_str(&format!("  Last checkpoint: {checkpoint_path}\n"));
    summary.push_str("  To resume: enable orchestrator (Ctrl+Shift+O)\n");
    summary
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

    #[test]
    fn fingerprint_stuck_after_n_identical() {
        let mut state = OrchestratorState::new(3);
        let fp = StateFingerprint {
            terminal_hash: 12345,
            soi_error_hash: Some(67890),
            git_diff_hash: None,
        };
        assert!(!state.record_fingerprint(fp.clone()));
        assert!(!state.record_fingerprint(fp.clone()));
        assert!(state.record_fingerprint(fp)); // 3rd identical
    }

    #[test]
    fn fingerprint_not_stuck_when_different() {
        let mut state = OrchestratorState::new(3);
        let fp1 = StateFingerprint {
            terminal_hash: 111,
            soi_error_hash: None,
            git_diff_hash: None,
        };
        let fp2 = StateFingerprint {
            terminal_hash: 222,
            soi_error_hash: None,
            git_diff_hash: None,
        };
        assert!(!state.record_fingerprint(fp1.clone()));
        assert!(!state.record_fingerprint(fp1));
        assert!(!state.record_fingerprint(fp2)); // different
    }

    #[test]
    fn fingerprint_reset_clears() {
        let mut state = OrchestratorState::new(3);
        let fp = StateFingerprint {
            terminal_hash: 111,
            soi_error_hash: None,
            git_diff_hash: None,
        };
        state.record_fingerprint(fp.clone());
        state.record_fingerprint(fp.clone());
        state.reset_stuck();
        assert!(!state.record_fingerprint(fp)); // reset, only 1
    }

    #[test]
    fn compute_fingerprint_hashes_lines() {
        let lines1 = vec!["hello".to_string(), "world".to_string()];
        let lines2 = vec!["hello".to_string(), "world".to_string()];
        let lines3 = vec!["different".to_string()];
        let fp1 = StateFingerprint::compute(&lines1, None, None);
        let fp2 = StateFingerprint::compute(&lines2, None, None);
        let fp3 = StateFingerprint::compute(&lines3, None, None);
        assert_eq!(fp1.terminal_hash, fp2.terminal_hash);
        assert_ne!(fp1.terminal_hash, fp3.terminal_hash);
    }

    #[test]
    fn compute_fingerprint_with_soi_and_git() {
        let lines = vec!["output".to_string()];
        let soi = vec!["Error[E0277]".to_string()];
        let git = "1 file changed";
        let fp = StateFingerprint::compute(&lines, Some(&soi), Some(git));
        assert!(fp.soi_error_hash.is_some());
        assert!(fp.git_diff_hash.is_some());
    }

    #[test]
    fn context_failed_with_soi() {
        let lines: Vec<String> = (0..50).map(|i| format!("line {i}")).collect();
        let context = build_orchestrator_context(
            &lines,
            Some(1),
            Some("cargo test: 3 failed"),
            &["src/main.rs:10 Error[E0277]: trait bound".to_string()],
        );
        assert!(context.contains("[COMMAND_FAILED]"));
        assert!(context.contains("exit code: 1"));
        assert!(context.contains("[SOI_SUMMARY]"));
        assert!(context.contains("cargo test: 3 failed"));
        assert!(context.contains("[SOI_ERRORS]"));
        assert!(context.contains("Error[E0277]"));
        assert!(context.contains("[RECENT_OUTPUT]"));
        // Should include last CONTEXT_LINES_ON_ERROR lines, not all 50
        assert!(!context.contains("line 0\n"));
        assert!(context.contains("line 49"));
    }

    #[test]
    fn context_success_with_soi() {
        let lines: Vec<String> = (0..50).map(|i| format!("line {i}")).collect();
        let context =
            build_orchestrator_context(&lines, Some(0), Some("cargo test: 45 passed"), &[]);
        assert!(context.contains("[COMMAND_OK]"));
        assert!(context.contains("[SOI_SUMMARY]"));
        assert!(context.contains("45 passed"));
        assert!(context.contains("[RECENT_OUTPUT]"));
        // Should include fewer lines on success
        assert!(!context.contains("line 0\n"));
    }

    #[test]
    fn context_no_soi() {
        let lines: Vec<String> = (0..100).map(|i| format!("line {i}")).collect();
        let context = build_orchestrator_context(&lines, None, None, &[]);
        assert!(!context.contains("[COMMAND_FAILED]"));
        assert!(!context.contains("[COMMAND_OK]"));
        assert!(!context.contains("[SOI_SUMMARY]"));
        assert!(context.contains("[RECENT_OUTPUT]"));
        // Should include CONTEXT_LINES_FALLBACK lines
        assert!(context.contains("line 99"));
        assert!(context.contains("line 20"));
    }

    #[test]
    fn context_empty_terminal() {
        let context = build_orchestrator_context(&[], Some(1), None, &[]);
        assert!(context.contains("[COMMAND_FAILED]"));
        assert!(context.contains("[RECENT_OUTPUT]"));
    }

    #[test]
    fn verify_result_no_regression_same_counts() {
        let baseline = vec![VerifyResult {
            command_name: "test".to_string(),
            exit_code: 0,
            tests_passed: Some(10),
            tests_failed: Some(0),
            errors: vec![],
        }];
        let current = vec![VerifyResult {
            command_name: "test".to_string(),
            exit_code: 0,
            tests_passed: Some(10),
            tests_failed: Some(0),
            errors: vec![],
        }];
        assert!(!MetricBaseline::check_regression(&baseline, &current));
    }

    #[test]
    fn verify_result_regression_on_fail_increase() {
        let baseline = vec![VerifyResult {
            command_name: "test".to_string(),
            exit_code: 0,
            tests_passed: Some(10),
            tests_failed: Some(0),
            errors: vec![],
        }];
        let current = vec![VerifyResult {
            command_name: "test".to_string(),
            exit_code: 1,
            tests_passed: Some(8),
            tests_failed: Some(2),
            errors: vec!["error".to_string()],
        }];
        assert!(MetricBaseline::check_regression(&baseline, &current));
    }

    #[test]
    fn verify_result_no_regression_on_added_tests() {
        let baseline = vec![VerifyResult {
            command_name: "test".to_string(),
            exit_code: 0,
            tests_passed: Some(10),
            tests_failed: Some(0),
            errors: vec![],
        }];
        let current = vec![VerifyResult {
            command_name: "test".to_string(),
            exit_code: 0,
            tests_passed: Some(15),
            tests_failed: Some(0),
            errors: vec![],
        }];
        assert!(!MetricBaseline::check_regression(&baseline, &current));
    }

    #[test]
    fn verify_result_regression_on_build_failure() {
        let baseline = vec![VerifyResult {
            command_name: "build".to_string(),
            exit_code: 0,
            tests_passed: None,
            tests_failed: None,
            errors: vec![],
        }];
        let current = vec![VerifyResult {
            command_name: "build".to_string(),
            exit_code: 1,
            tests_passed: None,
            tests_failed: None,
            errors: vec!["compile error".to_string()],
        }];
        assert!(MetricBaseline::check_regression(&baseline, &current));
    }

    #[test]
    fn auto_detect_rust_project() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
        let cmds = auto_detect_verify_commands(dir.path().to_str().unwrap());
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].cmd, "cargo test");
    }

    #[test]
    fn auto_detect_no_project_returns_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let cmds = auto_detect_verify_commands(dir.path().to_str().unwrap());
        assert!(cmds.is_empty());
    }

    #[test]
    fn parse_verify_commands() {
        let raw = r#"GLASS_VERIFY: {"commands": [{"name": "integration", "cmd": "./test.sh"}]}"#;
        match parse_agent_response(raw) {
            AgentResponse::Verify { commands } => {
                assert_eq!(commands.len(), 1);
                assert_eq!(commands[0].name, "integration");
                assert_eq!(commands[0].cmd, "./test.sh");
            }
            other => panic!("Expected Verify, got {:?}", other),
        }
    }

    #[test]
    fn bounded_stop_when_limit_reached() {
        let mut state = OrchestratorState::new(3);
        state.max_iterations = Some(10);
        state.iteration = 9;
        assert!(!state.should_stop_bounded());
        state.iteration = 10;
        assert!(state.should_stop_bounded());
    }

    #[test]
    fn bounded_stop_unlimited() {
        let mut state = OrchestratorState::new(3);
        state.max_iterations = None;
        state.iteration = 1000;
        assert!(!state.should_stop_bounded());
    }

    #[test]
    fn bounded_stop_zero_means_unlimited() {
        let mut state = OrchestratorState::new(3);
        state.max_iterations = Some(0);
        assert!(!state.should_stop_bounded());
    }

    #[test]
    fn summary_with_metric_guard() {
        let mut baseline = MetricBaseline::new();
        baseline.commands = vec![VerifyCommand {
            name: "cargo test".to_string(),
            cmd: "cargo test".to_string(),
        }];
        baseline.keep_count = 12;
        baseline.revert_count = 3;
        baseline.baseline_results = vec![VerifyResult {
            command_name: "test".to_string(),
            exit_code: 0,
            tests_passed: Some(45),
            tests_failed: Some(0),
            errors: vec![],
        }];
        baseline.last_results = vec![VerifyResult {
            command_name: "test".to_string(),
            exit_code: 0,
            tests_passed: Some(52),
            tests_failed: Some(0),
            errors: vec![],
        }];
        let summary = build_bounded_summary(25, Some(&baseline), ".glass/checkpoint.md");
        assert!(summary.contains("25/25"));
        assert!(summary.contains("12 kept"));
        assert!(summary.contains("3 reverted"));
        assert!(summary.contains("45"));
        assert!(summary.contains("52"));
    }

    #[test]
    fn summary_without_metric_guard() {
        let summary = build_bounded_summary(10, None, ".glass/checkpoint.md");
        assert!(summary.contains("10/10"));
        assert!(!summary.contains("Metric guard"));
        assert!(summary.contains("checkpoint"));
    }
}
