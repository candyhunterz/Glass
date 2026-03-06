# Phase 19: MCP + Config + Polish - Research

**Researched:** 2026-03-06
**Domain:** MCP tool registration (rmcp), TOML config extension, SQL aggregate queries
**Confidence:** HIGH

## Summary

Phase 19 is the final phase of the v1.3 Pipe Visualization milestone. It adds two new MCP capabilities and a configuration section. All three requirements build on well-established patterns already present in the codebase:

1. **MCP-01 (GlassPipeInspect):** A new MCP tool that retrieves pipe stage data from the `pipe_stages` database table. The existing `glass_history::db::get_pipe_stages(command_id)` method already returns `Vec<PipeStageRow>` ordered by stage index. The new tool needs a params struct, a handler function following the exact same `#[tool]` + `spawn_blocking` + `HistoryDb::open` pattern as the four existing tools, and optional stage-level filtering.

2. **MCP-02 (GlassContext pipeline stats):** Extends the existing `ContextSummary` struct and `build_context_summary()` function with three new SQL aggregate fields computed from the `pipe_stages` table. The existing function already queries `commands` with an optional `after` filter -- the new queries follow the same pattern with a JOIN to `pipe_stages`.

3. **CONF-01 ([pipes] config section):** Adds a `PipesSection` to `GlassConfig`, following the identical `Option<PipesSection>` pattern used by `HistorySection` and `SnapshotSection`. The three config fields (`enabled`, `max_capture_mb`, `auto_expand`) must be wired into: (a) the pipeline capture path in `main.rs`, (b) the `BufferPolicy` construction, and (c) the auto-expand logic in `BlockManager`.

**Primary recommendation:** Implement as a single plan with three sequential tasks (MCP tool, context stats, config section) since they are independent and each is small (~50-100 lines of new code).

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| MCP-01 | `GlassPipeInspect(command_id, stage)` MCP tool returns intermediate output for a pipeline stage | Existing `get_pipe_stages()` DB method, rmcp `#[tool]` macro pattern from tools.rs, PipeStageRow serialization |
| MCP-02 | `GlassContext` tool updated to include pipeline stats in activity summaries | Existing `build_context_summary()` in context.rs, SQL JOIN to pipe_stages table, ContextSummary struct extension |
| CONF-01 | `[pipes]` section in config.toml with enabled, max_capture_mb, and auto_expand settings | Existing `GlassConfig` pattern with `Option<Section>`, `#[serde(default)]`, config consumption in main.rs |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| rmcp | 1.x | MCP server framework | Already used for all 4 existing tools; `#[tool]` macro generates JSON schema |
| schemars | 1.0 | JSON Schema generation for MCP params | Already used for all param structs |
| rusqlite | 0.38.0 | SQLite queries for pipe_stages | Already used throughout glass_history |
| toml + serde | 1.0.x | Config deserialization | Already used for GlassConfig |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| serde_json | 1.0 | JSON response construction | For MCP tool responses, already in glass_mcp deps |
| tokio | 1.50.0 | Async runtime for spawn_blocking | MCP handlers run DB ops on blocking thread pool |

### Alternatives Considered
None -- all libraries are already in use. No new dependencies needed.

## Architecture Patterns

### Recommended Changes Structure
```
crates/glass_mcp/src/tools.rs       # Add PipeInspectParams + glass_pipe_inspect handler
crates/glass_mcp/src/context.rs     # Add pipeline stats fields to ContextSummary + SQL
crates/glass_core/src/config.rs     # Add PipesSection to GlassConfig
src/main.rs                         # Wire config.pipes into capture + auto-expand paths
```

