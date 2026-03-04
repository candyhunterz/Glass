# Architecture Research

**Domain:** GPU-accelerated Rust terminal emulator (Glass, forking alacritty_terminal)
**Researched:** 2026-03-04
**Confidence:** HIGH (primary sources: Alacritty source, winit/wgpu official docs, Microsoft shell integration docs)

---

## Standard Architecture

### System Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Application Layer                            │
│  glass_app (binary)                                                 │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────────┐  │
│  │  Event Loop  │  │  WindowCtx   │  │   Config / CLI           │  │
│  │  (winit)     │  │  (per-window)│  │   (TOML)                 │  │
│  └──────┬───────┘  └──────┬───────┘  └──────────────────────────┘  │
├─────────┼─────────────────┼────────────────────────────────────────┤
│         │    Core Service Layer                                     │
│  ┌──────▼───────────────┐ │ ┌──────────────┐  ┌─────────────────┐  │
│  │  glass_terminal      │ │ │glass_renderer│  │  glass_core     │  │
│  │  (PTY + VTE + Term)  │ │ │ (wgpu + GPU) │  │  (events, cfg)  │  │
│  └──────────────────────┘ │ └──────────────┘  └─────────────────┘  │
│                           │                                         │
├───────────────────────────┼─────────────────────────────────────────┤
│                  Embedded External Crates                           │
│  ┌──────────────────┐  ┌──────────────┐  ┌────────────────────────┐│
│  │  alacritty_      │  │    winit     │  │  wgpu                  ││
│  │  terminal        │  │   (0.30+)    │  │  (DX12/Vulkan/GL)      ││
│  │  (grid, VTE,     │  │              │  │                        ││
│  │   ConPTY)        │  │              │  │                        ││
│  └──────────────────┘  └──────────────┘  └────────────────────────┘│
└─────────────────────────────────────────────────────────────────────┘
```

### Component Responsibilities

| Component | Responsibility | Communicates With |
|-----------|----------------|-------------------|
| `glass_core` | Event types, shared config structs, error types | All crates |
| `glass_terminal` | PTY spawn, VTE → Grid, shell integration OSC parsing | `glass_core`, `alacritty_terminal` |
| `glass_renderer` | wgpu surface, glyph atlas, frame draw from grid snapshot | `glass_core`, winit window |
| `glass_app` (binary) | winit event loop, wires terminal to renderer, input routing | All crates |
| `alacritty_terminal` | Term<T> grid, VTE parsing, ConPTY on Windows, scrollback | (external dep) |
| `glass_history` (stub) | Structured command history DB | `glass_core`, `glass_terminal` |
| `glass_snapshot` (stub) | Filesystem snapshot at command boundaries | `glass_core` |
| `glass_pipes` (stub) | Pipe stage visualization | `glass_core`, `glass_terminal` |
| `glass_mcp` (stub) | MCP server exposing history/snapshot data | `glass_core`, `glass_history` |

---

## Recommended Cargo Workspace Structure

```
Glass/
├── Cargo.toml                  # workspace members = [...]
├── Cargo.lock
├── .planning/
├── crates/
│   ├── glass_core/             # shared types, no deps on other glass crates
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── event.rs        # AppEvent enum (TerminalOutput, Resize, ShellHook, ...)
│   │       ├── config.rs       # GlassConfig (font, font_size, shell override)
│   │       └── error.rs
│   │
│   ├── glass_terminal/         # PTY management + VTE + shell integration
│   │   ├── Cargo.toml          # deps: alacritty_terminal, glass_core
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── pty.rs          # spawn PTY, read loop, write channel
│   │       ├── grid_snapshot.rs# copy Term grid for renderer (lock-free handoff)
│   │       ├── shell_hook.rs   # OSC 133 / OSC 7 parsing, block boundary logic
│   │       └── event_proxy.rs  # implements alacritty_terminal::EventListener
│   │
│   ├── glass_renderer/         # wgpu rendering pipeline
│   │   ├── Cargo.toml          # deps: wgpu, winit, glyphon, glass_core
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── surface.rs      # wgpu Device/Queue/Surface init
│   │       ├── glyph_cache.rs  # TextAtlas + TextRenderer (glyphon)
│   │       ├── grid_renderer.rs# walk GridSnapshot, batch glyph draws
│   │       ├── rect_renderer.rs# wgpu pipeline for colored quads (bg, cursor, blocks)
│   │       └── frame.rs        # orchestrate full frame: clear → cells → rects → present
│   │
│   ├── glass_history/          # stub — Phase 2
│   │   └── src/lib.rs
│   ├── glass_snapshot/         # stub — Phase 3
│   │   └── src/lib.rs
│   ├── glass_pipes/            # stub — Phase 4
│   │   └── src/lib.rs
│   └── glass_mcp/              # stub — Phase 2
│       └── src/lib.rs
│
└── src/                        # glass_app binary (thin wiring layer)
    └── main.rs                 # winit event loop, Processor impl, window creation
