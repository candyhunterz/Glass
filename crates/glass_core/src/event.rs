/// Unique identifier for a terminal session within a SessionMux.
/// Wraps a u64 counter. Used to route PTY events to the correct session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(u64);

impl SessionId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }
    pub fn val(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "session-{}", self.0)
    }
}

/// Lightweight verification result for cross-crate event passing.
#[derive(Debug, Clone)]
pub struct VerifyEventResult {
    pub command_name: String,
    pub exit_code: i32,
    pub tests_passed: Option<u32>,
    pub tests_failed: Option<u32>,
    pub output: String,
}

/// The purpose of an ephemeral agent session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EphemeralPurpose {
    /// Synthesize a checkpoint summary from recent history.
    CheckpointSynthesis,
    /// Verify quality of a completed implementation.
    QualityVerification,
    /// Qualitative LLM analysis of orchestrator run for Tier 3 findings.
    FeedbackAnalysis,
    /// Generate a Tier 4 Rhai script from feedback analysis.
    ScriptGeneration,
}

/// Result from a successful ephemeral agent session.
#[derive(Debug, Clone)]
pub struct EphemeralAgentResult {
    /// The assistant's response text (markdown fences stripped).
    pub text: String,
    /// API cost in USD, if reported.
    pub cost_usd: Option<f64>,
    /// Wall-clock duration in milliseconds, if reported.
    pub duration_ms: Option<u64>,
}

/// Error from an ephemeral agent session.
#[derive(Debug, Clone)]
pub enum EphemeralAgentError {
    /// Failed to spawn the `claude` process or create temp files.
    SpawnFailed(String),
    /// Session exceeded the timeout.
    Timeout,
    /// Response could not be parsed.
    ParseError(String),
}

/// Which SmartTrigger priority fired the silence detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerSource {
    /// Prompt regex matched end of output.
    Prompt,
    /// OSC 133;A shell prompt detected.
    ShellPrompt,
    /// Output velocity dropped (fast trigger).
    Fast,
    /// Periodic slow fallback.
    Slow,
}

/// Events produced by shell integration OSC sequences.
///
/// Mirrors `OscEvent` from glass_terminal but lives in glass_core
/// to avoid circular crate dependencies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellEvent {
    /// OSC 133;A - Shell prompt has started
    PromptStart,
    /// OSC 133;B - User input / command line has started
    CommandStart,
    /// OSC 133;C - Command is being executed
    CommandExecuted,
    /// OSC 133;D[;exit_code] - Command finished with optional exit code
    CommandFinished { exit_code: Option<i32> },
    /// OSC 7 or OSC 9;9 - Current working directory changed
    CurrentDirectory(String),
    /// OSC 133;S - Pipeline capture started
    PipelineStart { stage_count: usize },
    /// OSC 133;P - Pipeline stage data available
    PipelineStage {
        index: usize,
        total_bytes: usize,
        temp_path: String,
    },
}

/// Git repository information for the status bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitStatus {
    /// Current branch name
    pub branch: String,
    /// Number of dirty (modified/untracked) files
    pub dirty_count: usize,
}

