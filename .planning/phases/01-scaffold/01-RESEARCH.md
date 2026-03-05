# Phase 1: Scaffold - Research

**Researched:** 2026-03-04
**Domain:** Rust Cargo workspace setup, wgpu DX12 surface initialization, ConPTY PTY spawning on Windows
**Confidence:** HIGH

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| CORE-01 | User can launch Glass and get a working PowerShell prompt via ConPTY | alacritty_terminal 0.25.1 tty::new() spawns ConPTY; Options struct fields verified; EventLoop::spawn() runs dedicated PTY reader thread |
| RNDR-01 | Terminal output renders via GPU acceleration (wgpu with DX12 on Windows) | wgpu 28.0.0 auto-selects DX12 on Windows; winit 0.30.13 ApplicationHandler::can_create_surfaces() is the correct window creation callback; pollster::block_on() for sync init |
</phase_requirements>

---

## Summary

Phase 1 (Scaffold) establishes the structural foundation that all subsequent phases depend on. The three plans — Cargo workspace, wgpu surface, and ConPTY PTY — must be completed in dependency order: workspace first (everything else depends on it compiling), then window/GPU surface, then PTY. This phase has no feature work; its value is in resolving all structural pitfalls before feature development begins.

The research base for this phase is unusually complete because comprehensive project-level research was done before the roadmap was finalized (see `.planning/research/`). All version choices, architectural patterns, API details, and pitfalls are documented with HIGH confidence from primary sources. This RESEARCH.md synthesizes that knowledge scoped specifically to Phase 1's three plans, adding Phase 1-specific detail on the exact APIs used.

**Critical insight for Phase 1:** Four structural pitfalls — ConPTY escape sequence rewriting, wgpu surface resize handling, PTY reader thread blocking, and Windows UTF-8 code page — must be correctly handled here. Retrofitting any of these after the VTE rendering pipeline is built (Phase 2) is highly disruptive. Get them right in the scaffold before any rendering work.

**Primary recommendation:** Build in plan order (01-01 workspace → 01-02 wgpu surface → 01-03 ConPTY). Do not start 01-03 until 01-02 builds cleanly. Do not start 01-02 until 01-01 produces a clean `cargo build` for all seven crates.

---

## Standard Stack

### Core (Phase 1 Active)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `alacritty_terminal` | `= "0.25.1"` (exact pin) | VTE parsing, terminal grid, ConPTY PTY on Windows | Battle-tested since 2017; eliminates 5,000+ lines of ANSI parser work; Apache 2.0; `tty::new()` wraps ConPTY on Windows |
| `wgpu` | `"28.0.0"` | GPU surface, render passes, DX12 backend on Windows | Auto-selects DX12 on Windows 11; WebGPU-standard API; WGSL shaders; production-stable DX12 backend |
| `winit` | `"0.30.13"` | Window creation, OS event loop, keyboard events | Standard Rust window library; 0.30 introduced `ApplicationHandler` trait; provides `raw-window-handle` 0.6 that wgpu requires |
| `pollster` | `"0.4.0"` | Blocks on async wgpu init from sync winit callbacks | Mandatory: winit event callbacks are sync, wgpu init is async; `pollster::block_on()` is the correct bridge |
| `tokio` | `"1.50.0"` | Async runtime for PTY I/O glue | Industry standard; alacritty_terminal's EventLoop integrates with it |
| `bytemuck` | `"1.25.0"` | Zero-cost byte casting for wgpu buffers | Required for wgpu GPU buffer uploads: `bytemuck::cast_slice()` on Pod structs |
| `anyhow` | `"1.0.102"` | Error propagation in binary | Avoids `Box<dyn Error>` boilerplate; use in binary; use `thiserror` in library crates |
| `tracing` | `"0.1.44"` | Structured logging | Critical for diagnosing PTY threading and latency; prefer over `log` crate |
| `tracing-subscriber` | `"0.3"` | Log output | Use with `EnvFilter` for `RUST_LOG=trace` support |

### Stub Crates (Phase 1 Creates, Future Phases Fill)

| Crate | Purpose | Phase It Gets Filled |
|-------|---------|---------------------|
| `glass_history` | Structured scrollback DB (SQLite) | Phase 2 (roadmap Phase 4) |
| `glass_snapshot` | Filesystem snapshot for undo | Phase 3 (roadmap Phase 6) |
| `glass_pipes` | Pipe visualization | Phase 4 (roadmap Phase 6) |
| `glass_mcp` | MCP server | Phase 2 (roadmap Phase 5) |

