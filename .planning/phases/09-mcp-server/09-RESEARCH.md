# Phase 9: MCP Server - Research

**Researched:** 2026-03-05
**Domain:** MCP protocol / JSON-RPC 2.0 over stdio / rmcp Rust SDK
**Confidence:** HIGH

## Summary

Phase 9 implements an MCP (Model Context Protocol) server that exposes Glass terminal history to AI assistants via two tools: GlassHistory (filtered command query) and GlassContext (activity summary). The server runs over stdio using JSON-RPC 2.0, launched by `glass mcp serve`.

The project has already decided to use **rmcp** (the official Rust MCP SDK) rather than hand-rolling JSON-RPC. The `glass_mcp` crate exists as a stub, and the `glass mcp serve` subcommand is already routed in `main.rs` (currently prints "not yet implemented"). The `glass_history` crate provides all database query functionality needed -- `HistoryDb`, `QueryFilter`, `filtered_query()`, and `CommandRecord` are fully implemented and tested.

**Primary recommendation:** Implement two rmcp tools (`GlassHistory`, `GlassContext`) in the `glass_mcp` crate using the `#[tool]` / `#[tool_router]` / `#[tool_handler]` macro pattern, with a thin `run_mcp_server()` async entry point called from `main.rs`. All logging must go to stderr; stdout is reserved for JSON-RPC messages.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| MCP-01 | `glass mcp serve` runs an MCP server over stdio (JSON-RPC 2.0) | rmcp provides `stdio()` transport and `ServerHandler` trait; subcommand routing already exists in main.rs |
| MCP-02 | GlassHistory tool: query commands with filters (text, timeframe, status, cwd, limit) | `QueryFilter` and `HistoryDb::filtered_query()` already implement all filter logic; wrap as rmcp `#[tool]` |
| MCP-03 | GlassContext tool: returns high-level activity summary (command count, failures, files modified, time range) | Requires new aggregate SQL queries on the commands table; expose as rmcp `#[tool]` returning structured JSON |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| rmcp | 0.11+ | Official Rust MCP SDK | Project decision (STATE.md); provides #[tool] macros, ServerHandler trait, stdio transport |
| tokio | 1.50 (workspace) | Async runtime for rmcp | rmcp requires tokio; already in workspace |
| schemars | 1.0 | JSON Schema generation for tool parameters | rmcp uses schemars to auto-generate tool parameter schemas for AI discovery |
| serde | 1.0.228 (workspace) | Serialization for tool params/results | Already in workspace |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| serde_json | 1.0 | JSON serialization for tool results | Content::json() for structured tool responses |
| chrono | 0.4 (workspace) | Time parsing for timeframe filters | Already in workspace, reuse parse_time() from glass_history |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| rmcp | Hand-rolled JSON-RPC | Rejected per project decision -- rmcp handles protocol compliance, schema generation, error formatting |

**Installation (glass_mcp/Cargo.toml):**
```toml
[dependencies]
rmcp = { version = "0.11", features = ["server", "transport-io"] }
tokio = { workspace = true }
serde = { workspace = true }
serde_json = "1.0"
schemars = "1.0"
glass_history = { path = "../glass_history" }
chrono = { workspace = true }
tracing = { workspace = true }
anyhow = { workspace = true }
```

**Root binary (Cargo.toml addition):**
```toml
glass_mcp = { path = "crates/glass_mcp" }
tokio = { workspace = true }  # already present
```

## Architecture Patterns

### Recommended Project Structure
```
crates/glass_mcp/
  src/
    lib.rs           # pub fn run_mcp_server() entry point
    tools.rs         # GlassServer struct with #[tool_router], tool impls
    context.rs       # GlassContext aggregate query logic
```

