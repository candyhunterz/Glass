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

/// Configuration for spawning a PTY session.
pub struct PtySpawnConfig<'a> {
    pub shell_override: Option<&'a str>,
    pub working_directory: Option<&'a std::path::Path>,
    pub max_output_capture_kb: u32,
    pub pipes_enabled: bool,
    pub orchestrator_silence_secs: u64,
    pub orchestrator_fast_trigger_secs: u64,
    pub orchestrator_prompt_pattern: Option<String>,
    pub min_output_bytes: usize,
    pub scrollback: Option<usize>,
}

/// Event delivery sinks passed through the PTY loop and read functions.
struct PtyEventSinks {
    event_proxy: EventProxy,
    app_proxy: winit::event_loop::EventLoopProxy<AppEvent>,
    window_id: WindowId,
}

/// Configuration values forwarded to the PTY event loop thread.
struct PtyLoopConfig {
    max_output_capture_kb: u32,
    orchestrator_silence_secs: u64,
    orchestrator_fast_trigger_secs: u64,
    orchestrator_prompt_pattern: Option<String>,
    min_output_bytes: usize,
}

/// Owned parser/scanner state for the PTY read loop.
struct PtyReadState {
    scanner: OscScanner,
    parser: ansi::Processor<ansi::StdSyncHandler>,
    buf: [u8; READ_BUFFER_SIZE],
    output_buffer: OutputBuffer,
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
    config: &PtySpawnConfig<'_>,
) -> anyhow::Result<(PtySender, Arc<FairMutex<Term<EventProxy>>>)> {
    // Use configured shell if provided, otherwise detect platform default
    let shell_program = if let Some(shell) = config.shell_override {
        shell.to_owned()
    } else {
        default_shell_program()
    };

    let options = TtyOptions {
        shell: Some(Shell::new(shell_program, vec![])),
        working_directory: config.working_directory.map(|p| p.to_path_buf()),
        drain_on_exit: true,
        #[cfg(target_os = "windows")]
        escape_args: false,
        env: {
            let mut env = std::collections::HashMap::from([
                ("TERM".to_owned(), "xterm-256color".to_owned()),
                ("COLORTERM".to_owned(), "truecolor".to_owned()),
            ]);
            if !config.pipes_enabled {
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

    let mut pty = tty::new(&options, window_size, 0)
        .map_err(|e| anyhow::anyhow!("Failed to spawn PTY: {e}"))?;

    let term_size = TermSize {
        columns: 80,
        lines: 24,
    };
    let term_config = TermConfig {
        scrolling_history: config.scrollback.unwrap_or(10_000),
        ..TermConfig::default()
    };
    let term = Arc::new(FairMutex::new(Term::new(
        term_config,
        &term_size,
        event_proxy.clone(),
    )));

    // Set up polling infrastructure (mirrors alacritty's event_loop.rs)
    let poll: Arc<polling::Poller> = polling::Poller::new()
        .map_err(|e| anyhow::anyhow!("Failed to create poller: {e}"))?
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
            .map_err(|e| anyhow::anyhow!("Failed to register PTY with poller: {e}"))?;
    }

    let term_clone = Arc::clone(&term);
    let sinks = PtyEventSinks {
        event_proxy,
        app_proxy: proxy,
        window_id,
    };
    let loop_config = PtyLoopConfig {
        max_output_capture_kb: config.max_output_capture_kb,
        orchestrator_silence_secs: config.orchestrator_silence_secs,
        orchestrator_fast_trigger_secs: config.orchestrator_fast_trigger_secs,
        orchestrator_prompt_pattern: config.orchestrator_prompt_pattern.clone(),
        min_output_bytes: config.min_output_bytes,
    };

    // Spawn the dedicated PTY reader thread with OscScanner integration
    std::thread::Builder::new()
        .name("Glass PTY reader".into())
        .spawn(move || {
            glass_pty_loop(pty, term_clone, sinks, rx, poll, loop_config);
        })
        .map_err(|e| anyhow::anyhow!("Failed to spawn PTY reader thread: {e}"))?;

    Ok((pty_sender, term))
}

/// Custom PTY event loop with integrated OscScanner.
///
/// This replaces alacritty_terminal's EventLoop to intercept PTY bytes
/// through the OscScanner before they reach the VTE parser. The overall
/// structure closely follows alacritty's event_loop.rs for correctness.
#[cfg_attr(feature = "perf", tracing::instrument(skip_all))]
fn glass_pty_loop(
    mut pty: tty::Pty,
    terminal: Arc<FairMutex<Term<EventProxy>>>,
    sinks: PtyEventSinks,
    rx: Receiver<PtyMsg>,
    poll: Arc<polling::Poller>,
    config: PtyLoopConfig,
) {
    let mut read_state = PtyReadState {
        scanner: OscScanner::new(),
        parser: ansi::Processor::<ansi::StdSyncHandler>::new(),
        buf: [0u8; READ_BUFFER_SIZE],
        output_buffer: OutputBuffer::new((config.max_output_capture_kb as usize) * 1024),
    };
    let mut write_list: VecDeque<Cow<'static, [u8]>> = VecDeque::new();
    let mut events = Events::with_capacity(NonZeroUsize::new(1024).unwrap());
    let mut interest = PollingEvent::readable(0);

    // Throttle Wakeup events to prevent flooding the winit event loop.
    // On Windows, EventLoopProxy::send_event uses PostMessage which has
    // higher priority than WM_PAINT. Without throttling, rapid Wakeups
    // starve rendering — the terminal grid updates but never repaints.
    let mut last_wakeup = Instant::now();
    let wakeup_interval = std::time::Duration::from_millis(16); // ~60fps
    let mut wakeup_pending = false; // true if a wakeup was suppressed by throttle

    // Orchestrator silence detection (periodic, not one-shot)
    let mut smart_trigger = if config.orchestrator_silence_secs > 0 {
        Some(crate::silence::SmartTrigger::new(
            config.orchestrator_silence_secs,
            config.orchestrator_fast_trigger_secs,
            config.orchestrator_prompt_pattern,
        ))
    } else {
        None
    };
    if let Some(ref mut trigger) = smart_trigger {
        trigger.set_min_output_bytes(config.min_output_bytes);
    }

    'event_loop: loop {
        // Handle synchronized update timeout.
        let mut timeout = read_state
            .parser
            .sync_timeout()
            .sync_timeout()
            .map(|st| st.saturating_duration_since(Instant::now()));

        // Cap poll timeout to silence tracker's next check time.
        if let Some(ref mut trigger) = smart_trigger {
            let silence_timeout = trigger.poll_timeout();
            timeout = Some(match timeout {
                Some(t) => t.min(silence_timeout),
                None => silence_timeout,
            });
        }

        // If a wakeup was suppressed by the throttle, cap the poll timeout
        // so we wake up when the throttle expires and send it. Without this,
        // the poller can block for seconds (silence timeout) leaving the
        // terminal grid updated but never triggering a redraw.
        if wakeup_pending {
            let elapsed = Instant::now().duration_since(last_wakeup);
            let remaining = wakeup_interval.saturating_sub(elapsed);
            timeout = Some(match timeout {
                Some(t) => t.min(remaining),
                None => remaining,
            });
        }

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

        // Flush any suppressed wakeup now that the throttle has expired.
        if wakeup_pending {
            let now = Instant::now();
            if now.duration_since(last_wakeup) >= wakeup_interval {
                sinks.event_proxy.send_event(Event::Wakeup);
                last_wakeup = now;
                wakeup_pending = false;
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
            read_state.parser.stop_sync(&mut *terminal.lock());
            sinks.event_proxy.send_event(Event::Wakeup);

            // Check SmartTrigger even on idle timeouts — this is the primary
            // path for silence detection when the terminal has no activity.
            if let Some(ref mut trigger) = smart_trigger {
                let silence_ms = trigger.silence_duration().as_millis() as u64;
                if let Some(source) = trigger.should_fire() {
                    let _ = sinks.app_proxy.send_event(AppEvent::OrchestratorSilence {
                        window_id: sinks.window_id,
                        session_id: sinks.event_proxy.session_id(),
                        trigger_source: source,
                        silence_duration_ms: silence_ms,
                    });
                }
            }

            continue;
        }

        for event in events.iter() {
            match event.key {
                PTY_CHILD_EVENT_TOKEN => {
                    if let Some(tty::ChildEvent::Exited(code)) = pty.next_child_event() {
                        if let Some(code) = code {
                            sinks.event_proxy.send_event(Event::ChildExit(code));
                        }
                        // Drain remaining bytes on exit (always send final wakeup)
                        let _ = pty_read_with_scan(
                            &mut pty,
                            &terminal,
                            &sinks,
                            &mut read_state,
                            smart_trigger.as_mut(),
                        );
                        terminal.lock().exit();
                        sinks.event_proxy.send_event(Event::Wakeup); // final wakeup on exit
                        break 'event_loop;
                    }
                }
                PTY_READ_WRITE_TOKEN => {
                    if event.is_interrupt() {
                        continue;
                    }

                    if event.readable {
                        match pty_read_with_scan(
                            &mut pty,
                            &terminal,
                            &sinks,
                            &mut read_state,
                            smart_trigger.as_mut(),
                        ) {
                            Ok(needs_wakeup) => {
                                // Throttle Wakeup events to ~60fps so the main
                                // thread's WM_PAINT isn't starved on Windows.
                                if needs_wakeup {
                                    let now = Instant::now();
                                    if now.duration_since(last_wakeup) >= wakeup_interval {
                                        sinks.event_proxy.send_event(Event::Wakeup);
                                        last_wakeup = now;
                                        wakeup_pending = false;
                                    } else {
                                        // Suppressed — will be flushed when throttle
                                        // expires (poll timeout is capped above).
                                        wakeup_pending = true;
                                    }
                                }
                            }
                            Err(err) => {
                                tracing::error!("Error reading from PTY: {err}");
                                break 'event_loop;
                            }
                        }
                    }

                    if event.writable {
                        if let Err(err) = pty_write(pty.writer(), &mut write_list) {
                            tracing::error!("Error writing to PTY: {err}");
                            break 'event_loop;
                        }
                    }
                }
                _ => {}
            }
        }

        // Orchestrator silence detection (fires periodically while quiet)
        if let Some(ref mut trigger) = smart_trigger {
            let silence_ms = trigger.silence_duration().as_millis() as u64;
            if let Some(source) = trigger.should_fire() {
                let _ = sinks.app_proxy.send_event(AppEvent::OrchestratorSilence {
                    window_id: sinks.window_id,
                    session_id: sinks.event_proxy.session_id(),
                    trigger_source: source,
                    silence_duration_ms: silence_ms,
                });
            }
        }

        // Update write interest based on pending write data.
        let needs_write = !write_list.is_empty();
        if needs_write != interest.writable {
            interest.writable = needs_write;
            if let Err(e) = pty.reregister(&poll, interest, PollMode::Level) {
                tracing::error!("PTY reregister failed: {e}");
                break;
            }
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
fn pty_read_with_scan(
    pty: &mut tty::Pty,
    terminal: &Arc<FairMutex<Term<EventProxy>>>,
    sinks: &PtyEventSinks,
    state: &mut PtyReadState,
    mut smart_trigger: Option<&mut crate::silence::SmartTrigger>,
) -> io::Result<bool> {
    let mut unprocessed = 0;
    let mut processed = 0;

    // Reserve the next terminal lock for PTY reading.
    let _terminal_lease = Some(terminal.lease());
    let mut term_guard = None;

    loop {
        // Read from the PTY.
        match pty.reader().read(&mut state.buf[unprocessed..]) {
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

        let data = &state.buf[..unprocessed];

        // Pre-scan for OSC shell integration sequences before VTE parsing.
        let osc_events = state.scanner.scan(data);
        for osc_event in &osc_events {
            // Get absolute cursor line for block tracking.
            // cursor.point.line is viewport-relative (0 = top of screen).
            // Convert to absolute: history_size + viewport_line.
            let history = terminal_ref.grid().history_size();
            let line = history + terminal_ref.grid().cursor.point.line.0 as usize;
            let shell_event = convert_osc_to_shell(osc_event.clone());
            let _ = sinks.app_proxy.send_event(AppEvent::Shell {
                window_id: sinks.window_id,
                session_id: sinks.event_proxy.session_id(),
                event: shell_event,
                line,
            });
            if matches!(osc_event, crate::osc_scanner::OscEvent::PromptStart) {
                if let Some(ref mut trigger) = smart_trigger {
                    trigger.on_shell_prompt();
                }
            }
        }

        // Output capture: accumulate bytes between CommandExecuted and CommandFinished.
        // Check alt-screen sequences in raw bytes (avoids locking terminal for TermMode).
        state.output_buffer.check_alt_screen(data);
        state.output_buffer.append(data);

        if let Some(ref mut trigger) = smart_trigger {
            trigger.on_output_bytes(data);
        }

        // Handle capture lifecycle based on shell integration events.
        for osc_event in &osc_events {
            match osc_event {
                crate::osc_scanner::OscEvent::CommandExecuted => {
                    state.output_buffer.start_capture();
                    // Re-append current chunk so output in the same PTY read
                    // (fast commands) is captured after start_capture activates.
                    state.output_buffer.append(data);
                }
                crate::osc_scanner::OscEvent::CommandFinished { .. } => {
                    if let Some(raw_bytes) = state.output_buffer.finish() {
                        if !raw_bytes.is_empty() {
                            let _ = sinks.app_proxy.send_event(AppEvent::CommandOutput {
                                window_id: sinks.window_id,
                                session_id: sinks.event_proxy.session_id(),
                                raw_output: raw_bytes,
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        // Parse the incoming bytes through the VTE parser (updates terminal grid).
        state.parser.advance(&mut **terminal_ref, data);

        processed += unprocessed;
        unprocessed = 0;

        // Avoid blocking the terminal too long.
        if processed >= MAX_LOCKED_READ {
            break;
        }
    }

    // Return whether a wakeup is needed (data was processed and not fully
    // synchronized). The caller throttles Wakeup sends to avoid flooding the
    // event loop — on Windows, PostMessage has higher priority than WM_PAINT,
    // so rapid Wakeup events starve rendering.
    if state.parser.sync_bytes_count() < processed && processed > 0 {
        return Ok(true);
    }

    Ok(false)
}

/// Write pending data from the write list to a writer (PTY stdin).
///
/// Accepts `&mut dyn Write` rather than a concrete PTY type for testability.
/// Empty entries are skipped to prevent a stall: `write(b"")` returns `Ok(0)`
/// which would otherwise leave the empty entry permanently at the front.
fn pty_write(
    writer: &mut dyn Write,
    write_list: &mut VecDeque<Cow<'static, [u8]>>,
) -> io::Result<()> {
    while let Some(data) = write_list.front() {
        // Skip empty entries — write(b"") returns Ok(0) which hits the
        // break below, permanently stalling the queue.
        if data.is_empty() {
            write_list.pop_front();
            continue;
        }
        match writer.write(data.as_ref()) {
            Ok(0) => break,
            Ok(n) => {
                if n >= data.len() {
                    write_list.pop_front();
                } else {
                    // Partial write — replace front with remainder.
                    // front_mut() is guaranteed Some: we entered via front() above
                    // and only the n >= len branch pops.
                    let remaining = data[n..].to_vec();
                    if let Some(front) = write_list.front_mut() {
                        *front = Cow::Owned(remaining);
                    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pty_write_empty_list() {
        let mut writer = Vec::new();
        let mut write_list = VecDeque::new();
        pty_write(&mut writer, &mut write_list).unwrap();
        assert!(write_list.is_empty());
        assert!(writer.is_empty());
    }

    #[test]
    fn test_pty_write_single_entry() {
        let mut writer = Vec::new();
        let mut write_list = VecDeque::new();
        write_list.push_back(Cow::Borrowed(b"hello" as &[u8]));
        pty_write(&mut writer, &mut write_list).unwrap();
        assert_eq!(writer, b"hello");
        assert!(write_list.is_empty());
    }

    #[test]
    fn test_pty_write_multiple_entries() {
        let mut writer = Vec::new();
        let mut write_list = VecDeque::new();
        write_list.push_back(Cow::Borrowed(b"hello " as &[u8]));
        write_list.push_back(Cow::Borrowed(b"world" as &[u8]));
        pty_write(&mut writer, &mut write_list).unwrap();
        assert_eq!(writer, b"hello world");
        assert!(write_list.is_empty());
    }

    #[test]
    fn test_pty_write_skips_empty_data() {
        let mut writer = Vec::new();
        let mut write_list = VecDeque::new();
        write_list.push_back(Cow::Borrowed(b"" as &[u8]));
        write_list.push_back(Cow::Borrowed(b"hello" as &[u8]));
        pty_write(&mut writer, &mut write_list).unwrap();
        assert_eq!(writer, b"hello");
        assert!(write_list.is_empty());
    }

    #[test]
    fn test_pty_write_all_empty_entries() {
        let mut writer = Vec::new();
        let mut write_list = VecDeque::new();
        write_list.push_back(Cow::Borrowed(b"" as &[u8]));
        write_list.push_back(Cow::Borrowed(b"" as &[u8]));
        write_list.push_back(Cow::Borrowed(b"" as &[u8]));
        pty_write(&mut writer, &mut write_list).unwrap();
        assert!(writer.is_empty());
        assert!(write_list.is_empty());
    }

    #[test]
    fn test_pty_write_partial_write() {
        /// A writer that accepts at most `max_per_write` bytes per call.
        struct PartialWriter {
            inner: Vec<u8>,
            max_per_write: usize,
        }
        impl io::Write for PartialWriter {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                let n = buf.len().min(self.max_per_write);
                self.inner.extend_from_slice(&buf[..n]);
                Ok(n)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let mut writer = PartialWriter {
            inner: Vec::new(),
            max_per_write: 3,
        };
        let mut write_list = VecDeque::new();
        write_list.push_back(Cow::Borrowed(b"hello" as &[u8]));
        pty_write(&mut writer, &mut write_list).unwrap();
        // Only 3 bytes written
        assert_eq!(writer.inner, b"hel");
        // Remainder stays in the write list
        assert_eq!(write_list.len(), 1);
        assert_eq!(write_list[0].as_ref(), b"lo");
    }

    #[test]
    fn test_pty_write_would_block_retains_data() {
        struct BlockingWriter;
        impl io::Write for BlockingWriter {
            fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
                Err(io::Error::new(ErrorKind::WouldBlock, "would block"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let mut writer = BlockingWriter;
        let mut write_list = VecDeque::new();
        write_list.push_back(Cow::Borrowed(b"hello" as &[u8]));
        // WouldBlock is handled gracefully (not propagated as error)
        pty_write(&mut writer, &mut write_list).unwrap();
        // Data remains in queue for retry
        assert_eq!(write_list.len(), 1);
    }

    #[test]
    fn test_pty_write_io_error_propagates() {
        struct ErrorWriter;
        impl io::Write for ErrorWriter {
            fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
                Err(io::Error::new(ErrorKind::BrokenPipe, "broken pipe"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let mut writer = ErrorWriter;
        let mut write_list = VecDeque::new();
        write_list.push_back(Cow::Borrowed(b"hello" as &[u8]));
        let result = pty_write(&mut writer, &mut write_list);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), ErrorKind::BrokenPipe);
    }
}
