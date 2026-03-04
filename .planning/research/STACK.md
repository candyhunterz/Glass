# Stack Research

**Domain:** Rust GPU-accelerated terminal emulator (Windows-first)
**Researched:** 2026-03-04
**Confidence:** HIGH (all versions verified via crates.io API; architecture patterns verified via official project sources)

---

## Recommended Stack

### Core Technologies

| Technology | Version | Purpose | Why Recommended |
|------------|---------|---------|-----------------|
| `alacritty_terminal` | 0.25.1 | VTE/escape-code parsing, terminal grid, PTY event loop | Battle-tested since 2017; provides `Term<T>` + `Grid<Cell>` API that handles the entire ANSI/VTE state machine. Eliminates ~5,000 lines of escape-code parser work. Apache 2.0. ConPTY supported on Windows via `windows-sys` + `miow`. |
| `wgpu` | 28.0.0 | GPU-accelerated rendering surface | Cross-platform WebGPU-standard API. On Windows it auto-selects DX12 (preferred) then Vulkan then OpenGL. DX12 backend has been production-stable for years. No OpenGL legacy baggage. WGSL shaders compile ahead-of-time. |
| `winit` | 0.30.13 | Cross-platform window creation and OS event loop | Standard Rust window library. 0.30 introduced `ApplicationHandler` trait (breaking from 0.29). Used by Alacritty, wgpu examples, and nearly every Rust GPU app. Provides `raw-window-handle` 0.6 surface handles that wgpu requires. |
| `tokio` | 1.50.0 | Async runtime for PTY I/O, shell output streaming | Industry-standard Rust async runtime. PTY reads/writes are inherently I/O-bound; tokio lets the render thread stay unblocked. `alacritty_terminal`'s EventLoop runs on a thread that feeds a channel; tokio is the right glue for the Glass event pipeline. |
| `glyphon` | 0.10.0 | GPU font rendering via cosmic-text + wgpu | The definitive wgpu text renderer. Internally uses cosmic-text (shaping + layout) → etagere (atlas packing) → wgpu (render pass). Requires wgpu `^28.0.0` — perfect version alignment. Used by COSMIC terminal emulator in production. |
| `cosmic-text` | 0.15.x | Text shaping, font fallback, glyph rasterization | Pulled in transitively by glyphon 0.10 (`^0.15`). Provides `FontSystem` (font discovery + fallback), `SwashCache` (rasterizer), `Buffer` (layout), and `Attrs` (text attributes). Handles emoji, ligatures, and bidirectional text. Pure Rust. |

### Supporting Libraries

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `pollster` | 0.4.0 | Block on async wgpu init from sync winit `resumed()` | Required: winit 0.30 event handlers are not async, but `wgpu::Instance::request_adapter()` is. Use `pollster::block_on()` inside `ApplicationHandler::resumed()` to initialize the GPU surface without spawning a separate thread. |
| `serde` | 1.0.228 | Derive `Serialize`/`Deserialize` for config structs | TOML config deserialization. Use with `derive` feature. |
| `toml` | 1.0.4 | Parse `glass.toml` config file | Standard Rust TOML parser; used by Cargo itself. Pair with `serde`. `1.0.4` (spec 1.1.0 compliant). |
| `anyhow` | 1.0.102 | Error handling across crate boundaries | For `glass_core` and `main` error propagation. Avoids boilerplate `Box<dyn Error>`. Use `thiserror` inside library crates for typed errors; `anyhow` in binaries. |
| `tracing` | 0.1.44 | Structured logging and spans for performance profiling | Prefer over `log` crate — structured fields help debug latency issues. Use `tracing-subscriber` for output. Critical for diagnosing the <5ms input latency requirement. |
| `bytemuck` | 1.25.0 | Safe byte casting for wgpu vertex/uniform buffers | Required pattern with wgpu: GPU buffer uploads need `&[u8]`; `bytemuck::cast_slice()` provides zero-cost conversion from typed structs. Use `derive(Pod, Zeroable)` on all GPU structs. |
| `etagere` | 0.2.15 | Glyph atlas bin-packing (used by glyphon transitively) | Pulled in by glyphon. Manages the texture atlas that holds rasterized glyphs. Do not use directly — glyphon's `TextAtlas` wraps it. |
| `notify` | 8.2.0 | Filesystem watcher for config hot-reload | Watches `glass.toml` for changes; send event to main loop to reload config without restart. Cross-platform; uses ReadDirectoryChangesW on Windows. |
| `rusqlite` | 0.38.0 | SQLite bindings for structured scrollback (Phase 2) | Defer to Phase 2. Listed here because the workspace should stub the `glass_history` crate now to avoid future refactoring. Bundles libsqlite3 via `bundled` feature — no system dependency required on Windows. |

