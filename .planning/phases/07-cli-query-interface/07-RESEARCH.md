# Phase 7: CLI Query Interface - Research

**Researched:** 2026-03-05
**Domain:** Rust CLI (clap derive), SQLite query building, terminal output formatting
**Confidence:** HIGH

## Summary

Phase 7 adds the `glass history` CLI subcommand that queries the existing SQLite history database and displays results as structured terminal output. The foundation is solid: Phase 5-6 built the `glass_history` crate with `HistoryDb`, `CommandRecord`, `SearchResult`, FTS5 search, and the `resolve_db_path` function. The `Commands::History` variant already exists in clap but is a unit variant with no arguments -- it needs to be expanded with subcommands (`search`, `list`) and filter flags (`--exit`, `--after`, `--before`, `--cwd`, `--limit`).

The core work is: (1) expand clap CLI definition for history subcommands/flags, (2) add a filtered query method to `HistoryDb` that combines SQL WHERE clauses with FTS5 MATCH, (3) format results as readable terminal output with columns for command text, exit code, timestamp, duration, and cwd.

**Primary recommendation:** Keep all query logic in `glass_history` crate (new `query` module with `QueryFilter` struct and SQL builder). Keep formatting/display in the main binary's history handler. Use plain `println!` formatting with fixed-width columns -- no external TUI library needed for tabular output.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| CLI-01 | `glass history` subcommand queries the history database | Expand existing `Commands::History` clap variant with subcommands; use `HistoryDb::open` + `resolve_db_path` from glass_history |
| CLI-02 | Filter by exit code, time range, cwd, and text content | New `QueryFilter` struct + parameterized SQL builder combining WHERE clauses with FTS5 MATCH |
| CLI-03 | Results formatted as structured terminal output | Format `CommandRecord` fields into aligned columns via `println!` with fixed-width formatting |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| clap | 4.5 (workspace) | CLI argument parsing with derive macros | Already in project, derive API for nested subcommands |
| rusqlite | 0.38.0 (workspace) | SQLite queries with parameterized SQL | Already in glass_history, supports dynamic WHERE building |
| glass_history | 0.1.0 (local) | HistoryDb, CommandRecord, resolve_db_path | Foundation from Phase 5-6 |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| chrono | 0.4 | Human-readable time parsing ("1 hour ago") and timestamp formatting | Parse `--after`/`--before` relative time strings, format epoch timestamps for display |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| chrono for relative time | humantime | humantime parses durations ("1h") but not dates; chrono handles both relative and absolute |
| println! formatting | comfy-table or tabled | External dep for what is ~30 lines of format!() code; not worth it for 5 columns |
| chrono | time crate | time is lighter but chrono's `NaiveDateTime::from_timestamp` and relative parsing are more ergonomic |

**Installation:**
```bash
cargo add chrono --package glass_history
# Or add to workspace Cargo.toml:
# chrono = "0.4"
```

Note: `chrono` may also be useful in Phase 9 (MCP server) for timestamp handling. Consider adding as workspace dependency.

## Architecture Patterns

### Recommended Project Structure
```
crates/glass_history/src/
    db.rs            # Existing -- HistoryDb, CommandRecord
    search.rs        # Existing -- FTS5 search
    query.rs         # NEW -- QueryFilter struct + filtered_query() function
    lib.rs           # Add: pub mod query; pub use query::QueryFilter;

src/
    main.rs          # Expand Commands::History with subcommands + flags
                     # Add history_main() handler function
    history.rs       # NEW -- display formatting + history subcommand dispatch
    tests.rs         # Add tests for new CLI args
```

