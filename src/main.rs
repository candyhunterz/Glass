use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Term, TermMode};
use clap::{Parser, Subcommand};
use glass_core::config::GlassConfig;
use glass_core::event::{AppEvent, GitStatus, ShellEvent};
use glass_renderer::{FontSystem, FrameRenderer, GlassRenderer};
use glass_terminal::{
    BlockManager, DefaultColors, EventProxy, OscEvent, PtyMsg, PtySender, StatusState,
    encode_key, query_git_status, snapshot_term,
};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

// ---------------------------------------------------------------------------
// CLI definition (clap derive)
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "glass", version, about = "GPU-accelerated terminal emulator")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug, PartialEq)]
enum Commands {
    /// Query command history
    History,
    /// MCP server commands
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
}

#[derive(Subcommand, Debug, PartialEq)]
enum McpAction {
    /// Start the MCP server over stdio
    Serve,
}

/// Simple grid dimensions for Term::resize().
struct TermDimensions {
    columns: usize,
    screen_lines: usize,
}

impl Dimensions for TermDimensions {
    fn total_lines(&self) -> usize {
        self.screen_lines
    }
    fn screen_lines(&self) -> usize {
        self.screen_lines
    }
    fn columns(&self) -> usize {
        self.columns
    }
}

/// Per-window state: OS window handle, GPU renderer, and PTY connection.
struct WindowContext {
    window: Arc<Window>,
    renderer: GlassRenderer,
    /// GPU text rendering pipeline.
    frame_renderer: FrameRenderer,
    /// Sender to write input to the PTY or resize it.
    pty_sender: PtySender,
    /// Shared terminal state grid.
    term: Arc<FairMutex<Term<EventProxy>>>,
    /// Default terminal colors for snapshot resolution.
    default_colors: DefaultColors,
    /// Block manager tracking command lifecycle via shell integration.
    block_manager: BlockManager,
    /// Status bar state: CWD and git info.
    status: StatusState,
    /// Whether the first-frame cold start metric has been logged.
    first_frame_logged: bool,
}

/// Top-level application state. Holds all open windows.
///
/// The proxy is created from `EventLoop<AppEvent>` before `run_app()` is called,
/// because `ActiveEventLoop` (passed in callbacks) does not have `create_proxy()`.
struct Processor {
    windows: HashMap<WindowId, WindowContext>,
    /// Pre-created proxy for sending AppEvent from PTY threads to the winit event loop.
    proxy: EventLoopProxy<AppEvent>,
    /// Current keyboard modifier state, updated by ModifiersChanged events.
    modifiers: ModifiersState,
    /// User configuration loaded from ~/.glass/config.toml at startup.
    config: GlassConfig,
    /// Instant captured at program start for cold start measurement.
    cold_start: std::time::Instant,
}

/// Convert a ShellEvent (from glass_core) back to OscEvent (from glass_terminal)
/// so BlockManager.handle_event() can process it.
fn shell_event_to_osc(event: &ShellEvent) -> OscEvent {
    match event {
        ShellEvent::PromptStart => OscEvent::PromptStart,
        ShellEvent::CommandStart => OscEvent::CommandStart,
        ShellEvent::CommandExecuted => OscEvent::CommandExecuted,
        ShellEvent::CommandFinished { exit_code } => OscEvent::CommandFinished {
            exit_code: *exit_code,
        },
        ShellEvent::CurrentDirectory(path) => OscEvent::CurrentDirectory(path.clone()),
    }
}