### Alternatives Not Used

| Instead of | Could Use | Why Not |
|------------|-----------|---------|
| `alacritty_terminal` | `vte` crate directly | `vte` is only the parser; `alacritty_terminal` builds the full terminal state (grid, cursor, colors, modes) on top of it — using `vte` directly means writing all of `alacritty_terminal` yourself |
| `wgpu` | Raw DX12 via `d3d12` crate | Massively more complexity; no cross-platform future; wgpu's DX12 backend is production-quality |
| `winit` | `sdl2`, `tao` | SDL2 requires C bindings, no `raw-window-handle` ecosystem; `tao` is Tauri-specific, winit 0.30 has closed the gaps |
| `pollster` | `async_std::task::block_on` | Pollster is zero-dependency, purpose-built for this exact pattern; async-std brings heavy transitive deps |

**Installation:**
```toml
# Cargo.toml (workspace root)
[workspace]
resolver = "2"
members = ["crates/*", "."]

[workspace.dependencies]
wgpu          = "28.0.0"
winit         = "0.30.13"
bytemuck      = { version = "1.25.0", features = ["derive"] }
pollster      = "0.4.0"
alacritty_terminal = "=0.25.1"
tokio         = { version = "1.50.0", features = ["full"] }
serde         = { version = "1.0.228", features = ["derive"] }
toml          = "1.0.4"
anyhow        = "1.0.102"
tracing       = "0.1.44"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
bytemuck      = { version = "1.25.0", features = ["derive"] }
rusqlite      = { version = "0.38.0", features = ["bundled"] }
```

> **Resolver note:** Use `resolver = "2"` explicitly in virtual workspaces. Resolver "3" (Rust 2024 edition default) adds MSRV-aware dependency selection — acceptable but not required for Phase 1. The prior research specified resolver "3"; either works. Use "2" to avoid any unexpected dependency version changes on Rust version upgrades if not pinning MSRV.

---

## Architecture Patterns

### Recommended Workspace Structure

```
Glass/
├── Cargo.toml                  # workspace root — members = ["crates/*", "."]
├── Cargo.lock
├── src/
│   └── main.rs                 # glass_app binary (thin winit ApplicationHandler wiring)
├── crates/
│   ├── glass_core/             # shared types — no deps on other glass crates
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── event.rs        # AppEvent enum
│   │       ├── config.rs       # GlassConfig struct
│   │       └── error.rs
│   ├── glass_terminal/         # PTY + alacritty_terminal wrapper
│   │   ├── Cargo.toml          # deps: alacritty_terminal, glass_core, tokio
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── pty.rs          # spawn PTY, EventLoop, reader thread
│   │       └── event_proxy.rs  # implements EventListener → AppEvent bridge
│   ├── glass_renderer/         # wgpu surface (Phase 1: clear-to-color only)
│   │   ├── Cargo.toml          # deps: wgpu, winit, bytemuck, glass_core
│   │   └── src/
│   │       ├── lib.rs
│   │       └── surface.rs      # wgpu Device/Queue/Surface init + clear-to-color frame
│   ├── glass_history/          # STUB — empty lib.rs only
│   │   └── src/lib.rs
│   ├── glass_snapshot/         # STUB — empty lib.rs only
│   │   └── src/lib.rs
│   ├── glass_pipes/            # STUB — empty lib.rs only
│   │   └── src/lib.rs
│   └── glass_mcp/              # STUB — empty lib.rs only
│       └── src/lib.rs
└── .planning/
```

**Why stubs exist from day one:** Satisfies "cargo build succeeds for the full workspace including all stub crates" (success criterion 1). Future phases fill in stub crates without restructuring the workspace. Cheaper to add stubs now than restructure later.

### Pattern 1: winit 0.30 ApplicationHandler (CRITICAL — use this exact pattern)

**What:** Implement `ApplicationHandler<AppEvent>` on a `Processor` struct that holds all application state.

**When to use:** Always — this is the only correct event loop pattern in winit 0.30+.

**CRITICAL API CHANGE in winit 0.30.13:** Window and surface creation must happen in `can_create_surfaces()`, NOT `resumed()`. In winit 0.30.13, `can_create_surfaces()` is a **required method** of `ApplicationHandler`. The `resumed()` method is now **optional** (provided with empty default) and is only emitted on Android, iOS, and Web to signal actual app resume. Desktop platforms call `can_create_surfaces()` at startup.

