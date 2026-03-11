# Technology Stack

**Analysis Date:** 2026-03-08

## Languages

**Primary:**
- Rust (Edition 2021) - All application code across 8 workspace crates and the main binary

**Secondary:**
- TOML - Configuration (`~/.glass/config.toml`, all `Cargo.toml` manifests)
- WGSL - Not used; rendering uses wgpu's built-in pipeline (no custom shaders detected)

## Runtime

**Environment:**
- Native binary (no VM or interpreter)
- GPU: wgpu with platform-specific backends (DX12 on Windows, Metal on macOS, Vulkan on Linux)
- Async runtime: Tokio (full features) for MCP server; main event loop is synchronous via `winit` + `pollster`

**Package Manager:**
- Cargo (Rust standard)
- Lockfile: `Cargo.lock` present (version 4)

## Frameworks

**Core:**
- `winit` 0.30.13 - Cross-platform windowing and event loop
- `wgpu` 28.0.0 - GPU-accelerated rendering (WebGPU API)
- `alacritty_terminal` =0.25.1 (exact pin) - Terminal emulation (VT100/xterm parsing, PTY management)
- `glyphon` 0.10.0 - GPU text rendering / glyph rasterization

**Testing:**
- Built-in `#[test]` with `cargo test --workspace`
- `criterion` 0.5 - Benchmarks (`benches/perf_benchmarks.rs`)
- `tempfile` 3 - Temporary directories in tests

**Build/Dev:**
- `cargo clippy` - Linting (enforced in CI with `-D warnings`)
- `cargo fmt` - Formatting (enforced in CI)
- `cargo-wix` - Windows MSI installer builds
- `cargo-deb` - Linux `.deb` package builds

## Workspace Structure

The project is a Cargo workspace with 8 internal crates:

| Crate | Path | Purpose |
|-------|------|---------|
| `glass` (binary) | `src/main.rs` | Main application entry point, event loop, CLI |
| `glass_core` | `crates/glass_core/` | Config, events, error types, update checker, config watcher |
| `glass_terminal` | `crates/glass_terminal/` | PTY management, block detection, input handling, OSC parsing |
| `glass_renderer` | `crates/glass_renderer/` | GPU rendering pipeline, glyph cache, block/grid/tab rendering |
| `glass_history` | `crates/glass_history/` | SQLite command history database, search, retention |
| `glass_snapshot` | `crates/glass_snapshot/` | File snapshot/undo system with content-addressed blob store |
| `glass_mcp` | `crates/glass_mcp/` | Model Context Protocol server (stdio transport) |
| `glass_mux` | `crates/glass_mux/` | Session multiplexing, tabs, splits, layout management |
| `glass_pipes` | `crates/glass_pipes/` | Shell pipe tokenization and pipeline parsing |

## Key Dependencies

**Critical (core functionality):**
- `wgpu` 28.0.0 - All GPU rendering; without it, nothing displays
- `winit` 0.30.13 - Window creation, input events, event loop
- `alacritty_terminal` =0.25.1 - Terminal grid, PTY spawning, VT parsing (exact version pinned)
- `glyphon` 0.10.0 - Text layout and glyph rasterization on GPU
- `tokio` 1.50.0 (full) - Async runtime for MCP server subprocess

