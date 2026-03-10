use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{self, ErrorKind, Read, Write};
use std::num::NonZeroUsize;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

use alacritty_terminal::event::{Event, EventListener, OnResize, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config as TermConfig, Term};
use alacritty_terminal::tty::{self, EventedPty, EventedReadWrite, Options as TtyOptions, Shell};
use polling::{Event as PollingEvent, Events, PollMode};
use vte::ansi;
use winit::window::WindowId;

use crate::event_proxy::EventProxy;
use crate::osc_scanner::OscScanner;
use crate::output_capture::OutputBuffer;
use glass_core::event::{AppEvent, ShellEvent};

/// Max bytes to read from the PTY before forced terminal synchronization.
const READ_BUFFER_SIZE: usize = 0x10_0000;

/// Max bytes to read from the PTY while the terminal is locked.
const MAX_LOCKED_READ: usize = u16::MAX as usize;

// PTY polling event tokens.
// On Windows, alacritty_terminal re-exports these as pub; on Unix they're pub(crate).
// The values differ per platform (Unix: 0/1, Windows: 2/1), so we define our own
// matching the upstream values for each platform.
#[cfg(target_os = "windows")]
const PTY_READ_WRITE_TOKEN: usize = 2;
#[cfg(not(target_os = "windows"))]
const PTY_READ_WRITE_TOKEN: usize = 0;

// Child event token is 1 on both platforms.
const PTY_CHILD_EVENT_TOKEN: usize = 1;

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

/// Messages sent from the main thread to the PTY reader thread.
#[derive(Debug)]
pub enum PtyMsg {
    /// Data that should be written to the PTY.
    Input(Cow<'static, [u8]>),
    /// Instruction to resize the PTY.
    Resize(WindowSize),
    /// Indicates the event loop should shut down.
    Shutdown,
}

/// Sender handle for communicating with the PTY reader thread.
///
/// Wraps an mpsc sender and the polling::Poller so that sends wake the
/// event loop via poller notification.
#[derive(Clone)]
pub struct PtySender {
    sender: Sender<PtyMsg>,
    poller: Arc<polling::Poller>,
}

impl PtySender {
    pub fn send(&self, msg: PtyMsg) -> Result<(), String> {
        self.sender
            .send(msg)
            .map_err(|e| format!("PTY send error: {e}"))?;
        self.poller
            .notify()
            .map_err(|e| format!("PTY notify error: {e}"))
    }
}

/// Convert an OscEvent from the scanner into a ShellEvent for the core crate.
fn convert_osc_to_shell(osc: crate::osc_scanner::OscEvent) -> ShellEvent {
    use crate::osc_scanner::OscEvent;
    match osc {
        OscEvent::PromptStart => ShellEvent::PromptStart,
        OscEvent::CommandStart => ShellEvent::CommandStart,
        OscEvent::CommandExecuted => ShellEvent::CommandExecuted,
        OscEvent::CommandFinished { exit_code } => ShellEvent::CommandFinished { exit_code },
        OscEvent::CurrentDirectory(path) => ShellEvent::CurrentDirectory(path),
        OscEvent::PipelineStart { stage_count } => ShellEvent::PipelineStart { stage_count },
        OscEvent::PipelineStage {
            index,
            total_bytes,
            temp_path,
        } => ShellEvent::PipelineStage {
            index,
            total_bytes,
            temp_path,
        },
    }
}

/// Return the platform-appropriate default shell program.
///
/// - Windows: probes for `pwsh` (PowerShell 7), falls back to `powershell` (5.1)
/// - Unix: reads `$SHELL`, falls back to `/bin/sh`
fn default_shell_program() -> String {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW (0x08000000) prevents a visible console flash
        // when probing for pwsh from a GUI subsystem process.
        if std::process::Command::new("pwsh")
            .arg("--version")
            .creation_flags(0x08000000)
            .output()
            .is_ok()
        {
            "pwsh".to_owned()
        } else {
            "powershell".to_owned()
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned())
    }
}