```

### Structure Rationale

- **`glass_core` as leaf:** No circular deps. All crates can import it. Config and event types defined once.
- **`glass_terminal` wraps `alacritty_terminal`:** Keeps the external API surface isolated. Glass-specific concerns (shell hooks, block detection) live here, not scattered in the binary.
- **`glass_renderer` is pure wgpu:** No terminal knowledge — takes a `GridSnapshot` struct, not a live `Term<T>`. This decouples rendering from terminal state locking.
- **Stub crates exist immediately:** Satisfies workspace structure requirement; compile to empty `lib.rs`. Future milestones fill them in without restructuring.
- **Binary crate is thin:** Just wires crates together. Heavy logic belongs in library crates so it can be tested independently.

---

## Event Loop Architecture

### winit 0.30 ApplicationHandler Pattern

winit 0.30 replaced the closure-based API with the `ApplicationHandler<T>` trait. Glass implements this on its `Processor` struct.

```rust
// glass_app: main.rs / event.rs
struct Processor {
    windows: HashMap<WindowId, WindowContext>,
    config: Arc<GlassConfig>,
    scheduler: Scheduler,
}

impl ApplicationHandler<AppEvent> for Processor {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Create window + wgpu surface + PTY here (after event loop starts)
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop,
                    window_id: WindowId, event: WindowEvent) {
        // Route keyboard → PTY, resize → PTY + renderer, close → cleanup
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        // PTY wakeup → mark dirty → request_redraw()
        // ShellHook event → update block state
        // ConfigReload → propagate to renderer
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // If any window is dirty: request_redraw() again
        // Set control flow Wait vs WaitUntil for cursor blink timer
    }
}
```

Key lifecycle points:
- `resumed()` fires when the event loop is ready. Create the window here, not before.
- `user_event()` is the primary mechanism for waking the event loop from PTY thread.
- `about_to_wait()` is used for cursor blink scheduling and coalescing redraws.
- `EventLoopProxy<AppEvent>` is the thread-safe handle given to the PTY thread so it can send wakeups.

### WindowContext: Per-Window State Bundle

```rust
struct WindowContext {
    // Rendering
    window: Arc<winit::window::Window>,
    renderer: GlassRenderer,           // wgpu surface + pipeline
    // Terminal state
    terminal: Arc<Mutex<Term<EventProxy>>>,
    pty_sender: Sender<PtyMsg>,        // write/resize channel to PTY thread
    // Block UI state
    block_manager: BlockManager,       // tracks command boundaries from OSC 133
    dirty: bool,                       // needs redraw this frame
}
```

### Thread Model

```
Main Thread (winit event loop)
│
├── PTY I/O Thread (per terminal)
│   ├── Reads ConPTY output bytes in a loop
│   ├── Locks Term<EventProxy>, feeds vte::Parser
│   ├── Parser calls Term::handler methods → updates Grid
│   ├── Unlocks Term
│   └── Sends AppEvent::TerminalDirty via EventLoopProxy
│
└── (future) MCP Server Thread
    └── Reads from glass_history DB, serves MCP requests
```

There is no separate render thread in Milestone 1. Rendering happens on the main thread in `window_event(WindowEvent::RedrawRequested)`. This matches Alacritty's architecture and is correct for wgpu on Windows.

---

## Data Flow: PTY Output to GPU Frame

This is the critical pipeline to get right. Each stage has a clear owner.

```
Shell Process (PowerShell / bash)
    │ writes bytes to ConPTY pipe
    ▼
PTY Read Loop (glass_terminal, dedicated thread)
    │ reads raw bytes
    ▼
vte::Parser (inside alacritty_terminal)
    │ state machine parses VT100/ANSI escape sequences
    │ calls Handler trait methods on Term<EventProxy>
    ▼