impl ApplicationHandler<AppEvent> for Processor {
    /// Called at startup on desktop (Windows) and on app resume on mobile/web.
    ///
    /// In winit 0.30.13, `resumed` is the required method called once at startup on Windows.
    /// This is where the window, GPU surface, and PTY must be created.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Only create a window if we don't have one yet (handles re-resume on mobile)
        if !self.windows.is_empty() {
            return;
        }

        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes().with_title("Glass"))
                .expect("Failed to create window"),
        );

        // Parallelize font discovery with GPU init — FontSystem::new() enumerates
        // all system fonts (~35ms) and doesn't need the GPU device.
        let font_handle = std::thread::spawn(FontSystem::new);

        // wgpu init is async; block via pollster from this sync callback
        let renderer = pollster::block_on(GlassRenderer::new(Arc::clone(&window)));

        // Join font thread — should already be done since GPU init takes longer
        let font_system = font_handle.join().expect("Font system thread panicked");

        // Create FrameRenderer with pre-loaded font system
        let scale_factor = window.scale_factor() as f32;
        let frame_renderer = FrameRenderer::with_font_system(
            font_system,
            renderer.device(),
            renderer.queue(),
            renderer.surface_format(),
            &self.config.font_family,
            self.config.font_size,
            scale_factor,
        );

        // Compute initial terminal size from font metrics.
        // Subtract 1 line for the status bar so the PTY resize reflects actual content area.
        let (cell_w, cell_h) = frame_renderer.cell_size();
        let size = window.inner_size();
        let num_cols = (size.width as f32 / cell_w).floor().max(1.0) as u16;
        let num_lines = ((size.height as f32 / cell_h).floor().max(2.0) as u16).saturating_sub(1);

        tracing::info!(
            "Font metrics: cell={}x{} grid={}x{} (status bar reserves 1 line) scale={}",
            cell_w, cell_h, num_cols, num_lines, scale_factor
        );

        // Create EventProxy using the pre-created proxy (EventLoopProxy is Clone)
        let event_proxy = EventProxy::new(self.proxy.clone(), window.id());

        // Spawn shell via ConPTY with a dedicated reader thread + OscScanner
        let (pty_sender, term) = glass_terminal::spawn_pty(
            event_proxy,
            self.proxy.clone(),
            window.id(),
            self.config.shell.as_deref(),
        );

        // Send initial resize with correct font-metrics-based cell dimensions
        let initial_size = WindowSize {
            num_lines,
            num_cols,
            cell_width: cell_w as u16,
            cell_height: cell_h as u16,
        };
        let _ = pty_sender.send(PtyMsg::Resize(initial_size));

        // Also resize the Term grid to match
        term.lock().resize(TermDimensions {
            columns: num_cols as usize,
            screen_lines: num_lines as usize,
        });

        tracing::info!("PTY spawned — PowerShell is running");

        let default_colors = DefaultColors::default();

        let id = window.id();
        self.windows.insert(
            id,
            WindowContext {
                window,
                renderer,
                frame_renderer,
                pty_sender,
                term,
                default_colors,
                block_manager: BlockManager::new(),
                status: StatusState::default(),
                first_frame_logged: false,
            },
        );
    }

    /// Handle per-window OS events.
    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(ctx) = self.windows.get_mut(&window_id) else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => {
                self.windows.remove(&window_id);
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                // Lock Term briefly for snapshot only, then release
                let snapshot = {
                    let term = ctx.term.lock();
                    snapshot_term(&term, &ctx.default_colors)
                };

                // Get visible blocks for the current viewport
                let visible_blocks = ctx.block_manager.visible_blocks(
                    snapshot.display_offset,
                    snapshot.screen_lines,
                );

                // Get surface texture
                let Some(frame) = ctx.renderer.get_current_texture() else {
                    return;
                };
                let view = frame.texture.create_view(&Default::default());
                let sc = ctx.renderer.surface_config();

                // Draw frame using FrameRenderer with block decorations and status bar
                ctx.frame_renderer.draw_frame(
                    ctx.renderer.device(),
                    ctx.renderer.queue(),
                    &view,
                    sc.width,
                    sc.height,
                    &snapshot,
                    &visible_blocks,
                    Some(&ctx.status),
                );

                frame.present();

                if !ctx.first_frame_logged {
                    ctx.first_frame_logged = true;
                    tracing::info!("PERF cold_start={:?}", self.cold_start.elapsed());
                    if let Some(usage) = memory_stats::memory_stats() {
                        tracing::info!(
                            "PERF memory_physical={:.1}MB",
                            usage.physical_mem as f64 / 1_048_576.0
                        );
                    }
                }

                ctx.frame_renderer.trim();
            }
            WindowEvent::Resized(size) => {
                if size.width == 0 || size.height == 0 {
                    return;
                }
                ctx.renderer.resize(size.width, size.height);

                // Compute terminal grid size from font metrics.
                // Subtract 1 line for the status bar.
                let (cell_w, cell_h) = ctx.frame_renderer.cell_size();
                let num_cols = (size.width as f32 / cell_w).floor().max(1.0) as u16;
                let num_lines =
                    ((size.height as f32 / cell_h).floor().max(2.0) as u16).saturating_sub(1);

                // Notify PTY of the new terminal size with real cell dimensions
                let new_window_size = WindowSize {
                    num_lines,
                    num_cols,
                    cell_width: cell_w as u16,
                    cell_height: cell_h as u16,
                };
                let _ = ctx.pty_sender.send(PtyMsg::Resize(new_window_size));

                // Also resize the Term grid so content reflows (CORE-07)
                ctx.term.lock().resize(TermDimensions {
                    columns: num_cols as usize,
                    screen_lines: num_lines as usize,
                });

                // Request a redraw after resize so the surface is repainted immediately
                ctx.window.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    let modifiers = self.modifiers;
                    let mode = *ctx.term.lock().mode();

                    // Check for Glass-handled keys first
                    if modifiers.control_key() && modifiers.shift_key() {
                        match &event.logical_key {
                            Key::Character(c)
                                if c.as_str().eq_ignore_ascii_case("c") =>
                            {
                                clipboard_copy(&ctx.term);
                                return;
                            }
                            Key::Character(c)
                                if c.as_str().eq_ignore_ascii_case("v") =>
                            {
                                clipboard_paste(&ctx.pty_sender, mode);
                                return;
                            }
                            _ => {}
                        }
                    }

                    // Shift+PageUp/Down: scrollback
                    if modifiers.shift_key()
                        && !modifiers.control_key()
                        && !modifiers.alt_key()
                    {
                        match &event.logical_key {
                            Key::Named(NamedKey::PageUp) => {
                                ctx.term.lock().scroll_display(Scroll::PageUp);
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::PageDown) => {
                                ctx.term.lock().scroll_display(Scroll::PageDown);
                                ctx.window.request_redraw();
                                return;
                            }
                            _ => {}
                        }
                    }

                    // Forward to PTY via encoder
                    let key_start = std::time::Instant::now();
                    if let Some(bytes) =
                        encode_key(&event.logical_key, modifiers, mode)
                    {
                        let _ = ctx
                            .pty_sender
                            .send(PtyMsg::Input(Cow::Owned(bytes)));
                        tracing::trace!("PERF key_latency={:?}", key_start.elapsed());
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as i32,
                    MouseScrollDelta::PixelDelta(pos) => {
                        let (_, cell_h) = ctx.frame_renderer.cell_size();
                        (pos.y / cell_h as f64) as i32
                    }
                };
                if lines != 0 {
                    // Negative delta = scroll down (towards bottom), positive = scroll up
                    ctx.term.lock().scroll_display(Scroll::Delta(-lines));
                    ctx.window.request_redraw();
                }
            }
            _ => {}
        }
    }

    /// Handle custom AppEvents sent from the PTY reader thread.
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::TerminalDirty { window_id } => {
                if let Some(ctx) = self.windows.get(&window_id) {
                    tracing::trace!("Terminal output received — requesting redraw");
                    ctx.window.request_redraw();
                }
            }
            AppEvent::SetTitle { window_id, title } => {
                if let Some(ctx) = self.windows.get(&window_id) {
                    ctx.window.set_title(&title);
                }
            }
            AppEvent::TerminalExit { window_id } => {
                tracing::info!("Shell exited — closing window");
                self.windows.remove(&window_id);
                // Exit the event loop when the shell exits
                event_loop.exit();
            }
            AppEvent::Shell {
                window_id,
                event: shell_event,
                line,
            } => {
                if let Some(ctx) = self.windows.get_mut(&window_id) {
                    // Convert ShellEvent to OscEvent for BlockManager
                    let osc_event = shell_event_to_osc(&shell_event);
                    ctx.block_manager.handle_event(&osc_event, line);

                    // On CurrentDirectory events, update status and query git info
                    if let ShellEvent::CurrentDirectory(ref cwd) = shell_event {
                        ctx.status.set_cwd(cwd.clone());

                        // Spawn background thread for git status query
                        // to avoid blocking the render thread
                        if !ctx.status.git_query_pending {
                            ctx.status.git_query_pending = true;
                            let cwd_owned = cwd.clone();
                            let proxy = self.proxy.clone();
                            let wid = window_id;
                            std::thread::Builder::new()
                                .name("Glass git query".into())
                                .spawn(move || {
                                    let git_info = query_git_status(&cwd_owned);
                                    let info = git_info.map(|gi| GitStatus {
                                        branch: gi.branch,
                                        dirty_count: gi.dirty_count,
                                    });
                                    let _ = proxy.send_event(AppEvent::GitInfo {
                                        window_id: wid,
                                        info,
                                    });
                                })
                                .ok();
                        }
                    }

                    // Request redraw to reflect block state changes
                    ctx.window.request_redraw();
                }
            }
            AppEvent::GitInfo { window_id, info } => {
                if let Some(ctx) = self.windows.get_mut(&window_id) {
                    ctx.status.git_query_pending = false;
                    let git_info = info.map(|gi| glass_terminal::GitInfo {
                        branch: gi.branch,
                        dirty_count: gi.dirty_count,
                    });
                    ctx.status.set_git_info(git_info);
                    ctx.window.request_redraw();
                }
            }
        }
    }

    /// Called when the event queue is drained. No-op for Phase 1.
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {}
}