### Pattern 1: rmcp Tool Router
**What:** Struct-based tool routing using rmcp macros
**When to use:** All MCP tool definitions
**Example:**
```rust
// Source: https://www.shuttle.dev/blog/2025/07/18/how-to-build-a-stdio-mcp-server-in-rust
use rmcp::{
    handler::server::tool::ToolRouter,
    model::{CallToolResult, Content, Implementation, ProtocolVersion,
            ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, ServerHandler,
    ErrorData as McpError,
    ServiceExt, transport::stdio,
};
use serde::Deserialize;

#[derive(Clone)]
pub struct GlassServer {
    tool_router: ToolRouter<Self>,
    db_path: std::path::PathBuf,
}

#[tool_router]
impl GlassServer {
    pub fn new(db_path: std::path::PathBuf) -> Self {
        Self {
            tool_router: Self::tool_router(),
            db_path,
        }
    }

    #[tool(description = "Query Glass terminal command history with filters")]
    async fn glass_history(
        &self,
        Parameters(params): Parameters<HistoryParams>,
    ) -> Result<CallToolResult, McpError> {
        // Build QueryFilter from params, query db, return JSON
    }

    #[tool(description = "Get a summary of recent terminal activity")]
    async fn glass_context(
        &self,
        Parameters(params): Parameters<ContextParams>,
    ) -> Result<CallToolResult, McpError> {
        // Aggregate queries, return summary JSON
    }
}

#[tool_handler]
impl ServerHandler for GlassServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            server_info: Implementation::new("glass-mcp", env!("CARGO_PKG_VERSION")),
            instructions: Some(
                "Glass terminal history server. Tools: glass_history, glass_context".into()
            ),
        }
    }
}
```

### Pattern 2: Parameter Types with schemars
**What:** Derive schemars::JsonSchema on tool parameter structs for auto-schema generation
**When to use:** Every tool parameter struct
**Example:**
```rust
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HistoryParams {
    #[schemars(description = "Search text to filter commands")]
    pub text: Option<String>,
    #[schemars(description = "Only commands after this time (e.g. '1h', '2d', '2024-01-15')")]
    pub after: Option<String>,
    #[schemars(description = "Only commands before this time")]
    pub before: Option<String>,
    #[schemars(description = "Filter by exit code (0 for success)")]
    pub exit_code: Option<i32>,
    #[schemars(description = "Filter by working directory prefix")]
    pub cwd: Option<String>,
    #[schemars(description = "Maximum number of results (default 25)")]
    pub limit: Option<usize>,
}
```

### Pattern 3: Blocking DB in Async Context
**What:** HistoryDb uses rusqlite (synchronous). Use `tokio::task::spawn_blocking` or open DB per-request.
**When to use:** All tool handler methods that query the database
**Example:**
```rust
#[tool(description = "Query command history")]
async fn glass_history(
    &self,
    Parameters(params): Parameters<HistoryParams>,
) -> Result<CallToolResult, McpError> {
    let db_path = self.db_path.clone();
    let result = tokio::task::spawn_blocking(move || {
        let db = HistoryDb::open(&db_path)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let filter = build_filter(params);
        db.filtered_query(&filter)
            .map_err(|e| McpError::internal_error(e.to_string(), None))
    }).await
    .map_err(|e| McpError::internal_error(e.to_string(), None))??;

    let content = Content::json(&result)
        .map_err(|e| McpError::internal_error(e.to_string(), None))?;
    Ok(CallToolResult::success(vec![content]))
}
```

### Pattern 4: Stderr-Only Logging
**What:** All tracing/logging must go to stderr to avoid corrupting the JSON-RPC stdio channel
**When to use:** MCP server entry point
**Example:**
```rust
pub async fn run_mcp_server() -> anyhow::Result<()> {
    // Logging MUST go to stderr -- stdout is JSON-RPC
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let db_path = glass_history::resolve_db_path(
        &std::env::current_dir().unwrap_or_default()
    );
    let server = GlassServer::new(db_path);
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
```

### Anti-Patterns to Avoid
- **println! or stdout logging in MCP mode:** Any non-JSON-RPC bytes on stdout corrupt the protocol. Use eprintln! or tracing with stderr writer.
- **Holding HistoryDb across await points:** rusqlite Connection is not Send. Open per-request in spawn_blocking or use a pool.
- **Returning raw text instead of structured JSON:** AI assistants work better with structured Content::json() responses, not Content::text() dumps.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| JSON-RPC 2.0 protocol | Custom JSON-RPC parser/dispatcher | rmcp | MCP has precise protocol requirements (initialize handshake, capabilities negotiation, error codes); rmcp handles all of this |
| Tool parameter schemas | Manual JSON Schema construction | schemars derive macro | Auto-generates correct JSON Schema 2020-12 from Rust types |
| Tool dispatch/routing | match-based tool name dispatcher | rmcp #[tool_router] macro | Handles tool listing, schema reporting, parameter validation, error formatting |
| Time string parsing | New parser for MCP timeframe params | glass_history::query::parse_time() | Already handles relative (1h, 2d) and ISO formats |

**Key insight:** rmcp eliminates 90% of protocol boilerplate. The implementation is essentially: define parameter structs, write query logic, return Content::json().

## Common Pitfalls