Term<EventProxy>.Grid (2D array of Cell structs)
    │ Grid[row][col] = Cell { char, fg_color, bg_color, flags }
    │ OSC sequences → EventProxy::send_event() called
    ▼
EventProxy::send_event(AppEvent::TerminalDirty)
    │ sends via EventLoopProxy (thread-safe)
    ▼
winit Main Event Loop wakes up
    │ ApplicationHandler::user_event() fires
    │ sets window_context.dirty = true
    │ calls window.request_redraw()
    ▼
WindowEvent::RedrawRequested fires
    │ lock Term briefly → copy GridSnapshot (unlocks immediately)
    ▼
GridSnapshot (owned, lock-free copy of grid state)
    │ Vec<RenderedCell> with position, char, colors, cursor flag
    ▼
glass_renderer::frame::draw()
    │
    ├── rect_renderer: background quads per cell (wgpu draw call)
    │
    ├── glyph_cache: lookup char in TextAtlas
    │   │ if miss: rasterize via cosmic-text → pack into texture atlas (glyphon)
    │   └── if hit: return atlas UV coordinates
    │
    ├── TextRenderer::render() — batch all glyph instances
    │   └── GPU: sample atlas texture → blit to framebuffer
    │
    ├── rect_renderer: cursor rect, block borders
    │
    └── surface.present() — swap buffers → display on screen
```

### OSC Sequence Side-Channel (Shell Integration)

OSC sequences arrive in the same PTY byte stream but route differently:

```
PTY bytes containing "ESC ] 133 ; A ST" (prompt start)
    │
    ▼
vte::Parser identifies OSC sequence
    │ calls Handler::osc_dispatch(&[b"133", b"A"])
    ▼
EventProxy::send_event(AppEvent::ShellHook(HookEvent::PromptStart))
    │
    ▼
Main Thread: user_event() receives ShellHook
    │
    ▼
BlockManager.on_hook_event(HookEvent::PromptStart)
    │ closes previous command block (records end time, exit code)
    └── opens new prompt boundary in scrollback index
```

OSC 7 (CWD) flows similarly:
```
"ESC ] 7 ; file:///C:/Users/... ST"
    │
    ▼
Handler::osc_dispatch(&[b"7", b"file:///C:/Users/..."])
    │
    ▼
AppEvent::ShellHook(HookEvent::CwdChanged(path))
    │
    ▼
WindowContext.cwd = path  →  status bar redraws on next frame
```

---

## Rendering Pipeline (VTE Grid → GPU Frame)

### wgpu Initialization Sequence

```rust
// glass_renderer/src/surface.rs
let instance = wgpu::Instance::default();  // auto-selects DX12 on Windows
let surface = instance.create_surface(&window);
let adapter = instance.request_adapter(&RequestAdapterOptions {
    power_preference: PowerPreference::HighPerformance,
    compatible_surface: Some(&surface),
    ..
}).await;
let (device, queue) = adapter.request_device(&DeviceDescriptor::default(), None).await;
```

wgpu auto-selects DX12 on Windows 11. No manual backend selection needed.

### Text Rendering via glyphon

glyphon is the recommended wgpu text renderer (2024-2025). It wraps cosmic-text (font shaping) + etagere (atlas packing) + wgpu (GPU sampling).

```rust
// glass_renderer/src/glyph_cache.rs
let mut font_system = FontSystem::new();
let mut text_atlas = TextAtlas::new(&device, &queue, &swapchain_format);
let mut text_renderer = TextRenderer::new(&mut text_atlas, &device, MultisampleState::default(), None);

// Per-frame: prepare text buffer from GridSnapshot
let mut buffer = Buffer::new(&mut font_system, Metrics::new(font_size, line_height));
for cell in &grid_snapshot.cells {
    // add each character as a text run with color attrs
    buffer.set_text(&mut font_system, &cell.ch.to_string(), Attrs::new().color(cell.fg));
}
text_renderer.prepare(&device, &queue, &mut font_system, &mut text_atlas, resolution, &[TextArea { ... }]);