### Pattern 1: Clap Nested Subcommands with Shared Flags
**What:** Expand `Commands::History` from unit variant to struct variant with nested subcommands
**When to use:** When a subcommand needs its own args and sub-subcommands
**Example:**
```rust
#[derive(Subcommand, Debug, PartialEq)]
enum Commands {
    /// Query command history
    History {
        #[command(subcommand)]
        action: Option<HistoryAction>,
    },
    // ...
}

#[derive(Subcommand, Debug, PartialEq)]
enum HistoryAction {
    /// Search command history by text
    Search {
        /// Search term (FTS5 query)
        query: String,

        #[command(flatten)]
        filters: HistoryFilters,
    },
    /// List recent commands
    List {
        #[command(flatten)]
        filters: HistoryFilters,
    },
}

/// Shared filter flags for history queries
#[derive(clap::Args, Debug, PartialEq)]
struct HistoryFilters {
    /// Filter by exit code (e.g., --exit 1 for failures)
    #[arg(long)]
    exit: Option<i32>,

    /// Show commands after this time (e.g., "1 hour ago", "2024-01-01")
    #[arg(long)]
    after: Option<String>,

    /// Show commands before this time
    #[arg(long)]
    before: Option<String>,

    /// Filter by working directory
    #[arg(long)]
    cwd: Option<String>,

    /// Maximum number of results (default: 25)
    #[arg(long, short = 'n', default_value_t = 25)]
    limit: usize,
}
```

### Pattern 2: QueryFilter for Dynamic SQL Building
**What:** A struct that maps CLI flags to SQL WHERE clauses, built incrementally
**When to use:** When combining optional filters into a single parameterized query
**Example:**
```rust
// In crates/glass_history/src/query.rs

pub struct QueryFilter {
    pub text: Option<String>,       // FTS5 MATCH
    pub exit_code: Option<i32>,     // WHERE exit_code = ?
    pub after: Option<i64>,         // WHERE started_at >= ? (epoch)
    pub before: Option<i64>,        // WHERE started_at <= ? (epoch)
    pub cwd: Option<String>,        // WHERE cwd = ?
    pub limit: usize,               // LIMIT ?
}

impl QueryFilter {
    pub fn new() -> Self {
        Self {
            text: None,
            exit_code: None,
            after: None,
            before: None,
            cwd: None,
            limit: 25,
        }
    }
}

/// Execute a filtered query, combining FTS5 and SQL WHERE clauses.
pub fn filtered_query(conn: &Connection, filter: &QueryFilter) -> Result<Vec<CommandRecord>> {
    // Build SQL dynamically based on which filters are set
    // Use FTS5 JOIN only when text filter is present
    // Otherwise query commands table directly
}
```

### Pattern 3: Display Formatting
**What:** Format CommandRecord into aligned terminal output
**When to use:** Printing query results to stdout
**Example:**
```rust
fn format_timestamp(epoch: i64) -> String {
    // Convert epoch seconds to "2024-01-15 14:30:05" or relative "2h ago"
}

fn format_duration(ms: i64) -> String {
    if ms < 1000 { format!("{}ms", ms) }
    else if ms < 60_000 { format!("{:.1}s", ms as f64 / 1000.0) }
    else { format!("{}m{}s", ms / 60_000, (ms % 60_000) / 1000) }
}

fn print_results(records: &[CommandRecord]) {
    if records.is_empty() {
        println!("No matching commands found.");
        return;
    }
    // Header
    println!("{:<50} {:>4} {:>8} {:>20} {}", "COMMAND", "EXIT", "DURATION", "TIME", "CWD");
    println!("{}", "-".repeat(100));
    for r in records {
        println!("{:<50} {:>4} {:>8} {:>20} {}",
            truncate(&r.command, 50),
            r.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "-".to_string()),
            format_duration(r.duration_ms),
            format_timestamp(r.started_at),
            r.cwd,
        );
    }
    println!("\n{} result(s)", records.len());
}
```