### Pattern 1: MCP Tool Registration (rmcp #[tool] macro)
**What:** Each MCP tool is a method on `GlassServer` annotated with `#[tool(description = "...")]` inside a `#[tool_router] impl` block.
**When to use:** Adding any new MCP tool.
**Example:**
```rust
// Source: crates/glass_mcp/src/tools.rs (existing pattern)
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PipeInspectParams {
    #[schemars(description = "The command ID to inspect pipe stages for")]
    pub command_id: i64,
    #[schemars(description = "Optional stage index (0-based). If omitted, returns all stages")]
    pub stage: Option<i64>,
}

#[tool(description = "Inspect intermediate output from a pipeline stage. Returns captured output for each pipe stage of a command.")]
async fn glass_pipe_inspect(
    &self,
    Parameters(params): Parameters<PipeInspectParams>,
) -> Result<CallToolResult, McpError> {
    let db_path = self.db_path.clone();
    let result = tokio::task::spawn_blocking(move || {
        let db = HistoryDb::open(&db_path).map_err(internal_err)?;
        db.get_pipe_stages(params.command_id).map_err(internal_err)
    })
    .await
    .map_err(internal_err)??;
    // ... filter by stage index if provided, serialize to JSON
}
```

### Pattern 2: Context Summary Extension
**What:** Add new fields to the `ContextSummary` struct and new SQL queries to `build_context_summary()`.
**When to use:** Enriching the GlassContext MCP response.
**Example:**
```rust
// Source: crates/glass_mcp/src/context.rs (extension pattern)
pub struct ContextSummary {
    // ... existing fields ...
    /// Number of commands that had pipeline stages.
    pub pipeline_count: i64,
    /// Average number of stages across pipeline commands.
    pub avg_pipeline_stages: f64,
    /// Failure rate of pipeline commands (non-zero exit code).
    pub pipeline_failure_rate: f64,
}

// SQL query pattern (with optional WHERE started_at >= ?1):
// SELECT COUNT(DISTINCT ps.command_id),
//        CAST(COUNT(*) AS REAL) / NULLIF(COUNT(DISTINCT ps.command_id), 0)
// FROM pipe_stages ps
// JOIN commands c ON c.id = ps.command_id
// [WHERE c.started_at >= ?1]
```

### Pattern 3: Config Section with Defaults
**What:** A TOML section struct with `#[serde(default = "fn")]` on each field, wrapped in `Option<T>` on `GlassConfig`.
**When to use:** Adding any new config section.
**Example:**
```rust
// Source: crates/glass_core/src/config.rs (existing pattern)
/// Pipe visualization configuration in the `[pipes]` TOML section.
#[derive(Debug, Clone, Deserialize)]
pub struct PipesSection {
    #[serde(default = "default_pipes_enabled")]
    pub enabled: bool,           // default: true
    #[serde(default = "default_max_capture_mb")]
    pub max_capture_mb: u32,     // default: 10
    #[serde(default = "default_auto_expand")]
    pub auto_expand: bool,       // default: true
}
```

### Pattern 4: Config Consumption in main.rs
**What:** Access `self.config.pipes` in the event loop to control capture and auto-expand behavior.
**When to use:** Wiring config values into runtime behavior.
**Key integration points:**
1. **Capture skip:** In the `CommandExecuted` or `PipelineStage` handler, check `config.pipes.enabled` -- if false, skip pipeline rewriting entirely (or skip reading temp files)
2. **Buffer policy:** Replace `BufferPolicy::default()` at line 814 with `BufferPolicy::new(max_capture_mb * 1024 * 1024, ...)` using config value
3. **Auto-expand:** In `BlockManager::handle_event` for `CommandFinished`, check the auto_expand config setting to override the `pipeline_expanded = failed || stage_count > 2` logic

**Important design decision for auto_expand wiring:** The `BlockManager` currently has no access to config. Two options:
- (a) Pass an auto_expand flag through the `handle_event` call or store it on `BlockManager`
- (b) Override the auto-expand result in `main.rs` after `handle_event` returns

Option (b) is simpler and maintains the existing architecture where `main.rs` is the integration point and `BlockManager` is config-agnostic.

### Anti-Patterns to Avoid
- **Adding glass_core dependency to glass_mcp:** The MCP crate should not depend on glass_core for config. Config wiring belongs in main.rs.
- **Querying pipe_stages in a loop per command:** Use a single aggregate JOIN query for pipeline stats, not N+1 queries.
- **Making auto_expand a tri-state (auto/always/never):** Requirements specify a boolean, keep it simple.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| MCP tool schema | Manual JSON Schema definition | `schemars::JsonSchema` derive on params struct | Automatic schema generation, type-safe, already used |
| SQL pipeline stats | Multiple sequential queries | Single aggregate JOIN query | Atomic, faster, simpler |
| Config defaults | Manual if-let chains | `#[serde(default = "fn")]` | Handles missing fields, partial configs, malformed TOML gracefully |