// In render pass:
text_renderer.render(&text_atlas, &mut render_pass);
```

For monospace terminals specifically: since all cells are fixed-width, the grid layout is trivial (col * cell_width, row * cell_height). No shaping needed beyond glyph lookup. A simplified atlas that caches by (codepoint, style) suffices.

### Rect Renderer (background fills, cursor, block UI)

A separate wgpu pipeline using a simple quad vertex shader. Draws:
- Per-cell background color rectangles
- Cursor block/underline/bar
- Block separator lines (command boundary UI)
- Status bar background

This is two draw calls: one for cell backgrounds, one for decorations. Instanced rendering sends all rects as a vertex buffer in one call.

---

## Shell Integration Architecture

### OSC 133 Sequence Set (FTCS — FinalTerm Command Semantics)

| Sequence | Name | Meaning | When Emitted |
|----------|------|---------|--------------|
| `ESC ] 133 ; A ST` | FTCS_PROMPT | Prompt start | Before drawing the prompt |
| `ESC ] 133 ; B ST` | FTCS_COMMAND_START | Input start | After prompt, before user types |
| `ESC ] 133 ; C ST` | FTCS_COMMAND_EXECUTED | Output start | After Enter, before command output |
| `ESC ] 133 ; D ; <exit_code> ST` | FTCS_COMMAND_FINISHED | Output end | In next prompt, before A |
| `ESC ] 7 ; file://host/path ST` | OSC7 | CWD update | Every prompt |
| `ESC ] 9 ; 9 ; "<path>" ST` | OSC9.9 | CWD (Windows Terminal style) | Every prompt (PowerShell variant) |

### PowerShell Hook Implementation

Glass injects this into the user's PowerShell profile (or into a session via `-NoExit -Command`):

```powershell
# Glass shell integration for PowerShell
$Global:__GlassLastHistoryId = -1

function Global:__Glass-GetExitCode {
    if ($? -eq $True) { return 0 }
    $lastEntry = $(Get-History -Count 1)
    $isPsError = $Error[0].InvocationInfo.HistoryId -eq $lastEntry.Id
    if ($isPsError) { return -1 }
    return $LastExitCode
}

function prompt {
    $gle = $(__Glass-GetExitCode)
    $lastEntry = $(Get-History -Count 1)

    # Emit D (command finished) with exit code from PREVIOUS command
    if ($Global:__GlassLastHistoryId -ne -1) {
        if ($lastEntry.Id -eq $Global:__GlassLastHistoryId) {
            # No new history = Ctrl+C or empty enter, no exit code
            Write-Host -NoNewline "`e]133;D`a"
        } else {
            Write-Host -NoNewline "`e]133;D;$gle`a"
        }
    }

    $loc = $($executionContext.SessionState.Path.CurrentLocation)

    # A: prompt start
    Write-Host -NoNewline "`e]133;A`a"
    # OSC 9;9: CWD (Windows Terminal compatible format)
    Write-Host -NoNewline "`e]9;9;`"$loc`"`a"

    # (actual prompt text here)
    Write-Host -NoNewline "PS $loc> "

    # B: command input start (end of prompt)
    Write-Host -NoNewline "`e]133;B`a"

    $Global:__GlassLastHistoryId = $lastEntry.Id
}
```

Note: `133;C` (command executed) fires naturally when the user presses Enter — it must be emitted via `$env:PROMPT` or a `PSReadLine` key handler, not from the prompt function. For Milestone 1, A/B/D + OSC7 is sufficient for block boundary detection.

### Bash Hook Implementation

```bash
# Glass shell integration for bash (~/.bashrc)
__GLASS_LAST_EXIT=0

function __glass_preexec() {
    printf '\e]133;C\e\\'   # FTCS_COMMAND_EXECUTED: output starting
}

function __glass_precmd() {
    local exit=$?
    printf '\e]133;D;%s\e\\' "$exit"   # FTCS_COMMAND_FINISHED with exit code
    printf '\e]133;A\e\\'               # FTCS_PROMPT: prompt starting
    printf '\e]7;file://%s%s\e\\' "$(hostname)" "$(pwd)"  # OSC7 CWD
    __GLASS_LAST_EXIT=$exit
}

function __glass_postprompt() {
    printf '\e]133;B\e\\'   # FTCS_COMMAND_START: input area starts
}

# Hook via PROMPT_COMMAND and PS0
PROMPT_COMMAND="__glass_precmd; $PROMPT_COMMAND"
PS0='$(__glass_preexec)'
PROMPT_DIRTRIM=0
trap '__glass_postprompt' DEBUG   # or append to PS1
```

### BlockManager: Command Boundary State Machine

```rust
// glass_terminal/src/shell_hook.rs
pub enum HookEvent {
    PromptStart,                          // OSC 133;A
    CommandStart,                         // OSC 133;B
    CommandExecuted,                      // OSC 133;C
    CommandFinished { exit_code: i32 },   // OSC 133;D
    CwdChanged(PathBuf),                  // OSC 7 / OSC 9;9
}