**Infrastructure:**
- `rusqlite` 0.38.0 (bundled SQLite) - History DB (`glass_history`) and snapshot DB (`glass_snapshot`)
- `rmcp` 1 - Model Context Protocol SDK for AI assistant integration
- `serde` 1.0.228 + `toml` 1.0.4 - Config file serialization/deserialization
- `notify` 8.0/8.2 - Filesystem watching (config hot-reload in `glass_core`, file change detection in `glass_snapshot`)
- `polling` 3 - Low-level PTY I/O polling in `glass_terminal`
- `vte` 0.15 - VT escape sequence parsing in `glass_terminal`
- `arboard` 3 - Clipboard read/write
- `blake3` 1.8.3 - Content-addressed hashing for snapshot blob store
- `ureq` 3 - HTTP client for GitHub Releases API (update checker)
- `clap` 4.5 (derive) - CLI argument parsing
- `anyhow` 1.0.102 - Error handling
- `tracing` 0.1.44 + `tracing-subscriber` 0.3 - Structured logging
- `dirs` 6 - Platform-specific directory paths (`~/.glass/`)
- `url` 2 - URL parsing for OSC 7 `file://` paths
- `semver` 1 - Version comparison for auto-updater
- `chrono` 0.4 - Timestamps in history records
- `ignore` 0.4 - Gitignore-aware file traversal in `glass_snapshot`
- `shlex` 1.3.0 - Shell command tokenization
- `strip-ansi-escapes` 0.2 - ANSI escape removal for stored command output
- `memory-stats` 1.2 - Runtime memory usage reporting

**Optional (feature-gated):**
- `tracing-chrome` 0.7 - Chrome trace profiling output (behind `perf` feature flag)

**Platform-specific:**
- `windows-sys` 0.59 (`Win32_System_Console`) - UTF-8 console code page on Windows
- `tempfile` 3 - Temp directory for MSI download on Windows update flow

## Feature Flags

```toml
[features]
perf = ["glass_terminal/perf", "glass_renderer/perf", "dep:tracing-chrome"]
```

The `perf` feature enables Chrome-trace-format profiling output. Both `glass_terminal` and `glass_renderer` have their own `perf` feature flags for instrumentation.

## Configuration

**User Configuration:**
- Location: `~/.glass/config.toml` (resolved via `dirs` crate)
- Format: TOML with serde deserialization
- Hot-reload: Config watcher monitors parent directory for atomic save support (`crates/glass_core/src/config_watcher.rs`)
- Key settings: `font_family`, `font_size`, `shell`, `[history]`, `[snapshot]`, `[pipes]`

**Data Storage Locations:**
- History DB: Project-local `.glass/history.db` (SQLite, WAL mode)
- Snapshot DB: Project-local `.glass/snapshots.db` (SQLite, WAL mode)
- Blob store: Project-local `.glass/blobs/` (content-addressed by BLAKE3 hash)

**Build Configuration:**
- `Cargo.toml` workspace root with shared dependency versions via `[workspace.dependencies]`
- No `rust-toolchain.toml`, `.rustfmt.toml`, or `clippy.toml` detected (uses defaults)
- Benchmark harness disabled for `perf_benchmarks` (uses Criterion)

## Platform Requirements

**Development:**
- Rust stable toolchain (no nightly features used)
- GPU with DX12 (Windows), Metal (macOS), or Vulkan (Linux) support
- Linux: requires `libwayland-dev`, `libxkbcommon-dev`, `libx11-dev`, `libxi-dev`, `libxtst-dev`

**Production:**
- Native binary, no runtime dependencies beyond OS-provided GPU drivers
- SQLite is bundled (no external database needed)

**Supported Platforms:**
- Windows (x86_64-pc-windows-msvc) - Primary development target
- macOS (aarch64-apple-darwin) - Apple Silicon
- Linux (x86_64-unknown-linux-gnu) - X11/Wayland

## CI/CD

**GitHub Actions Workflows:**
- `.github/workflows/ci.yml` - Build (3 platforms), test, clippy, rustfmt check
- `.github/workflows/release.yml` - Tag-triggered builds producing MSI (Windows), DMG (macOS), DEB (Linux)
- `.github/workflows/docs.yml` - Documentation deployment

**Packaging:**
- Windows: `cargo-wix` MSI installer (`packaging/winget/` manifests)
- macOS: DMG via `packaging/macos/build-dmg.sh` with `Info.plist`
- Linux: `cargo-deb` DEB package with `packaging/linux/glass.desktop`
- Homebrew: `packaging/homebrew/glass.rb` formula

**Documentation:**
- mdBook (`docs/book.toml`) for user-facing documentation

---

*Stack analysis: 2026-03-08*
