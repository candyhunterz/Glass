# Phase 53: SOI MCP Tools - Research

**Researched:** 2026-03-13
**Domain:** Rust MCP server tool extension (glass_mcp), SQLite SOI query layer (glass_history), compression engine
**Confidence:** HIGH

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SOIM-01 | glass_query tool returns structured output by command_id/scope/file/budget with token-budgeted response | compress_output() + get_output_records() already implement the full pipeline; tool is a thin wrapper |
| SOIM-02 | glass_query_trend compares last N runs of same command pattern, detecting regressions | get_previous_run_records() + diff_compress() provide the diff engine; trend loop needs a new DB helper |
| SOIM-03 | glass_query_drill expands a specific record_id to full detail (context lines, stack trace) | output_records table has `data` JSON column with all fields; single-row fetch by id |
| SOIM-04 | glass_context and glass_compressed_context updated to include SOI summaries for recent commands | context.rs and the build_*_section helpers in tools.rs need SOI summary rows appended |
</phase_requirements>

---

## Summary

Phase 53 adds three new MCP tools (`glass_query`, `glass_query_trend`, `glass_query_drill`) and updates two existing tools (`glass_context`, `glass_compressed_context`) to surface SOI data. All the infrastructure is already built across phases 48-52: the SOI types live in `glass_soi`, the storage layer in `glass_history/soi.rs`, and the compression engine in `glass_history/compress.rs`. The MCP server pattern (`#[tool]` / `#[tool_router]` / `#[tool_handler]` from rmcp) is proven across 28 existing tools. Phase 53 is a pure integration layer — no new crates, no new DB tables, no new compression logic.

The most complex piece is `glass_query_trend`: it needs a new helper on `HistoryDb` that fetches the last N command_ids for a given command text pattern, then calls `get_output_records()` and `diff_compress()` for each consecutive pair. The STATE.md flags an open concern: "MCP tool token footprint of 25 existing tools unmeasured — audit required before Phase 53 adds more." The planner must budget for a token footprint audit in Wave 0 (measuring tool descriptions) and write tight descriptions for the three new tools.

**Primary recommendation:** Implement `glass_query`, `glass_query_trend`, and `glass_query_drill` as `spawn_blocking` closures in `tools.rs` following the existing `glass_pipe_inspect` pattern. Update `glass_context` by extending `build_context_summary()` in `context.rs` to optionally return SOI summaries, and update `glass_compressed_context` by adding a SOI section to the balanced/errors focus paths.

---

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| rmcp | (workspace) | `#[tool]`, `#[tool_router]`, `#[tool_handler]` macros | Project standard; all 28 existing tools use it |
| rusqlite | 0.38 (bundled) | Direct SQL on HistoryDb | All SOI tables live here; no new dep needed |
| serde_json | (workspace) | Deserialize `output_records.data` JSON column | Already used throughout glass_mcp |
| tokio | (workspace) | `spawn_blocking` for DB work off async executor | All existing tools use this pattern |
| schemars | (workspace) | JsonSchema derive for param structs | Required by rmcp `#[tool]` macro |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| glass_history | (workspace) | HistoryDb, compress(), diff_compress(), TokenBudget | All DB and compression work |
| glass_history::compress | (workspace) | format_record(), estimate_tokens() | Building drill-down detail text |

**Installation:** No new dependencies. All required crates are already in the workspace.

---

## Architecture Patterns

### MCP Tool Pattern (established)

Every tool follows this structure in `tools.rs`:

1. Parameter struct with `#[derive(Debug, Deserialize, schemars::JsonSchema)]` and `#[schemars(description = "...")]` on each field
2. Response/row struct with `#[derive(Debug, Serialize)]` if non-trivial
3. `#[tool(description = "...")]` async method on `GlassServer`
4. Body wraps all DB work in `tokio::task::spawn_blocking(move || { ... })`
5. Returns `Ok(CallToolResult::success(vec![Content::json(&val)?]))` on success
6. Returns `Ok(CallToolResult::error(vec![Content::text(msg)]))` on expected error
7. Propagates join errors via `.map_err(internal_err)??`

