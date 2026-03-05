use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::event_loop::{EventLoopSender, Msg as PtyMsg};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
use glass_core::config::GlassConfig;
use glass_core::event::AppEvent;
use glass_renderer::{FrameRenderer, GlassRenderer};
use glass_terminal::{DefaultColors, EventProxy, snapshot_term};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId};

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
    pty_sender: EventLoopSender,
    /// Shared terminal state grid.
    term: Arc<FairMutex<Term<EventProxy>>>,
    /// Default terminal colors for snapshot resolution.
    default_colors: DefaultColors,
}

/// Top-level application state. Holds all open windows.
///
/// The proxy is created from `EventLoop<AppEvent>` before `run_app()` is called,
/// because `ActiveEventLoop` (passed in callbacks) does not have `create_proxy()`.
struct Processor {
    windows: HashMap<WindowId, WindowContext>,
    /// Pre-created proxy for sending AppEvent from PTY threads to the winit event loop.
    proxy: EventLoopProxy<AppEvent>,
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

        // wgpu init is async; block via pollster from this sync callback
        let renderer = pollster::block_on(GlassRenderer::new(Arc::clone(&window)));

        // Create FrameRenderer with font config
        let config = GlassConfig::default();
        let scale_factor = window.scale_factor() as f32;
        let frame_renderer = FrameRenderer::new(
            renderer.device(),
            renderer.queue(),
            renderer.surface_format(),
            &config.font_family,
            config.font_size,
            scale_factor,
        );

        // Compute initial terminal size from font metrics
        let (cell_w, cell_h) = frame_renderer.cell_size();
        let size = window.inner_size();
        let num_cols = (size.width as f32 / cell_w).floor().max(1.0) as u16;
        let num_lines = (size.height as f32 / cell_h).floor().max(1.0) as u16;

        tracing::info!(
            "Font metrics: cell={}x{} grid={}x{} scale={}",
            cell_w, cell_h, num_cols, num_lines, scale_factor
        );

        // Create EventProxy using the pre-created proxy (EventLoopProxy is Clone)
        let event_proxy = EventProxy::new(self.proxy.clone(), window.id());

        // Spawn PowerShell via ConPTY with a dedicated reader thread
        let (pty_sender, term) = glass_terminal::spawn_pty(event_proxy);

        // Send initial resize with correct font-metrics-based cell dimensions
        let initial_size = WindowSize {
            num_lines,
            num_cols,
            cell_width: cell_w as u16,
            cell_height: cell_h as u16,
        };
        let _ = pty_sender.send(PtyMsg::Resize(initial_size));

        // Also resize the Term grid to match
        term.lock().resize(TermDimensions { columns: num_cols as usize, screen_lines: num_lines as usize });

        tracing::info!("PTY spawned — PowerShell is running");

        let default_colors = DefaultColors::default();

        let id = window.id();
        self.windows.insert(id, WindowContext {
            window,
            renderer,
            frame_renderer,
            pty_sender,
            term,
            default_colors,
        });
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

                // Get surface texture
                let Some(frame) = ctx.renderer.get_current_texture() else {
                    return;
                };
                let view = frame.texture.create_view(&Default::default());
                let sc = ctx.renderer.surface_config();

                // Draw frame using FrameRenderer (no Term lock held)
                ctx.frame_renderer.draw_frame(
                    ctx.renderer.device(),
                    ctx.renderer.queue(),
                    &view,
                    sc.width,
                    sc.height,
                    &snapshot,
                );

                frame.present();
                ctx.frame_renderer.trim();
            }
            WindowEvent::Resized(size) => {
                if size.width == 0 || size.height == 0 {
                    return;
                }
                ctx.renderer.resize(size.width, size.height);

                // Compute terminal grid size from font metrics
                let (cell_w, cell_h) = ctx.frame_renderer.cell_size();
                let num_cols = (size.width as f32 / cell_w).floor().max(1.0) as u16;
                let num_lines = (size.height as f32 / cell_h).floor().max(1.0) as u16;

                // Notify PTY of the new terminal size with real cell dimensions
                let new_window_size = WindowSize {
                    num_lines,
                    num_cols,
                    cell_width: cell_w as u16,
                    cell_height: cell_h as u16,
                };
                let _ = ctx.pty_sender.send(PtyMsg::Resize(new_window_size));

                // Also resize the Term grid so content reflows (CORE-07)
                ctx.term.lock().resize(TermDimensions { columns: num_cols as usize, screen_lines: num_lines as usize });

                // Request a redraw after resize so the surface is repainted immediately
                ctx.window.request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                // Forward keyboard text input to the PTY on key press.
                // This is a minimal handler for ASCII text — Phase 2 Plan 03 adds full
                // escape sequence encoding for Ctrl, Alt, arrows, function keys, etc.
                if event.state == ElementState::Pressed {
                    if let Some(text) = event.text {
                        let bytes: Cow<'static, [u8]> =
                            Cow::Owned(text.as_bytes().to_vec());
                        if !bytes.is_empty() {
                            let _ = ctx.pty_sender.send(PtyMsg::Input(bytes));
                            tracing::trace!("Key input: {:?}", text);
                        }
                    }
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
        }
    }

    /// Called when the event queue is drained. No-op for Phase 1.
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {}
}

fn main() {
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

    tracing::info!("Glass starting");

    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .expect("Failed to create event loop");

    // Create proxy BEFORE run_app() — EventLoopProxy<AppEvent> is Clone + Send,
    // so the PTY EventProxy stores a clone of this.
    let proxy = event_loop.create_proxy();

    let mut processor = Processor {
        windows: HashMap::new(),
        proxy,
    };

    event_loop
        .run_app(&mut processor)
        .expect("Event loop exited with error");
}

#[cfg(test)]
mod tests;
