# Technology Stack

**Analysis Date:** 2026-03-15

## Languages

**Primary:**
- Rust 2021 edition - All source code and workspaces

**Secondary:**
- Shell scripts - Shell integration for bash, zsh, fish, PowerShell (injected at runtime)

## Runtime

**Environment:**
- Tokio async runtime 1.50.0 - Async task execution

**Package Manager:**
- Cargo - Rust package management
- Lockfile: Cargo.lock present (locked dependencies)

## Frameworks

**Core:**
- winit 0.30.13 - Window management and event loop
- tokio 1.50.0 (full features) - Async runtime
- alacritty_terminal 0.25.1 (pinned exact) - VTE parsing and terminal emulation

**Rendering:**
- wgpu 28.0.0 - GPU rendering via Direct3D/Vulkan/Metal
- glyphon 0.10.0 - GPU-accelerated glyph rendering
- bytemuck 1.25.0 (with derive) - GPU memory layout serialization

**Terminal/PTY:**
- rustix (via alacritty_terminal) - PTY spawning and control
- polling 3 - Cross-platform I/O readiness

**Config & Data:**
- serde 1.0.228 (with derive) - Serialization/deserialization
- toml 1.0.4 - TOML config parsing
- rusqlite 0.38.0 (bundled SQLite) - Local database with FTS5

**History & Snapshots:**
- blake3 1.8.3 - Content-addressed blob hashing
- notify 8.0/8.2 - Filesystem event watching
- ignore 0.4 - .gitignore-aware file traversal

**UI & Interaction:**
- arboard 3 - Clipboard access
- url 2 - URL/URI parsing (OSC 7 file:// paths)

**CLI & Utilities:**
- clap 4.5 (with derive) - Command-line argument parsing
- chrono 0.4 - Date/time handling
- tracing 0.1.44 - Structured logging
- tracing-subscriber 0.3 (with env-filter) - Log filtering
- anyhow 1.0.102 - Error handling
- uuid 1 (with v4 feature) - UUID generation

**Development Tools:**
- git2 0.20 - Git operations (worktree isolation)
- diffy 0.4 - Diff calculation

**Performance & Debugging:**
- memory-stats 1.2 - Memory measurement
- criterion 0.5 (dev-only) - Benchmarking with HTML reports
- tracing-chrome 0.7 (optional, perf feature) - Chrome DevTools timeline export

## Key Dependencies

**Critical:**
- alacritty_terminal 0.25.1 - EXACT pinned version, no ^ or ~ substitution allowed. Handles all VT escape sequence parsing and PTY state machine.
- rusqlite 0.38.0 - Bundled SQLite with FTS5 full-text search. Stores command history, pipe stages, and coordination data.
- wgpu 28.0.0 - GPU rendering backend. Abstracts across Direct3D (Windows), Metal (macOS), Vulkan/OpenGL (Linux).

**Infrastructure:**
- blake3 - Content-addressed blob storage for file snapshots
- notify - Platform-native filesystem watching (inotify/FSEvents/ReadDirectoryChangesW)
- git2 - Agent worktree isolation and diff operations

## Platform-Specific Dependencies

**Windows:**
- windows-sys 0.59 (features: Win32_System_Console, Win32_System_JobObjects, Win32_Foundation, Win32_System_Threading) - ConPTY API, UTF-8 console code page, Job Object orphan prevention
- winresource 0.1 (build-only) - Embedding resources in executable

**Unix (macOS/Linux):**
- libc 0.2 - POSIX process control (prctl for orphan prevention)
- rustix (via alacritty_terminal) - forkpty and Unix PTY handling

**macOS specific:**
- FSEvents support (via notify) - Native filesystem watching

**Linux specific:**
- System dependencies (not in Cargo.toml): libxkbcommon-dev, libx11-dev, libxi-dev, libxtst-dev, libwayland-dev
- inotify support (via notify) - Kernel filesystem watching

## Configuration

**Environment:**
- Config file: `~/.glass/config.toml` (TOML format, hot-reloaded via notify watcher)
- Databases: `.glass/history.db` (project-local) or `~/.glass/global-history.db` (fallback)
- Snapshots: `.glass/snapshots/` (blob store with blake3 content addressing)
- Coordination: `~/.glass/agents.db` (SQLite WAL mode, shared by all agents)
- OAuth token: `~/.claude/.credentials.json` (read by usage tracker)

**Build:**
- Workspace resolver: version 2
- Feature flags:
  - `perf` - Enables tracing-chrome instrumentation for performance profiling
- Release binary at: `target/release/glass`
- Debian packaging config at: `[package.metadata.deb]`

## Workspace Structure

Nine internal crates plus main binary:

1. `crates/glass_core` - Config loading, event loop integration, update checker, agent runtime
2. `crates/glass_terminal` - PTY spawning, shell integration, VT parsing, block manager, OSC scanner
3. `crates/glass_renderer` - wgpu rendering, frame composition, grid/blocks/tabs/search UI
4. `crates/glass_mux` - Session/tab/pane multiplexing, binary split tree layout
5. `crates/glass_history` - SQLite command history DB with FTS5, output compression, query engine
6. `crates/glass_snapshot` - File snapshotting, blob store, undo engine, command safety parsing
7. `crates/glass_pipes` - Pipeline parsing and visualization
8. `crates/glass_mcp` - MCP server exposing Glass tools over stdio/JSON-RPC 2.0
9. `crates/glass_coordination` - Multi-agent registry, advisory locks, inter-agent messaging (SQLite)
10. `crates/glass_agent` - Agent worktree isolation, git operations
11. `crates/glass_soi` - Structured Output Intelligence: command output classification
12. `crates/glass_errors` - Shared error types

## Build & Test

```bash
cargo build --release          # Release binary (~5-10 MB)
cargo test --workspace         # ~420 tests (cross-platform gated)
cargo fmt --all -- --check     # Formatting check (enforced)
cargo clippy --workspace -- -D warnings  # Linting (all warnings = errors)
cargo bench                    # Criterion benchmarks with HTML reports
cargo build --features perf    # Build with tracing instrumentation for profiling
```

## Minimum Dependency Versions

All dependencies locked in Cargo.lock. Key pinned:
- `alacritty_terminal = "=0.25.1"` (exact, no range)
- `rusqlite = "0.38.0"` (FTS5 bundled)
- `wgpu = "28.0.0"` (API stability)

## CI/CD

**GitHub Actions** (`.github/workflows/ci.yml`):
- Runs on: Ubuntu (fmt), Windows (clippy), macOS + Linux + Windows (build+test matrix)
- Passes all checks before merge to main
- Artifact upload via release workflow

---

*Stack analysis: 2026-03-15*
