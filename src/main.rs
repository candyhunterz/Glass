use std::collections::HashMap;
use std::sync::Arc;

use glass_core::event::AppEvent;
use glass_renderer::GlassRenderer;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

/// Per-window state: the OS window handle and its GPU renderer.
struct WindowContext {
    window: Arc<Window>,
    renderer: GlassRenderer,
}

/// Top-level application state. Holds all open windows.
struct Processor {
    windows: HashMap<WindowId, WindowContext>,
}

impl ApplicationHandler<AppEvent> for Processor {
    /// Called at startup on desktop (Windows) and on app resume on mobile/web.
    ///
    /// In winit 0.30.13, `resumed` is the required method called once at startup on Windows.
    /// This is where the window and GPU surface must be created.
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

        let id = window.id();
        self.windows.insert(id, WindowContext { window, renderer });
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
                ctx.renderer.draw();
            }
            WindowEvent::Resized(size) => {
                ctx.renderer.resize(size.width, size.height);
                // Request a redraw after resize so the surface is repainted immediately
                ctx.window.request_redraw();
            }
            _ => {}
        }
    }

    /// Handle custom AppEvents sent from other threads (e.g., PTY reader thread).
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::TerminalDirty { window_id } => {
                if let Some(ctx) = self.windows.get(&window_id) {
                    ctx.window.request_redraw();
                }
            }
            AppEvent::SetTitle { window_id, title } => {
                if let Some(ctx) = self.windows.get(&window_id) {
                    ctx.window.set_title(&title);
                }
            }
            AppEvent::TerminalExit { window_id } => {
                self.windows.remove(&window_id);
                // If no windows remain, exit the event loop
                if self.windows.is_empty() {
                    // Note: we don't have event_loop here; exit will happen on next event
                    // In Phase 3 this will be handled more gracefully
                }
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

    let mut processor = Processor {
        windows: HashMap::new(),
    };

    event_loop
        .run_app(&mut processor)
        .expect("Event loop exited with error");
}