### Development Tools

| Tool | Purpose | Notes |
|------|---------|-------|
| `cargo clippy` | Lint enforcement | Run with `--all-targets --all-features -- -D warnings` in CI. Catches common Rust pitfalls early. |
| `cargo fmt` | Code formatting | `edition = "2021"` formatting. Enforce in CI with `--check`. |
| `cargo nextest` | Faster test runner | Parallel test execution; significantly faster than `cargo test` for workspace-wide runs. Install: `cargo install cargo-nextest`. |
| `wgpu`'s `WGPU_BACKEND` env var | Force specific GPU backend during development | Set `WGPU_BACKEND=dx12` to force DX12, `vulkan` for Vulkan. Useful for comparing backends on Windows. |
| `RUST_LOG=trace` / `tracing-subscriber` | Runtime log filtering | Combine with `tracing` crate. Use `EnvFilter` to scope per-crate without recompiling. |

---

## Workspace Structure

```
glass/
  Cargo.toml              # workspace root, resolver = "3"
  crates/
    glass_core/           # shared types: Event, Config, BlockId, etc.
    glass_terminal/       # alacritty_terminal integration, PTY spawning, VTE grid
    glass_renderer/       # wgpu surface, glyphon text, cell grid rendering
    glass_history/        # stub — SQLite scrollback (Phase 2)
    glass_snapshot/       # stub — filesystem snapshots (Phase 3)
    glass_pipes/          # stub — pipe visualization (Phase 4)
    glass_mcp/            # stub — MCP server (Phase 2)
  src/
    main.rs               # winit ApplicationHandler, top-level event loop
```

**Why this structure:** Dependency boundaries prevent circular imports. `glass_renderer` depends on `glass_core` but NOT `glass_terminal` — the renderer receives pre-processed cell data, not raw PTY bytes. `glass_terminal` owns the `alacritty_terminal::Term` and converts grid cells to a renderer-facing `ScreenFrame` type.

---

## Installation

```toml
# Cargo.toml (workspace root)
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.dependencies]
# Core rendering
wgpu          = "28.0.0"
winit         = "0.30.13"
glyphon       = "0.10.0"
bytemuck      = { version = "1.25.0", features = ["derive"] }
pollster      = "0.4.0"

# Terminal emulation
alacritty_terminal = "0.25.1"
tokio         = { version = "1.50.0", features = ["full"] }

# Config
serde         = { version = "1.0.228", features = ["derive"] }
toml          = "1.0.4"

# Utilities
anyhow        = "1.0.102"
tracing       = "0.1.44"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
notify        = "8.2.0"

# Phase 2 (stub crate, add when ready)
rusqlite      = { version = "0.38.0", features = ["bundled"] }
```

---

## Critical Integration Notes

### winit 0.30 + wgpu 28: The Async Init Pattern

winit 0.30 uses a trait-based `ApplicationHandler` where event methods are synchronous. wgpu's `request_adapter()` and `request_device()` are async. The established pattern (confirmed by community consensus, 2024-2025):

