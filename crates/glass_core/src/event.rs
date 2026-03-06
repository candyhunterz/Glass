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

#[derive(Debug, Clone)]
pub enum AppEvent {
    /// Any terminal output received -- triggers redraw. NO session_id because
    /// any dirty terminal triggers a full redraw regardless of which session.
    TerminalDirty { window_id: winit::window::WindowId },
    SetTitle { window_id: winit::window::WindowId, session_id: SessionId, title: String },
    TerminalExit { window_id: winit::window::WindowId, session_id: SessionId },
    /// Shell integration event from the PTY reader thread's OscScanner.
    Shell { window_id: winit::window::WindowId, session_id: SessionId, event: ShellEvent, line: usize },
    /// Git status result from a background query thread.
    GitInfo { window_id: winit::window::WindowId, session_id: SessionId, info: Option<GitStatus> },
    /// Captured command output from the PTY reader thread.
    /// Contains raw bytes accumulated between CommandExecuted and CommandFinished.
    /// Processing (ANSI stripping, binary detection, truncation) happens on the
    /// main thread to avoid glass_terminal depending on glass_history.
    CommandOutput { window_id: winit::window::WindowId, session_id: SessionId, raw_output: Vec<u8> },
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
            ShellEvent::PipelineStage { index, total_bytes, temp_path } => {
                assert_eq!(index, 1);
                assert_eq!(total_bytes, 2048);
                assert_eq!(temp_path, "/tmp/glass/stage_1");
            }
            _ => panic!("Expected PipelineStage"),
        }
    }
}