**Key insight:** Every pattern needed for this phase already exists in the codebase. The work is extension, not invention.

## Common Pitfalls

### Pitfall 1: N+1 Query for Pipeline Stats
**What goes wrong:** Querying commands first, then pipe_stages per command to compute stats.
**Why it happens:** Instinct to reuse existing `get_pipe_stages()` method.
**How to avoid:** Write a single aggregate SQL query with a JOIN: `SELECT COUNT(DISTINCT ps.command_id), ... FROM pipe_stages ps JOIN commands c ON c.id = ps.command_id`.
**Warning signs:** More than one SQL query for the pipeline stats section.

### Pitfall 2: Division by Zero in Pipeline Stats
**What goes wrong:** `avg_pipeline_stages` divides by pipeline_count which could be 0.
**Why it happens:** No pipeline commands in the time window.
**How to avoid:** Use `NULLIF(COUNT(DISTINCT ps.command_id), 0)` in SQL, and default to 0.0 in Rust.
**Warning signs:** NaN or Infinity in JSON response.

### Pitfall 3: Config Not Wired to BufferPolicy
**What goes wrong:** Config `max_capture_mb` is parsed but `BufferPolicy::default()` is still used in main.rs line 814.
**Why it happens:** Config is on `Processor`, but the pipeline stage reading code doesn't reference it.
**How to avoid:** Explicitly replace `BufferPolicy::default()` with config-driven construction.
**Warning signs:** Changing `max_capture_mb` in config.toml has no effect.

### Pitfall 4: Pipeline Failure Rate Counting Non-Pipeline Commands
**What goes wrong:** Computing pipeline failure rate from all commands, not just those with pipe stages.
**Why it happens:** Reusing the existing `failure_count / command_count` pattern.
**How to avoid:** Only count commands that have entries in `pipe_stages` for the pipeline-specific stats.
**Warning signs:** Pipeline failure rate matches overall failure rate.

### Pitfall 5: Stage Index Off-by-One
**What goes wrong:** `stage` parameter in GlassPipeInspect is 1-based but data is 0-based, or vice versa.
**Why it happens:** User-facing vs internal index mismatch.
**How to avoid:** Document the parameter as "0-based stage index" in the schemars description and filter using direct equality with `stage_index`.
**Warning signs:** Stage 0 returns no data, or requesting the last stage returns nothing.

### Pitfall 6: auto_expand Config Disabling But Not Respecting Existing Logic
**What goes wrong:** Setting `auto_expand = false` prevents ALL auto-expansion, even on failures.
**Why it happens:** Simple boolean check replacing the nuanced `failed || stage_count > 2` logic.
**How to avoid:** When `auto_expand = false`, set `pipeline_expanded = false` unconditionally. When `auto_expand = true` (default), keep existing logic. This matches the requirement: users who set it to false want collapsed pipelines always.
**Warning signs:** Failed pipelines still auto-expand when auto_expand is false.

## Code Examples

### GlassPipeInspect Response Format
```rust
// Each stage serialized as:
#[derive(Debug, Serialize)]
pub struct PipeStageEntry {
    pub stage_index: i64,
    pub command: String,
    pub output: Option<String>,
    pub total_bytes: i64,
    pub is_binary: bool,
    pub is_sampled: bool,
}

// Response shape:
// { "command_id": 42, "stages": [ { "stage_index": 0, ... }, ... ] }
// Or with stage filter:
// { "command_id": 42, "stage": { "stage_index": 1, ... } }
```