/// Spawn a PTY and start the dedicated reader thread
/// with integrated OscScanner for shell integration.
///
/// Returns:
/// - `PtySender`: Send `PtyMsg::Input(bytes)` to write to PTY stdin, or
///   `PtyMsg::Resize(WindowSize)` to notify of terminal resize.
/// - `Arc<FairMutex<Term<EventProxy>>>`: Lock to read terminal state (grid contents).
///
/// The PTY reader thread runs independently on a dedicated std::thread (NOT a Tokio task).
/// It pre-scans PTY output through OscScanner before feeding bytes to the VTE parser,
/// sending ShellEvent variants to the winit event loop for block/status tracking.
///
/// If `shell_override` is `Some`, that shell program is used directly (e.g. "powershell",
/// "bash"). If `None`, the default detection logic runs: on Windows, pwsh 7 if available
/// else PowerShell 5.1; on Unix, `$SHELL` or `/bin/sh`.
pub fn spawn_pty(
    event_proxy: EventProxy,
    proxy: winit::event_loop::EventLoopProxy<AppEvent>,
    window_id: WindowId,
    shell_override: Option<&str>,
    working_directory: Option<&std::path::Path>,
    max_output_capture_kb: u32,
    pipes_enabled: bool,
) -> (PtySender, Arc<FairMutex<Term<EventProxy>>>) {
    // Use configured shell if provided, otherwise detect platform default
    let shell_program = if let Some(shell) = shell_override {
        shell.to_owned()
    } else {
        default_shell_program()
    };

    let options = TtyOptions {
        shell: Some(Shell::new(shell_program, vec![])),
        working_directory: working_directory.map(|p| p.to_path_buf()),
        drain_on_exit: true,
        #[cfg(target_os = "windows")]
        escape_args: false,
        env: {
            let mut env = std::collections::HashMap::from([
                ("TERM".to_owned(), "xterm-256color".to_owned()),
                ("COLORTERM".to_owned(), "truecolor".to_owned()),
            ]);
            if !pipes_enabled {
                env.insert("GLASS_PIPES_DISABLED".to_owned(), "1".to_owned());
            }
            env
        },
    };

    // Default 80x24 terminal size — will be resized by main.rs after font metrics are computed
    let window_size = WindowSize {
        num_lines: 24,
        num_cols: 80,
        cell_width: 8,
        cell_height: 16,
    };

    let mut pty = tty::new(&options, window_size, 0).expect("Failed to spawn PTY");

    let term_size = TermSize {
        columns: 80,
        lines: 24,
    };
    let term_config = TermConfig {
        scrolling_history: 10_000, // CORE-05: 10,000 lines scrollback
        ..TermConfig::default()
    };
    let term = Arc::new(FairMutex::new(Term::new(
        term_config,
        &term_size,
        event_proxy.clone(),
    )));

    // Set up polling infrastructure (mirrors alacritty's event_loop.rs)
    let poll: Arc<polling::Poller> = polling::Poller::new()
        .expect("Failed to create poller")
        .into();
    let (tx, rx) = mpsc::channel();

    let pty_sender = PtySender {
        sender: tx,
        poller: Arc::clone(&poll),
    };

    // Register PTY with the poller for read/write events
    let poll_opts = PollMode::Level;
    let interest = PollingEvent::readable(0);
    unsafe {
        pty.register(&poll, interest, poll_opts)
            .expect("Failed to register PTY with poller");
    }

    let term_clone = Arc::clone(&term);

    // Spawn the dedicated PTY reader thread with OscScanner integration
    std::thread::Builder::new()
        .name("Glass PTY reader".into())
        .spawn(move || {
            glass_pty_loop(
                pty,
                term_clone,
                event_proxy,
                proxy,
                window_id,
                rx,
                poll,
                max_output_capture_kb,
            );
        })
        .expect("Failed to spawn PTY reader thread");

    (pty_sender, term)
}