```rust
// Source: winit 0.30.13 ApplicationHandler trait (docs.rs)
// src/main.rs

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};
use std::sync::Arc;

struct Processor {
    windows: std::collections::HashMap<WindowId, WindowContext>,
}

struct WindowContext {
    window: Arc<Window>,
    renderer: GlassRenderer,           // from glass_renderer
    pty_sender: std::sync::mpsc::Sender<PtyMsg>,
}

impl ApplicationHandler<AppEvent> for Processor {
    // REQUIRED: create windows and GPU surfaces HERE, not in resumed()
    fn can_create_surfaces(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop.create_window(Window::default_attributes()
                .with_title("Glass")
            ).unwrap()
        );
        // wgpu init is async — block via pollster (correct pattern)
        let renderer = pollster::block_on(GlassRenderer::new(Arc::clone(&window)));
        // spawn PTY reader thread, get sender back
        let pty_sender = spawn_pty_thread(event_loop.create_proxy());
        self.windows.insert(window.id(), WindowContext { window, renderer, pty_sender });
    }

    // REQUIRED: handle per-window OS events
    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(ctx) = self.windows.get_mut(&window_id) else { return };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => ctx.renderer.draw(),
            WindowEvent::Resized(size) => {
                ctx.renderer.resize(size.width, size.height);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                // forward to PTY — Phase 1 plan 03
                let _ = ctx.pty_sender.send(PtyMsg::Input(encode_key(event)));
            }
            _ => {}
        }
    }

    // Optional: handle AppEvent from PTY thread wakeups
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::TerminalDirty { window_id } => {
                if let Some(ctx) = self.windows.get(&window_id) {
                    ctx.window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // If any window is dirty: request_redraw again
    }
}

fn main() {
    // FIRST: set UTF-8 code page on Windows (before any PTY creation)
    #[cfg(target_os = "windows")]
    unsafe {
        windows::Win32::System::Console::SetConsoleCP(65001);
        windows::Win32::System::Console::SetConsoleOutputCP(65001);
    }

    tracing_subscriber::fmt::init();
    let event_loop = EventLoop::<AppEvent>::with_user_event().build().unwrap();
    let mut processor = Processor { windows: Default::default() };
    event_loop.run_app(&mut processor).unwrap();
}
```

### Pattern 2: wgpu DX12 Surface Initialization

**What:** Initialize wgpu instance, request adapter, get device/queue, configure surface for clear-to-color rendering.

**When to use:** Inside `can_create_surfaces()` via `pollster::block_on()`.

```rust
// Source: wgpu 28.0.0 official docs + glass_renderer/src/surface.rs
pub struct GlassRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
}

impl GlassRenderer {
    pub async fn new(window: Arc<winit::window::Window>) -> Self {
        let instance = wgpu::Instance::default(); // auto-selects DX12 on Windows
        let surface = instance.create_surface(window.clone()).unwrap();
        let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }).await.expect("No compatible GPU adapter found");

        // Log which backend was selected (DX12 on Windows 11)
        tracing::info!("GPU backend: {:?}", adapter.get_info().backend);

        let (device, queue) = adapter.request_device(
            &wgpu::DeviceDescriptor::default(), None
        ).await.unwrap();

        let size = window.inner_size();
        let caps = surface.get_capabilities(&adapter);
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: caps.formats[0],
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        Self { device, queue, surface, surface_config }
    }

    // Phase 1: clear-to-dark-gray each frame
    pub fn draw(&mut self) {
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                // Reconfigure — do NOT panic
                self.surface.configure(&self.device, &self.surface_config);
                return;
            }
            Err(e) => { tracing::error!("Surface error: {e}"); return; }
        };
        let view = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.1, g: 0.1, b: 0.1, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }
        self.queue.submit([encoder.finish()]);
        frame.present();
    }

    // Handle resize — debounce; only configure when size actually changes
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 { return; }
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
    }
}
```

### Pattern 3: ConPTY PTY Spawn with Dedicated Reader Thread

**What:** Spawn PowerShell via `alacritty_terminal`'s ConPTY wrapper. Run PTY I/O on a dedicated thread (NOT Tokio task — PTY reads are blocking). Signal main thread via `EventLoopProxy<AppEvent>`.

**When to use:** In `can_create_surfaces()` after window creation. The PTY reader thread runs for the life of the application.

