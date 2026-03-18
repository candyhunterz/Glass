# Technology Stack

**Analysis Date:** 2026-03-18

## Languages

**Primary:**
- Rust 2021 edition - Core application, all crates, and binary (`src/main.rs`)

**Build scripts:**
- Rust build.rs (`build.rs`) - Windows resource compilation via `winresource`

## Runtime

**Environment:**
- Tokio 1.50.0 - Async runtime (full features)
- Rust standard library (no stable version pinning beyond edition)

**Package Manager:**
- Cargo - Workspace manager (resolver = "2")
- Lockfile: `Cargo.lock` - Present and pinned for reproducible builds

## Frameworks

**Core Application:**
- winit 0.30.13 - Window management and event loop
- wgpu 28.0.0 - GPU rendering (Metal on macOS, Vulkan/OpenGL on Linux, DX12 on Windows)
- glyphon 0.10.0 - GPU-accelerated text rendering

**Terminal Emulation:**
- alacritty_terminal 0.25.1 - Exact version pin (=0.25.1, no ^ or ~). VTE parsing, PTY integration
- vte 0.15 - Virtual terminal emulator (used by alacritty_terminal)

**Testing:**
- Criterion 0.5 - Benchmarking with HTML reports
- tempfile 3 - Temporary test fixtures

**Build/Dev:**
- Pollster 0.4.0 - Async runtime polling for rendering loop
- Bytemuck 1.25.0 - Derive-based GPU data serialization

## Key Dependencies

**Critical (core functionality):**
- tokio 1.50.0 - Async/concurrent operations, process spawning, IPC
- alacritty_terminal =0.25.1 - Terminal emulation (exact pin prevents silent breakage)
- wgpu 28.0.0 - GPU rendering pipeline
- glyphon 0.10.0 - Text rendering on GPU
- rusqlite 0.38.0 - SQLite database with bundled SQLite + FTS5 full-text search
- notify 8.0/8.2 - Cross-platform filesystem watching (config hot-reload, snapshot detection)

**Configuration & Serialization:**
- serde 1.0.228 - Serialization framework (derive, used by config + JSON)
- toml 1.0.4 - TOML parsing for config files
- serde_json 1.0 - JSON serialization (usage API responses, MCP payloads)

**Data Processing:**
- blake3 1.8.3 - Content-addressed hashing for snapshot blob store
- regex 1 - Regular expression parsing (command detection, SOI classification)
- strip-ansi-escapes 0.2 - ANSI escape stripping for history storage

**Utilities:**
- anyhow 1.0.102 - Ergonomic error handling (Result<T, Box<dyn Error>>)
- tracing 0.1.44 - Structured logging
- tracing-subscriber 0.3 - Log filtering and output formatting
- tracing-chrome 0.7 - Optional Chrome tracing instrumentation (feature: perf)
- dirs 6 - Cross-platform home/config directory resolution
- chrono 0.4 - Date/time parsing and formatting
- arboard 3 - Cross-platform clipboard
- shlex 1.3.0 - Shell command tokenization
- url 2 - OSC 7 file:// URI parsing

**Platform Abstraction:**
- windows-sys 0.59 - Windows API bindings (UTF-8 console, Job Objects, Threading)
- libc 0.2 - Unix syscall bindings (forkpty, prctl for orphan prevention)

**System Monitoring:**
- memory-stats 1.2 - Memory usage reporting for status bar

**CLI & Scripting:**
- clap 4.5 - Command-line argument parsing (derive macros)
- rhai 1 - Embedded scripting language (custom Rhai scripts for app automation)
- schemars 1 - JSON Schema generation for MCP tool parameters

**External Integration:**
- ureq 3 - Synchronous HTTP client (usage API polling, update checks)
- rmcp 1 - MCP (Model Context Protocol) server framework with transport-io and server features
- git2 0.20 - Git operations (repository discovery, worktree management)
- diffy 0.4 - Diff computation for file comparisons
- uuid 1 - UUID v4 generation for agent sessions
- similar 2 - Diff/patch computation (MCP file diff tool)

**Performance:**
- criterion 0.5 - Benchmarking harness (dev-only)
- image 0.25 - PNG loading (default features disabled except png)

## Configuration

**Environment:**
- No .env files required - credentials stored in:
  - OAuth token: `~/.claude/.credentials.json` (read from `claudeAiOauth.accessToken`)
  - Agent coordination: `~/.glass/agents.db` (SQLite, WAL mode)
  - Project-specific history: `~/.glass/[project-hash]/history.db` (SQLite with FTS5)
  - Snapshots: `~/.glass/[project-hash]/blobs/` (content-addressed blob store)

**Config file:**
- Location: `~/.glass/config.toml` - Hot-reloaded via notify watcher
- Sections: font, shell, history, snapshot, pipes, soi, agent (with orchestrator subsection), scripting
- Defaults applied for missing sections or malformed TOML (silent fallback)

**Key config options:**
- `font_family` - Platform defaults: Consolas (Windows), Menlo (macOS), Monospace (Linux)
- `shell` - Fallback shell if not set in environment
- `[history]` max_output_capture_kb (default 50)
- `[snapshot]` enabled, max_count (1000), max_size_mb (500), retention_days (30)
- `[pipes]` enabled, max_capture_mb (10), auto_expand (true)
- `[soi]` enabled, shell_summary (false), format (oneline)
- `[agent]` mode (Off), max_budget_usd (1.0), cooldown_secs (30), allowed_tools
- `[agent.orchestrator]` enabled (false), silence_timeout_secs (60), prd_path (PRD.md), checkpoint_path (.glass/checkpoint.md)
- `[scripting]` enabled (true), max_operations, max_timeout_ms, max_scripts_per_hook

**Build configuration:**
- Feature flag `perf` - Enables tracing-chrome instrumentation for profiling
- Platform-specific: Windows build uses `winresource` for icon embedding

## Platform Requirements

**Development:**
- Rust 1.70+ (2021 edition)
- Cargo
- Platform-specific build tools:
  - Windows: MSVC toolchain (via Visual Studio)
  - macOS: Xcode with metal support
  - Linux: libxkbcommon, libwayland-dev, libxcb-render0-dev (see CI for full list)

**Production:**
- Linux: x86_64 architecture, X11 or Wayland
- macOS: aarch64 or x86_64, Metal GPU support
- Windows: x86_64, ConPTY support (Windows 10+)

**Network:**
- Optional: GitHub API access for update checks (api.github.com)
- Optional: Anthropic usage API for token tracking (api.anthropic.com/api/oauth/usage)

---

*Stack analysis: 2026-03-18*