### Anti-Patterns to Avoid
- **Building SQL with string interpolation:** Always use parameterized queries (`?1`, `?2`) to avoid injection. Even though this is a local DB, it sets good precedent.
- **Locking the terminal for CLI subcommands:** The history subcommand must NOT create a window or event loop. It should open the DB, query, print, and exit. The current `main.rs` already handles this correctly by matching on `cli.command` before creating the event loop.
- **Using FTS5 MATCH for non-text filters:** FTS5 only indexes command text. Exit code, time range, and cwd filters must use standard SQL WHERE clauses on the `commands` table.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Relative time parsing | Custom regex for "1 hour ago" | chrono + simple parser | Edge cases: months, DST, overflow |
| Timestamp display | Manual epoch-to-string | chrono::NaiveDateTime | Timezone, formatting, locale |
| CLI argument parsing | Manual argv parsing | clap derive macros | Validation, help text, error messages |
| FTS5 query sanitization | Custom escaper | Quote user input with FTS5 double-quote syntax | FTS5 special chars: *, ^, NEAR, OR |

**Key insight:** The hardest part of this phase is not the query logic (straightforward SQL) but the human-readable time parsing for `--after`/`--before` flags. Use chrono rather than building a date parser.

## Common Pitfalls

### Pitfall 1: FTS5 Query Syntax Errors
**What goes wrong:** User passes text that contains FTS5 operators (`*`, `OR`, `NEAR`, `-`, `"`) causing rusqlite to return an error.
**Why it happens:** FTS5 MATCH interprets certain characters as operators.
**How to avoid:** Wrap user search terms in double quotes for literal matching: `format!("\"{}\"", term.replace('"', "\"\""))`. Allow advanced users to use raw FTS5 syntax via a `--raw` flag if desired.
**Warning signs:** `rusqlite::Error` when searching for terms containing special characters.

### Pitfall 2: Empty Command Text in Existing Records
**What goes wrong:** Phase 6 decision was to leave command text empty (`""`). FTS5 search will not match empty strings, and display will show blank command column.
**Why it happens:** Command text extraction from the terminal grid was deferred.
**How to avoid:** Handle gracefully in display (show `<no command text>` or similar). For `glass history list` (no text search), this works fine since it queries the commands table directly. For `glass history search`, results with empty command text won't appear in FTS results -- this is acceptable behavior.
**Warning signs:** Search returns 0 results even though records exist.

### Pitfall 3: Database Path Resolution in CLI Mode
**What goes wrong:** `resolve_db_path` walks up from cwd looking for `.glass/` directory. In CLI mode, cwd is wherever the user runs `glass history`, which may not be the project directory.
**Why it happens:** The CLI subcommand runs in a different context than the terminal emulator.
**How to avoid:** Use `resolve_db_path(&std::env::current_dir().unwrap_or_default())` -- same as the terminal does. Optionally add a `--db` flag to specify the database path directly. Consider also adding `--global` flag to force querying `~/.glass/global-history.db`.
**Warning signs:** "No matching commands" when user knows commands were recorded -- they're in a different database.

### Pitfall 4: Relative Time Parsing Ambiguity
**What goes wrong:** "1 hour ago" could mean different things depending on parsing approach. Users might type "1h", "1 hour ago", "1hour", etc.
**Why it happens:** No standard format for relative time in CLIs.
**How to avoid:** Support a small set of formats: `Nh` (hours), `Nm` (minutes), `Nd` (days), or ISO 8601 dates. Document supported formats in `--help`. Use humantime-style duration parsing as a fallback.
**Warning signs:** Parse errors on valid-looking time strings.

### Pitfall 5: CWD Filter Matching
**What goes wrong:** `--cwd /project` doesn't match records with cwd `/project/` (trailing slash) or `C:\Users\...` (Windows paths).
**Why it happens:** String equality vs path semantics.
**How to avoid:** Normalize paths before comparison. Use `starts_with` semantics so `--cwd /project` matches `/project/subdir`. Use SQL LIKE with trailing `%`.
**Warning signs:** Filters return empty results despite matching records existing.

## Code Examples

### Expanding the Clap CLI Definition
```rust
// In src/main.rs -- modify existing Commands enum
#[derive(Subcommand, Debug, PartialEq)]
enum Commands {
    /// Query command history
    History {
        #[command(subcommand)]
        action: Option<HistoryAction>,
    },
    /// MCP server commands
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
}
```

