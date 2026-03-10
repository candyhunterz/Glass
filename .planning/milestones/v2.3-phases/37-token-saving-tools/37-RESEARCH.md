# Phase 37: Token-Saving Tools - Research

**Researched:** 2026-03-10
**Domain:** MCP tool implementation (Rust, rmcp, glass_mcp crate)
**Confidence:** HIGH

## Summary

Phase 37 adds four MCP tools to the `glass_mcp` crate that help AI agents retrieve command results with minimal token overhead. The tools break down into two categories: (1) tools that operate on live terminal output via IPC to the GUI (TOKEN-01), and (2) tools that operate on the history/snapshot databases directly within the MCP server process (TOKEN-02, TOKEN-03, TOKEN-04).

The existing codebase already has strong precedent for both patterns. The `glass_tab_output` tool demonstrates IPC-based output retrieval with regex filtering and line limits. The `glass_file_diff` tool demonstrates snapshot database access with blob content retrieval. The `glass_context` tool demonstrates aggregate query building from the history DB. All four new tools follow these established patterns exactly.

**Primary recommendation:** Implement TOKEN-01 as a new IPC method in main.rs + MCP tool, TOKEN-02/03 as history+snapshot DB tools in glass_mcp, and TOKEN-04 as a composite tool that queries history DB and formats a budget-constrained summary. Use the char-based token approximation (1 token ~ 4 chars) per REQUIREMENTS.md "Out of Scope" decision.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| TOKEN-01 | Agent can retrieve filtered command output (by pattern, line count, head/tail) via MCP | Extends existing `glass_tab_output` pattern; needs head/tail mode param and command-id-based lookup in addition to tab-based |
| TOKEN-02 | Agent can check if a previous command's result is still valid (cached result with file-change staleness detection) via MCP | Uses history DB `get_command` + snapshot DB `get_snapshots_by_command` + `get_snapshot_files` + filesystem stat check |
| TOKEN-03 | Agent can see which files a command modified with unified diffs via MCP | Extends existing `glass_file_diff` tool; adds unified diff generation comparing pre-snapshot blob to current file content |
| TOKEN-04 | Agent can request compressed context with a token budget and focus mode via MCP | Builds on `build_context_summary` pattern; adds token budget truncation using char/4 approximation |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| rmcp | 1.x | MCP server framework | Already in use, provides `#[tool]`, `#[tool_router]`, `#[tool_handler]` macros |
| schemars | 1.0 | JSON Schema for tool params | Already in use, auto-generates MCP tool input schemas |
| serde / serde_json | 1.0 | Serialization | Already in use throughout |
| regex | 1.x | Pattern filtering | Already a dependency of glass_mcp |
| rusqlite | 0.38 (workspace) | SQLite access | Already in use for history and snapshot DBs |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| similar | 2.x | Unified diff generation | TOKEN-03 needs to produce unified diffs from pre-snapshot vs current file content |
| chrono | (workspace) | Time parsing | Already in use for time filter parsing |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `similar` for diffs | Hand-rolled line diff | `similar` is well-maintained, handles edge cases (binary, encoding); hand-rolling diffs is error-prone |
| char/4 token approx | `tiktoken-rs` | REQUIREMENTS.md explicitly puts exact token counting out of scope; char/4 is sufficient |

**Installation:**
```bash
cargo add similar@2 -p glass_mcp
```

## Architecture Patterns

### Pattern 1: MCP-Only Tool (TOKEN-02, TOKEN-03, TOKEN-04)
**What:** Tools that query databases directly in the MCP server process without needing the GUI
**When to use:** When data lives in history DB or snapshot store (not live terminal state)
**Example (from existing glass_file_diff):**
```rust
#[tool(description = "...")]
async fn glass_command_diff(
    &self,
    Parameters(params): Parameters<CommandDiffParams>,
) -> Result<CallToolResult, McpError> {
    let glass_dir = self.glass_dir.clone();
    let db_path = self.db_path.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<_, McpError> {
        let store = glass_snapshot::SnapshotStore::open(&glass_dir).map_err(internal_err)?;
        let db = HistoryDb::open(&db_path).map_err(internal_err)?;
        // ... query and build response
        Ok(response)
    }).await.map_err(internal_err)??;
    let content = Content::json(&result)?;
    Ok(CallToolResult::success(vec![content]))
}
```