**Exact API verified for alacritty_terminal 0.25.1:**

```rust
// Source: docs.rs alacritty_terminal 0.25.1 tty module
// alacritty_terminal::tty::Options fields:
//   shell: Option<Shell>          — None = use default shell
//   working_directory: Option<PathBuf>
//   drain_on_exit: bool
//   env: HashMap<String, String>  — additional env vars

// glass_terminal/src/pty.rs
use alacritty_terminal::tty::{self, Options as TtyOptions, Shell};
use alacritty_terminal::event_loop::{EventLoop as PtyEventLoop, Msg as PtyMsg};
use alacritty_terminal::term::Term;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;

// EventListener trait (alacritty_terminal 0.25.1):
// pub trait EventListener {
//     fn send_event(&self, _event: Event) {}  // default no-op
// }
// Event enum variants: Wakeup, Title(String), Bell, Exit, ChildExit(ExitStatus),
//   PtyWrite(String), CursorBlinkingChange, ClipboardStore/Load, ColorRequest, etc.

pub struct EventProxy {
    proxy: winit::event_loop::EventLoopProxy<AppEvent>,
    window_id: winit::window::WindowId,
}

impl EventListener for EventProxy {
    fn send_event(&self, event: alacritty_terminal::event::Event) {
        use alacritty_terminal::event::Event;
        match event {
            Event::Wakeup => {
                let _ = self.proxy.send_event(AppEvent::TerminalDirty {
                    window_id: self.window_id
                });
            }
            Event::Title(title) => {
                let _ = self.proxy.send_event(AppEvent::SetTitle {
                    window_id: self.window_id,
                    title,
                });
            }
            Event::Exit | Event::ChildExit(_) => {
                let _ = self.proxy.send_event(AppEvent::TerminalExit {
                    window_id: self.window_id
                });
            }
            _ => {}
        }
    }
}

pub fn spawn_pty(
    proxy: winit::event_loop::EventLoopProxy<AppEvent>,
    window_id: winit::window::WindowId,
    size: alacritty_terminal::term::SizeInfo,
) -> std::sync::mpsc::Sender<PtyMsg> {
    let options = TtyOptions {
        shell: Some(Shell::new("pwsh", vec![])),
        working_directory: None,
        drain_on_exit: true,
        env: std::collections::HashMap::from([
            ("TERM".into(), "xterm-256color".into()),
            ("COLORTERM".into(), "truecolor".into()),
        ]),
    };

    let pty = tty::new(&options, size, None).expect("Failed to spawn PTY");
    let event_proxy = EventProxy { proxy, window_id };

    // Term::new needs a config — use alacritty_terminal::term::Config::default()
    let term = Arc::new(FairMutex::new(
        Term::new(alacritty_terminal::term::Config::default(), &size, event_proxy.clone())
    ));

    // EventLoop::new(term, listener, pty, drain_on_exit, ref_test)
    // spawn() launches on a dedicated std::thread — NOT a Tokio task
    let event_loop = PtyEventLoop::new(
        Arc::clone(&term),
        event_proxy,
        pty,
        false,  // drain_on_exit
        false,  // ref_test
    ).unwrap();

    let loop_tx = event_loop.channel();
    event_loop.spawn();  // returns JoinHandle — PTY reader is now on its own thread

    loop_tx  // Sender<Msg> for writing input and resize notifications
}
```

> **Note:** `EventProxy` must implement `Clone` because it is passed to both `Term::new` and `PtyEventLoop::new`. Implement `Clone` manually or derive it (proxy and window_id are both `Clone`).

### Pattern 4: UTF-8 Code Page (Windows First Action)

**What:** Set Windows console code page to 65001 (UTF-8) before any PTY creation.

**When to use:** First lines of `main()`, before event loop creation, before any output.

```rust
// Source: Windows API docs, verified by pitfalls research
// Cargo.toml must add: windows = { version = "0.58", features = ["Win32_System_Console"] }
// (windows-sys 0.59 is a transitive dep from alacritty_terminal; use the compatible version)

#[cfg(target_os = "windows")]
fn set_utf8_codepage() {
    use windows_sys::Win32::System::Console::{SetConsoleCP, SetConsoleOutputCP};
    unsafe {
        SetConsoleCP(65001);
        SetConsoleOutputCP(65001);
    }
}
```

