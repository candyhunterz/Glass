use std::sync::Arc;

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::event_loop::{EventLoop as PtyEventLoop, EventLoopSender};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::tty::{self, Options as TtyOptions, Shell};

use crate::event_proxy::EventProxy;

/// A minimal terminal size implementing the Dimensions trait for Term initialization.
struct TermSize {
    columns: usize,
    lines: usize,
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.lines
    }

    fn screen_lines(&self) -> usize {
        self.lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

/// Spawn a PowerShell PTY via ConPTY and start the dedicated reader thread.
///
/// Returns:
/// - `EventLoopSender`: Send `PtyMsg::Input(bytes)` to write to PTY stdin, or
///   `PtyMsg::Resize(WindowSize)` to notify of terminal resize.
/// - `Arc<FairMutex<Term<EventProxy>>>`: Lock to read terminal state (grid contents).
///
/// The PTY reader thread runs independently on a dedicated std::thread (NOT a Tokio task).
/// When the PTY produces output, it calls `event_proxy.send_event(Event::Wakeup)`, which
/// sends `AppEvent::TerminalDirty` to the winit event loop to request a redraw.
pub fn spawn_pty(
    event_proxy: EventProxy,
) -> (EventLoopSender, Arc<FairMutex<Term<EventProxy>>>) {
    // Prefer pwsh (PowerShell 7) if available; fall back to powershell (Windows PowerShell 5.1)
    let shell_program = if std::process::Command::new("pwsh").arg("--version").output().is_ok() {
        "pwsh".to_owned()
    } else {
        "powershell".to_owned()
    };

    let options = TtyOptions {
        shell: Some(Shell::new(shell_program, vec![])),
        working_directory: None,
        drain_on_exit: true,
        escape_args: false,
        env: std::collections::HashMap::from([
            ("TERM".to_owned(), "xterm-256color".to_owned()),
            ("COLORTERM".to_owned(), "truecolor".to_owned()),
        ]),
    };

    // Default 80x24 terminal size — Phase 2 will compute actual size from font metrics
    let window_size = WindowSize {
        num_lines: 24,
        num_cols: 80,
        cell_width: 8,
        cell_height: 16,
    };

    let pty = tty::new(&options, window_size, 0).expect("Failed to spawn ConPTY (pwsh)");

    let term_size = TermSize { columns: 80, lines: 24 };
    let term_config = TermConfig {
        scrolling_history: 10_000, // CORE-05: 10,000 lines scrollback
        ..TermConfig::default()
    };
    let term = Arc::new(FairMutex::new(Term::new(
        term_config,
        &term_size,
        event_proxy.clone(),
    )));

    // EventLoop takes ownership of event_proxy — it needs a separate clone since
    // Term::new() already consumed the first clone above.
    let event_loop =
        PtyEventLoop::new(Arc::clone(&term), event_proxy, pty, false, false)
            .expect("Failed to create PTY event loop");

    let loop_tx = event_loop.channel();

    // Spawn the dedicated reader thread — NOT a Tokio task (see RESEARCH.md Pitfall 4).
    // This thread reads PTY stdout, parses it into the Term grid, and calls event_proxy
    // to wake the main thread when new data arrives.
    event_loop.spawn();

    (loop_tx, term)
}