### Pattern 2: IPC-Forwarded Tool (TOKEN-01)
**What:** Tools that forward a request through IPC to the GUI event loop for live terminal data
**When to use:** When data is only available in the running GUI (terminal grid contents, block state)
**Example (from existing glass_tab_output):**
```rust
#[tool(description = "...")]
async fn glass_output_filter(
    &self,
    Parameters(input): Parameters<OutputFilterParams>,
) -> Result<CallToolResult, McpError> {
    let client = match self.ipc_client.as_ref() {
        Some(c) => c,
        None => return Ok(CallToolResult::error(vec![Content::text("...")])),
    };
    let mut params = serde_json::json!({...});
    match client.send_request("output_filter", params).await {
        Ok(resp) => Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&resp).unwrap_or_default(),
        )])),
        Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("...: {}", e))])),
    }
}
```

### Pattern 3: Parameter Structs with schemars
**What:** Each tool has a dedicated parameter struct with `#[derive(Debug, Deserialize, schemars::JsonSchema)]`
**When to use:** Always -- rmcp requires this for tool schema generation
**Example:**
```rust
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OutputFilterParams {
    /// 0-based tab index (provide this OR session_id).
    #[schemars(description = "0-based tab index (provide this OR session_id)")]
    pub tab_index: Option<u64>,
    /// Stable session ID (provide this OR tab_index).
    #[schemars(description = "Stable session ID (provide this OR tab_index)")]
    pub session_id: Option<u64>,
    // ... more fields
}
```

### Pattern 4: Tab/Session Resolution
**What:** Reuse `resolve_tab_index()` helper for tab identification by index or session_id
**When to use:** Any tool that targets a specific tab (TOKEN-01)

### Anti-Patterns to Avoid
- **Exact token counting:** REQUIREMENTS.md explicitly marks this out of scope. Use char/4 approximation.
- **Per-caller state:** REQUIREMENTS.md marks delta tracking between polls as out of scope. Each call is stateless.
- **Large Content::text responses:** For structured data, use `Content::json()` to enable machine parsing. Only use `Content::text()` for simple messages.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Unified diff generation | Custom line-by-line diff | `similar` crate `TextDiff::from_lines` | Handles edge cases (no trailing newline, binary), produces standard unified format |
| Token estimation | Character counting | `text.len() / 4` (char-based approx) | REQUIREMENTS.md decision; no tokenizer dependency needed |
| Regex compilation | Custom pattern matching | `regex::Regex::new()` | Already used in tab_output; handles error reporting |
| Time filter parsing | Custom date parser | `glass_history::query::parse_time()` | Already handles "1h", "2d", ISO dates |

**Key insight:** Most of the infrastructure already exists. TOKEN-01 extends tab_output's pattern. TOKEN-02/03 extend file_diff's pattern. TOKEN-04 extends context's pattern. The new tools compose existing primitives rather than building new infrastructure.

## Common Pitfalls

### Pitfall 1: Blocking the Tokio Runtime with DB Queries
**What goes wrong:** Calling rusqlite synchronously on a tokio async thread causes blocking
**Why it happens:** rusqlite is not async-aware
**How to avoid:** Always wrap DB operations in `tokio::task::spawn_blocking()` -- every existing tool does this
**Warning signs:** Compiler won't warn; look for rusqlite calls outside spawn_blocking

### Pitfall 2: IPC Handler Not Added to main.rs
**What goes wrong:** MCP tool sends IPC request, but main.rs event loop returns "Unknown method"
**Why it happens:** New IPC method names must be manually added to the match in `AppEvent::McpRequest` handler
**How to avoid:** For TOKEN-01, add a new match arm (e.g., "output_filter") in main.rs alongside "tab_output"
**Warning signs:** Tool returns "Unknown method: output_filter" error

### Pitfall 3: Token Budget Producing Empty Output
**What goes wrong:** Budget too small for even one section header, returning nothing useful
**Why it happens:** No minimum content guarantee
**How to avoid:** Always include at least a summary line regardless of budget; budget controls detail level
**Warning signs:** Response is empty string or just headers

### Pitfall 4: Unified Diff on Binary Files
**What goes wrong:** Producing multi-megabyte diff output for binary files
**Why it happens:** Pre-snapshot blob is raw bytes, current file is raw bytes, diff treats as text
**How to avoid:** Check if content is binary (contains null bytes or matches known binary extensions); return "[binary file]" placeholder
**Warning signs:** Very large diff output for small file changes

### Pitfall 5: File Not Found During Staleness Check
**What goes wrong:** Snapshot references a file that was deleted, causing an error
**Why it happens:** File deleted after command but before staleness check
**How to avoid:** Handle `std::fs::metadata()` errors gracefully -- file deletion means cache is stale
**Warning signs:** Unwrap on metadata calls