### Pipeline Stats SQL Query
```sql
-- Single aggregate query for pipeline stats (with optional time filter)
SELECT
    COUNT(DISTINCT ps.command_id) as pipeline_count,
    CAST(COUNT(*) AS REAL) / NULLIF(COUNT(DISTINCT ps.command_id), 0) as avg_stages,
    CAST(SUM(CASE WHEN c.exit_code != 0 THEN 1 ELSE 0 END) AS REAL)
        / NULLIF(COUNT(DISTINCT ps.command_id), 0) as failure_rate
FROM pipe_stages ps
JOIN commands c ON c.id = ps.command_id
WHERE c.started_at >= ?1  -- omit WHERE clause when no time filter
```

**Note on failure_rate:** The SUM/COUNT approach above counts each stage row as a failure if exit_code != 0, which overcounts. A correct approach uses a subquery or `COUNT(DISTINCT CASE ...)`:
```sql
SELECT
    COUNT(DISTINCT ps.command_id) as pipeline_count,
    CAST(COUNT(*) AS REAL) / NULLIF(COUNT(DISTINCT ps.command_id), 0) as avg_stages
FROM pipe_stages ps
JOIN commands c ON c.id = ps.command_id
WHERE c.started_at >= ?1;

-- Separate query or subquery for failure count:
SELECT COUNT(DISTINCT ps.command_id)
FROM pipe_stages ps
JOIN commands c ON c.id = ps.command_id
WHERE c.exit_code != 0 AND c.started_at >= ?1;
```

### Config TOML Example
```toml
[pipes]
enabled = true
max_capture_mb = 10
auto_expand = true
```

### Config Wiring in main.rs
```rust
// At pipeline stage read (line ~814):
let max_bytes = self.config.pipes.as_ref()
    .map(|p| (p.max_capture_mb as usize) * 1024 * 1024)
    .unwrap_or(10 * 1024 * 1024);
let policy = glass_pipes::BufferPolicy::new(max_bytes, 512 * 1024);
let mut stage_buf = glass_pipes::StageBuffer::new(policy);

// At auto-expand (after handle_event for CommandFinished):
let auto_expand = self.config.pipes.as_ref()
    .map(|p| p.auto_expand)
    .unwrap_or(true);
if !auto_expand {
    if let Some(block) = ctx.block_manager.current_block_mut() {
        block.pipeline_expanded = false;
    }
}

// At capture decision (before pipeline rewriting):
let pipes_enabled = self.config.pipes.as_ref()
    .map(|p| p.enabled)
    .unwrap_or(true);
// If !pipes_enabled, skip reading temp files / processing stages
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| N/A | All 4 MCP tools use rmcp `#[tool]` macro | Phase 9 (v1.1) | Follow same pattern for 5th tool |
| No pipe_stages table | pipe_stages with ON DELETE CASCADE | Phase 18 | DB methods already exist |
| Hardcoded BufferPolicy | BufferPolicy::default() in main.rs | Phase 16 | Ready to replace with config-driven |

**No deprecated patterns detected.** All existing infrastructure is current.

## Open Questions

1. **Should `pipes.enabled = false` retroactively hide existing pipeline UI?**
   - What we know: Currently, `enabled` would only affect future captures. Existing pipeline data in blocks and DB would persist.
   - What's unclear: Should the UI hide pipeline rows when `enabled = false`?
   - Recommendation: `enabled = false` only disables future capture (no temp file reading, no tee rewriting). Existing pipeline data still renders. This is consistent with how `snapshot.enabled` works.

2. **Pipeline failure rate: per-pipeline or per-stage?**
   - What we know: The requirement says "failure rate." A pipeline has one exit code (from the last stage, or PIPESTATUS). Pipe stages do not have individual exit codes.
   - What's unclear: Whether "failure rate" means "percentage of pipeline commands that failed."
   - Recommendation: Count distinct pipeline commands with non-zero exit_code / total distinct pipeline commands. This is the most intuitive interpretation.

3. **Where does `pipes.enabled` intercept?**
   - What we know: Pipeline rewriting happens in bash/zsh shell scripts, and temp file reading happens in main.rs.
   - What's unclear: Config is loaded at Glass startup. Shell scripts are static (installed once). Config cannot easily disable shell-side rewriting at runtime.
   - Recommendation: `enabled = false` skips the temp file reading and stage buffer processing in main.rs. Shell scripts still emit OSC sequences, but Glass ignores them. This is the simplest approach and avoids modifying shell scripts dynamically.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (Rust built-in) |
