# Phase 51: SOI Compression Engine - Research

**Researched:** 2026-03-13
**Domain:** Rust pure-computation library design, token-budget algorithms, diff/change-detection, glass_soi types
**Confidence:** HIGH

## Summary

Phase 51 builds a pure-Rust compression engine that transforms the `Vec<OutputRecord>` produced by Phase 48's parsers and stored by Phase 49's schema into token-budgeted summaries. The engine has no I/O of its own — it operates on data already fetched from the DB and returns value types. There is no new crate needed; the engine lives in `glass_soi` alongside the existing types and parsers.

The four budget levels are defined in requirements: OneLine (~10 tokens), Summary (~100), Detailed (~500), Full (~1000+). "Token" is used loosely — the project already uses a word-count heuristic in `OutputSummary.token_estimate` (`split_whitespace().count()`), and this phase continues that convention rather than adding a real tokenizer. Each budget level controls how many records are included and how much detail each record exposes. Smart truncation (SOIC-02) means errors come before warnings come before info/success records within each budget, and more recent records beat older ones when space is equal. Drill-down (SOIC-03) returns the DB row `id` values from `output_records` so callers can expand a specific record via `get_output_records(cmd_id, ...)` filtered to that id — no new schema needed. Diff-aware compression (SOIC-04) requires fetching `output_records` for the previous run of the same command text, computing a symmetric difference on record fingerprints, and generating a "compared to last run" change summary.

The compression engine's outputs are consumed immediately by Phase 52 (display), Phase 53 (MCP tools), and Phase 55 (activity stream). All three consumers call into `glass_soi::compress` directly or through `HistoryDb` helpers — no new AppEvent variants are required for this phase.

**Primary recommendation:** Implement a `compress` module in `glass_soi` that exposes a `compress(records: &[OutputRecord], summary: &OutputSummary, budget: TokenBudget) -> CompressedOutput` function and a `diff_compress(current: &CompressedOutput, previous: &[OutputRecord]) -> DiffSummary` function. Add a `get_previous_run_records` helper to `HistoryDb` for SOIC-04. No new crates or dependencies required.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SOIC-01 | Compression engine produces summaries at 4 token-budget levels: OneLine (~10 tokens), Summary (~100), Detailed (~500), Full (~1000+) | Token-budget enum + record-selection loop using existing word-count heuristic from `OutputSummary.token_estimate`; all record data available in-memory from `Vec<OutputRecord>` |
| SOIC-02 | Smart truncation prioritizes errors over warnings, recent over old within budget | Sort records by (severity rank DESC, record_id ASC) before budget loop; `output_records.id` ASC = insertion order = parse order = recency within a single run |
| SOIC-03 | Drill-down support returns record IDs for expanding specific items to full detail | `OutputRecordRow.id` (i64) from `glass_history::soi::OutputRecordRow` is the stable DB row id; compress returns a `Vec<i64>` of record IDs included in the compressed output; callers pass these to `get_output_records` |
| SOIC-04 | Diff-aware compression produces "compared to last run" change summaries | Needs a new DB helper `get_previous_run_records(command_text, current_command_id) -> Vec<OutputRecordRow>`; diff is symmetric difference on (record_type, file_path, severity, message digest) fingerprints |
</phase_requirements>

---

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| glass_soi | workspace | `OutputRecord`, `OutputSummary`, `Severity`, `ParsedOutput` — input types | All compression input data is already in these types; no import overhead |
| glass_history | workspace | `OutputRecordRow`, `CommandOutputSummaryRow`, `HistoryDb::get_output_records` | Storage layer already holds all record data with DB ids; SOIC-03 drill-down uses row ids from this table |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| serde / serde_json | workspace | Serialize `CompressedOutput` for MCP tools (Phase 53) | Already on `glass_soi`; derive `Serialize` on new types |
| rusqlite | workspace | New `get_previous_run_records` query in `glass_history` | Already the project's DB access layer |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Word-count heuristic for token estimate | tiktoken or tokenizers crate | A real tokenizer would be more accurate but adds a dependency and is overkill — the project already uses `split_whitespace().count() + 2` as the convention in `freeform_parse`. Consistent heuristic is better than mixed accuracy. |
| Symmetric diff on record fingerprints | Full text diff (similar_crate or diff) | Records are structured (typed enum); fingerprinting individual fields is more semantically meaningful than text diff and requires no external crate |
| Separate `glass_compression` crate | Expand `glass_soi` | `glass_soi` already owns classification, parsing, and the types. Adding a `compress` submodule keeps the crate cohesive and avoids workspace churn |