### Pitfall 1: Stdout Corruption
**What goes wrong:** Any output to stdout that isn't valid JSON-RPC breaks the MCP client connection.
**Why it happens:** Default tracing subscriber, println! debugging, or library code writing to stdout.
**How to avoid:** Initialize tracing with `.with_writer(std::io::stderr)` BEFORE serving. Remove all println! calls. Set tracing init in run_mcp_server() not main.rs (main.rs already has its own tracing init for terminal mode).
**Warning signs:** MCP client disconnects immediately or reports "parse error".

### Pitfall 2: Blocking the Tokio Runtime
**What goes wrong:** rusqlite operations block the async runtime, causing timeout on concurrent requests.
**Why it happens:** HistoryDb::open() and filtered_query() are synchronous.
**How to avoid:** Wrap all DB operations in `tokio::task::spawn_blocking()`. Open a fresh connection per request (SQLite WAL mode handles concurrent readers fine).
**Warning signs:** MCP requests timeout or server becomes unresponsive.

### Pitfall 3: Double Tracing Init
**What goes wrong:** main.rs already calls `tracing_subscriber::fmt().init()`. If glass_mcp also calls `.init()`, it panics ("a global default subscriber has already been set").
**Why it happens:** The MCP path in main.rs must use a DIFFERENT tracing setup (stderr writer, no ANSI).
**How to avoid:** Move tracing init for MCP mode into the MCP branch of main.rs, BEFORE the existing terminal-mode init. The MCP branch should init tracing then call run_mcp_server(). The terminal branch keeps its existing init.
**Warning signs:** Panic on startup when running `glass mcp serve`.

### Pitfall 4: Forgetting to Serialize CommandRecord
**What goes wrong:** Content::json() requires Serialize on the data type.
**Why it happens:** CommandRecord in glass_history derives Debug + Clone but not Serialize.
**How to avoid:** Add `#[derive(Serialize)]` to CommandRecord, or create a separate MCP-specific response struct that derives Serialize. A dedicated response struct is better since it can exclude internal fields like `id`.
**Warning signs:** Compile error on Content::json().

### Pitfall 5: schemars Version Mismatch
**What goes wrong:** rmcp 0.11+ requires schemars 1.0 (not 0.8). Using the wrong version causes trait bound errors.
**Why it happens:** There are two major schemars versions with incompatible APIs.
**How to avoid:** Use `schemars = "1.0"` in Cargo.toml. The derive macro is `schemars::JsonSchema`.
**Warning signs:** Trait bound errors mentioning JsonSchema.

## Code Examples

### GlassHistory Tool Response Structure
```rust
// Response type for the GlassHistory tool
#[derive(Serialize)]
struct HistoryEntry {
    command: String,
    cwd: String,
    exit_code: Option<i32>,
    started_at: i64,
    finished_at: i64,
    duration_ms: i64,
    output_preview: Option<String>,  // truncated output
}

impl From<CommandRecord> for HistoryEntry {
    fn from(r: CommandRecord) -> Self {
        Self {
            command: r.command,
            cwd: r.cwd,
            exit_code: r.exit_code,
            started_at: r.started_at,
            finished_at: r.finished_at,
            duration_ms: r.duration_ms,
            output_preview: r.output.map(|o| {
                if o.len() > 500 { format!("{}...", &o[..500]) } else { o }
            }),
        }
    }
}
```

### GlassContext Aggregate Query
```rust
// Source: new SQL against existing commands table schema
fn build_context_summary(
    conn: &rusqlite::Connection,
    after: Option<i64>,
) -> Result<ContextSummary> {
    let time_clause = after
        .map(|t| format!("WHERE started_at >= {}", t))
        .unwrap_or_default();

    let sql = format!(
        "SELECT
            COUNT(*) as total,
            SUM(CASE WHEN exit_code != 0 THEN 1 ELSE 0 END) as failures,
            MIN(started_at) as earliest,
            MAX(finished_at) as latest
         FROM commands {}",
        time_clause
    );
    let mut stmt = conn.prepare(&sql)?;
    let summary = stmt.query_row([], |row| {
        Ok(ContextSummary {
            command_count: row.get(0)?,
            failure_count: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
            earliest_timestamp: row.get(2)?,
            latest_timestamp: row.get(3)?,
        })
    })?;

    // Get recent distinct directories
    let dir_sql = format!(
        "SELECT DISTINCT cwd FROM commands {} ORDER BY started_at DESC LIMIT 10",
        time_clause
    );
    // ...

    Ok(summary)
}
```