```rust
// In ApplicationHandler::resumed():
fn resumed(&mut self, event_loop: &ActiveEventLoop) {
    let window = Arc::new(event_loop.create_window(WindowAttributes::default()).unwrap());
    let state = pollster::block_on(GpuState::new(Arc::clone(&window)));
    self.state = Some(state);
}
```

Do NOT attempt to make the event loop async — winit's designers have explicitly stated it will not become async. `pollster::block_on` is the correct pattern. The GPU init happens once at startup; the blocking is acceptable.

### alacritty_terminal ConPTY on Windows

Verified (crates.io dependency metadata for 0.25.1): `alacritty_terminal` uses `windows-sys ^0.59.0` with `Win32_System_Console` features, plus `miow ^0.6.0` for Windows pipe I/O. The `Pty` abstraction in `alacritty_terminal::tty` provides platform-specific backends — on Windows this wraps ConPTY via the Win32 Console APIs.

**Important caveat:** Alacritty's ConPTY uses ConHost (not OpenConsole). This is the same ConPTY used by older Windows Terminal builds. It is functional and production-grade but may have slightly higher latency than OpenConsole's implementation. For Milestone 1 this is acceptable; revisit if input latency testing shows issues.

**Spawning pattern:**
```rust
use alacritty_terminal::tty::{self, Options as TtyOptions};
use alacritty_terminal::event_loop::EventLoop;
use alacritty_terminal::term::Term;
use alacritty_terminal::sync::FairMutex;

let pty_options = TtyOptions {
    shell: Some(Program::WithArgs { program: "pwsh".into(), args: vec![] }),
    working_directory: None,
    hold: false,
};
let pty = tty::new(&pty_options, size_info, None).unwrap();
```

### Font Rendering Pipeline

The rendering pipeline for a monospace terminal grid:

```
FontSystem (cosmic-text)
    ├── Loads system fonts + any bundled fonts
    ├── Resolves font family + fallback chain per cell character
    └── SwashCache → rasterizes glyphs to pixel masks

TextAtlas (glyphon)
    └── Packs rasterized glyphs into a GPU texture atlas (via etagere)

TextRenderer (glyphon)
    └── Each frame: build TextArea list from terminal grid cells
        → upload changed glyph data to atlas
        → record render pass commands (single draw call for all text)

wgpu RenderPass
    └── Executes: background quads (cell backgrounds) + text pass (glyphon)
```

For terminal use, each cell in the `Grid<Cell>` maps to a fixed-size rect. Glyphon's `TextArea` API takes a position + `Buffer` (cosmic-text layout object). The efficient approach is to use one `Buffer` per cell character (or per line), not one giant buffer for the whole screen — this allows dirty-cell tracking to skip re-layout for unchanged cells.

### Shell Integration: OSC 133 Sequences

Verified via Microsoft Learn (Windows Terminal documentation). The standard protocol uses FTCS (Final Term Command Sequences) via OSC 133:

| Sequence | Meaning | When |
|----------|---------|------|
| `OSC 133 ; A ST` | Prompt start | Before prompt text |
| `OSC 133 ; B ST` | Command start | After prompt, before user types |
| `OSC 133 ; C ST` | Command executed | After Enter, before output |
| `OSC 133 ; D ; <exit_code> ST` | Command finished | After output, before next prompt |
| `OSC 9 ; 9 ; "<path>" ST` | CWD notification | Inside prompt function |

Glass must parse these sequences out of the VTE stream (they pass through `alacritty_terminal` as unrecognized OSC sequences and can be intercepted via the `EventListener` trait). The PowerShell integration requires injecting a custom `prompt` function into the user's PowerShell profile or auto-injecting via `$PROFILE` — the exact prompt code is documented in Microsoft Learn.

**PowerShell hook mechanism:** Override the `prompt {}` function. No native `preexec` exists in PowerShell; command start/end detection relies on `Get-History -Count 1` comparisons inside the prompt function. This is the same approach used by Windows Terminal, WezTerm, and Starship.