**No new Cargo.toml changes needed.** `glass_soi` already has `serde` and `serde_json`. `glass_history` already has `rusqlite`.

---

## Architecture Patterns

### Where Things Live

```
crates/glass_soi/src/compress.rs        # New: compression engine (SOIC-01, SOIC-02, SOIC-03)
crates/glass_soi/src/lib.rs             # Re-export compress module's public API
crates/glass_history/src/soi.rs         # New: get_previous_run_records helper (SOIC-04)
crates/glass_history/src/db.rs          # Delegation method for get_previous_run_records
```

No changes to `src/main.rs`, `glass_core`, or `glass_mux` in this phase.

### Pattern 1: TokenBudget Enum

**What:** A four-variant enum controlling record inclusion and detail level.

**When to use:** Passed by callers (Phase 52, 53, 55) to select compression level.

```rust
// Source: crates/glass_soi/src/compress.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TokenBudget {
    /// ~10 tokens: error count + first error file only
    OneLine,
    /// ~100 tokens: top errors with file:line, test summary
    Summary,
    /// ~500 tokens: errors prioritized over warnings, recent over old
    Detailed,
    /// No truncation: complete record set
    Full,
}

impl TokenBudget {
    /// Approximate token limit for this budget level.
    pub fn token_limit(self) -> usize {
        match self {
            TokenBudget::OneLine => 10,
            TokenBudget::Summary => 100,
            TokenBudget::Detailed => 500,
            TokenBudget::Full => usize::MAX,
        }
    }
}
```

### Pattern 2: CompressedOutput Type

**What:** The result type returned by the compression engine. Contains the text summary at the requested budget level, the list of included DB record IDs (for drill-down), and the token count consumed.

```rust
// Source: crates/glass_soi/src/compress.rs
#[derive(Debug, Clone, Serialize)]
pub struct CompressedOutput {
    /// The budget level used.
    pub budget: TokenBudget,
    /// Human/agent readable summary at this budget level.
    pub text: String,
    /// DB row ids from output_records that are included in this summary.
    /// Empty for OneLine and Full (OneLine has no per-record expansion;
    /// Full means "use get_output_records directly").
    pub record_ids: Vec<i64>,
    /// Approximate token count of `text`.
    pub token_count: usize,
    /// True if records were truncated to fit the budget.
    pub truncated: bool,
}
```

### Pattern 3: compress() Entry Point

**What:** The main compression function. Takes the stored records and summary, plus a budget, and returns `CompressedOutput`.

**When to use:** Called by Phase 52 display, Phase 53 MCP tools, Phase 55 activity stream.

```rust
// Source: crates/glass_soi/src/compress.rs
/// Compress output records to fit a token budget.
///
/// `records` comes from `HistoryDb::get_output_records` (already filtered
/// to a single command_id). `summary` is the pre-computed one-line summary
/// stored during Phase 50 pipeline execution.
pub fn compress(
    records: &[OutputRecordRow],
    summary: &CommandOutputSummaryRow,
    budget: TokenBudget,
) -> CompressedOutput
```

Where `OutputRecordRow` and `CommandOutputSummaryRow` are from `glass_history::soi`.

**Sort order for smart truncation (SOIC-02):** Before iterating, sort records by:
1. Severity rank descending: Error(0) > Warning(1) > Info(2) > Success(3) > None(4)
2. Record id ascending (lower id = earlier in parse run = "more recent" within a run, consistent with insertion order)

**Budget loop:** Accumulate records greedily until `token_count + record_token_estimate > budget.token_limit()`. Records that fit are included; the first record that would exceed the limit stops the loop.