### Dynamic SQL Builder with Optional Filters
```rust
// In crates/glass_history/src/query.rs
use rusqlite::{params_from_iter, Connection, types::Value};
use crate::db::CommandRecord;
use anyhow::Result;

pub fn filtered_query(conn: &Connection, filter: &QueryFilter) -> Result<Vec<CommandRecord>> {
    let mut conditions = Vec::new();
    let mut param_values: Vec<Value> = Vec::new();

    // FTS5 text search requires JOIN
    let use_fts = filter.text.is_some();

    if let Some(ref text) = filter.text {
        // Quote for literal FTS5 matching
        let escaped = format!("\"{}\"", text.replace('"', "\"\""));
        conditions.push("commands_fts MATCH ?".to_string());
        param_values.push(Value::Text(escaped));
    }

    if let Some(exit_code) = filter.exit_code {
        conditions.push("c.exit_code = ?".to_string());
        param_values.push(Value::Integer(exit_code as i64));
    }

    if let Some(after) = filter.after {
        conditions.push("c.started_at >= ?".to_string());
        param_values.push(Value::Integer(after));
    }

    if let Some(before) = filter.before {
        conditions.push("c.started_at <= ?".to_string());
        param_values.push(Value::Integer(before));
    }

    if let Some(ref cwd) = filter.cwd {
        conditions.push("c.cwd LIKE ?".to_string());
        param_values.push(Value::Text(format!("{}%", cwd)));
    }

    param_values.push(Value::Integer(filter.limit as i64));

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sql = if use_fts {
        format!(
            "SELECT c.id, c.command, c.cwd, c.exit_code, c.started_at,
                    c.finished_at, c.duration_ms, c.output
             FROM commands_fts f
             JOIN commands c ON c.id = f.rowid
             {} ORDER BY c.started_at DESC LIMIT ?",
            where_clause
        )
    } else {
        format!(
            "SELECT c.id, c.command, c.cwd, c.exit_code, c.started_at,
                    c.finished_at, c.duration_ms, c.output
             FROM commands c
             {} ORDER BY c.started_at DESC LIMIT ?",
            where_clause
        )
    };

    let mut stmt = conn.prepare(&sql)?;
    let results = stmt
        .query_map(params_from_iter(param_values.iter()), |row| {
            Ok(CommandRecord {
                id: Some(row.get(0)?),
                command: row.get(1)?,
                cwd: row.get(2)?,
                exit_code: row.get(3)?,
                started_at: row.get(4)?,
                finished_at: row.get(5)?,
                duration_ms: row.get(6)?,
                output: row.get(7)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(results)
}
```

