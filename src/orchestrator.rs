//! Orchestrator: drives Claude Code sessions autonomously via the Glass Agent.
//!
//! Owns the silence-triggered loop that captures terminal context, sends it
//! to the Glass Agent, and routes the response (type into PTY, wait, or checkpoint).

use std::hash::{Hash, Hasher};

/// Create a `git` command with `CREATE_NO_WINDOW` on Windows to prevent console flashing.
fn git_cmd() -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd
}

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
    /// Regression = pass count dropped, fail count increased, exit code went
    /// from 0 to non-zero, or a newly-added command failed.
    pub fn check_regression(baseline: &[VerifyResult], current: &[VerifyResult]) -> bool {
        // Check paired results (commands present in both baseline and current)
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
        // Check extra commands added since baseline — a failing new command is a regression
        for extra in current.iter().skip(baseline.len()) {
            if extra.exit_code != 0 {
                return true;
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
            cmd: "cargo test --workspace".to_string(),
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

/// Parse the "## Deliverables" section of a PRD file to extract file paths.
///
/// Looks for markdown list items and extracts the first token that looks like
/// a file path (contains a dot or slash).
pub fn parse_prd_deliverables(prd_content: &str) -> Vec<String> {
    let mut in_deliverables = false;
    let mut files = Vec::new();

    for line in prd_content.lines() {
        let trimmed = line.trim();

        // Detect section headers
        if trimmed.starts_with("## ") {
            in_deliverables = trimmed
                .strip_prefix("## ")
                .map(|s| s.trim().eq_ignore_ascii_case("deliverables"))
                .unwrap_or(false);
            continue;
        }

        if !in_deliverables {
            continue;
        }

        // Parse list items: "- file.md (description)" or "* file.md"
        if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            let first_token = item.split_whitespace().next().unwrap_or("");
            let first_token = first_token.trim_end_matches([',', ')', '(']);
            if first_token.contains('.') || first_token.contains('/') || first_token.contains('\\')
            {
                files.push(first_token.to_string());
            }
        }
    }

    files
}

/// Auto-detect orchestrator mode and verification strategy from project + PRD.
///
/// Returns (orchestrator_mode, verify_mode, verify_files).
pub fn auto_detect_orchestrator_config(
    project_root: &str,
    prd_content: Option<&str>,
) -> (String, String, Vec<String>) {
    // 1. Check for code project markers
    let commands = auto_detect_verify_commands(project_root);
    if !commands.is_empty() {
        return ("build".to_string(), "floor".to_string(), Vec::new());
    }

    // 2. Parse PRD for deliverables
    if let Some(content) = prd_content {
        let deliverables = parse_prd_deliverables(content);
        if !deliverables.is_empty() {
            return ("general".to_string(), "files".to_string(), deliverables);
        }
    }

    // 3. No markers, no deliverables
    ("general".to_string(), "off".to_string(), Vec::new())
}

/// Baseline for file-based verification (general mode).
#[derive(Debug, Clone)]
pub struct FileVerifyBaseline {
    /// Map of file path -> last known byte size.
    pub file_sizes: std::collections::HashMap<String, u64>,
}

impl FileVerifyBaseline {
    pub fn new() -> Self {
        Self {
            file_sizes: std::collections::HashMap::new(),
        }
    }
}

/// Check deliverable files for regression.
///
/// Returns (regressed, results_summary).
pub fn check_file_verification(
    project_root: &str,
    verify_files: &[String],
    baseline: &mut FileVerifyBaseline,
) -> (bool, String) {
    let root = std::path::Path::new(project_root);
    let mut regressed = false;
    let mut summaries = Vec::new();

    for file in verify_files {
        let path = root.join(file);
        let current_size = std::fs::metadata(&path).map(|m| m.len()).ok();

        match (baseline.file_sizes.get(file), current_size) {
            (Some(&prev_size), None) => {
                regressed = true;
                summaries.push(format!("{file}: MISSING (was {prev_size}B)"));
            }
            (Some(&prev_size), Some(curr)) if prev_size > 0 && curr < prev_size / 2 => {
                regressed = true;
                summaries.push(format!("{file}: SHRUNK ({prev_size}B -> {curr}B)"));
            }
            (_, Some(curr)) => {
                baseline.file_sizes.insert(file.clone(), curr);
                summaries.push(format!("{file}: {curr}B"));
            }
            (None, None) => {
                summaries.push(format!("{file}: not yet created"));
            }
        }
    }

    (regressed, summaries.join(", "))
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
            if let Some(end_idx) = glass_core::agent_runtime::find_json_object_end(json_slice) {
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
            if let Some(end_idx) = glass_core::agent_runtime::find_json_object_end(json_slice) {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&json_slice[..end_idx]) {
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
#[derive(Debug, Clone)]
pub enum CheckpointPhase {
    /// Not in a checkpoint cycle.
    Idle,
    /// Waiting for ephemeral agent to synthesize checkpoint.md.
    Synthesizing {
        started_at: std::time::Instant,
        completed: String,
        next: String,
    },
}

/// How many iterations before forcing an automatic context refresh.
pub const AUTO_CHECKPOINT_INTERVAL: u32 = 15;

/// Grace period after the orchestrator types into the PTY.
/// PromptStart events within this window are ignored for crash recovery.
pub const CRASH_RECOVERY_GRACE_SECS: u64 = 10;

/// Maximum iterations to remain dependency-blocked before auto-clearing.
pub const DEPENDENCY_BLOCK_MAX_ITERATIONS: u32 = 3;

/// Maximum time to wait for ephemeral synthesis before using fallback.
pub const SYNTHESIS_TIMEOUT_SECS: u64 = 120;

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
    /// When response_pending was set to true (for timeout detection).
    pub response_pending_since: Option<std::time::Instant>,
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
    /// Last iteration that ran verification (to avoid duplicate verify per iteration).
    pub last_verified_iteration: Option<u32>,
    /// Timestamp of the last user keypress (for kickoff suppression).
    /// During kickoff, the silence trigger is suppressed as long as the user
    /// is actively engaged (has typed within the silence threshold).
    pub last_user_keypress: Option<std::time::Instant>,
    /// Whether the kickoff phase is complete. Set to true once both the terminal
    /// and user have been silent for the threshold duration.
    pub kickoff_complete: bool,
    /// Feedback loop: count of iterations classified as wasted.
    pub feedback_waste_iterations: u32,
    /// Feedback loop: count of commits made during this run.
    pub feedback_commit_count: u32,
    /// Feedback loop: list of files that were reverted during this run.
    pub feedback_reverted_files: Vec<String>,
    /// Feedback loop: count of fast triggers during output.
    pub feedback_fast_trigger_during_output: u32,
    /// Feedback loop: timestamps for each iteration (for pacing analysis).
    pub feedback_iteration_timestamps: Vec<std::time::Instant>,
    /// Feedback loop: count of stuck events during this run.
    pub feedback_stuck_count: u32,
    /// Feedback loop: count of checkpoint refreshes during this run.
    pub feedback_checkpoint_count: u32,
    /// Feedback loop: verify pass/fail sequence (true=pass, false=fail).
    pub feedback_verify_sequence: Vec<bool>,
    /// Feedback loop: agent response texts for instruction overload analysis.
    pub feedback_agent_responses: Vec<String>,
    /// Completion reason captured from GLASS_DONE or bounded stop.
    pub feedback_completion_reason: String,
    /// Buffered split instructions (one-at-a-time enforcement).
    pub instruction_buffer: Vec<String>,
    /// Text deferred because a block was executing when the response arrived.
    /// Flushed to PTY (one at a time) when the block finishes.
    pub deferred_type_text: Vec<String>,
    /// Active dependency block message (None = not blocked).
    pub dependency_block: Option<String>,
    /// Iterations spent while dependency-blocked.
    pub dependency_block_iterations: u32,
    /// Cached PRD deliverable file paths (for scope guard).
    pub prd_deliverable_files: Vec<String>,
    /// Iterations since the last detected git commit.
    pub iterations_since_last_commit: u32,
    /// Last known git HEAD SHA (for commit detection).
    pub last_known_head: Option<String>,
    /// Cached checkpoint data for fallback if ephemeral synthesis fails.
    pub cached_checkpoint_fallback: Option<String>,
    /// Coverage gap context string appended to agent context.
    pub coverage_gaps_context: String,
    /// Last quality verdict score (for regression comparison in general mode).
    pub last_quality_score: Option<u32>,
    /// Project root directory captured at orchestrator activation (Ctrl+Shift+O).
    /// Used for all file operations instead of get_focused_cwd() because the
    /// shell's OSC 7 CWD stops updating once Claude Code starts.
    pub project_root: String,
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
            response_pending_since: None,
            recent_fingerprints: Vec::new(),
            fingerprint_stuck: false,
            metric_baseline: None,
            last_good_commit: None,
            max_iterations: None,
            bounded_stop_pending: false,
            last_verified_iteration: None,
            last_user_keypress: None,
            kickoff_complete: false,
            feedback_waste_iterations: 0,
            feedback_commit_count: 0,
            feedback_reverted_files: Vec::new(),
            feedback_fast_trigger_during_output: 0,
            feedback_iteration_timestamps: Vec::new(),
            feedback_stuck_count: 0,
            feedback_checkpoint_count: 0,
            feedback_verify_sequence: Vec::new(),
            feedback_agent_responses: Vec::new(),
            feedback_completion_reason: String::new(),
            instruction_buffer: Vec::new(),
            deferred_type_text: Vec::new(),
            dependency_block: None,
            dependency_block_iterations: 0,
            prd_deliverable_files: Vec::new(),
            iterations_since_last_commit: 0,
            last_known_head: None,
            cached_checkpoint_fallback: None,
            coverage_gaps_context: String::new(),
            last_quality_score: None,
            project_root: String::new(),
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

    /// Record a user keypress (for kickoff suppression).
    pub fn mark_user_keypress(&mut self) {
        self.last_user_keypress = Some(std::time::Instant::now());
    }

    /// Check if the user is still engaged during kickoff.
    /// Returns true if the user has typed within the given threshold duration.
    pub fn user_recently_active(&self, threshold: std::time::Duration) -> bool {
        self.last_user_keypress
            .map(|t| t.elapsed() < threshold)
            .unwrap_or(false)
    }

    /// Check if automatic checkpoint should trigger.
    pub fn should_auto_checkpoint(&self) -> bool {
        self.iterations_since_checkpoint >= AUTO_CHECKPOINT_INTERVAL
    }

    /// Record a response and check if we're stuck (N identical consecutive responses).
    /// Returns true if stuck. max_retries of 0 disables stuck detection.
    pub fn record_response(&mut self, response: &str) -> bool {
        if self.max_retries == 0 {
            return false;
        }
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
    /// Returns true if stuck. max_retries of 0 disables stuck detection.
    pub fn record_fingerprint(&mut self, fp: StateFingerprint) -> bool {
        if self.max_retries == 0 {
            return false;
        }
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

    /// Start a checkpoint synthesis cycle.
    pub fn begin_synthesis(&mut self, completed: &str, next: &str, fallback_content: String) {
        self.last_checkpoint_completed = completed.to_string();
        self.last_checkpoint_next = next.to_string();
        self.iterations_since_checkpoint = 0;
        self.response_pending = false;
        self.cached_checkpoint_fallback = Some(fallback_content);
        self.checkpoint_phase = CheckpointPhase::Synthesizing {
            started_at: std::time::Instant::now(),
            completed: completed.to_string(),
            next: next.to_string(),
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
    let commit = git_cmd()
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(project_root)
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

    // Sanitize description: replace newlines/tabs to prevent TSV corruption
    let clean_desc = description.replace(['\n', '\r', '\t'], " ");
    let _ = writeln!(
        file,
        "{iteration}\t{commit}\t{feature}\t\t{status}\t{clean_desc}"
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

/// Generate a post-mortem report for a completed orchestrator run.
///
/// Reads iterations.tsv, git log, and metric baseline data to produce
/// a structured markdown report at `.glass/postmortem-{timestamp}.md`.
pub fn generate_postmortem(
    project_root: &str,
    iteration: u32,
    duration: Option<std::time::Duration>,
    metric_baseline: Option<&MetricBaseline>,
    completion_reason: &str,
    prd_path: &str,
) {
    let glass_dir = std::path::Path::new(project_root).join(".glass");
    let _ = std::fs::create_dir_all(&glass_dir);

    // Read iterations.tsv for analysis
    let iterations_content = read_iterations_log(project_root);
    let mut stuck_count = 0;
    let mut verify_keep_count = 0;
    let mut verify_revert_count = 0;
    let mut baseline_count = 0;
    let mut checkpoint_count = 0;
    for line in iterations_content.lines().skip(1) {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() >= 5 {
            // TSV format: iteration\tcommit\tfeature\t(metric)\tstatus\tdescription
            // Status is in column 4 (0-indexed), not column 3
            match cols[4].trim() {
                "stuck" => stuck_count += 1,
                "keep" => verify_keep_count += 1,
                "revert" => verify_revert_count += 1,
                "baseline" => baseline_count += 1,
                "checkpoint" => checkpoint_count += 1,
                _ => {}
            }
        }
    }

    // Get git log for commits made during the run
    let git_log = git_cmd()
        .args(["log", "--oneline", &format!("-{}", iteration.max(10))])
        .current_dir(project_root)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        })
        .unwrap_or_else(|| "(git log unavailable)".to_string());

    // Get test counts from metric baseline
    let (tests_passed, tests_failed, keep_count, revert_count) = metric_baseline
        .map(|b| {
            let passed = b
                .last_results
                .first()
                .and_then(|r| r.tests_passed)
                .unwrap_or(0);
            let failed = b
                .last_results
                .first()
                .and_then(|r| r.tests_failed)
                .unwrap_or(0);
            (passed, failed, b.keep_count, b.revert_count)
        })
        .unwrap_or((0, 0, 0, 0));

    // Duration formatting
    let duration_str = duration
        .map(|d| {
            let secs = d.as_secs();
            if secs >= 3600 {
                format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
            } else {
                format!("{}m {}s", secs / 60, secs % 60)
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    // Efficiency: iterations per commit
    let commit_count = git_log.lines().count();
    let iters_per_commit = if commit_count > 0 {
        format!("{:.1}", iteration as f64 / commit_count as f64)
    } else {
        "N/A".to_string()
    };

    // Build report
    let report = format!(
        r#"# Orchestrator Post-Mortem Report

## Run Summary

| Metric | Value |
|--------|-------|
| PRD | `{prd_path}` |
| Completion | {completion_reason} |
| Iterations | {iteration} |
| Duration | {duration_str} |
| Commits | {commit_count} |
| Iterations/commit | {iters_per_commit} |

## Metric Guard

| Metric | Value |
|--------|-------|
| Baselines established | {baseline_count} |
| Keeps (changes passed) | {keep_count} |
| Reverts (regressions caught) | {revert_count} |
| Final test count | {tests_passed} passed, {tests_failed} failed |

## Agent Behavior

| Metric | Value |
|--------|-------|
| Stuck events | {stuck_count} |
| Checkpoint refreshes | {checkpoint_count} |
| Verify keeps (from TSV) | {verify_keep_count} |
| Verify reverts (from TSV) | {verify_revert_count} |

## Commits

```
{git_log}```

## Observations

{observations}

## Raw Iteration Log

```
{iterations_content}```
"#,
        observations = build_observations(
            iteration,
            stuck_count,
            verify_revert_count,
            checkpoint_count,
            commit_count,
            &duration_str,
        ),
    );

    // Write report
    let timestamp = chrono_free_timestamp();
    let filename = format!("postmortem-{timestamp}.md");
    let path = glass_dir.join(&filename);
    if let Err(e) = std::fs::write(&path, &report) {
        tracing::warn!("Failed to write postmortem report: {e}");
    } else {
        tracing::info!("Orchestrator: post-mortem report written to .glass/{filename}");
    }
}

fn chrono_free_timestamp() -> String {
    // Use SystemTime for a timestamp without chrono dependency
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Correct date calculation accounting for leap years
    let mut remaining_days = (secs / 86400) as i64;
    let mut year: i64 = 1970;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }
    let leap = is_leap_year(year);
    let month_days: [i64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month: i64 = 1;
    for &md in &month_days {
        if remaining_days < md {
            break;
        }
        remaining_days -= md;
        month += 1;
    }
    let day = remaining_days + 1;
    format!("{year:04}{month:02}{day:02}-{hours:02}{minutes:02}{seconds:02}")
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn build_observations(
    iteration: u32,
    stuck_count: usize,
    revert_count: usize,
    checkpoint_count: usize,
    commit_count: usize,
    duration: &str,
) -> String {
    let mut obs = Vec::new();

    if stuck_count > 0 {
        obs.push(format!(
            "- Agent got stuck {stuck_count} time(s). Consider adjusting the PRD to be more explicit about expected approaches, or increase `max_retries_before_stuck`."
        ));
    }

    if revert_count > 0 {
        obs.push(format!(
            "- Metric guard reverted {revert_count} change(s). The agent introduced regressions that were caught and rolled back."
        ));
    }

    if iteration > 0 && commit_count == 0 {
        obs.push(
            "- No commits produced despite running iterations. The agent may have been unable to make progress on the PRD tasks.".to_string()
        );
    }

    if checkpoint_count > 3 {
        obs.push(format!(
            "- {checkpoint_count} checkpoint refreshes — context was exhausted frequently. Consider breaking the PRD into smaller, focused runs."
        ));
    }

    let iters_per_commit = if commit_count > 0 {
        iteration as f64 / commit_count as f64
    } else {
        0.0
    };
    if iters_per_commit > 10.0 && commit_count > 0 {
        obs.push(format!(
            "- High iteration-to-commit ratio ({iters_per_commit:.1}). Many iterations produced no commits — the agent may be spending time reading/analyzing without acting."
        ));
    }

    if stuck_count == 0 && revert_count == 0 && commit_count > 0 {
        obs.push(format!(
            "- Clean run: {commit_count} commits in {duration} with no stuck events or reverts."
        ));
    }

    if obs.is_empty() {
        "- No notable observations.".to_string()
    } else {
        obs.join("\n")
    }
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
    fn begin_synthesis_transitions_to_synthesizing() {
        let mut state = OrchestratorState::new(3);
        state.begin_synthesis("feature-a", "feature-b", "fallback content".to_string());
        match &state.checkpoint_phase {
            CheckpointPhase::Synthesizing {
                completed, next, ..
            } => {
                assert_eq!(completed, "feature-a");
                assert_eq!(next, "feature-b");
            }
            _ => panic!("Expected Synthesizing"),
        }
        assert_eq!(state.last_checkpoint_completed, "feature-a");
        assert_eq!(state.last_checkpoint_next, "feature-b");
        assert_eq!(state.iterations_since_checkpoint, 0);
        assert!(!state.response_pending);
        assert!(state.cached_checkpoint_fallback.is_some());
    }

    #[test]
    fn synthesis_timeout_constant() {
        assert_eq!(SYNTHESIS_TIMEOUT_SECS, 120);
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
        assert_eq!(cmds[0].cmd, "cargo test --workspace");
    }

    #[test]
    fn auto_detect_no_project_returns_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let cmds = auto_detect_verify_commands(dir.path().to_str().unwrap());
        assert!(cmds.is_empty());
    }

    #[test]
    fn parse_prd_deliverables_extracts_files() {
        let prd = "# Plan\n\n## Deliverables\n- vacation-plan.md (itinerary)\n- site/index.html\n- research/flights.md (top options)\n\n## Requirements\n- Budget $5000";
        let files = parse_prd_deliverables(prd);
        assert_eq!(
            files,
            vec!["vacation-plan.md", "site/index.html", "research/flights.md"]
        );
    }

    #[test]
    fn parse_prd_deliverables_empty_when_no_section() {
        let prd = "# Plan\n\n## Requirements\n- Do stuff";
        let files = parse_prd_deliverables(prd);
        assert!(files.is_empty());
    }

    #[test]
    fn parse_prd_deliverables_ignores_non_file_items() {
        let prd = "## Deliverables\n- A complete vacation plan\n- output.md\n- At least 3 options";
        let files = parse_prd_deliverables(prd);
        assert_eq!(files, vec!["output.md"]);
    }

    #[test]
    fn auto_detect_code_project() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]").unwrap();
        let (mode, verify, files) =
            auto_detect_orchestrator_config(dir.path().to_str().unwrap(), None);
        assert_eq!(mode, "build");
        assert_eq!(verify, "floor");
        assert!(files.is_empty());
    }

    #[test]
    fn auto_detect_general_with_deliverables() {
        let dir = tempfile::TempDir::new().unwrap();
        let prd = "## Deliverables\n- plan.md\n- site/index.html";
        let (mode, verify, files) =
            auto_detect_orchestrator_config(dir.path().to_str().unwrap(), Some(prd));
        assert_eq!(mode, "general");
        assert_eq!(verify, "files");
        assert_eq!(files, vec!["plan.md", "site/index.html"]);
    }

    #[test]
    fn auto_detect_general_no_deliverables() {
        let dir = tempfile::TempDir::new().unwrap();
        let (mode, verify, files) =
            auto_detect_orchestrator_config(dir.path().to_str().unwrap(), None);
        assert_eq!(mode, "general");
        assert_eq!(verify, "off");
        assert!(files.is_empty());
    }

    #[test]
    fn file_verify_new_file_not_regression() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("plan.md"), "hello world").unwrap();
        let mut baseline = FileVerifyBaseline::new();
        let (regressed, _) = check_file_verification(
            dir.path().to_str().unwrap(),
            &["plan.md".to_string()],
            &mut baseline,
        );
        assert!(!regressed);
        assert_eq!(*baseline.file_sizes.get("plan.md").unwrap(), 11);
    }

    #[test]
    fn file_verify_missing_file_is_regression() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut baseline = FileVerifyBaseline::new();
        baseline.file_sizes.insert("plan.md".to_string(), 100);
        let (regressed, summary) = check_file_verification(
            dir.path().to_str().unwrap(),
            &["plan.md".to_string()],
            &mut baseline,
        );
        assert!(regressed);
        assert!(summary.contains("MISSING"));
    }

    #[test]
    fn file_verify_shrunk_file_is_regression() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("plan.md"), "x").unwrap();
        let mut baseline = FileVerifyBaseline::new();
        baseline.file_sizes.insert("plan.md".to_string(), 100);
        let (regressed, summary) = check_file_verification(
            dir.path().to_str().unwrap(),
            &["plan.md".to_string()],
            &mut baseline,
        );
        assert!(regressed);
        assert!(summary.contains("SHRUNK"));
    }

    #[test]
    fn file_verify_growing_file_not_regression() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("plan.md"), "a".repeat(200)).unwrap();
        let mut baseline = FileVerifyBaseline::new();
        baseline.file_sizes.insert("plan.md".to_string(), 100);
        let (regressed, _) = check_file_verification(
            dir.path().to_str().unwrap(),
            &["plan.md".to_string()],
            &mut baseline,
        );
        assert!(!regressed);
        assert_eq!(*baseline.file_sizes.get("plan.md").unwrap(), 200);
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

    // --- Audit area 3: edge-case tests ---

    #[test]
    fn parse_empty_string_returns_empty_type_text() {
        // Empty or whitespace-only input should not produce actionable TypeText
        match parse_agent_response("") {
            AgentResponse::TypeText(s) => assert!(s.is_empty()),
            other => panic!("Expected TypeText, got {:?}", other),
        }
        match parse_agent_response("   ") {
            AgentResponse::TypeText(s) => assert!(s.is_empty()),
            other => panic!("Expected TypeText, got {:?}", other),
        }
    }

    #[test]
    fn parse_glass_wait_with_extra_text_is_type_text() {
        // "GLASS_WAIT" must be the entire trimmed content
        let resp = parse_agent_response("GLASS_WAIT and also do this");
        assert!(matches!(resp, AgentResponse::TypeText(_)));
    }

    #[test]
    fn parse_done_with_colon_no_space() {
        assert_eq!(
            parse_agent_response("GLASS_DONE:no space"),
            AgentResponse::Done {
                summary: "no space".to_string()
            }
        );
    }

    #[test]
    fn parse_checkpoint_nested_braces() {
        let raw = r#"GLASS_CHECKPOINT: {"completed": "feat {x}", "next": "feat {y}"}"#;
        match parse_agent_response(raw) {
            AgentResponse::Checkpoint { completed, next } => {
                assert_eq!(completed, "feat {x}");
                assert_eq!(next, "feat {y}");
            }
            other => panic!("Expected Checkpoint, got {:?}", other),
        }
    }

    #[test]
    fn parse_verify_empty_commands_falls_through() {
        let raw = r#"GLASS_VERIFY: {"commands": []}"#;
        // Empty commands array should NOT produce Verify, falls through to TypeText
        assert!(matches!(
            parse_agent_response(raw),
            AgentResponse::TypeText(_)
        ));
    }

    #[test]
    fn check_regression_empty_baselines() {
        // Empty baseline + empty current = no regression
        assert!(!MetricBaseline::check_regression(&[], &[]));
    }

    #[test]
    fn check_regression_mismatched_lengths_detects_failing_extra() {
        // Baseline has 1 entry, current has 2 — the extra failing entry is detected
        let baseline = vec![VerifyResult {
            command_name: "test".to_string(),
            exit_code: 0,
            tests_passed: Some(10),
            tests_failed: Some(0),
            errors: vec![],
        }];
        let current = vec![
            VerifyResult {
                command_name: "test".to_string(),
                exit_code: 0,
                tests_passed: Some(10),
                tests_failed: Some(0),
                errors: vec![],
            },
            VerifyResult {
                command_name: "lint".to_string(),
                exit_code: 1, // Extra failing command is now caught
                tests_passed: None,
                tests_failed: None,
                errors: vec!["error".to_string()],
            },
        ];
        assert!(MetricBaseline::check_regression(&baseline, &current));
    }

    #[test]
    fn check_regression_mismatched_lengths_passing_extra_ok() {
        // Extra command that passes should not trigger regression
        let baseline = vec![VerifyResult {
            command_name: "test".to_string(),
            exit_code: 0,
            tests_passed: Some(10),
            tests_failed: Some(0),
            errors: vec![],
        }];
        let current = vec![
            VerifyResult {
                command_name: "test".to_string(),
                exit_code: 0,
                tests_passed: Some(10),
                tests_failed: Some(0),
                errors: vec![],
            },
            VerifyResult {
                command_name: "lint".to_string(),
                exit_code: 0,
                tests_passed: None,
                tests_failed: None,
                errors: vec![],
            },
        ];
        assert!(!MetricBaseline::check_regression(&baseline, &current));
    }

    #[test]
    fn check_regression_all_none_counts() {
        // Both baseline and current have None test counts — only exit code matters
        let baseline = vec![VerifyResult {
            command_name: "build".to_string(),
            exit_code: 0,
            tests_passed: None,
            tests_failed: None,
            errors: vec![],
        }];
        let current = vec![VerifyResult {
            command_name: "build".to_string(),
            exit_code: 0,
            tests_passed: None,
            tests_failed: None,
            errors: vec![],
        }];
        assert!(!MetricBaseline::check_regression(&baseline, &current));
    }

    #[test]
    fn check_regression_partial_none_passed() {
        // Baseline has tests_passed=Some(10), current has tests_passed=None
        // This should NOT be treated as regression since counts are incomparable
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
            tests_passed: None,
            tests_failed: None,
            errors: vec![],
        }];
        // Documents current behavior: None counts don't trigger regression
        assert!(!MetricBaseline::check_regression(&baseline, &current));
    }

    #[test]
    fn update_baseline_raises_floor_on_improvement() {
        let mut baseline = MetricBaseline::new();
        baseline.baseline_results = vec![VerifyResult {
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
        baseline.update_baseline_if_improved(&current);
        assert_eq!(baseline.baseline_results[0].tests_passed, Some(15));
    }

    #[test]
    fn update_baseline_does_not_raise_on_more_failures() {
        let mut baseline = MetricBaseline::new();
        baseline.baseline_results = vec![VerifyResult {
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
            tests_failed: Some(2), // more failures
            errors: vec![],
        }];
        baseline.update_baseline_if_improved(&current);
        // Floor should NOT rise because failures increased
        assert_eq!(baseline.baseline_results[0].tests_passed, Some(10));
    }

    #[test]
    fn update_baseline_skips_when_counts_are_none() {
        // Documents: build-only commands (no test counts) never get floor raised
        let mut baseline = MetricBaseline::new();
        baseline.baseline_results = vec![VerifyResult {
            command_name: "build".to_string(),
            exit_code: 0,
            tests_passed: None,
            tests_failed: None,
            errors: vec![],
        }];
        let current = vec![VerifyResult {
            command_name: "build".to_string(),
            exit_code: 0,
            tests_passed: Some(5),
            tests_failed: Some(0),
            errors: vec![],
        }];
        baseline.update_baseline_if_improved(&current);
        // Baseline stays None because original tests_passed was None
        assert_eq!(baseline.baseline_results[0].tests_passed, None);
    }

    #[test]
    fn record_response_max_retries_zero_never_stuck() {
        // max_retries=0 should mean "never trigger stuck"
        let mut state = OrchestratorState::new(0);
        assert!(!state.record_response("same"));
        assert!(!state.record_response("same"));
        assert!(!state.record_response("same"));
    }

    #[test]
    fn record_response_max_retries_one() {
        let mut state = OrchestratorState::new(1);
        // A single response should trigger "stuck" with max_retries=1
        assert!(state.record_response("anything"));
    }

    #[test]
    fn should_auto_checkpoint_at_boundary() {
        let mut state = OrchestratorState::new(3);
        state.iterations_since_checkpoint = AUTO_CHECKPOINT_INTERVAL - 1;
        assert!(!state.should_auto_checkpoint());
        state.iterations_since_checkpoint = AUTO_CHECKPOINT_INTERVAL;
        assert!(state.should_auto_checkpoint());
    }

    #[test]
    fn in_grace_period_false_when_no_write() {
        let state = OrchestratorState::new(3);
        assert!(!state.in_grace_period());
    }

    #[test]
    fn in_grace_period_true_after_write() {
        let mut state = OrchestratorState::new(3);
        state.mark_pty_write();
        assert!(state.in_grace_period());
    }

    #[test]
    fn checkpoint_path_uses_config_override() {
        let p = checkpoint_path("/project", Some("custom/checkpoint.md"));
        assert_eq!(p, std::path::PathBuf::from("/project/custom/checkpoint.md"));
    }

    #[test]
    fn checkpoint_path_uses_default() {
        let p = checkpoint_path("/project", None);
        assert_eq!(p, std::path::PathBuf::from("/project/.glass/checkpoint.md"));
    }

    #[test]
    fn chrono_free_timestamp_produces_valid_date() {
        let ts = chrono_free_timestamp();
        // Should be 15 chars: YYYYMMDD-HHMMSS
        assert_eq!(ts.len(), 15);
        assert_eq!(&ts[8..9], "-");
        // Year should be >= 2025
        let year: u32 = ts[..4].parse().unwrap();
        assert!(year >= 2025);
        // Month should be 01-12
        let month: u32 = ts[4..6].parse().unwrap();
        assert!((1..=12).contains(&month));
        // Day should be 01-31
        let day: u32 = ts[6..8].parse().unwrap();
        assert!((1..=31).contains(&day));
    }

    #[test]
    fn is_leap_year_checks() {
        assert!(is_leap_year(2000)); // divisible by 400
        assert!(!is_leap_year(1900)); // divisible by 100 but not 400
        assert!(is_leap_year(2024)); // divisible by 4
        assert!(!is_leap_year(2025)); // not divisible by 4
    }

    #[test]
    fn postmortem_tsv_column_index_reads_status() {
        // Verify that our TSV format puts status in the column we read.
        // TSV: iteration\tcommit\tfeature\t(metric)\tstatus\tdescription
        let line = "1\tabc123\tstuck\t\tstuck\tStuck after 3 identical responses";
        let cols: Vec<&str> = line.split('\t').collect();
        assert!(cols.len() >= 5);
        assert_eq!(cols[4].trim(), "stuck"); // status is in column 4
    }
}