/// Copy the current terminal selection to the system clipboard.
fn clipboard_copy(term: &Arc<FairMutex<Term<EventProxy>>>) {
    let term = term.lock();
    if let Some(selection) = term.selection_to_string() {
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(selection);
        }
    }
}

/// Paste text from the system clipboard into the PTY.
/// Wraps with bracketed paste sequences when BRACKETED_PASTE mode is active.
fn clipboard_paste(sender: &PtySender, mode: TermMode) {
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if let Ok(text) = clipboard.get_text() {
            let bytes = if mode.contains(TermMode::BRACKETED_PASTE) {
                let mut buf = Vec::new();
                buf.extend_from_slice(b"\x1b[200~");
                buf.extend_from_slice(text.as_bytes());
                buf.extend_from_slice(b"\x1b[201~");
                buf
            } else {
                text.into_bytes()
            };
            let _ = sender.send(PtyMsg::Input(Cow::Owned(bytes)));
        }
    }
}

fn main() {
    let cold_start = std::time::Instant::now();

    // FIRST: set UTF-8 console code page on Windows before any PTY creation (Pitfall 5)
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::System::Console::{SetConsoleCP, SetConsoleOutputCP};
        SetConsoleCP(65001);
        SetConsoleOutputCP(65001);
    }

    // Initialize structured logging; use RUST_LOG env var to control verbosity
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Parse CLI BEFORE creating the event loop — subcommands must not open a window.
    let cli = Cli::parse();

    match cli.command {
        None => {
            // No subcommand: launch the terminal GUI (default behavior)
            tracing::info!("Glass starting");

            let config = GlassConfig::load();
            tracing::info!("Config: font_family={}, font_size={}, shell={:?}",
                config.font_family, config.font_size, config.shell);

            let event_loop = EventLoop::<AppEvent>::with_user_event()
                .build()
                .expect("Failed to create event loop");

            // Create proxy BEFORE run_app() — EventLoopProxy<AppEvent> is Clone + Send,
            // so the PTY EventProxy stores a clone of this.
            let proxy = event_loop.create_proxy();

            let mut processor = Processor {
                windows: HashMap::new(),
                proxy,
                modifiers: ModifiersState::empty(),
                config,
                cold_start,
            };

            event_loop
                .run_app(&mut processor)
                .expect("Event loop exited with error");
        }
        Some(Commands::History) => {
            eprintln!("glass history: not yet implemented (Phase 7)");
            std::process::exit(1);
        }
        Some(Commands::Mcp { action: McpAction::Serve }) => {
            eprintln!("glass mcp serve: not yet implemented (Phase 9)");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests;
