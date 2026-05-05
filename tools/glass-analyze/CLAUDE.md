# glass analyze — CLI Subcommand

## What Is This
A `glass analyze` CLI subcommand that embeds the run-analyzer dashboard in the Glass binary and serves it over localhost with auto-loaded `.glass/` data. See `PRD.md` for full spec.

## This Is a Two-Part Implementation

### Part 1: Dashboard Adapter (run-analyzer changes)
Location: `tools/run-analyzer/src/`

Add a `dataSources/` layer so the dashboard can load data from either:
- File System Access API (existing, for standalone use)
- HTTP API at `/api/files/*` (new, for `glass analyze` serving)

**Build & verify:**
```bash
cd tools/run-analyzer
npm run build
npm test
```

**Constraints:**
- DataSource interface: `listFiles()`, `readFile(name)`, `label()`
- Auto-detect on mount: try `GET /api/files`, fall back to folder picker
- Must not break standalone mode (npm run dev + folder picker)
- No new dependencies — use fetch API

### Part 2: Rust HTTP Server (Glass binary changes)
Location: `src/analyze.rs` + CLI in `src/main.rs`

**Build & verify:**
```bash
cargo build --release
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

**Constraints:**
- Use `rust-embed` to embed `tools/run-analyzer/dist/` at compile time
- Use `axum` for HTTP server (already using tokio)
- Serve static assets with correct MIME types
- Serve `.glass/` files via `/api/files` and `/api/files/<name>` routes
- Default port: 3927
- Auto-open browser (platform-specific: cmd /C start on Windows, open on macOS, xdg-open on Linux)
- Ctrl+C graceful shutdown

**CLI addition to `Commands` enum:**
```rust
/// Open the orchestrator run analyzer dashboard
Analyze {
    /// Path to .glass/ directory (default: .glass/ in CWD)
    #[arg(long)]
    dir: Option<String>,
    /// HTTP server port (default: 3927)
    #[arg(long, default_value_t = 3927)]
    port: u16,
    /// Don't auto-open browser
    #[arg(long)]
    no_open: bool,
},
```

## Build Order
1. Build Part 1 first (dashboard adapter) — `npm run build` produces updated dist/
2. Build Part 2 (Rust server) — `cargo build` embeds the new dist/
3. Test both: `glass analyze` should serve dashboard with data pre-loaded

## Important
- Do NOT add axum/rust-embed as workspace-wide dependencies — add them only to the root Cargo.toml behind a feature flag or directly
- The `tools/run-analyzer/dist/` directory must exist before `cargo build` — the embed macro fails at compile time if missing
- Keep the run-analyzer standalone mode working — it's still useful for development


## Glass Terminal Integration

Glass terminal history and context are available via MCP tools. Use `glass_history` to search past commands and output across sessions. Use `glass_context` for a summary of recent activity.