### Relative Time Parsing (Simple Approach)
```rust
/// Parse a relative or absolute time string into epoch seconds.
/// Supports: "30m", "1h", "2d", "7d", or ISO 8601 "2024-01-15" / "2024-01-15T14:30:00"
fn parse_time(input: &str) -> Result<i64> {
    let now = chrono::Utc::now();

    // Try relative: "30m", "1h", "2d"
    if let Some(num_str) = input.strip_suffix('m') {
        if let Ok(mins) = num_str.trim().parse::<i64>() {
            return Ok((now - chrono::Duration::minutes(mins)).timestamp());
        }
    }
    if let Some(num_str) = input.strip_suffix('h') {
        if let Ok(hours) = num_str.trim().parse::<i64>() {
            return Ok((now - chrono::Duration::hours(hours)).timestamp());
        }
    }
    if let Some(num_str) = input.strip_suffix('d') {
        if let Ok(days) = num_str.trim().parse::<i64>() {
            return Ok((now - chrono::Duration::days(days)).timestamp());
        }
    }

    // Try "N unit ago" format: "1 hour ago", "30 minutes ago"
    // (simplified -- full implementation in plan)

    // Try ISO 8601 date
    if let Ok(dt) = chrono::NaiveDate::parse_from_str(input, "%Y-%m-%d") {
        return Ok(dt.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp());
    }

    // Try ISO 8601 datetime
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(input, "%Y-%m-%dT%H:%M:%S") {
        return Ok(dt.and_utc().timestamp());
    }

    anyhow::bail!("Cannot parse time: '{}'. Use formats like '1h', '30m', '2d', or '2024-01-15'", input)
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| clap 3 app/arg builder | clap 4 derive macros | clap 4.0 (2022) | Project already uses derive -- continue pattern |
| Manual SQL string concat | rusqlite params_from_iter | rusqlite 0.29+ | Safe dynamic parameter binding for variable-length param lists |

**Deprecated/outdated:**
- `chrono::NaiveDateTime::from_timestamp()` was deprecated in chrono 0.4.35 -- use `DateTime::from_timestamp(secs, 0)` instead

## Open Questions

1. **Should `glass history` with no subcommand default to `list`?**
   - What we know: Current `Commands::History` is a unit variant. The requirements say `glass history search <term>` for text search.
   - What's unclear: What happens with bare `glass history`?
   - Recommendation: Make `action: Option<HistoryAction>` and default to `list` with default limit of 25. This matches shell history UX conventions.

2. **Should `--cwd` filter use exact match or prefix match?**
   - What we know: Records store full cwd path (e.g., `/home/user/project`).
   - What's unclear: Whether `--cwd /home/user` should match `/home/user/project/subdir`.
   - Recommendation: Use prefix matching (SQL LIKE with trailing `%`). More useful for filtering by project root.

3. **How to handle the empty command text from Phase 6?**
   - What we know: Phase 6 decision was to store empty command text. Records have cwd, exit_code, timestamps, duration, and output -- but command field is `""`.
   - What's unclear: Whether Phase 7 should also extract command text from the terminal, or just work with what's there.
   - Recommendation: Phase 7 should display whatever is in the database. Show `<empty>` for blank commands. Command text extraction is orthogonal and can be addressed separately.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test (cargo test) |
| Config file | Cargo.toml (workspace) |
| Quick run command | `cargo test -p glass_history --lib query` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| CLI-01 | `glass history search <term>` returns matching commands | unit | `cargo test -p glass --lib -- subcommand_tests` | Partial (existing subcommand tests need expansion) |
| CLI-02 | Filters combine correctly: --exit, --after, --cwd, --limit | unit | `cargo test -p glass_history --lib query` | No -- Wave 0 |
| CLI-03 | Results display as structured terminal output | unit | `cargo test -p glass --lib -- history_display_tests` | No -- Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_history --lib query && cargo test -p glass --lib`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_history/src/query.rs` -- new module with QueryFilter + filtered_query, needs tests
- [ ] `src/history.rs` -- new module for display formatting, needs tests
- [ ] Expand `src/tests.rs` -- add tests for new `HistoryAction` subcommand parsing
- [ ] chrono dependency -- `cargo add chrono` to glass_history or workspace

## Sources

### Primary (HIGH confidence)
- Project source code: `src/main.rs`, `crates/glass_history/src/*.rs` -- direct inspection of existing CLI structure and HistoryDb API
- clap 4.5 derive API -- already in use in project, derive patterns verified from existing code
- rusqlite 0.38 -- `params_from_iter` and `Connection` API verified from existing usage in glass_history

### Secondary (MEDIUM confidence)
- chrono 0.4 API -- `NaiveDateTime`, `Duration`, timestamp methods; standard Rust datetime library
- FTS5 query syntax -- SQLite official documentation for MATCH, quoting, and special characters

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - all libraries already in project except chrono (well-known, stable)
- Architecture: HIGH - extends existing patterns (clap derive, glass_history modules, CommandRecord)
- Pitfalls: HIGH - identified from direct code inspection (empty command text, db path resolution, FTS5 syntax)

**Research date:** 2026-03-05
**Valid until:** 2026-04-05 (stable domain, no fast-moving dependencies)
