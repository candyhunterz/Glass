# External Integrations

**Analysis Date:** 2026-03-15

## APIs & External Services

**Anthropic API:**
- OAuth Usage API - Monitors 5-hour and 7-day utilization for rate limit enforcement
  - Endpoint: `https://api.anthropic.com/api/oauth/usage`
  - SDK: `ureq` (sync HTTP client)
  - Auth: OAuth access token from `~/.claude/.credentials.json` → `claudeAiOauth.accessToken`
  - Header: `Authorization: Bearer {token}`, `anthropic-beta: oauth-2025-04-20`
  - Implementation: `src/usage_tracker.rs` - Polls every 60 seconds, auto-pauses orchestrator at 80%, hard-stops at 95%
  - Related: `src/main.rs` - Displays usage in status bar, pauses agent execution on thresholds

**GitHub:**
- Release Download - Checks for Glass updates
  - Repository: `https://github.com/nkngu/Glass/releases`
  - SDK: None (manual URL construction in `crates/glass_core/src/updater.rs`)
- Issue Filing - Pre-fills crash reports
  - Template: `https://github.com/candyhunterz/Glass/issues/new?title={}&body={}&labels=bug,crash`
  - Implementation: `src/main.rs` - Launches browser on panic

**Claude AI Desktop:**
- MCP Tools Exposure - Exposes Glass tools to Claude Code
  - Protocol: Model Context Protocol (MCP) over stdio
  - SDK: `rmcp` (Rust MCP library)
  - Entry Point: `crates/glass_mcp/src/lib.rs::run_mcp_server()`
  - Tools provided: `glass_query`, `glass_context`, `glass_undo`, `glass_file_diff`, `glass_pipes`, `glass_agent_*`
  - Implementation: `crates/glass_mcp/src/tools.rs` - JSON-RPC 2.0 service
  - Stderr only: Logging goes to stderr, stdout reserved for JSON-RPC
  - Related: `crates/glass_coordination/src/lib.rs` - Agent registry in `~/.glass/agents.db`

**Shell Integration:**
- OS Shells - Command boundary detection via OSC 133 sequences
  - Location: `shell-integration/` (bash, zsh, fish, PowerShell scripts)
  - Emission: Injected at PTY startup by `crates/glass_terminal/src/pty.rs`
  - Sequences: OSC 133 (command lifecycle), custom OSC 133;S (pipeline start), OSC 133;P (stage data)
  - Parser: `crates/glass_terminal/src/osc_scanner.rs`
  - Implementation: Not a network integration; shell hooks emit terminal sequences parsed in-process

## Data Storage

**Databases:**
- **Command History** (SQLite + FTS5)
  - Location: `.glass/history.db` (project-local) or `~/.glass/global-history.db` (fallback)
  - Resolved by: `crates/glass_history/src/lib.rs::resolve_db_path()`
  - Client: `rusqlite` (bundled SQLite with FTS5 extension)
  - Schema: Commands, pipe stages, output (compressed), full-text index
  - Implementation: `crates/glass_history/src/db.rs`
  - Access: All crates via `crates/glass_history` public API

- **File Snapshots** (SQLite metadata + blake3 blobs)
  - Snapshot directory: `.glass/snapshots/`
  - Metadata DB: `.glass/snapshots/meta.db` (rusqlite)
  - Blob store: Blake3 content-addressed storage in `.glass/snapshots/blobs/`
  - Implementation: `crates/glass_snapshot/src/lib.rs`, `blob_store.rs`, `undo.rs`
  - Watch trigger: Filesystem watcher (notify) on command execution

- **Agent Coordination** (SQLite WAL mode)
  - Location: `~/.glass/agents.db`
  - Resolved by: `crates/glass_coordination/src/lib.rs::resolve_db_path()`
  - Mode: WAL (write-ahead logging) for concurrent agent access
  - Tables: Agent registry, advisory locks, message queue
  - Implementation: `crates/glass_coordination/src/lib.rs`
  - Access: Via MCP tools (`glass_agent_register`, `glass_agent_lock`, `glass_agent_messages`, etc.)

