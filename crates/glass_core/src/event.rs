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
    TerminalDirty { window_id: winit::window::WindowId },
    SetTitle { window_id: winit::window::WindowId, title: String },
    TerminalExit { window_id: winit::window::WindowId },
    /// Shell integration event from the PTY reader thread's OscScanner.
    Shell { window_id: winit::window::WindowId, event: ShellEvent, line: usize },
    /// Git status result from a background query thread.
    GitInfo { window_id: winit::window::WindowId, info: Option<GitStatus> },
    /// Captured command output from the PTY reader thread.
    /// Contains raw bytes accumulated between CommandExecuted and CommandFinished.
    /// Processing (ANSI stripping, binary detection, truncation) happens on the
    /// main thread to avoid glass_terminal depending on glass_history.
    CommandOutput { window_id: winit::window::WindowId, raw_output: Vec<u8> },
}
