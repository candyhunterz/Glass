# External Integrations

**Analysis Date:** 2026-03-08

## APIs & External Services

**GitHub Releases API:**
- Purpose: Auto-update checker queries latest release to compare versions
- SDK/Client: `ureq` 3 (blocking HTTP client)
- Endpoint: `https://api.github.com/repos/nkngu/Glass/releases/latest`
- Auth: None (public repo, unauthenticated requests)
- Implementation: `crates/glass_core/src/updater.rs`
- Headers: `User-Agent: glass-terminal`, `Accept: application/vnd.github.v3+json`
- Behavior: Runs on a background thread at startup; non-fatal on failure
- Response parsing: Extracts `tag_name` for semver comparison, `assets[].browser_download_url` for platform-specific installer download

## Data Storage

**Databases:**
- SQLite (bundled via `rusqlite` with `bundled` feature - no external SQLite installation needed)
  - History DB: `{project}/.glass/history.db`
    - Client: `rusqlite` 0.38.0 (`crates/glass_history/src/db.rs`)
    - Mode: WAL journal, `PRAGMA synchronous = NORMAL`, `PRAGMA busy_timeout = 5000`
    - Schema: `commands` table with FTS5 full-text search, `pipe_stages` table
    - Migrations: Version-tracked via `PRAGMA user_version` (current version: 2)
  - Snapshot DB: `{project}/.glass/snapshots.db`
    - Client: `rusqlite` 0.38.0 (`crates/glass_snapshot/src/db.rs`)
    - Mode: WAL journal, same pragmas as history DB
    - Schema: `snapshots` and `snapshot_files` tables with foreign key constraints
    - Migrations: Version-tracked via `PRAGMA user_version` (current version: 1)

**File Storage:**
- Content-addressed blob store at `{project}/.glass/blobs/`
  - Hash algorithm: BLAKE3 (`blake3` 1.8.3)
  - Implementation: `crates/glass_snapshot/src/blob_store.rs`
  - Purpose: Stores pre-command file states for undo capability
  - Pruning: `crates/glass_snapshot/src/pruner.rs` with configurable retention

**Caching:**
- Glyph cache for GPU text rendering (in-memory)
  - Implementation: `crates/glass_renderer/src/glyph_cache.rs`
  - No persistent disk cache

## Model Context Protocol (MCP)

**MCP Server:**
- Purpose: Exposes Glass terminal data to AI assistants (e.g., Claude)
- SDK: `rmcp` 1 with `server` and `transport-io` features
- Transport: stdio (JSON-RPC 2.0 over stdin/stdout)
- Implementation: `crates/glass_mcp/src/lib.rs`, `crates/glass_mcp/src/tools.rs`
- Invocation: `glass mcp serve` CLI subcommand
- Exposed tools:
  - `GlassHistory` - Query command history with filters (text, time, exit code, cwd, limit)
  - `GlassContext` - Activity summary with command counts, failure rate, directories
  - `GlassUndo` - Undo file-modifying commands by restoring pre-command snapshots
  - `GlassFileDiff` - Inspect pre-command file contents for a given command

## Terminal / PTY

**PTY Backend:**
- Provider: `alacritty_terminal` =0.25.1 (exact version pin)
- Implementation: `crates/glass_terminal/src/pty.rs`
- I/O polling: `polling` 3 crate for cross-platform non-blocking PTY reads
- VT parsing: `vte` 0.15 for escape sequence processing
- Platform tokens differ: Windows uses token 2/1, Unix uses 0/1

**Shell Integration:**
- OSC escape sequence scanning for shell events (`crates/glass_terminal/src/osc_scanner.rs`)
- OSC 7 file:// URL parsing for working directory tracking (`url` 2 crate)
- Git status querying from shell context (`glass_terminal::query_git_status`)

## GPU / Graphics

**GPU Backend:**
- API: wgpu 28.0.0 (WebGPU abstraction)
- Platform backends:
  - Windows: DirectX 12 (explicitly selected)
  - macOS: Metal (via `wgpu::Backends::all()`)
  - Linux: Vulkan (via `wgpu::Backends::all()`)
- Power preference: `HighPerformance` (discrete GPU preferred)
- Implementation: `crates/glass_renderer/src/surface.rs`

## Clipboard

**System Clipboard:**
- Provider: `arboard` 3
- Usage: Copy/paste in terminal (`crates/glass_terminal/`)
- Cross-platform (Windows, macOS, Linux)

## Filesystem Watching

**Config Hot-Reload:**
- Provider: `notify` 8.0
- Implementation: `crates/glass_core/src/config_watcher.rs`
- Watches parent directory of `~/.glass/config.toml` (non-recursive) for atomic save support

**Snapshot File Watching:**
- Provider: `notify` 8.2
- Implementation: `crates/glass_snapshot/src/watcher.rs`
- Purpose: Detects file changes for snapshot tracking
- Gitignore-aware: Uses `ignore` 0.4 crate for respecting `.gitignore` rules

## Authentication & Identity

**Auth Provider:**
- None. Glass is a local desktop application with no user accounts or authentication.

## Monitoring & Observability

**Structured Logging:**
- Framework: `tracing` 0.1.44 + `tracing-subscriber` 0.3
- Filter: Environment-based via `env-filter` feature (`RUST_LOG` env var)
- All crates use `tracing` for structured log output

**Performance Profiling:**
- Optional Chrome trace output via `tracing-chrome` 0.7 (behind `perf` feature flag)
- Memory usage tracking via `memory-stats` 1.2

**Error Tracking:**
- None (no external error tracking service)

## CI/CD & Deployment

**Hosting:**
- GitHub repository at `nkngu/Glass`
- Releases published as GitHub Releases with platform-specific installers

**CI Pipeline:**
- GitHub Actions (`.github/workflows/ci.yml`)
  - Build: 3 platforms (Windows, macOS, Linux)
  - Test: `cargo test --workspace`
  - Lint: `cargo clippy --workspace -- -D warnings`
  - Format: `cargo fmt --all -- --check`

**Release Pipeline:**
- GitHub Actions (`.github/workflows/release.yml`)
- Trigger: Git tag push matching `v*`
- Outputs: MSI (Windows via cargo-wix), DMG (macOS via build script), DEB (Linux via cargo-deb)
- Version verification: Tag must match `Cargo.toml` version

## Environment Configuration

**Required env vars:**
- None required for normal operation

**Optional env vars:**
- `RUST_LOG` - Controls tracing log level filter

**User config file:**
- `~/.glass/config.toml` - Font, shell, history, snapshot, pipes settings

**Secrets:**
- No secrets required. GitHub API access is unauthenticated.

## Webhooks & Callbacks

**Incoming:**
- None

**Outgoing:**
- None

---

*Integration audit: 2026-03-08*
