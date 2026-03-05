#[derive(Debug, Clone)]
pub enum AppEvent {
    TerminalDirty { window_id: winit::window::WindowId },
    SetTitle { window_id: winit::window::WindowId, title: String },
    TerminalExit { window_id: winit::window::WindowId },
    // Phase 3: ShellHook(HookEvent) — added when shell integration is built
}