pub struct CommandBlock {
    pub start_row: usize,        // grid row where command output starts
    pub end_row: Option<usize>,  // grid row where output ends (None = ongoing)
    pub exit_code: Option<i32>,
    pub start_time: Instant,
    pub duration: Option<Duration>,
    pub cwd: PathBuf,
    pub command_text: Option<String>,  // captured from 133;B..133;C range
}

pub struct BlockManager {
    blocks: Vec<CommandBlock>,
    current_cwd: PathBuf,
    state: BlockState,  // enum: Prompt | InputPhase | OutputPhase
}

impl BlockManager {
    pub fn on_hook(&mut self, event: HookEvent, current_row: usize) {
        match event {
            HookEvent::PromptStart => self.begin_prompt(current_row),
            HookEvent::CommandStart => self.begin_input(),
            HookEvent::CommandExecuted => self.begin_output(current_row),
            HookEvent::CommandFinished { exit_code } => self.close_block(exit_code),
            HookEvent::CwdChanged(path) => self.current_cwd = path,
        }
    }
}
```

---

## Architectural Patterns

### Pattern 1: Lock-Minimizing Grid Snapshot

The `Term<EventProxy>` is protected by a `Mutex`. The renderer must not hold this lock during GPU operations (which can take multiple milliseconds).

**What:** Lock briefly → copy minimal rendering data → unlock → render without lock.

**When to use:** Every frame. Mandatory for preventing PTY stalls during rendering.

```rust
// glass_app: in RedrawRequested handler
let snapshot = {
    let term = window_ctx.terminal.lock();
    GridSnapshot::from_term(&term)  // copy grid cells, cursor, colors
    // lock drops here
};
// renderer holds snapshot, not the lock
window_ctx.renderer.draw(&snapshot);
```

### Pattern 2: EventProxy Wakeup (Cross-Thread Signaling)

PTY thread cannot call winit directly. Uses `EventLoopProxy<AppEvent>` cloned at thread spawn.

**What:** PTY thread signals dirty via event loop proxy; main thread handles redraw.

```rust
// glass_terminal/src/event_proxy.rs
pub struct EventProxy(EventLoopProxy<AppEvent>);

impl alacritty_terminal::event::EventListener for EventProxy {
    fn send_event(&self, event: TerminalEvent) {
        match event {
            TerminalEvent::Wakeup => {
                let _ = self.0.send_event(AppEvent::TerminalDirty { window_id: self.window_id });
            }
            TerminalEvent::Title(title) => {
                let _ = self.0.send_event(AppEvent::SetTitle { window_id: self.window_id, title });
            }
            _ => {}
        }
    }
}
```

### Pattern 3: OSC Passthrough Handler

`alacritty_terminal::Term` handles OSC 0/1/2/4/7/8/10/11/12/52. For custom OSC sequences (133, 9;9), intercept in the EventProxy or use a middleware Handler wrapper.

**What:** Wrap the Term handler to intercept custom OSC sequences before they reach the default handler.

```rust
// glass_terminal/src/shell_hook.rs
struct ShellIntegrationHandler<H: Handler> {
    inner: H,
    proxy: EventProxy,
}