```rust
// Source: crates/glass_mcp/src/tools.rs (existing tools — e.g. glass_pipe_inspect)
#[tool(description = "...")]
async fn glass_query(
    &self,
    Parameters(params): Parameters<QueryParams>,
) -> Result<CallToolResult, McpError> {
    let db_path = self.db_path.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<serde_json::Value, McpError> {
        let db = HistoryDb::open(&db_path).map_err(internal_err)?;
        // ... DB work ...
        Ok(serde_json::json!({ ... }))
    })
    .await
    .map_err(internal_err)??;
    let content = Content::json(&result)?;
    Ok(CallToolResult::success(vec![content]))
}
```

### Recommended Project Structure for Phase 53

All changes live in existing files — no new files are strictly required. The planner may choose to extract SOI-specific helpers into a new `crates/glass_mcp/src/soi_tools.rs` module for cleanliness (the existing `context.rs` sets the precedent for helper extraction).

```
crates/glass_mcp/src/
├── tools.rs          # Add 3 new tool methods + update 2 existing
├── context.rs        # Extend build_context_summary() with optional SOI rows
└── soi_tools.rs      # (OPTIONAL) Helper: build_soi_section(), trend_rows(), etc.
crates/glass_history/src/
└── db.rs             # Add get_last_n_runs() helper for trend tool
```

### Pattern 1: glass_query (per-command SOI fetch)

**What:** Accepts `command_id`, optional `budget` string ("one_line"/"summary"/"detailed"/"full"), optional `severity` filter, optional `file` filter. Returns a `CompressedOutput` JSON blob.

**Implementation path:**
1. Parse `budget` param → `TokenBudget` enum
2. Open `HistoryDb`
3. Call `db.compress_output(command_id, budget)` — already implemented in `db.rs`
4. If `None` returned, command has no SOI data → return informative message
5. Serialize `CompressedOutput` as JSON

```rust
// Source: crates/glass_history/src/db.rs (existing)
pub fn compress_output(
    &self,
    command_id: i64,
    budget: TokenBudget,
) -> Result<Option<CompressedOutput>>
```

**Key insight:** `compress_output()` is the exact function needed. The tool is literally just parameter parsing + one function call.

### Pattern 2: glass_query_trend (multi-run regression detection)

**What:** Accepts `command` text pattern (e.g., "cargo test"), `n` (number of recent runs, default 5). Returns per-run summaries and a diff between each consecutive pair. Detects regression when previously passing tests now fail.

**New DB helper needed:** `get_last_n_runs(command_text, n)` — returns the N most recent `command_ids` for a given exact command text, ordered oldest-to-newest (so diffs show forward progression).

```sql
SELECT id FROM commands
WHERE command = ?1
ORDER BY started_at DESC
LIMIT ?2
```

Then reverse the result to oldest-first before building the diff chain.

**Implementation:**
1. Fetch last N command_ids for the command text
2. For each run: call `db.get_output_records(id, None, None, None, 10000)`
3. For each consecutive pair (prev, curr): call `diff_compress(curr_records, Some(prev_records))`
4. Collect per-run summary rows + diffs into a single JSON response
5. Detect regression: scan diffs for `new_count > 0` where `resolved_count == 0` on "TestResult" record types

**Regression detection is pattern-based:** A `TestResult` record in `new_records` whose `data` JSON has `status == "Failed"` indicates a new failure. The tool does not need special logic — the diff fingerprints (`record_type`, `severity`, `message_prefix`) will catch it. The description should instruct agents to look for `new_records` with TestResult type.

### Pattern 3: glass_query_drill (single-record expansion)

**What:** Accepts a `record_id` (integer row ID from `output_records` table). Returns the full `data` JSON for that record, including `context_lines`, `failure_message`, `failure_location`, etc.

**Implementation:** Single SQL query:
```sql
SELECT id, command_id, record_type, severity, file_path, data
FROM output_records WHERE id = ?1
```

The `data` column already stores the full JSON-serialized `OutputRecord` enum variant with all fields. No compression needed — this is the drill-down full-detail path.

**Available rich fields (from OutputRecord variants):**
- `CompilerError`: `context_lines` (surrounding source), `code` (E0308), `file`, `line`, `column`
- `TestResult`: `failure_message`, `failure_location` (file:line)
- Other variants: all fields stored in JSON

### Pattern 4: glass_context / glass_compressed_context SOI augmentation

**What:** Add a "SOI summaries" section listing the `one_line` summary and `severity` for the N most recent commands that have SOI data.