**Token estimation per record:** The `data` field in `OutputRecordRow` is a JSON blob. A rough estimate is `data.split_whitespace().count()` — consistent with the existing convention. For `OneLine` budget, skip the loop entirely and use `summary.one_line` directly.

### Pattern 4: DiffSummary and diff_compress()

**What:** Takes current compressed output and the previous run's records, produces a change summary.

```rust
// Source: crates/glass_soi/src/compress.rs
#[derive(Debug, Clone, Serialize)]
pub struct DiffSummary {
    /// New issues that didn't exist in the previous run.
    pub new_records: Vec<RecordFingerprint>,
    /// Issues that were present before but are now resolved.
    pub resolved_records: Vec<RecordFingerprint>,
    /// Total count of new issues.
    pub new_count: usize,
    /// Total count of resolved issues.
    pub resolved_count: usize,
    /// Human-readable change summary line.
    pub change_line: String,
}

/// Minimal identity key for a record used in diff comparison.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct RecordFingerprint {
    pub record_type: String,
    pub severity: Option<String>,
    pub file_path: Option<String>,
    /// First ~80 chars of message for identity (from JSON data field).
    pub message_prefix: String,
}
```

**Diff algorithm:** Compute fingerprints for current run records and previous run records. New = in current but not in previous. Resolved = in previous but not in current. Use `HashSet<RecordFingerprint>` with standard `Eq`/`Hash` — no external crate needed.

**Fingerprint extraction:** Parse the `data` JSON blob (already serde_json-serialized `OutputRecord`) to extract the message field. For `CompilerError`, the message is directly available. For `TestResult`, use the test name as the message. For `FreeformChunk`, skip (no meaningful diff possible).

### Pattern 5: get_previous_run_records() DB Helper

**What:** Finds the most recent prior run of the same command text (by exact `command` match, ordered `started_at DESC`, excluding the current command id), and returns its `output_records`.

```rust
// Source: crates/glass_history/src/soi.rs
/// Fetch output records for the most recent prior run of the same command text.
///
/// Excludes `current_command_id` so the current run is never compared to itself.
/// Returns `Ok(None)` if no prior run exists (first time this command ran).
pub fn get_previous_run_records(
    conn: &Connection,
    command_text: &str,
    current_command_id: i64,
) -> Result<Option<Vec<OutputRecordRow>>> {
    // Find the most recent command row with the same text, excluding current
    let prev_cmd_id: Option<i64> = conn.query_row(
        "SELECT id FROM commands WHERE command = ?1 AND id != ?2
         ORDER BY started_at DESC LIMIT 1",
        params![command_text, current_command_id],
        |row| row.get(0),
    ).optional()?;

    match prev_cmd_id {
        None => Ok(None),
        Some(prev_id) => {
            let records = get_output_records(conn, prev_id, None, None, None, 1000)?;
            Ok(Some(records))
        }
    }
}
```

This query uses the existing `idx_commands_cwd` index (covers `command` text lookup) — no new index required for reasonable performance.

### Pattern 6: Full Budget Behavior (SOIC-01 criterion 3)

For `TokenBudget::Full`, the compression engine returns all records without truncation. The `record_ids` field contains all IDs. This is a passthrough — the `text` field contains the multi-line expanded summary, and `truncated = false`.

### Anti-Patterns to Avoid