**File Storage:**
- Local filesystem only
  - Config: `~/.glass/config.toml` (user-writable)
  - Shells: `shell-integration/` (read at startup)
  - Blobs: `.glass/snapshots/blobs/` (blake3 hashed)

**Caching:**
- None. All data persisted to disk immediately.
- In-memory caches: Terminal grid, PTY state (volatile)

## Authentication & Identity

**Auth Provider:**
- Custom (OAuth via Anthropic)
  - Token location: `~/.claude/.credentials.json`
  - Format: JSON with `claudeAiOauth.accessToken` field
  - Used by: `src/usage_tracker.rs`
  - Scope: Read-only usage API access
  - No refresh flow: Token assumed to be managed by Claude Desktop

**Agent Authentication:**
- No explicit auth
  - Agent registration: Name + type (e.g., `claude-code`, `cursor`) stored in `~/.glass/agents.db`
  - Lock conflicts resolved via agent messaging (advisory locks, not enforced)
  - File access: Agent has full read/write on locked files (trust model)

## Monitoring & Observability

**Error Tracking:**
- None (no Sentry/Rollbar)
- Local panic handler: Generates GitHub issue template, logs to stderr

**Logs:**
- Approach: Structured logging via `tracing` crate
  - Subscriber: `tracing-subscriber` with `env-filter`
  - Filter: `RUST_LOG` environment variable (e.g., `RUST_LOG=debug`)
  - Output: stderr (stdout reserved for terminal output)
  - Levels: trace, debug, info, warn, error
  - Special logs:
    - MCP server: Only stderr (JSON-RPC on stdout)
    - Usage tracker: Warnings at 80%, 95%
    - Orchestrator: Iteration counts, stuck detection, checkpoint cycles

**Performance:**
- Optional instrumentation via `tracing-chrome` (perf feature)
  - Exported to Chrome DevTools timeline format
  - Build with: `cargo build --features perf`

## CI/CD & Deployment

**Hosting:**
- Not a hosted service. Desktop application distributed via:
  - GitHub Releases (precompiled binaries)
  - Debian packages (via cargo-deb for Linux)
  - MSI installer (Windows, experimental)

**CI Pipeline:**
- GitHub Actions (`.github/workflows/ci.yml`)
  - Format check: ubuntu-latest
  - Clippy: windows-latest (matches dev platform)
  - Build+test matrix: Linux (x86_64), macOS (aarch64), Windows (x86_64)
  - Triggers: push to main/master, pull requests

**Release Workflow:**
- Tagging: Git tags trigger release binaries
- Artifacts: Uploaded to GitHub Releases
- Installers: MSI for Windows (experimental)

## Environment Configuration

**Required env vars (optional, system defaults if absent):**
- `RUST_LOG` - Logging filter (default: info)
- `RUST_BACKTRACE` - Backtrace on panic (1 or full)

**Optional env vars:**
- Shell override (via `~/.glass/config.toml` → `shell` field)
- Font family/size (via config.toml)

**Secrets location:**
- OAuth token: `~/.claude/.credentials.json` (managed by Claude Desktop, NOT in .env)
- No other secrets required
- Config file (`~/.glass/config.toml`) is never committed

## Webhooks & Callbacks

**Incoming:**
- None. Glass is event-driven via terminal I/O and filesystem watches, not webhooks.

**Outgoing:**
- GitHub issue auto-filing (browser launch on panic, user must confirm)
- MCP tool callbacks: Claude Code calls Glass tools via stdio (request/response pattern)

## Third-Party Services Dependencies

None. Glass is fully self-contained:
- No CDNs
- No SaaS services (except optional Anthropic API usage polling)
- No analytics
- No telemetry

## IPC & Networking

**Inter-process:**
- MCP over stdio (Glass MCP server ↔ Claude Code process)
  - No network protocol, same machine only
  - JSON-RPC 2.0 over pipe/stdio
  - Managed by `rmcp` crate

**Local networking:**
- None (not a network service)

**PTY Spawning:**
- Platform-specific: ConPTY (Windows), forkpty (Unix)
- Handled by: `crates/glass_terminal/src/pty.rs`
- No SSH or remote execution

---

*Integration audit: 2026-03-15*