### Main.rs Integration
```rust
// In main.rs, replace the current MCP stub:
Some(Commands::Mcp { action: McpAction::Serve }) => {
    // MCP server mode: logging goes to stderr, stdout is JSON-RPC
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let rt = tokio::runtime::Runtime::new()
        .expect("Failed to create tokio runtime");
    if let Err(e) = rt.block_on(glass_mcp::run_mcp_server()) {
        eprintln!("MCP server error: {}", e);
        std::process::exit(1);
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Hand-rolled JSON-RPC | rmcp SDK with macros | 2025 (rmcp matured) | Eliminates protocol boilerplate, auto-generates schemas |
| schemars 0.8 | schemars 1.0 | 2025 | Breaking change; rmcp requires 1.0 |
| Custom tool dispatch | #[tool_router] macro | rmcp 0.3+ | Zero-boilerplate tool registration |

## Open Questions

1. **rmcp exact version to pin**
   - What we know: 0.11.0 works per verified blog posts; docs.rs shows 1.1.0 which may be a different release line
   - What's unclear: Whether to pin 0.11 or use a newer version
   - Recommendation: Start with `rmcp = "0.11"` (verified working); if compilation issues arise, check crates.io for latest compatible version

2. **ProtocolVersion enum variant**
   - What we know: Examples show `ProtocolVersion::V_2024_11_05` and `V_2025_06_18`
   - What's unclear: Which version rmcp 0.11 supports
   - Recommendation: Use the latest available variant; rmcp will handle protocol negotiation

3. **CommandRecord Serialize derive**
   - What we know: CommandRecord currently derives Debug + Clone, not Serialize
   - What's unclear: Whether to add Serialize to CommandRecord or create a separate response type
   - Recommendation: Create a separate `HistoryEntry` response type -- cleaner separation, can omit internal `id` field

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test + cargo test |
| Config file | Cargo.toml (workspace) |
| Quick run command | `cargo test -p glass_mcp` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| MCP-01 | `glass mcp serve` starts JSON-RPC server, completes initialize handshake | integration | `cargo test -p glass_mcp --test integration` | No -- Wave 0 |
| MCP-02 | GlassHistory tool returns filtered results as structured JSON | unit | `cargo test -p glass_mcp -- glass_history` | No -- Wave 0 |
| MCP-03 | GlassContext tool returns activity summary as structured JSON | unit | `cargo test -p glass_mcp -- glass_context` | No -- Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_mcp`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_mcp/src/lib.rs` -- module structure and run_mcp_server()
- [ ] `crates/glass_mcp/tests/` -- integration tests for MCP handshake
- [ ] Unit tests for tool parameter parsing and query building
- [ ] Unit tests for context aggregation SQL
- [ ] Serialize derive or response types for CommandRecord data

**Testing strategy notes:**
- Unit tests can test tool logic directly by calling the query/aggregation functions with a temp SQLite database (same pattern as glass_history tests)
- Integration test for MCP-01 can spawn the server as a subprocess, write JSON-RPC initialize request to stdin, and verify the response on stdout
- Alternatively, rmcp may provide test utilities for in-process server testing -- check rmcp docs during implementation

## Sources

### Primary (HIGH confidence)
- [Shuttle blog: How to Build a stdio MCP Server in Rust](https://www.shuttle.dev/blog/2025/07/18/how-to-build-a-stdio-mcp-server-in-rust) -- Complete rmcp code examples with version 0.3
- [rup12.net: Write your MCP servers in Rust](https://rup12.net/posts/write-your-mcps-in-rust/) -- rmcp 0.11 examples with full imports
- [docs.rs/rmcp](https://docs.rs/rmcp) -- Official API documentation

### Secondary (MEDIUM confidence)
- [MCP Protocol Specification](https://modelcontextprotocol.io/specification/2025-03-26/basic) -- Initialize handshake requirements
- [GitHub modelcontextprotocol/rust-sdk](https://github.com/modelcontextprotocol/rust-sdk) -- Official repository

### Tertiary (LOW confidence)
- rmcp version discrepancy (docs.rs 1.1.0 vs blog 0.11) -- needs validation during implementation

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- rmcp is the official SDK, project decision is locked, multiple verified examples
- Architecture: HIGH -- pattern is well-established (tool router + ServerHandler + stdio), glass_history API is fully tested
- Pitfalls: HIGH -- stdout corruption and blocking async are well-documented MCP server issues

**Research date:** 2026-03-05
**Valid until:** 2026-04-05 (rmcp API is stabilizing but may have minor version changes)