| Config file | Cargo.toml workspace + per-crate Cargo.toml |
| Quick run command | `cargo test -p glass_mcp` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| MCP-01 | GlassPipeInspect returns stage output | unit | `cargo test -p glass_mcp -- pipe_inspect` | Wave 0 |
| MCP-01 | PipeInspectParams deserializes correctly | unit | `cargo test -p glass_mcp -- pipe_inspect_params` | Wave 0 |
| MCP-01 | Stage filter returns single stage | unit | `cargo test -p glass_mcp -- pipe_inspect` | Wave 0 |
| MCP-01 | No stages returns empty/message | unit | `cargo test -p glass_mcp -- pipe_inspect` | Wave 0 |
| MCP-02 | ContextSummary includes pipeline stats | unit | `cargo test -p glass_mcp -- context` | Wave 0 |
| MCP-02 | Pipeline stats are zero for empty DB | unit | `cargo test -p glass_mcp -- context` | Wave 0 |
| MCP-02 | Pipeline stats with time filter | unit | `cargo test -p glass_mcp -- context` | Wave 0 |
| MCP-02 | Avg stages / failure rate correct | unit | `cargo test -p glass_mcp -- context` | Wave 0 |
| CONF-01 | PipesSection parses from TOML | unit | `cargo test -p glass_core -- pipes` | Wave 0 |
| CONF-01 | Empty [pipes] section uses defaults | unit | `cargo test -p glass_core -- pipes` | Wave 0 |
| CONF-01 | Partial [pipes] fields use defaults | unit | `cargo test -p glass_core -- pipes` | Wave 0 |
| CONF-01 | Missing [pipes] section = None | unit | `cargo test -p glass_core -- pipes` | Wave 0 |
| CONF-01 | max_capture_mb wired to BufferPolicy | integration | manual / `cargo test -p glass` | Wave 0 |
| CONF-01 | auto_expand=false disables auto-expand | integration | manual / `cargo test -p glass` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_mcp && cargo test -p glass_core`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full workspace test suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_mcp/src/tools.rs` -- add PipeInspectParams and glass_pipe_inspect handler tests
- [ ] `crates/glass_mcp/src/context.rs` -- add pipeline_count/avg_stages/failure_rate tests to existing test module
- [ ] `crates/glass_core/src/config.rs` -- add PipesSection tests to existing test module
- [ ] No new test files needed -- all tests go in existing `#[cfg(test)] mod tests` blocks

## Sources

### Primary (HIGH confidence)
- `crates/glass_mcp/src/tools.rs` -- existing MCP tool pattern (4 tools, all using identical `#[tool]` + `spawn_blocking` pattern)
- `crates/glass_mcp/src/context.rs` -- existing ContextSummary struct and build_context_summary() SQL pattern
- `crates/glass_history/src/db.rs` -- existing `get_pipe_stages(command_id)` and `PipeStageRow` type
- `crates/glass_core/src/config.rs` -- existing GlassConfig with HistorySection and SnapshotSection patterns
- `src/main.rs` -- existing config consumption (lines 250, 814, 873) and pipeline stage wiring (lines 988-1024)
- `crates/glass_terminal/src/block_manager.rs` -- auto-expand logic at line 180

### Secondary (MEDIUM confidence)
- rmcp crate documentation for `#[tool]`, `#[tool_router]`, `#[tool_handler]` macros (verified by examining working code)

### Tertiary (LOW confidence)
None -- all findings are from direct codebase inspection.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - all libraries already in use, no new dependencies
- Architecture: HIGH - all patterns directly copied from existing codebase
- Pitfalls: HIGH - derived from concrete code analysis (division by zero, N+1, off-by-one)
- Config wiring: HIGH - follows exact same pattern as history and snapshot config sections

**Research date:** 2026-03-06
**Valid until:** 2026-04-06 (stable patterns, no external dependency changes expected)