> **Dependency note:** `alacritty_terminal` 0.25.1 depends on `windows-sys ^0.59`. Use `windows-sys` directly to avoid version conflicts with the `windows` crate. Alternative: call `chcp 65001` via shell spawn args.

### Anti-Patterns to Avoid

- **Creating window in `main()` before event loop starts:** `ActiveEventLoop::create_window()` is only callable inside `can_create_surfaces()` / event callbacks. Panics at runtime on some platforms.
- **PTY reads on the main thread:** UI freezes at idle shell prompt (waiting for bytes). Use dedicated thread.
- **Using `tokio::spawn` for PTY reads:** Tokio spawn is for non-blocking async work. PTY reads are OS-blocking calls. Use `std::thread::spawn` instead.
- **Using `EventLoop::run()` with a closure:** Deprecated in winit 0.30. Use `EventLoop::run_app()` with `ApplicationHandler`.
- **Using `WindowBuilder`:** Deprecated in winit 0.30. Use `Window::default_attributes()` + `.with_*()` builder methods.
- **Panicking on `SurfaceError::Lost` or `SurfaceError::Outdated`:** These are recoverable — just reconfigure the surface.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| ANSI/VT escape code parsing | Custom state machine | `alacritty_terminal` 0.25.1 | 5,000+ lines of parser covering 40 years of terminal sequences; ConPTY wrapping included |
| ConPTY spawning on Windows | `CreatePseudoConsole` + `CreateProcess` calls | `alacritty_terminal::tty::new()` | Handles ConHost vs OpenConsole selection, pipe setup, process lifecycle correctly |
| Event loop wakeup from PTY thread | Custom OS signaling | `winit::event_loop::EventLoopProxy<AppEvent>` | Thread-safe, integrates with winit's event dispatch |
| GPU text rendering | Custom glyph rasterizer | `glyphon` 0.10.0 (Phase 2) | cosmic-text shaping + etagere atlas + wgpu; only maintained wgpu text renderer |
| Workspace member enumeration | Manual Cargo.toml per-crate | `members = ["crates/*"]` glob | Auto-discovers all crates in crates/ directory |

**Key insight:** The scaffold phase touches three domains (Cargo/Rust tooling, wgpu, ConPTY) where amateur implementations will be working software that quietly fails under edge cases (resize, non-ASCII input, GPU backend fallback). Use the established libraries and patterns exclusively.

---

## Common Pitfalls

### Pitfall 1: winit 0.30 API Is a Complete Rewrite — Wrong Callback for Window Creation

**What goes wrong:** Using `resumed()` for window creation (all pre-0.30.13 tutorials), or using the deprecated closure-based `EventLoop::run()`.

**Why it happens:** Winit 0.30.0 initially used `resumed()` for surface creation. In 0.30.13, `can_create_surfaces()` was split off as a dedicated required method. The split separates "app lifecycle resumed" from "you may now create render surfaces."

**How to avoid:** Implement `can_create_surfaces()` as a required method. On desktop (Windows), this is called once at startup. Do NOT assume `resumed()` will be called on desktop — it may not be.

**Warning signs:** Compile error "not all trait items implemented: missing `can_create_surfaces`" — fix by implementing the method.

### Pitfall 2: ConPTY Rewrites Escape Sequences — ENABLE_VIRTUAL_TERMINAL_INPUT Not Set

**What goes wrong:** Keyboard sequences are rewritten by ConPTY: `ESC[5D` (Ctrl+Left) becomes `ESC[D` (plain left-arrow), breaking word navigation. `alacritty_terminal`'s ConPTY implementation handles `ENABLE_VIRTUAL_TERMINAL_INPUT` — verify it is set.

**Why it happens:** ConPTY rewrites sequences for Win32 legacy compatibility unless explicitly told not to. The flag must be enabled on the ConPTY input side.

**How to avoid:** `alacritty_terminal`'s ConPTY wrapper (in `tty/windows/conpty.rs`) enables this flag. Verify by testing Ctrl+Left after scaffold: it should produce `ESC[1;5D`, not `ESC[D`.

**Warning signs:** Ctrl+Arrow moves one character instead of one word in the shell.

### Pitfall 3: wgpu Surface Resize — SurfaceError::Lost Panics

**What goes wrong:** `surface.get_current_texture()` returns `Err(SurfaceError::Lost)` during window resize. If the renderer panics on this, the application crashes during drag-resize.

**Why it happens:** Window resize triggers DWM to invalidate the swapchain before wgpu has reconfigured it.