/// Custom PTY event loop with integrated OscScanner.
///
/// This replaces alacritty_terminal's EventLoop to intercept PTY bytes
/// through the OscScanner before they reach the VTE parser. The overall
/// structure closely follows alacritty's event_loop.rs for correctness.
#[cfg_attr(feature = "perf", tracing::instrument(skip_all))]
#[allow(clippy::too_many_arguments)]
fn glass_pty_loop(
    mut pty: tty::Pty,
    terminal: Arc<FairMutex<Term<EventProxy>>>,
    event_proxy: EventProxy,
    app_proxy: winit::event_loop::EventLoopProxy<AppEvent>,
    window_id: WindowId,
    rx: Receiver<PtyMsg>,
    poll: Arc<polling::Poller>,
    max_output_capture_kb: u32,
) {
    let mut scanner = OscScanner::new();
    let mut parser = ansi::Processor::<ansi::StdSyncHandler>::new();
    let mut buf = [0u8; READ_BUFFER_SIZE];
    let mut write_list: VecDeque<Cow<'static, [u8]>> = VecDeque::new();
    let mut events = Events::with_capacity(NonZeroUsize::new(1024).unwrap());
    let mut interest = PollingEvent::readable(0);
    let mut output_buffer = OutputBuffer::new((max_output_capture_kb as usize) * 1024);

    'event_loop: loop {
        // Handle synchronized update timeout.
        let timeout = parser
            .sync_timeout()
            .sync_timeout()
            .map(|st| st.saturating_duration_since(Instant::now()));

        events.clear();
        if let Err(err) = poll.wait(&mut events, timeout) {
            match err.kind() {
                ErrorKind::Interrupted => continue,
                _ => {
                    tracing::error!("PTY poll error: {err}");
                    break 'event_loop;
                }
            }
        }

        // Drain the message channel BEFORE the empty-events check,
        // so messages are processed rather than consumed by try_recv().
        let mut got_messages = false;
        loop {
            match rx.try_recv() {
                Ok(PtyMsg::Input(data)) => {
                    write_list.push_back(data);
                    got_messages = true;
                }
                Ok(PtyMsg::Resize(size)) => {
                    pty.on_resize(size);
                    got_messages = true;
                }
                Ok(PtyMsg::Shutdown) => break 'event_loop,
                Err(_) => break,
            }
        }

        // Handle synchronized update timeout (no events and no messages).
        if events.is_empty() && !got_messages {
            parser.stop_sync(&mut *terminal.lock());
            event_proxy.send_event(Event::Wakeup);
            continue;
        }

        for event in events.iter() {
            match event.key {
                PTY_CHILD_EVENT_TOKEN => {
                    if let Some(tty::ChildEvent::Exited(code)) = pty.next_child_event() {
                        if let Some(code) = code {
                            event_proxy.send_event(Event::ChildExit(code));
                        }
                        // Drain remaining bytes on exit
                        let _ = pty_read_with_scan(
                            &mut pty,
                            &terminal,
                            &event_proxy,
                            &app_proxy,
                            window_id,
                            &mut scanner,
                            &mut parser,
                            &mut buf,
                            &mut output_buffer,
                        );
                        terminal.lock().exit();
                        event_proxy.send_event(Event::Wakeup);
                        break 'event_loop;
                    }
                }
                PTY_READ_WRITE_TOKEN => {
                    if event.is_interrupt() {
                        continue;
                    }

                    if event.readable {
                        if let Err(err) = pty_read_with_scan(
                            &mut pty,
                            &terminal,
                            &event_proxy,
                            &app_proxy,
                            window_id,
                            &mut scanner,
                            &mut parser,
                            &mut buf,
                            &mut output_buffer,
                        ) {
                            tracing::error!("Error reading from PTY: {err}");
                            break 'event_loop;
                        }
                    }

                    if event.writable {
                        if let Err(err) = pty_write(&mut pty, &mut write_list) {
                            tracing::error!("Error writing to PTY: {err}");
                            break 'event_loop;
                        }
                    }
                }
                _ => {}
            }
        }

        // Update write interest based on pending write data.
        let needs_write = !write_list.is_empty();
        if needs_write != interest.writable {
            interest.writable = needs_write;
            pty.reregister(&poll, interest, PollMode::Level).unwrap();
        }
    }

    // Deregister PTY from poller on exit.
    let _ = pty.deregister(&poll);
}