- **Implementing a real tokenizer:** The word-count heuristic is the established project convention. Do not add `tiktoken-rs` or any tokenizer crate — it adds a heavy dependency for marginal accuracy gain.
- **Storing CompressedOutput in the DB:** Compression is always on-demand, not persisted. The raw `output_records` rows are the canonical data; compression is a view over them.
- **Using lossy fingerprinting for diff:** Fingerprints must be stable across runs. Do not use record insertion order or DB ids as part of the fingerprint — those change between runs. Use (record_type, severity, file_path, message_prefix) only.
- **Panicking on JSON parse failure:** The `data` field in `OutputRecordRow` is a JSON blob written by `serde_json::to_string(record)`. If parsing fails (e.g., schema evolution), log a warning and skip that record — never panic.
- **Empty previous run producing a diff:** If `get_previous_run_records` returns `None` (first time this command ran), `diff_compress` must return a `DiffSummary` with `change_line = "first run — no comparison available"`, not an error.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Record deserialization for fingerprinting | Custom parsing of data JSON | `serde_json::from_str::<OutputRecord>(&row.data)` | Records were serialized with `serde_json::to_string(record)` in `soi.rs`; round-trip is guaranteed |
| Previous-run lookup | Custom command history scan | `get_previous_run_records` SQL query | The `commands` table with `started_at` index supports this O(log n) |
| Set difference for diff | External diff library | `HashSet<RecordFingerprint>` symmetric difference | Records are structured; set operations on fingerprints are correct and require no new dependency |
| Token counting | tiktoken-rs | `text.split_whitespace().count()` | Matches existing `OutputSummary.token_estimate` convention in `freeform_parse`; consistent, no dependency |

**Key insight:** Every piece of data needed for compression is already stored and queryable. The compression engine is pure computation over existing data — no new storage, no new I/O patterns.

---

## Common Pitfalls

### Pitfall 1: OneLine Budget Must Include Error Count AND First Error File

**What goes wrong:** The `summary.one_line` field produced by Phase 48 parsers says "3 errors" without a file name. The success criterion for SOIC-01 explicitly requires "error count and first error file" at OneLine budget.

**Why it happens:** `OutputSummary.one_line` was designed as a simple count string, not file-qualified. For `OneLine` budget, the compression engine cannot just return `summary.one_line` verbatim.

**How to avoid:** For `OneLine` budget, check if there are any `Error`-severity records. If so, extract the `file_path` of the first Error-severity record (sorted by id ASC) and format: `"N errors in path/to/file.rs"`. If no errors, fall back to `summary.one_line`. Token count for this format is predictably ~5-8 words — well under 10.

**Warning signs:** OneLine test fails because file name is missing from the summary.

### Pitfall 2: Record Ordering in CompressedOutput Differs from DB Order

**What goes wrong:** The compression engine re-sorts records by severity. The `record_ids` in `CompressedOutput` will be in severity-priority order, not DB insertion order. Callers who display records in `record_ids` order will see errors before warnings — which is correct and intentional.

**Why it happens:** Smart truncation (SOIC-02) requires severity-priority sort.

**How to avoid:** This is correct behavior. Document that `record_ids` ordering reflects priority, not insertion order. Callers wanting insertion order use `get_output_records` directly (which orders by `id ASC`).

### Pitfall 3: JSON Fingerprint Extraction for Non-CompilerError Records

**What goes wrong:** `RecordFingerprint.message_prefix` must be extracted from different JSON shapes depending on `record_type`. A `TestResult` has a `name` field, not a `message`. A `FreeformChunk` has a `text` field. Generic extraction that always looks for `"message"` will silently produce empty fingerprints for test results.

**Why it happens:** `OutputRecord` is an enum with different field names per variant.

**How to avoid:** After deserializing `serde_json::from_str::<OutputRecord>(&row.data)`, match on the variant to extract the identity field: `CompilerError.message`, `TestResult.name`, `PackageEvent.package`. `FreeformChunk` records are skipped entirely in diff computation (no stable identity).

### Pitfall 4: Diff Across Different Output Types Produces Noise

**What goes wrong:** The user runs `cargo build` which fails, fixes the code, then runs `cargo test`. The command text is different (`cargo build` vs `cargo test`), so `get_previous_run_records` will find the last `cargo test` run — which may be a completely different kind of output. The diff will show massive changes because the record types differ.

**Why it happens:** `get_previous_run_records` matches by exact command text. `cargo build` vs `cargo test` are different commands so this is fine. The risk is if command text is normalized (e.g., `cargo build --release` vs `cargo build`) — these are different strings, so there will be no previous run match, which is the correct and safe behavior (returns "first run").

**How to avoid:** Do not normalize command text before lookup. Exact match is conservative and correct. If the output type changed between runs (e.g., command now produces RustTest records when it previously produced RustCompiler records), add an output_type match guard: only diff records of the same `record_type`.