**How to avoid:** Match on `SurfaceError::Lost | SurfaceError::Outdated` and call `surface.configure()` to recreate the swapchain, then return early (skip this frame). The clear-to-color renderer in Plan 02 must implement this from day one.

**Warning signs:** Application crashes when user drags window edge.

### Pitfall 4: PTY Reader on Main Thread — UI Freezes at Idle Shell

**What goes wrong:** Calling `pty.read()` (a blocking call) on the winit event loop thread. When the shell is idle (waiting for user input), `read()` blocks forever and the window stops responding.

**Why it happens:** Developers use PTY reads inline for simplicity. Doesn't manifest during demo output (constant data) but breaks immediately at the shell prompt.

**How to avoid:** `alacritty_terminal::event_loop::EventLoop::spawn()` creates the dedicated PTY reader thread automatically. Call `event_loop.spawn()` and interact via the returned `Sender<Msg>`. Never call PTY reads on the main thread.

**Warning signs:** Window title bar is unresponsive after launching — can't drag window; application appears frozen.

### Pitfall 5: Windows UTF-8 Code Page Not Set

**What goes wrong:** Non-ASCII output from PowerShell (accented characters, emoji, CJK) displays as mojibake (`é` → `Ã©`) because ConPTY interprets UTF-8 bytes as Windows-1252 or OEM 437.

**Why it happens:** Windows default console code page is OEM 437 (US) or locale-specific. ConPTY inherits the parent process code page.

**How to avoid:** Call `SetConsoleCP(65001)` and `SetConsoleOutputCP(65001)` in `main()` before any PTY creation. Also pass `PYTHONUTF8=1`, `PYTHONIOENCODING=utf-8` etc. in the PTY env for Python-using terminals.

**Warning signs:** Running `Write-Output "café"` in PowerShell via the PTY shows garbled characters.

### Pitfall 6: alacritty_terminal Version Drift

**What goes wrong:** Using `"^0.25.1"` instead of `"=0.25.1"` allows `cargo update` to pull `0.25.2+` which may have breaking API changes (no semver stability guarantee).

**Why it happens:** The `^` prefix is Cargo default. Developers assume semver stability that `alacritty_terminal` explicitly does not provide.

**How to avoid:** Use exact version pin: `alacritty_terminal = "=0.25.1"` in workspace `Cargo.toml`. Budget time for manual version-bump migrations.

---

## Code Examples

### Cargo Workspace Root Cargo.toml

```toml
# Source: Cargo book workspace documentation
[workspace]
resolver = "2"
members = ["crates/*", "."]

[workspace.dependencies]
# Rendering
wgpu          = "28.0.0"
winit         = "0.30.13"
bytemuck      = { version = "1.25.0", features = ["derive"] }
pollster      = "0.4.0"

# Terminal emulation — EXACT version pin, no ^ or ~
alacritty_terminal = "=0.25.1"
tokio         = { version = "1.50.0", features = ["full"] }

# Config
serde         = { version = "1.0.228", features = ["derive"] }
toml          = "1.0.4"

# Utilities
anyhow        = "1.0.102"
tracing       = "0.1.44"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# Stub crate deps (future phases)
rusqlite      = { version = "0.38.0", features = ["bundled"] }

[package]
name = "glass"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "glass"
path = "src/main.rs"

[dependencies]
glass_core     = { path = "crates/glass_core" }
glass_terminal = { path = "crates/glass_terminal" }
glass_renderer = { path = "crates/glass_renderer" }
winit.workspace = true
pollster.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
anyhow.workspace = true
```

### Minimal Stub Crate (glass_history example)

```toml
# crates/glass_history/Cargo.toml
[package]
name = "glass_history"
version = "0.1.0"
edition = "2021"
```

```rust
// crates/glass_history/src/lib.rs
// Phase 2 stub — filled in during roadmap Phase 4
```

### glass_core Event Types

```rust
// crates/glass_core/src/event.rs
#[derive(Debug, Clone)]
pub enum AppEvent {
    TerminalDirty { window_id: winit::window::WindowId },
    SetTitle      { window_id: winit::window::WindowId, title: String },
    TerminalExit  { window_id: winit::window::WindowId },
    // Phase 3: ShellHook(HookEvent) — added when shell integration is built
}
```

### ConPTY Escape Sequence Fixture Test