**Bash hook mechanism:** `PROMPT_COMMAND` + `PS0` variable (available in bash 4.4+, which covers Git Bash on Windows). `PS0` fires before command execution (gives `OSC 133;C`), `PROMPT_COMMAND` fires before each prompt render (gives `OSC 133;D` with exit code).

---

## Alternatives Considered

| Recommended | Alternative | Why Not |
|-------------|-------------|---------|
| `glyphon` | `wgpu_glyph` | `wgpu_glyph` is unmaintained (last commit 2023); pinned to old wgpu versions. Do not use. |
| `glyphon` | `fontdue` directly | fontdue only rasterizes; no shaping, no atlas management, no wgpu integration. Would require building all of glyphon from scratch. |
| `glyphon` | `rusttype` / `glyph_brush` | Both deprecated/unmaintained. The ecosystem has moved to cosmic-text + glyphon. |
| `alacritty_terminal` | Custom VTE parser | VTE is ~5,000 lines of state machine covering 40+ years of terminal escape codes. Do not rebuild this. |
| `alacritty_terminal` | `vte` crate directly | The `vte` crate is the low-level parser; `alacritty_terminal` builds the full terminal state (grid, cursor, colors, modes) on top of it. Using `vte` directly means writing all of `alacritty_terminal` yourself. |
| `winit` | `sdl2` | SDL2 requires C bindings and does not integrate with the raw-window-handle ecosystem that wgpu expects. |
| `winit` | `tao` (Tauri's fork) | tao is maintained for Tauri's needs, not general use. winit 0.30 has caught up on most Windows-specific gaps that drove tao's creation. |
| `tokio` | `async-std` | Tokio has won the async runtime ecosystem war. alacritty_terminal, hyper, and every major async crate assumes tokio. Use tokio. |
| `wgpu` | Raw DX12 via `d3d12` crate | Massively more complexity, no cross-platform future, no WebGPU standard compliance. wgpu's DX12 backend is production-quality. |
| `rusqlite` (bundled) | System libsqlite3 | On Windows, system SQLite is not guaranteed to exist. `bundled` feature statically links SQLite — zero deployment dependency. |

---

## What NOT to Use

| Avoid | Why | Use Instead |
|-------|-----|-------------|
| `wgpu_glyph` | Unmaintained since 2023, incompatible with wgpu 28 | `glyphon` |
| `rusttype` | Deprecated; the maintainer recommends cosmic-text | `cosmic-text` (via glyphon) |
| `glyph_brush` | Built on rusttype; inherits deprecation; no wgpu 28 support | `glyphon` |
| `openssl` | Massive C dependency, painful on Windows. Not needed for a terminal. | `rustls` if TLS ever needed |
| `winpty` | Legacy Windows PTY shim for old ConPTY-less systems. Windows 10 1809+ has native ConPTY. Do not add this dependency. | `alacritty_terminal`'s built-in ConPTY support |
| `nix` crate | Unix-only; causes conditional compilation chaos in a Windows-first project | `windows-sys` directly (or let `alacritty_terminal` handle it) |
| `log` crate | Fine but `tracing` is strictly better: structured fields, spans, async-aware | `tracing` |
| `clap` (for now) | No CLI args needed for Milestone 1. Adds compile time. | Add in a future milestone if Glass gets CLI flags |

---

## Stack Patterns by Variant

**If wgpu DX12 init fails at startup:**
- wgpu automatically falls back to Vulkan, then OpenGL (via ANGLE on Windows)
- Do not override this — let `Backends::all()` (the default) handle selection
- Log which backend was selected via `adapter.get_info().backend`

**If a user's GPU does not support wgpu at all:**
- This is extremely rare on Windows 10/11 (requires very old or broken GPU driver)
- Do not implement a software fallback for Milestone 1
- Show an error dialog and exit gracefully

**If ConPTY PTY spawn fails:**
- Windows 10 builds before 1903 lack stable ConPTY. Target: Windows 10 1903+ minimum.
- Surface the error to the user with a clear message.

**If glyphon atlas overflows:**
- `TextAtlas` can grow dynamically but has practical limits
- For a terminal, glyph count is bounded by the character set × style combinations used
- A typical 256-color terminal session fits comfortably in a 1024×1024 atlas

---

## Version Compatibility Matrix

| Package | Version | Compatible With | Notes |
|---------|---------|-----------------|-------|
| `glyphon` | 0.10.0 | `wgpu ^28.0.0`, `cosmic-text ^0.15` | This is the exact version alignment. Do not mix glyphon versions with different wgpu. |
| `winit` | 0.30.13 | `wgpu 28.0.0` (via `raw-window-handle` 0.6) | wgpu 28 uses `raw-window-handle` 0.6; winit 0.30 provides it. Compatible. |
| `wgpu` | 28.0.0 | `winit 0.30.x`, `glyphon 0.10.0`, `bytemuck 1.x` | No winit dependency in wgpu itself — integration is via `raw-window-handle` trait, not a direct dep. |
| `alacritty_terminal` | 0.25.1 | `tokio 1.x`, `windows-sys 0.59` | No wgpu/winit dependency — it's a pure terminal state machine + PTY layer. |
| `rusqlite` | 0.38.0 | `bundled` feature = SQLite 3.47+ | Verify bundled SQLite version is acceptable for WAL mode (Phase 2 requirement). |
| `toml` | 1.0.4 | `serde 1.x` | Requires serde `Deserialize` derive. Both stable. |

---

## Sources

- **crates.io API** — Version verification for all crates listed above (fetched 2026-03-04):
  - `alacritty_terminal` 0.25.1: https://crates.io/api/v1/crates/alacritty_terminal
  - `wgpu` 28.0.0: https://crates.io/api/v1/crates/wgpu
  - `winit` 0.30.13: https://crates.io/api/v1/crates/winit
  - `glyphon` 0.10.0: https://crates.io/api/v1/crates/glyphon
  - `cosmic-text` 0.18.2 (crates.io), 0.15.x (glyphon-required): https://crates.io/api/v1/crates/cosmic-text
  - `tokio` 1.50.0: https://crates.io/api/v1/crates/tokio
  - `notify` 8.2.0: https://crates.io/api/v1/crates/notify
  - `rusqlite` 0.38.0: https://crates.io/api/v1/crates/rusqlite
  - `pollster` 0.4.0: https://crates.io/api/v1/crates/pollster
  - `anyhow` 1.0.102, `tracing` 0.1.44, `bytemuck` 1.25.0, `toml` 1.0.4, `serde` 1.0.228
- **glyphon dependency manifest** (verified): glyphon 0.10.0 requires `wgpu ^28.0.0` and `cosmic-text ^0.15` — https://crates.io/api/v1/crates/glyphon/0.10.0/dependencies
- **alacritty_terminal Windows deps** (verified): uses `windows-sys ^0.59`, `miow ^0.6`, `Win32_System_Console` — https://crates.io/api/v1/crates/alacritty_terminal/0.25.1/dependencies
- **glyphon GitHub**: https://github.com/grovesNL/glyphon — architecture confirmed (cosmic-text + etagere + wgpu pipeline)
- **cosmic-term** (reference implementation using this exact stack): https://github.com/pop-os/cosmic-term
- **winit + wgpu async init pattern** (community consensus, 2024-2025): https://users.rust-lang.org/t/how-to-integrate-winit-0-30-with-async/133747
- **Shell integration OSC 133 sequences** (Microsoft official docs): https://learn.microsoft.com/en-us/windows/terminal/tutorials/shell-integration
- **wgpu Windows backend docs**: https://docs.rs/wgpu/latest/wgpu/struct.Backends.html
- **wgpu releases** (DX12 transparency support in v27, v28 latest): https://github.com/gfx-rs/wgpu/releases

---
*Stack research for: Glass — Rust GPU-accelerated terminal emulator (Windows-first)*
*Researched: 2026-03-04*