### Pitfall 6: Missing tool_router Registration
**What goes wrong:** Tool compiles but is not discoverable by MCP clients
**Why it happens:** New tool handler not listed in the `#[tool_router]` impl block
**How to avoid:** Add each new handler inside the existing `#[tool_router] impl GlassServer { ... }` block
**Warning signs:** Tool not appearing in `tools/list` response

## Code Examples

### TOKEN-01: Filtered Output Retrieval (IPC method in main.rs)

The IPC handler in main.rs needs to support head/tail mode in addition to existing regex + line count. The current `extract_term_lines` already gets the last N lines. For head mode, take first N instead:

```rust
// In main.rs match arm for "output_filter":
let mode = request.params.get("mode")
    .and_then(|v| v.as_str())
    .unwrap_or("tail");  // "head" or "tail"

let mut lines = extract_term_lines(&session.term, n);

match mode {
    "head" => { lines.truncate(n); }  // extract_term_lines gets all, take first n
    _ => {}  // already last n from extract_term_lines
}

// Apply regex filter after head/tail slicing
if let Some(ref pat) = pattern {
    match regex::Regex::new(pat) {
        Ok(re) => { lines.retain(|l| re.is_match(l)); }
        Err(e) => { /* return error */ }
    }
}
```

Note: `extract_term_lines` currently always returns the LAST n lines. For head mode, we need a variant that returns the FIRST n lines. Best approach: extract ALL lines from the grid, then slice head or tail, then filter.

### TOKEN-02: Cache Staleness Check

```rust
// Check if snapshot files have been modified since the command ran
let command = db.get_command(command_id).map_err(internal_err)?;
let snapshots = store.db().get_snapshots_by_command(command_id).map_err(internal_err)?;
let mut stale = false;
let mut changed_files = Vec::new();

for snapshot in &snapshots {
    let files = store.db().get_snapshot_files(snapshot.id).map_err(internal_err)?;
    for file_rec in &files {
        if file_rec.source != "parser" { continue; }
        let path = std::path::Path::new(&file_rec.file_path);
        match std::fs::metadata(path) {
            Ok(meta) => {
                // Compare file modification time against command finish time
                if let Ok(modified) = meta.modified() {
                    let modified_epoch = modified
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64;
                    if let Some(cmd) = &command {
                        if modified_epoch > cmd.finished_at {
                            stale = true;
                            changed_files.push(file_rec.file_path.clone());
                        }
                    }
                }
            }
            Err(_) => {
                // File no longer exists -- definitely stale
                stale = true;
                changed_files.push(file_rec.file_path.clone());
            }
        }
    }
}
```

### TOKEN-03: Unified Diff Generation with `similar`

```rust
use similar::{ChangeTag, TextDiff};

let pre_content = match &file_rec.blob_hash {
    Some(hash) => match store.blobs().read_blob(hash) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(_) => String::new(),
    },
    None => String::new(), // File did not exist before
};

let current_content = match std::fs::read_to_string(&file_rec.file_path) {
    Ok(c) => c,
    Err(_) => String::new(), // File was deleted
};

let diff = TextDiff::from_lines(&pre_content, &current_content);
let unified = diff.unified_diff()
    .context_radius(3)
    .header(&file_rec.file_path, &file_rec.file_path)
    .to_string();
```

### TOKEN-04: Budget-Aware Context Compression

