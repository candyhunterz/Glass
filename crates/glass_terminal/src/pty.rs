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
use glass_core::event::{AppEvent, ShellEvent};

/// Max bytes to read from the PTY before forced terminal synchronization.
const READ_BUFFER_SIZE: usize = 0x10_0000;

/// Max bytes to read from the PTY while the terminal is locked.
const MAX_LOCKED_READ: usize = u16::MAX as usize;

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
        self.sender.send(msg).map_err(|e| format!("PTY send error: {e}"))?;
        self.poller.notify().map_err(|e| format!("PTY notify error: {e}"))
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
    }
}

/// Spawn a PowerShell PTY via ConPTY and start the dedicated reader thread
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
pub fn spawn_pty(
    event_proxy: EventProxy,
    proxy: winit::event_loop::EventLoopProxy<AppEvent>,
    window_id: WindowId,
) -> (PtySender, Arc<FairMutex<Term<EventProxy>>>) {
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

    // Default 80x24 terminal size — will be resized by main.rs after font metrics are computed
    let window_size = WindowSize {
        num_lines: 24,
        num_cols: 80,
        cell_width: 8,
        cell_height: 16,
    };

    let mut pty = tty::new(&options, window_size, 0).expect("Failed to spawn ConPTY (pwsh)");

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
            glass_pty_loop(pty, term_clone, event_proxy, proxy, window_id, rx, poll);
        })
        .expect("Failed to spawn PTY reader thread");

    (pty_sender, term)
}

/// Custom PTY event loop with integrated OscScanner.
///
/// This replaces alacritty_terminal's EventLoop to intercept PTY bytes
/// through the OscScanner before they reach the VTE parser. The overall
/// structure closely follows alacritty's event_loop.rs for correctness.
fn glass_pty_loop(
    mut pty: tty::Pty,
    terminal: Arc<FairMutex<Term<EventProxy>>>,
    event_proxy: EventProxy,
    app_proxy: winit::event_loop::EventLoopProxy<AppEvent>,
    window_id: WindowId,
    rx: Receiver<PtyMsg>,
    poll: Arc<polling::Poller>,
) {
    let mut scanner = OscScanner::new();
    let mut parser = ansi::Processor::<ansi::StdSyncHandler>::new();
    let mut buf = [0u8; READ_BUFFER_SIZE];
    let mut write_list: VecDeque<Cow<'static, [u8]>> = VecDeque::new();
    let mut events = Events::with_capacity(NonZeroUsize::new(1024).unwrap());
    let mut interest = PollingEvent::readable(0);

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

        // Handle synchronized update timeout (no events and no messages).
        if events.is_empty() && rx.try_recv().is_err() {
            parser.stop_sync(&mut *terminal.lock());
            event_proxy.send_event(Event::Wakeup);
            continue;
        }

        // Drain the message channel.
        loop {
            match rx.try_recv() {
                Ok(PtyMsg::Input(data)) => write_list.push_back(data),
                Ok(PtyMsg::Resize(size)) => pty.on_resize(size),
                Ok(PtyMsg::Shutdown) => break 'event_loop,
                Err(_) => break,
            }
        }

        for event in events.iter() {
            match event.key {
                tty::PTY_CHILD_EVENT_TOKEN => {
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
                        );
                        terminal.lock().exit();
                        event_proxy.send_event(Event::Wakeup);
                        break 'event_loop;
                    }
                }
                tty::PTY_READ_WRITE_TOKEN => {
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
fn pty_read_with_scan(
    pty: &mut tty::Pty,
    terminal: &Arc<FairMutex<Term<EventProxy>>>,
    event_proxy: &EventProxy,
    app_proxy: &winit::event_loop::EventLoopProxy<AppEvent>,
    window_id: WindowId,
    scanner: &mut OscScanner,
    parser: &mut ansi::Processor,
    buf: &mut [u8],
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
        for osc_event in osc_events {
            // Get approximate cursor line for block tracking
            let line = terminal_ref.grid().cursor.point.line.0 as usize;
            let shell_event = convert_osc_to_shell(osc_event);
            let _ = app_proxy.send_event(AppEvent::Shell {
                window_id,
                event: shell_event,
                line,
            });
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
fn pty_write(
    pty: &mut tty::Pty,
    write_list: &mut VecDeque<Cow<'static, [u8]>>,
) -> io::Result<()> {
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