/// Read from PTY with OscScanner pre-scanning.
///
/// This mirrors alacritty's `pty_read` but adds OscScanner scanning
/// before bytes are fed to the VTE parser. Shell integration events
/// are sent to the main thread via the app proxy.
#[inline]
#[cfg_attr(feature = "perf", tracing::instrument(skip_all))]
#[allow(clippy::too_many_arguments)]
fn pty_read_with_scan(
    pty: &mut tty::Pty,
    terminal: &Arc<FairMutex<Term<EventProxy>>>,
    event_proxy: &EventProxy,
    app_proxy: &winit::event_loop::EventLoopProxy<AppEvent>,
    window_id: WindowId,
    scanner: &mut OscScanner,
    parser: &mut ansi::Processor,
    buf: &mut [u8],
    output_buffer: &mut OutputBuffer,
) -> io::Result<()> {
    let mut unprocessed = 0;
    let mut processed = 0;

    // Reserve the next terminal lock for PTY reading.
    let _terminal_lease = Some(terminal.lease());
    let mut term_guard = None;

    loop {
        // Read from the PTY.
        match pty.reader().read(&mut buf[unprocessed..]) {
            Ok(0) if unprocessed == 0 => break,
            Ok(got) => unprocessed += got,
            Err(err) => match err.kind() {
                ErrorKind::Interrupted | ErrorKind::WouldBlock => {
                    if unprocessed == 0 {
                        break;
                    }
                }
                _ => return Err(err),
            },
        }

        // Attempt to lock the terminal.
        let terminal_ref = match &mut term_guard {
            Some(t) => t,
            None => term_guard.insert(match terminal.try_lock_unfair() {
                None if unprocessed >= READ_BUFFER_SIZE => terminal.lock_unfair(),
                None => continue,
                Some(t) => t,
            }),
        };

        let data = &buf[..unprocessed];

        // Pre-scan for OSC shell integration sequences before VTE parsing.
        let osc_events = scanner.scan(data);
        for osc_event in &osc_events {
            // Get absolute cursor line for block tracking.
            // cursor.point.line is viewport-relative (0 = top of screen).
            // Convert to absolute: history_size + viewport_line.
            let history = terminal_ref.grid().history_size();
            let line = history + terminal_ref.grid().cursor.point.line.0 as usize;
            let shell_event = convert_osc_to_shell(osc_event.clone());
            let _ = app_proxy.send_event(AppEvent::Shell {
                window_id,
                session_id: event_proxy.session_id(),
                event: shell_event,
                line,
            });
        }

        // Output capture: accumulate bytes between CommandExecuted and CommandFinished.
        // Check alt-screen sequences in raw bytes (avoids locking terminal for TermMode).
        output_buffer.check_alt_screen(data);
        output_buffer.append(data);

        // Handle capture lifecycle based on shell integration events.
        for osc_event in &osc_events {
            match osc_event {
                crate::osc_scanner::OscEvent::CommandExecuted => {
                    output_buffer.start_capture();
                }
                crate::osc_scanner::OscEvent::CommandFinished { .. } => {
                    if let Some(raw_bytes) = output_buffer.finish() {
                        if !raw_bytes.is_empty() {
                            let _ = app_proxy.send_event(AppEvent::CommandOutput {
                                window_id,
                                session_id: event_proxy.session_id(),
                                raw_output: raw_bytes,
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        // Parse the incoming bytes through the VTE parser (updates terminal grid).
        parser.advance(&mut **terminal_ref, data);

        processed += unprocessed;
        unprocessed = 0;

        // Avoid blocking the terminal too long.
        if processed >= MAX_LOCKED_READ {
            break;
        }
    }

    // Queue terminal redraw unless all processed bytes were synchronized.
    if parser.sync_bytes_count() < processed && processed > 0 {
        event_proxy.send_event(Event::Wakeup);
    }

    Ok(())
}

/// Write pending data to the PTY.
fn pty_write(pty: &mut tty::Pty, write_list: &mut VecDeque<Cow<'static, [u8]>>) -> io::Result<()> {
    while let Some(data) = write_list.front() {
        match pty.writer().write(data.as_ref()) {
            Ok(0) => break,
            Ok(n) => {
                if n >= data.len() {
                    write_list.pop_front();
                } else {
                    // Partial write — replace front with remainder
                    let remaining = data[n..].to_vec();
                    *write_list.front_mut().unwrap() = Cow::Owned(remaining);
                    break;
                }
            }
            Err(err) => match err.kind() {
                ErrorKind::Interrupted | ErrorKind::WouldBlock => break,
                _ => return Err(err),
            },
        }
    }
    Ok(())
}