impl<H: Handler> Handler for ShellIntegrationHandler<H> {
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        match params {
            [b"133", b"A"] => self.proxy.send(AppEvent::ShellHook(HookEvent::PromptStart)),
            [b"133", b"B"] => self.proxy.send(AppEvent::ShellHook(HookEvent::CommandStart)),
            [b"133", b"C"] => self.proxy.send(AppEvent::ShellHook(HookEvent::CommandExecuted)),
            [b"133", b"D", code] => {
                let exit = parse_exit_code(code);
                self.proxy.send(AppEvent::ShellHook(HookEvent::CommandFinished { exit_code: exit }));
            }
            [b"7", url] | [b"9", b"9", url] => {
                if let Some(path) = parse_file_url(url) {
                    self.proxy.send(AppEvent::ShellHook(HookEvent::CwdChanged(path)));
                }
            }
            _ => self.inner.osc_dispatch(params, _bell_terminated),
        }
    }

    // delegate all other methods to inner handler
}
```

Note: As of research date, `alacritty_terminal` does not natively support OSC 133. The wrapper handler approach above is the correct implementation path. Verify with `alacritty_terminal` API before coding — the exact trait names may differ from `vte`'s `Perform` trait.

---

## Component Boundary Map

| Boundary | Direction | Mechanism | Notes |
|----------|-----------|-----------|-------|
| `glass_terminal` → main thread | PTY data ready | `EventLoopProxy<AppEvent>` | Thread-safe |
| `glass_terminal` → `glass_renderer` | Frame data | `GridSnapshot` struct (owned, no lock) | Dropped after frame |
| Shell process → `glass_terminal` | Output bytes | ConPTY pipe (Windows) | OS-managed |
| Keyboard → Shell process | Input bytes | `Sender<PtyMsg>` → ConPTY write | mpsc channel |
| `glass_terminal` → `BlockManager` | Shell hooks | `AppEvent::ShellHook` in main thread | In WindowContext |
| `glass_renderer` → GPU | Draw calls | wgpu CommandEncoder / RenderPass | Submitted via Queue |
| Config file → app | TOML | serde deserialization at startup | No hot-reload in M1 |

---

## Build Order (Dependency Graph for Phase Sequencing)

Dependencies run bottom-to-top. Build in this order:

```
1. glass_core          (no deps on other glass crates)
        ↓
2. glass_terminal      (depends on glass_core, alacritty_terminal)
        ↓
3. glass_renderer      (depends on glass_core, wgpu, glyphon, winit)
        ↓
4. glass_app binary    (depends on all above — wires event loop)
        ↓
5. Shell integration   (PowerShell / bash scripts — no Rust deps)
        ↓
6. Block UI            (depends on BlockManager in glass_terminal,
                        block rendering in glass_renderer)