### Pitfall 5: Summary Row May Not Exist for Previous Run

**What goes wrong:** The previous run's command may have been inserted before Phase 49 shipped (schema v3). Its output was captured but never parsed by SOI. `get_previous_run_records` queries `output_records`, which has zero rows for pre-Phase-49 commands. The diff shows everything as "new" — false positives.

**Why it happens:** Schema migration is idempotent but not retroactive. Old commands have no `output_records` rows.

**How to avoid:** Before computing a diff, check if previous run has zero records. If `prev_records.is_empty()`, treat as "no comparable data" and return `change_line = "no structured data for previous run"` rather than showing all current records as "new errors".

### Pitfall 6: Token Estimate Overflow at Full Budget

**What goes wrong:** `TokenBudget::Full` passes `usize::MAX` as the limit. If the budget loop adds tokens without a ceiling, computing `token_count + record_token_estimate > usize::MAX` would overflow on 32-bit targets (not the primary dev platform, but possible).

**Why it happens:** `usize::MAX` overflow in addition on 32-bit.

**How to avoid:** For `Full` budget, bypass the budget loop entirely — copy all records and sum tokens without checking against the ceiling. This is also more efficient.

---

## Code Examples

Verified patterns from existing codebase sources:

### Existing Token Estimate Convention

```rust
// Source: crates/glass_soi/src/lib.rs — freeform_parse()
let one_line = format!("{} lines of unstructured output", raw_line_count);
let token_estimate = one_line.split_whitespace().count() + 2; // rough heuristic
```

Phase 51 compression uses the same `split_whitespace().count()` heuristic for consistency.

### Existing Record Query API (for callers of compress)

```rust
// Source: crates/glass_history/src/db.rs
pub fn get_output_records(
    &self,
    command_id: i64,
    severity: Option<&str>,
    file_path: Option<&str>,
    record_type: Option<&str>,
    limit: usize,
) -> Result<Vec<crate::soi::OutputRecordRow>>
```

Callers fetch records, then pass the `Vec<OutputRecordRow>` to `compress()`. The compression engine never opens a DB connection — all data is passed in.

### OutputRecordRow Fields Available for Compression

```rust
// Source: crates/glass_history/src/soi.rs
pub struct OutputRecordRow {
    pub id: i64,              // DB row id — used for drill-down (SOIC-03)
    pub command_id: i64,
    pub record_type: String,  // "CompilerError", "TestResult", etc.
    pub severity: Option<String>, // "Error", "Warning", "Info", "Success"
    pub file_path: Option<String>, // For CompilerError and GenericDiagnostic
    pub data: String,         // JSON-serialized OutputRecord — for fingerprinting
}
```

### CommandOutputSummaryRow Fields for OneLine Fallback

```rust
// Source: crates/glass_history/src/soi.rs
pub struct CommandOutputSummaryRow {
    pub id: i64,
    pub command_id: i64,
    pub output_type: String,  // "RustCompiler", "RustTest", etc.
    pub severity: String,     // Highest severity
    pub one_line: String,     // Pre-computed one-liner from Phase 48 parsers
    pub token_estimate: i64,
    pub raw_line_count: i64,
    pub raw_byte_count: i64,
}
```

### Severity Rank for Smart Truncation

```rust
// Implement as a local function in compress.rs
fn severity_rank(severity: Option<&str>) -> u8 {
    match severity {
        Some("Error")   => 0,
        Some("Warning") => 1,
        Some("Info")    => 2,
        Some("Success") => 3,
        None            => 4,
    }
}
```

Lower rank = higher priority = included first in budget.

### OneLine Budget Construction (SOIC-01 success criterion 1)