**Where to add:**
- `glass_context`: Extend `ContextSummary` struct in `context.rs` with `soi_summaries: Vec<SoiSummaryEntry>` field. `build_context_summary()` adds a JOIN query against `command_output_records`.
- `glass_compressed_context`: Add a `build_soi_section()` helper function (alongside existing `build_errors_section`, `build_history_section`, `build_files_section`) that formats SOI one_line summaries within a char budget.

**SQL for context SOI summaries:**
```sql
SELECT c.id, c.command, cor.output_type, cor.severity, cor.one_line
FROM commands c
JOIN command_output_records cor ON cor.command_id = c.id
WHERE c.started_at >= ?1
ORDER BY c.started_at DESC
LIMIT 10
```

**Anti-pattern to avoid:** Do NOT filter context SOI summaries to only failures. Include success summaries too (e.g., "247 tests passed") — agents need the full picture.

### Anti-Patterns to Avoid

- **Fetching raw output text in MCP tools:** The `output` column in `commands` table can be large. Use SOI summary rows (`command_output_records`) and compressed records instead. `glass_query` operates on `output_records`, not raw output.
- **Blocking the async executor:** All DB work MUST be in `spawn_blocking`. This is enforced by the established pattern but easy to miss on trivial queries.
- **Re-implementing TokenBudget parsing:** Parse budget string with a simple match arm in the tool, not a new type. The four strings "one_line", "summary", "detailed", "full" map directly to the enum.
- **Building trend logic in SQL:** Trend detection (new failure vs. resolved failure) should happen in Rust over the `DiffSummary` result, not in SQL. The SQL role is just to fetch ordered run IDs.
- **Adding new DB tables:** Phase 53 requires zero schema changes. All needed data is already in `command_output_records` and `output_records`.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Token-budgeted summary | Custom truncation logic | `compress_output()` in `db.rs` | Already handles 4 budget levels, priority ordering, record_ids |
| Diff detection | Manual set comparison | `diff_compress()` in `compress.rs` | Already fingerprints records, produces DiffSummary with new/resolved |
| Record formatting | Custom serializer | `format_record()` in `compress.rs` | Already produces human-readable lines from OutputRecordRow |
| Token estimation | Character counting | `estimate_tokens()` in `compress.rs` | Consistent with rest of compression engine |
| Time parsing | Custom parser | `query::parse_time()` | Already handles "1h", "2d", ISO dates across all tools |

---

## Common Pitfalls

### Pitfall 1: Tool Description Token Cost
**What goes wrong:** Adding 3 new tools with verbose descriptions inflates the tool schema sent to the LLM on every request.
**Why it happens:** rmcp sends all tool descriptions on connection. 28 existing tools' footprint is already flagged as unmeasured in STATE.md.
**How to avoid:** Write concise tool descriptions (< 40 words each). Measure the JSON schema size of the tool list before and after adding the 3 new tools. Wave 0 of the plan should include this audit.
**Warning signs:** Claude Code experiencing slower initial responses or context pressure after the phase.

### Pitfall 2: get_last_n_runs Ordering
**What goes wrong:** The trend tool returns runs newest-first (DESC) but the diff chain must be oldest-to-newest to show forward progression (run 1 → run 2 → ... → run N).
**Why it happens:** `ORDER BY started_at DESC LIMIT N` is the natural fetch order.
**How to avoid:** Reverse the result Vec in Rust after fetching: `ids.reverse()` before building the diff pairs. Tests must assert that a regression (pass → fail) is detected, not a recovery (fail → pass).

### Pitfall 3: No SOI Data Case
**What goes wrong:** `glass_query` called with a `command_id` for a command that completed before Phase 50 (SOI pipeline integration) → `get_output_summary()` returns `None` → panic or misleading empty result.
**Why it happens:** Historical commands in the DB predate SOI parsing.
**How to avoid:** Return a clear `Content::text("No SOI data for command {id}. Command may predate SOI integration.")` rather than an error or empty JSON.

### Pitfall 4: Drill Down Record ID from Wrong Phase
**What goes wrong:** An agent passes a `record_id` from `output_records` but mistakenly passes the `command_id` (the ID from `commands` table). These are different integer spaces.
**Why it happens:** Multiple "ID" fields in the system; agents may confuse them.
**How to avoid:** The tool description for `glass_query_drill` must explicitly say "record_id from the record_ids field of a glass_query response" and the response from `glass_query` must label its IDs as `record_ids` (which `CompressedOutput` already does).

