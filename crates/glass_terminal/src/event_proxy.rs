use alacritty_terminal::event::EventListener;
use glass_core::event::AppEvent;
use winit::event_loop::EventLoopProxy;
use winit::window::WindowId;

/// EventProxy bridges PTY events to the winit event loop.
///
/// When the PTY reader thread receives terminal output or state changes,
/// it calls EventProxy::send_event() which forwards an AppEvent to the
/// winit event loop via EventLoopProxy. This wakes the main thread so it
/// can request a redraw, update the window title, or handle shell exit.
#[derive(Clone)]
pub struct EventProxy {
    proxy: EventLoopProxy<AppEvent>,
    window_id: WindowId,
}

impl EventProxy {
    pub fn new(proxy: EventLoopProxy<AppEvent>, window_id: WindowId) -> Self {
        Self { proxy, window_id }
    }
}

impl EventListener for EventProxy {
    fn send_event(&self, event: alacritty_terminal::event::Event) {
        use alacritty_terminal::event::Event;
        match event {
            Event::Wakeup => {
                let _ = self
                    .proxy
                    .send_event(AppEvent::TerminalDirty { window_id: self.window_id });
            }
            Event::Title(title) => {
                let _ = self
                    .proxy
                    .send_event(AppEvent::SetTitle { window_id: self.window_id, title });
            }
            Event::Exit | Event::ChildExit(_) => {
                let _ = self
                    .proxy
                    .send_event(AppEvent::TerminalExit { window_id: self.window_id });
            }
            _ => {}
        }
    }
}