#[derive(Debug)]
pub enum AppEvent {
    /// Any terminal output received -- triggers redraw. NO session_id because
    /// any dirty terminal triggers a full redraw regardless of which session.
    TerminalDirty { window_id: winit::window::WindowId },
    SetTitle {
        window_id: winit::window::WindowId,
        session_id: SessionId,
        title: String,
    },
    TerminalExit {
        window_id: winit::window::WindowId,
        session_id: SessionId,
        exit_code: Option<i32>,
    },
    /// Shell integration event from the PTY reader thread's OscScanner.
    Shell {
        window_id: winit::window::WindowId,
        session_id: SessionId,
        event: ShellEvent,
        line: usize,
    },
    /// Git status result from a background query thread.
    GitInfo {
        window_id: winit::window::WindowId,
        session_id: SessionId,
        info: Option<GitStatus>,
    },
    /// Captured command output from the PTY reader thread.
    /// Contains raw bytes accumulated between CommandExecuted and CommandFinished.
    /// Processing (ANSI stripping, binary detection, truncation) happens on the
    /// main thread to avoid glass_terminal depending on glass_history.
    CommandOutput {
        window_id: winit::window::WindowId,
        session_id: SessionId,
        raw_output: Vec<u8>,
    },
    /// Config file changed on disk. Sent by the config watcher thread.
    /// When `error` is Some, `config` holds GlassConfig::default() (not applied).
    /// When `error` is None, `config` holds the successfully parsed new config.
    ConfigReloaded {
        config: Box<crate::config::GlassConfig>,
        error: Option<crate::config::ConfigError>,
    },
    /// A newer version of Glass is available for download.
    UpdateAvailable(crate::updater::UpdateInfo),
    /// Updated coordination state from the background poller.
    CoordinationUpdate(crate::coordination_poller::CoordinationState),
    /// MCP request received over the IPC channel; reply via the oneshot sender.
    McpRequest(crate::ipc::McpEventRequest),
    /// SOI parse completed for a finished command.
    /// Fired from the SOI worker thread via EventLoopProxy.
    SoiReady {
        window_id: winit::window::WindowId,
        session_id: SessionId,
        /// History DB row id for the completed command.
        command_id: i64,
        /// One-line human/agent readable summary.
        summary: String,
        /// Highest severity: "Error" | "Warning" | "Info" | "Success"
        severity: String,
        /// Raw line count of the command output (for min_lines threshold).
        raw_line_count: i64,
    },
    /// The agent subprocess returned a structured proposal for user review.
    AgentProposal(crate::agent_runtime::AgentProposalData),
    /// The agent subprocess emitted a structured handoff summary at session end.
    AgentHandoff {
        /// Claude session UUID from the system/init message.
        session_id: String,
        /// Parsed handoff data.
        handoff: crate::agent_runtime::AgentHandoffData,
        /// Canonicalized project root for DB storage.
        project_root: String,
        /// Raw JSON string of the handoff marker.
        raw_json: String,
    },
    /// The agent subprocess completed a query and reported its cost.
    AgentQueryResult { cost_usd: f64 },
    /// The agent subprocess terminated unexpectedly.
    /// `generation` identifies which agent instance crashed, so stale crashes
    /// from previously killed agents can be filtered out.
    AgentCrashed { generation: u64 },
    /// Orchestrator: the Glass Agent produced a response to route.
    OrchestratorResponse {
        /// The raw text from the Glass Agent.
        response: String,
    },
    /// Orchestrator: PTY silence threshold reached.
    OrchestratorSilence {
        window_id: winit::window::WindowId,
        session_id: SessionId,
        trigger_source: TriggerSource,
    },
    /// Orchestrator: background context gathering completed.
    OrchestratorContextReady {
        window_id: winit::window::WindowId,
        session_id: SessionId,
        terminal_lines: Vec<String>,
        exit_code: Option<i32>,
        soi_summary: Option<String>,
        soi_errors: Vec<String>,
        git_diff_stat: Option<String>,
        current_head: Option<String>,
        nudge: Option<String>,
        cwd: String,
    },
    /// Metric guard verification completed on background thread.
    VerifyComplete {
        window_id: winit::window::WindowId,
        session_id: SessionId,
        results: Vec<VerifyEventResult>,
    },
    /// Usage tracker: 5h utilization >= 80%, trigger graceful pause.
    UsagePause,
    /// Usage tracker: 5h utilization >= 95%, hard stop immediately.
    UsageHardStop,
    /// Usage tracker: 5h utilization dropped below 20%, safe to resume.
    UsageResume,
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
    /// Ephemeral agent session completed (checkpoint synthesis or quality check).
    EphemeralAgentComplete {
        result: Result<EphemeralAgentResult, EphemeralAgentError>,
        purpose: EphemeralPurpose,
    },
    /// Request a window redraw (used by orchestrator status spinner tick).
    RedrawRequest,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_id_display() {
        let sid = SessionId::new(42);
        assert_eq!(sid.val(), 42);
        assert_eq!(format!("{}", sid), "session-42");
    }

    #[test]
    fn session_id_copy_eq() {
        let a = SessionId::new(1);
        let b = a; // Copy
        assert_eq!(a, b);
    }

    #[test]
    fn shell_event_pipeline_start_variant() {
        let event = ShellEvent::PipelineStart { stage_count: 5 };
        match event {
            ShellEvent::PipelineStart { stage_count } => assert_eq!(stage_count, 5),
            _ => panic!("Expected PipelineStart"),
        }
    }

    #[test]
    fn shell_event_pipeline_stage_variant() {
        let event = ShellEvent::PipelineStage {
            index: 1,
            total_bytes: 2048,
            temp_path: "/tmp/glass/stage_1".into(),
        };
        match event {
            ShellEvent::PipelineStage {
                index,
                total_bytes,
                temp_path,
            } => {
                assert_eq!(index, 1);
                assert_eq!(total_bytes, 2048);
                assert_eq!(temp_path, "/tmp/glass/stage_1");
            }
            _ => panic!("Expected PipelineStage"),
        }
    }

    #[test]
    fn app_event_soi_ready_variant() {
        use winit::window::WindowId;
        let event = AppEvent::SoiReady {
            window_id: WindowId::dummy(),
            session_id: SessionId::new(1),
            command_id: 42,
            summary: "3 errors in src/main.rs".to_string(),
            severity: "Error".to_string(),
            raw_line_count: 15,
        };
        match event {
            AppEvent::SoiReady {
                command_id,
                summary,
                severity,
                raw_line_count,
                ..
            } => {
                assert_eq!(command_id, 42);
                assert_eq!(summary, "3 errors in src/main.rs");
                assert_eq!(severity, "Error");
                assert_eq!(raw_line_count, 15);
            }
            _ => panic!("Expected SoiReady"),
        }
    }

    #[test]
    fn app_event_agent_handoff_variant() {
        let handoff = crate::agent_runtime::AgentHandoffData {
            work_completed: "Refactored DB layer".to_string(),
            work_remaining: "Add MCP tools".to_string(),
            key_decisions: "Use WAL mode".to_string(),
            previous_session_id: None,
        };
        let event = AppEvent::AgentHandoff {
            session_id: "sess-xyz-789".to_string(),
            handoff,
            project_root: "/home/user/project".to_string(),
            raw_json: r#"{"work_completed":"Refactored DB layer","work_remaining":"Add MCP tools","key_decisions":"Use WAL mode"}"#.to_string(),
        };
        match event {
            AppEvent::AgentHandoff {
                session_id,
                handoff,
                project_root,
                raw_json,
            } => {
                assert_eq!(session_id, "sess-xyz-789");
                assert_eq!(handoff.work_completed, "Refactored DB layer");
                assert_eq!(handoff.work_remaining, "Add MCP tools");
                assert_eq!(handoff.key_decisions, "Use WAL mode");
                assert_eq!(project_root, "/home/user/project");
                assert!(raw_json.contains("work_completed"));
            }
            _ => panic!("Expected AgentHandoff"),
        }
    }
}