### Pitfall 5: Exact Command Text Matching for Trend
**What goes wrong:** `glass_query_trend` for "cargo test" misses runs where the user typed "cargo test --workspace" or "cargo test -- test_foo".
**Why it happens:** SQL `WHERE command = ?1` is exact match.
**How to avoid:** Support GLOB/LIKE pattern matching. Use `WHERE command LIKE ?1` with `%` wildcards, OR use SQLite `GLOB`. Expose as a `pattern` param that defaults to exact match but supports `%` wildcards. This is a MUST for the trend tool to be useful.

---

## Code Examples

### glass_query implementation sketch

```rust
// Param struct
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QueryParams {
    #[schemars(description = "Command ID from glass_history results")]
    pub command_id: i64,
    #[schemars(description = "Token budget: 'one_line', 'summary', 'detailed', 'full' (default: 'summary')")]
    pub budget: Option<String>,
    #[schemars(description = "Filter by severity: 'Error', 'Warning', 'Info', 'Success'")]
    pub severity: Option<String>,
    #[schemars(description = "Filter by file path (exact match)")]
    pub file: Option<String>,
}

// Parse budget string to enum
fn parse_budget(s: Option<&str>) -> TokenBudget {
    match s.unwrap_or("summary") {
        "one_line" => TokenBudget::OneLine,
        "detailed"  => TokenBudget::Detailed,
        "full"      => TokenBudget::Full,
        _           => TokenBudget::Summary,
    }
}
```

### glass_query_trend - new DB helper needed in db.rs

```rust
// Add to HistoryDb impl in crates/glass_history/src/db.rs
pub fn get_last_n_run_ids(
    &self,
    command_pattern: &str,
    n: usize,
) -> Result<Vec<i64>> {
    let mut stmt = self.conn.prepare(
        "SELECT id FROM commands WHERE command LIKE ?1
         ORDER BY started_at DESC LIMIT ?2"
    )?;
    let rows = stmt.query_map(params![command_pattern, n as i64], |row| row.get(0))?;
    let mut ids: Vec<i64> = rows.collect::<std::result::Result<_, _>>()?;
    ids.reverse(); // oldest-first for forward diff chain
    Ok(ids)
}
```

### glass_query_drill - single record fetch

```rust
// Inline SQL in tool method -- no new helper needed
let row = conn.query_row(
    "SELECT id, command_id, record_type, severity, file_path, data
     FROM output_records WHERE id = ?1",
    params![params.record_id],
    |row| Ok(OutputRecordRow {
        id: row.get(0)?,
        command_id: row.get(1)?,
        record_type: row.get(2)?,
        severity: row.get(3)?,
        file_path: row.get(4)?,
        data: row.get(5)?,
    }),
).optional()?;
```

### glass_context SOI extension