```rust
// Token budget approximation: 1 token ~ 4 chars
let char_budget = token_budget * 4;
let mut output = String::new();
let mut remaining = char_budget;

// Always include summary (minimum content)
let summary = format_summary(&context_summary);
output.push_str(&summary);
remaining = remaining.saturating_sub(summary.len());

// Add focused sections based on focus mode
match focus.as_deref() {
    Some("errors") => {
        let errors = format_failed_commands(&failed_commands, remaining);
        output.push_str(&errors);
    }
    Some("files") => {
        let files = format_file_changes(&file_changes, remaining);
        output.push_str(&files);
    }
    Some("history") => {
        let history = format_recent_history(&commands, remaining);
        output.push_str(&history);
    }
    _ => {
        // Default: balanced across all sections
        let per_section = remaining / 3;
        // ... distribute budget across sections
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Return full command output | Filtered output with head/tail/regex | This phase | Reduces token usage by 80-90% for large outputs |
| Re-run commands to check status | Staleness check via file mtime | This phase | Eliminates unnecessary re-execution |
| Return raw pre-command blob | Unified diff format | This phase | Shows exactly what changed, not full file content |
| Fixed-size context dumps | Budget-aware compression | This phase | Agent controls token cost per request |

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in (`#[cfg(test)] mod tests`) + cargo test |
| Config file | Cargo workspace (already configured) |
| Quick run command | `cargo test -p glass_mcp` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| TOKEN-01 | OutputFilterParams deserializes with mode, pattern, lines, tab_index/session_id | unit | `cargo test -p glass_mcp -- test_output_filter_params -x` | Wave 0 |
| TOKEN-01 | Head/tail mode extracts correct line subsets | unit | `cargo test -- test_extract_term_lines_head -x` | Wave 0 |
| TOKEN-02 | CacheCheckParams deserializes with command_id | unit | `cargo test -p glass_mcp -- test_cache_check_params -x` | Wave 0 |
| TOKEN-02 | Staleness detection: stale when file mtime > command finish time | unit | `cargo test -p glass_mcp -- test_staleness_detection -x` | Wave 0 |
| TOKEN-02 | Staleness detection: valid when file unchanged | unit | `cargo test -p glass_mcp -- test_cache_valid -x` | Wave 0 |
| TOKEN-03 | CommandDiffParams deserializes with command_id | unit | `cargo test -p glass_mcp -- test_command_diff_params -x` | Wave 0 |
| TOKEN-03 | Unified diff generated correctly for modified file | unit | `cargo test -p glass_mcp -- test_unified_diff -x` | Wave 0 |
| TOKEN-04 | CompressedContextParams deserializes with budget and focus | unit | `cargo test -p glass_mcp -- test_compressed_context_params -x` | Wave 0 |
| TOKEN-04 | Budget truncation respects char limit | unit | `cargo test -p glass_mcp -- test_budget_truncation -x` | Wave 0 |
| TOKEN-04 | Focus mode filters sections correctly | unit | `cargo test -p glass_mcp -- test_focus_mode -x` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_mcp`
- **Per wave merge:** `cargo test --workspace && cargo clippy --workspace -- -D warnings`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- All tests above are new (Wave 0 items)
- No new test infrastructure needed -- existing test patterns in tools.rs cover param deserialization and can be extended
- `similar` crate needs to be added to `[dependencies]` in glass_mcp/Cargo.toml

## Open Questions

1. **TOKEN-01: Should `glass_output_filter` be a new tool or extend `glass_tab_output`?**
   - What we know: The existing `glass_tab_output` already supports regex pattern and line count. TOKEN-01 adds head/tail mode.
   - What's unclear: Whether to add `mode` param to existing tool or create new tool.
   - Recommendation: **Extend `glass_tab_output`** by adding an optional `mode` param (default "tail"). This avoids tool proliferation and is backward compatible. The existing IPC method "tab_output" can also be extended with the mode param.

2. **TOKEN-01: Command-ID-based output retrieval vs tab-based**
   - What we know: Requirement says "retrieve command results" which could mean by command_id (from history DB output field) or by tab (live terminal).
   - What's unclear: Whether TOKEN-01 should also support command_id lookup from history DB.
   - Recommendation: Support both. Tab-based (live terminal) is the primary path. Also support `command_id` param that falls back to history DB's `output` field if available. This enables filtering of past command outputs without the GUI.

3. **TOKEN-03: Relationship to existing `glass_file_diff`**
   - What we know: `glass_file_diff` returns pre-command file contents. TOKEN-03 wants unified diffs.
   - What's unclear: Whether to create a new tool or extend glass_file_diff.
   - Recommendation: **Create a new tool `glass_command_diff`** that returns unified diffs. The existing `glass_file_diff` serves a different purpose (raw pre-command content for undo preview). The new tool focuses on showing changes.

## Sources

### Primary (HIGH confidence)
- Direct codebase inspection of `crates/glass_mcp/src/tools.rs` -- all 22 existing tool patterns
- Direct codebase inspection of `src/main.rs` -- IPC handler pattern (lines 2438-2690)
- Direct codebase inspection of `crates/glass_snapshot/src/` -- SnapshotStore, SnapshotDb, types
- Direct codebase inspection of `crates/glass_history/src/db.rs` -- CommandRecord, HistoryDb API
- `.planning/REQUIREMENTS.md` -- TOKEN-01 through TOKEN-04 definitions, out-of-scope decisions

### Secondary (MEDIUM confidence)
- `similar` crate API: well-known Rust diff library, used in `insta` and `cargo-nextest`

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - all libraries already in use except `similar` (well-known, stable)
- Architecture: HIGH - follows exact patterns from Phase 35/36 implementation
- Pitfalls: HIGH - derived from direct code inspection of existing tool implementations

**Research date:** 2026-03-10
**Valid until:** 2026-04-10 (stable domain, no external API dependencies)