```rust
// Inside compress(), OneLine branch:
// Find the first Error-severity record (lowest id among Error records)
let first_error = records.iter()
    .filter(|r| r.severity.as_deref() == Some("Error"))
    .min_by_key(|r| r.id);

let text = if let Some(err) = first_error {
    let error_count = records.iter()
        .filter(|r| r.severity.as_deref() == Some("Error"))
        .count();
    if let Some(ref fp) = err.file_path {
        format!("{} error{} in {}", error_count, if error_count == 1 { "" } else { "s" }, fp)
    } else {
        summary.one_line.clone()
    }
} else {
    summary.one_line.clone()
};
```

### Previous Run Lookup SQL

```rust
// Source: New method in crates/glass_history/src/soi.rs
// Uses OptionalExtension already imported in db.rs
let prev_cmd_id: Option<i64> = conn.query_row(
    "SELECT id FROM commands WHERE command = ?1 AND id != ?2
     ORDER BY started_at DESC LIMIT 1",
    params![command_text, current_command_id],
    |row| row.get(0),
).optional()?;
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| No compression — raw output in AppEvent | `OutputSummary.one_line` per command | Phase 50 | Now have a pre-computed one-liner; Phase 51 extends this to 4 budget levels |
| No per-record IDs | `output_records.id` (PK) per record | Phase 49 | Drill-down by stable id is already possible; Phase 51 exposes these ids to callers |
| No diff-awareness | Diff computed on-demand from DB history | Phase 51 | New capability; no prior approach |

**Nothing deprecated:** Phase 51 adds to Phase 48-50 infrastructure without replacing any existing API.

---

## Open Questions

1. **Message extraction from JSON data for fingerprinting**
   - What we know: `data` is `serde_json::to_string(record)` of an `OutputRecord` enum variant
   - What's unclear: Whether it's safer to deserialize to `OutputRecord` or to `serde_json::Value` for fingerprint extraction
   - Recommendation: Deserialize to `serde_json::Value` (looser coupling; avoids re-importing `glass_soi` types into `glass_history`). Extract `["message"]`, `["name"]`, or `["package"]` keys by record_type string. This keeps `compress.rs` in `glass_soi` using native types and `soi.rs` in `glass_history` using JSON-level extraction.

2. **Where does compress() live — in glass_soi or glass_history?**
   - What we know: `compress()` takes `OutputRecordRow` (from `glass_history`) and returns `CompressedOutput`; `diff_compress()` also takes `OutputRecordRow`; both use `Severity` strings (not the enum) since they operate on DB rows
   - What's unclear: Whether `compress()` belongs in `glass_soi` (types crate) or `glass_history` (storage crate)
   - Recommendation: Put `compress.rs` in `glass_history` — it operates on `OutputRecordRow` (a `glass_history` type) and is more naturally co-located with the storage layer. This avoids a dependency from `glass_soi` onto `glass_history`. The `TokenBudget` enum and `CompressedOutput` type can live in `glass_history::compress` since downstream consumers (Phase 52, 53, 55) already depend on `glass_history`.

3. **Exact token limit thresholds**
   - What we know: Requirements say "~10 tokens", "~100 tokens", "~500 tokens", "~1000+"
   - What's unclear: Whether these are hard limits or soft targets; whether truncation cuts at the first record that exceeds the limit or at the last record that fits
   - Recommendation: Use the limits as soft ceilings. Include a record if adding it keeps total <= limit. The first record that would push total over limit stops the loop. Partial record inclusion is not supported — include whole records or none.

---

## Validation Architecture

> `workflow.nyquist_validation` is `true` in `.planning/config.json` — section included.

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in (`#[test]`) |
| Config file | None — tests inline per project convention |
| Quick run command | `cargo test -p glass_history -- compress` |
| Full suite command | `cargo test --workspace 2>&1` |