```rust
// In context.rs -- new field on ContextSummary
pub struct ContextSummary {
    // ... existing fields ...
    pub soi_summaries: Vec<SoiSummaryEntry>,
}

#[derive(Debug, Serialize)]
pub struct SoiSummaryEntry {
    pub command_id: i64,
    pub command: String,
    pub output_type: String,
    pub severity: String,
    pub one_line: String,
}

// New query in build_context_summary()
let soi_sql = if after.is_some() {
    "SELECT c.id, c.command, cor.output_type, cor.severity, cor.one_line \
     FROM commands c \
     JOIN command_output_records cor ON cor.command_id = c.id \
     WHERE c.started_at >= ?1 \
     ORDER BY c.started_at DESC LIMIT 10"
} else {
    "SELECT c.id, c.command, cor.output_type, cor.severity, cor.one_line \
     FROM commands c \
     JOIN command_output_records cor ON cor.command_id = c.id \
     ORDER BY c.started_at DESC LIMIT 10"
};
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| glass_extract_errors (Phase 38) parses raw output at query time | glass_query reads pre-parsed records from DB | Phase 49 | No re-parsing; structured, filterable, token-budgeted |
| glass_context returned only command counts + dirs | glass_context will include SOI summaries | Phase 53 | Agents recover structured failure context after /clear |
| No trend detection existed | glass_query_trend compares N runs | Phase 53 | Agents detect CI regressions automatically |

---

## Open Questions

1. **LIKE pattern vs exact match for glass_query_trend command parameter**
   - What we know: exact match misses variations; LIKE with `%` wildcards are well-supported in SQLite
   - What's unclear: should the default be exact match (predictable) or LIKE (useful)?
   - Recommendation: accept a `command` param with `%` wildcard support via LIKE; document this in the tool description; default behavior without `%` is effectively exact match

2. **Should glass_query support filtering by record_type?**
   - What we know: `get_output_records()` already accepts `record_type: Option<&str>`; the storage layer supports it
   - What's unclear: whether agents benefit from type-filtering vs. severity-filtering
   - Recommendation: include `record_type` as an optional param; no extra DB work needed

3. **Token footprint audit scope**
   - What we know: STATE.md flags 25 existing tools as unmeasured; adding 3 more is a concern
   - What's unclear: what the actual tool schema JSON size is
   - Recommendation: Wave 0 task: serialize `GlassServer::tool_router()` tool list to JSON and measure bytes. If > 8KB, consider trimming existing descriptions before adding new tools.

---

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in (`cargo test`) |
| Config file | `Cargo.toml` workspace — no separate test config |
| Quick run command | `cargo test -p glass_mcp -- --nocapture` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SOIM-01 | glass_query returns CompressedOutput for command_id | unit | `cargo test -p glass_mcp tests::test_glass_query_*` | Wave 0 |
| SOIM-01 | glass_query returns "no SOI data" for unknown command_id | unit | `cargo test -p glass_mcp tests::test_glass_query_no_soi` | Wave 0 |
| SOIM-02 | glass_query_trend detects new TestResult failure across 2 runs | unit | `cargo test -p glass_mcp tests::test_glass_query_trend_regression` | Wave 0 |
| SOIM-02 | get_last_n_run_ids returns correct ordering (oldest-first) | unit | `cargo test -p glass_history soi::tests::test_get_last_n_run_ids` | Wave 0 |
| SOIM-03 | glass_query_drill returns full data JSON for valid record_id | unit | `cargo test -p glass_mcp tests::test_glass_query_drill_found` | Wave 0 |
| SOIM-03 | glass_query_drill returns error for unknown record_id | unit | `cargo test -p glass_mcp tests::test_glass_query_drill_not_found` | Wave 0 |
| SOIM-04 | glass_context includes soi_summaries field with SOI data | unit | `cargo test -p glass_mcp context::tests::test_context_soi_summaries` | Wave 0 |
| SOIM-04 | glass_compressed_context balanced mode includes SOI section | unit | `cargo test -p glass_mcp tests::test_compressed_context_soi_section` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_mcp -p glass_history`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] Test helpers for creating HistoryDb with SOI data (reuse pattern from `glass_history/src/soi.rs` test module)
- [ ] `crates/glass_mcp/src/tools.rs` tests section — needs tempfile + HistoryDb + insert_parsed_output setup fixtures
- [ ] `get_last_n_run_ids()` method on HistoryDb in `glass_history/src/db.rs` — covered by SOIM-02 test

---

## Sources

### Primary (HIGH confidence)
- `crates/glass_mcp/src/tools.rs` — 28 existing tool implementations (canonical pattern source)
- `crates/glass_mcp/src/context.rs` — build_context_summary() pattern for SOIM-04
- `crates/glass_history/src/compress.rs` — compress(), diff_compress(), TokenBudget, format_record()
- `crates/glass_history/src/soi.rs` — get_output_records(), get_previous_run_records(), OutputRecordRow
- `crates/glass_history/src/db.rs` — compress_output(), get_command_text(), get_last_n_run_ids() (to be added)
- `.planning/STATE.md` — phase decisions, token footprint concern
- `CLAUDE.md` — Rust conventions, clippy -D warnings, test placement rules

### Secondary (MEDIUM confidence)
- rmcp crate patterns — inferred from macro usage in tools.rs; `#[tool_router]`, `#[tool_handler]`, `Parameters<T>` wrapper

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all dependencies already in workspace, no new crates
- Architecture patterns: HIGH — directly verified from 28 existing tool implementations
- Pitfalls: HIGH — derived from STATE.md decisions + direct code inspection
- DB helpers needed: HIGH — `get_last_n_run_ids()` is the only new function required across the whole codebase

**Research date:** 2026-03-13
**Valid until:** 2026-04-12 (stable codebase; rmcp API is pinned)