```

For Milestone 1 development order:
1. Cargo workspace scaffold with all crates (even stubs) — verifies build
2. `glass_core`: Config + AppEvent types
3. `glass_renderer`: wgpu surface + clear to color (no text yet)
4. `glass_terminal`: ConPTY spawn → keyboard → shell round-trip
5. VTE → grid → GridSnapshot → renderer text path (basic terminal functional)
6. Shell integration scripts + OSC 133 parsing + BlockManager
7. Block UI rendering (collapsible, exit code badge, duration)
8. Status bar (CWD from OSC 7, git branch)
9. Polish pass (font config, scrolling, selection)

---

## Anti-Patterns

### Anti-Pattern 1: Holding Term Lock During Render

**What people do:** Pass `Arc<Mutex<Term>>` directly to the renderer and lock it for the entire draw call.

**Why it's wrong:** The PTY thread blocks on the lock while GPU draw calls run (1-5ms+). This causes PTY read starvation, visible as input lag or delayed output.

**Do this instead:** Copy a `GridSnapshot` under a brief lock, drop the lock, render from the snapshot.

### Anti-Pattern 2: Parsing OSC 133 With String Splitting in Main Thread

**What people do:** Pipe raw terminal output through a regex or string split in the main event loop to find shell hooks.

**Why it's wrong:** The VTE parser already handles framing. Doing it again is redundant, error-prone (partial sequences across read boundaries), and executes on the main thread blocking rendering.

**Do this instead:** Hook into the vte `Handler::osc_dispatch()` method which fires only after the parser has fully assembled the sequence.

### Anti-Pattern 3: Spawning Shell With CreateProcess Instead of ConPTY

**What people do:** Use `std::process::Command` to spawn the shell on Windows.

**Why it's wrong:** PowerShell checks for a real console. Without ConPTY, ANSI escape sequences are not emitted, interactive features break, and programs like PSReadLine malfunction.

**Do this instead:** Use `alacritty_terminal`'s ConPTY support (`tty::windows::conpty`). It loads `conpty.dll` from Windows Terminal if available (better implementation) and falls back to the inbox ConHost API.

### Anti-Pattern 4: One Render Pipeline for Both Text and Rectangles

**What people do:** Draw background rects and text in a single interleaved pass using the same pipeline state.

**Why it's wrong:** Text and rect rendering require different pipeline states (blending modes, vertex formats). Switching pipelines repeatedly per cell is expensive.

**Do this instead:** Two separate wgpu render pipelines: `RectPipeline` for all colored quads (backgrounds, cursor, borders), `TextPipeline` for all glyph instances. Render all rects in one pass, all text in one pass.

### Anti-Pattern 5: Rebuilding Font Atlas Every Frame

**What people do:** Re-rasterize all visible glyphs every frame.

**Why it's wrong:** Glyph rasterization is expensive (microseconds per glyph). A 200×50 terminal has 10,000 cells — rasterizing all of them every frame at 60fps is millions of operations per second.

**Do this instead:** Maintain a `TextAtlas` that persists across frames. Only rasterize on cache miss. With monospace fonts and a fixed character set, the entire visible atlas fits in a few hundred KB of VRAM.

---

## Integration Points

### External Services

| Service | Integration Pattern | Notes |
|---------|---------------------|-------|
| ConPTY (Windows) | `alacritty_terminal::tty::windows::conpty` | Loads `conpty.dll` from Windows Terminal if present |
| PowerShell | Script injected via `-NoExit -Command . glass_integration.ps1` | OSC 133 emitting prompt function |
| bash | Script sourced via `GLASS_INTEGRATION=1` env var | PROMPT_COMMAND + PS0 hooks |
| wgpu backends | Auto-selected by wgpu adapter negotiation | DX12 primary on Windows 11 |
| System fonts | cosmic-text via glyphon FontSystem | Reads from Windows font directories |

### Internal Boundaries

| Boundary | Communication Method | Considerations |
|----------|---------------------|----------------|
| PTY thread ↔ main thread | `EventLoopProxy<AppEvent>` (send_event) | Non-blocking from PTY side; may drop events if proxy closed |
| Main thread → PTY thread | `mpsc::Sender<PtyMsg>` | Bounded channel; backpressure if shell is slow |
| `glass_terminal` ↔ `glass_renderer` | `GridSnapshot` value (owned struct) | No sharing after handoff; renderer owns during draw |
| `BlockManager` ↔ `glass_renderer` | `Vec<CommandBlock>` copied per frame | Cheap copy (10s of blocks max) |

---

## Scaling Considerations

This is a local desktop application, not a web service. "Scaling" means handling performance at increasing terminal complexity.

| Scale | Architecture Approach |
|-------|-----------------------|
| Small terminal (80×24) | Any approach works. Baseline. |
| Large terminal (200×60) | GridSnapshot copy is ~100KB. TextAtlas covers full charset. Fine. |
| Massive scrollback (100k lines) | Store scrollback in glass_history DB (Phase 2), not in Term grid. |
| High throughput (cat bigfile) | PTY read loop has MAX_LOCKED_READ (65k bytes) fairness cap from alacritty_terminal — prevents starving renderer. |
| Many concurrent terminals | Multi-window support via Processor HashMap (same pattern as Alacritty). Each window = independent WindowContext + PTY thread. |

---

## Sources

- [Alacritty DeepWiki: Event Loop and Architecture](https://deepwiki.com/alacritty/alacritty/3.6-multi-window-and-ipc)
- [alacritty_terminal crate docs.rs](https://docs.rs/alacritty_terminal/latest/alacritty_terminal/)
- [winit ApplicationHandler trait](https://docs.rs/winit/latest/winit/application/trait.ApplicationHandler.html)
- [winit 0.30 changelog](https://rust-windowing.github.io/winit/winit/changelog/v0_30/index.html)
- [glyphon: Fast wgpu text renderer](https://github.com/grovesNL/glyphon)
- [Windows Terminal Shell Integration (OSC 133, PowerShell, bash)](https://learn.microsoft.com/en-us/windows/terminal/tutorials/shell-integration)
- [WezTerm Shell Integration (OSC 7)](https://wezterm.org/shell-integration.html)
- [Warp: Adventures in Text Rendering](https://www.warp.dev/blog/adventures-text-rendering-kerning-glyph-atlases)
- [Alacritty GitHub: event.rs source](https://github.com/alacritty/alacritty/blob/master/alacritty/src/event.rs)
- [Alacritty GitHub: renderer/mod.rs source](https://github.com/alacritty/alacritty/blob/master/alacritty/src/renderer/mod.rs)
- [Alacritty GitHub: display/mod.rs source](https://github.com/alacritty/alacritty/blob/master/alacritty/src/display/mod.rs)
- [Alacritty GitHub: tty/windows/conpty.rs source](https://github.com/alacritty/alacritty/blob/master/alacritty_terminal/src/tty/windows/conpty.rs)
- [vte crate: VT100 parser](https://github.com/alacritty/vte)

---

*Architecture research for: Glass GPU-accelerated terminal emulator (Rust + wgpu + alacritty_terminal)*
*Researched: 2026-03-04*