### Phase Requirements -> Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SOIC-01 | `OneLine` budget for failed cargo build returns summary under 10 tokens with error count and first error file | unit | `cargo test -p glass_history -- compress_one_line_failed_build` | Wave 0 |
| SOIC-01 | `Summary` budget returns up to ~100 tokens | unit | `cargo test -p glass_history -- compress_summary_budget` | Wave 0 |
| SOIC-01 | `Detailed` budget returns up to ~500 tokens | unit | `cargo test -p glass_history -- compress_detailed_budget` | Wave 0 |
| SOIC-01 | `Full` budget returns all records with no truncation | unit | `cargo test -p glass_history -- compress_full_budget_no_truncation` | Wave 0 |
| SOIC-02 | Smart truncation puts errors before warnings within budget | unit | `cargo test -p glass_history -- compress_errors_before_warnings` | Wave 0 |
| SOIC-03 | Drill-down returns non-empty record_ids for non-Full budgets with records | unit | `cargo test -p glass_history -- compress_drill_down_record_ids` | Wave 0 |
| SOIC-04 | Diff-aware compression returns new/resolved counts for second run | unit | `cargo test -p glass_history -- diff_compress_second_run` | Wave 0 |
| SOIC-04 | Diff on first run (no prior) returns "first run" change line | unit | `cargo test -p glass_history -- diff_compress_first_run_no_prior` | Wave 0 |
| SOIC-04 | Diff with empty previous records returns "no structured data" | unit | `cargo test -p glass_history -- diff_compress_empty_previous` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_history -- compress`
- **Per wave merge:** `cargo test --workspace && cargo clippy --workspace -- -D warnings`
- **Phase gate:** Full suite green + no clippy warnings before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_history/src/compress.rs` — new module with `TokenBudget`, `CompressedOutput`, `DiffSummary`, `RecordFingerprint`, `compress()`, `diff_compress()` and all tests
- [ ] `crates/glass_history/src/soi.rs` — add `get_previous_run_records()` function
- [ ] `crates/glass_history/src/db.rs` — add `get_previous_run_records()` delegation method
- [ ] `crates/glass_history/src/lib.rs` — re-export `compress` module

*(No test framework install needed — Rust built-in tests already in use project-wide.)*

---

## Sources

### Primary (HIGH confidence)
- Direct code reading: `crates/glass_soi/src/types.rs` — `OutputRecord` enum variants and fields, `Severity`, `OutputSummary.token_estimate` convention
- Direct code reading: `crates/glass_soi/src/lib.rs` — `freeform_parse` token heuristic (`split_whitespace().count() + 2`)
- Direct code reading: `crates/glass_history/src/soi.rs` — `OutputRecordRow`, `CommandOutputSummaryRow`, `get_output_records` signature with severity/file/type filters, `insert_parsed_output` confirming data is `serde_json::to_string(record)`
- Direct code reading: `crates/glass_history/src/db.rs` — full schema (v3), `get_output_records` delegation, `get_command_text` (available for diff lookup), `idx_commands_started_at` / `idx_commands_cwd` indexes
- Direct code reading: `crates/glass_history/src/query.rs` — confirms `filtered_query` supports `command` text filtering via FTS5
- Direct code reading: `crates/glass_mux/src/session.rs` — `SoiSummary` struct shape (confirms `one_line` + `severity` as String fields)
- Direct code reading: `.planning/REQUIREMENTS.md` — SOIC-01 through SOIC-04 exact text and the success criteria
- Direct code reading: `.planning/phases/50-soi-pipeline-integration/50-VERIFICATION.md` — confirmed Phase 50 complete, all SOIL-01..04 satisfied, pipeline is live

### Secondary (MEDIUM confidence)
- `.planning/STATE.md` decisions section — "AppEvent::SoiReady.severity is String not glass_soi::Severity to keep glass_core dep-free of glass_soi" — informs why compression operates on String severity, not enum
- `.planning/phases/50-soi-pipeline-integration/50-RESEARCH.md` — confirmed no new crates were added in Phase 50; compression should follow same pattern

### Tertiary (LOW confidence)
- None — all findings verified by direct source reading

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all types and DB methods confirmed by reading source code
- Architecture: HIGH — compression as pure computation over existing data types is verified by the actual type signatures in glass_history and glass_soi
- Pitfalls: HIGH — OneLine format mismatch with success criteria verified by reading both REQUIREMENTS.md criteria and OutputSummary.one_line convention; diff pitfalls verified by reading schema and query patterns

**Research date:** 2026-03-13
**Valid until:** 2026-04-13 (stable domain — internal types are the only invalidation risk)