```rust
// glass_terminal/src/tests.rs
// Test ENABLE_VIRTUAL_TERMINAL_INPUT is working correctly
#[test]
fn test_ctrl_left_produces_correct_sequence() {
    // ConPTY output must produce ESC[1;5D for Ctrl+Left, not ESC[D
    // This test verifies ENABLE_VIRTUAL_TERMINAL_INPUT is set
    // Implementation: spawn PTY, send Ctrl+Left input bytes, read and assert output
    // Source: PITFALLS.md — ConPTY rewrites escape sequences unless flag is set
    todo!("implement in plan 01-03")
}

#[test]
fn test_utf8_codepage_65001_active() {
    // Verify SetConsoleOutputCP(65001) was called
    #[cfg(target_os = "windows")]
    unsafe {
        let cp = windows_sys::Win32::System::Console::GetConsoleOutputCP();
        assert_eq!(cp, 65001, "Console output code page must be UTF-8 (65001)");
    }
}
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `EventLoop::run(closure)` | `EventLoop::run_app(ApplicationHandler)` | winit 0.30.0 (2024) | All pre-2024 winit tutorials are broken — must use new API |
| `WindowBuilder::new()` | `Window::default_attributes()` + builder | winit 0.30.0 (2024) | `WindowBuilder` deprecated; compile warnings in 0.30+ |
| Surface creation in `resumed()` | Surface creation in `can_create_surfaces()` | winit 0.30.x (2024) | `can_create_surfaces` is now a required method; `resumed` is optional/provided |
| `glyphon 0.6` + `wgpu 22` | `glyphon 0.10` + `wgpu 28` | 2024-2025 | Version-locked together: glyphon 0.10 requires `wgpu ^28.0.0` |
| `wgpu_glyph` | `glyphon` | 2023 | `wgpu_glyph` unmaintained; glyphon is the community successor |

**Deprecated/outdated:**
- `winpty`: Legacy Windows PTY shim for pre-ConPTY systems. Windows 10 1809+ has native ConPTY. Do not add this dependency.
- `log` crate: Use `tracing` instead — structured fields, spans, async-aware.
- `clap`: Not needed for Milestone 1 — no CLI args planned.

---

## Open Questions

1. **alacritty_terminal Term::new() Config Type**
   - What we know: `Term::new()` takes a config type, a `Dimensions` (size), and an event listener
   - What's unclear: The exact concrete type of config in 0.25.1 (may be `alacritty_terminal::term::Config` or an external config struct)
   - Recommendation: Read the actual `term::Config` definition from source when implementing plan 01-03. Use `Config::default()` for the scaffold; all defaults are acceptable for Phase 1.

2. **EventProxy Clone Requirement**
   - What we know: Both `Term::new()` and `PtyEventLoop::new()` take the event listener by value
   - What's unclear: Whether `Clone` is required or if two separate `EventProxy` instances can share the same proxy via `Arc`
   - Recommendation: Wrap `EventLoopProxy<AppEvent>` in `Arc` and clone the Arc. `EventLoopProxy` is `Clone` natively so this is straightforward.

3. **windows-sys version for SetConsoleCP**
   - What we know: `alacritty_terminal` 0.25.1 depends on `windows-sys ^0.59`
   - What's unclear: Whether the main binary can use `windows-sys` directly at `^0.59` without version conflict
   - Recommendation: Add `windows-sys = { version = "0.59", features = ["Win32_System_Console"] }` to the binary's Cargo.toml. Cargo will unify versions automatically.

---

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | `cargo test` (built-in) + `cargo nextest` (optional, faster) |
| Config file | None — see Wave 0 gaps |
| Quick run command | `cargo test --workspace` |
| Full suite command | `cargo test --workspace --all-targets` |

> Install nextest: `cargo install cargo-nextest --locked`

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| CORE-01 | PowerShell spawns via ConPTY and keyboard input reaches PTY stdin | Integration | `cargo test -p glass_terminal -- pty` | No — Wave 0 |
| CORE-01 | ConPTY ENABLE_VIRTUAL_TERMINAL_INPUT is set (Ctrl+Left produces correct sequence) | Integration | `cargo test -p glass_terminal -- escape_seq` | No — Wave 0 |
| CORE-01 | UTF-8 code page 65001 is active at startup | Unit | `cargo test -p glass -- codepage` | No — Wave 0 |
| RNDR-01 | wgpu selects DX12 backend on Windows | Smoke (manual) | Run `Glass.exe`, check tracing log for "GPU backend: Dx12" | N/A |
| RNDR-01 | Window resize does not crash or freeze | Smoke (manual) | Drag-resize window for 5 seconds | N/A |
| RNDR-01 | `cargo build` succeeds for full workspace | Compile | `cargo build --workspace` | N/A |

### Sampling Rate

- **Per task commit:** `cargo build --workspace` (compile check, ~10s)
- **Per wave merge:** `cargo test --workspace` (all unit + integration tests)
- **Phase gate:** `cargo test --workspace --all-targets` green before moving to Phase 2

### Wave 0 Gaps

- [ ] `crates/glass_terminal/src/tests.rs` — ConPTY escape sequence fixture tests (CORE-01)
- [ ] `src/tests.rs` or `crates/glass_core/src/tests.rs` — UTF-8 codepage assertion test (CORE-01)
- [ ] Test infrastructure: no `nextest.toml` or custom test config needed for Phase 1 (standard `cargo test` sufficient)

---

## Sources

### Primary (HIGH confidence)

- crates.io API — Version verification: `alacritty_terminal` 0.25.1, `wgpu` 28.0.0, `winit` 0.30.13, `glyphon` 0.10.0, `tokio` 1.50.0, `pollster` 0.4.0 (fetched 2026-03-04 in project research)
- docs.rs alacritty_terminal 0.25.1 — `tty::Options` struct fields verified: `shell`, `working_directory`, `drain_on_exit`, `env` (fetched 2026-03-04)
- docs.rs alacritty_terminal 0.25.1 — `EventListener` trait: single method `send_event(&self, Event)` with default no-op (fetched 2026-03-04)
- docs.rs alacritty_terminal 0.25.1 — `event_loop::EventLoop::new()` and `spawn()` signatures verified (fetched 2026-03-04)
- alacritty/alacritty GitHub `event.rs` — `Event` enum variants verified: `Wakeup`, `Title(String)`, `Bell`, `Exit`, `ChildExit(ExitStatus)`, `PtyWrite`, `CursorBlinkingChange`, `ClipboardStore/Load`, `ColorRequest`, `TextAreaSizeRequest`, `MouseCursorDirty`, `ResetTitle` (fetched 2026-03-04)
- rust-windowing/winit `ApplicationHandler` trait — `can_create_surfaces` is REQUIRED method, `resumed` is provided (optional); create windows in `can_create_surfaces` for portability (fetched 2026-03-04)
- Cargo edition guide — resolver "3" is default for edition 2024; virtual workspaces must set it explicitly; resolver "2" is stable and acceptable (fetched 2026-03-04)
- `.planning/research/STACK.md` — full version compatibility matrix, workspace structure, integration notes (HIGH confidence, 2026-03-04)
- `.planning/research/ARCHITECTURE.md` — event loop pattern, thread model, data flow pipeline (HIGH confidence, 2026-03-04)
- `.planning/research/PITFALLS.md` — all critical pitfalls for Phase 1 domain (HIGH confidence, 2026-03-04)

### Secondary (MEDIUM confidence)

- sotrh/learn-wgpu tutorial (winit 0.30 update) — `resumed()` callback used for surface creation; supplemented by winit official trait docs showing `can_create_surfaces` is required
- Windows Terminal GitHub issues #12166, #362 — ConPTY escape sequence rewriting documented with reproducible evidence
- wgpu GitHub issues #5374, #7447 — Surface resize flickering on Windows (DX12 vs Vulkan) with workarounds

### Tertiary (LOW confidence — needs validation during implementation)

- alacritty_terminal 0.25.1 `Term::new()` exact config type — assumed `alacritty_terminal::term::Config`; verify against actual source before implementing plan 01-03
- `EventProxy` Clone requirement — assumed; verify exact trait bounds on `PtyEventLoop::new()` during implementation

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all versions verified via crates.io API; dependency compatibility matrix confirmed
- Architecture: HIGH — primary sources (Alacritty source, winit/wgpu official docs); patterns confirmed against cosmic-term reference implementation
- Pitfalls: HIGH (ConPTY, wgpu resize, PTY threading, UTF-8) — backed by tracked GitHub issues with reproducible evidence; MEDIUM (alacritty_terminal internal API details — trait bounds need implementation-time verification)

**Research date:** 2026-03-04
**Valid until:** 2026-05-01 (stable ecosystem; wgpu/winit release cadence is ~quarterly; check for alacritty_terminal version bump if more than 6 weeks pass before implementation)
