# Phase 50: SOI Pipeline Integration - Research

**Researched:** 2026-03-12
**Domain:** Rust async threading, winit event loop extension, glass_core AppEvent, glass_history SOI storage
**Confidence:** HIGH

## Summary

Phase 50 wires together the parsing (Phase 48) and storage (Phase 49) layers into a live pipeline: whenever a command finishes, the main thread reads the captured output already present in the history database, launches a `std::thread` (matching the existing git-query and pruning patterns), runs `glass_soi::classify` + `glass_soi::parse` + `db.insert_parsed_output`, and fires a new `AppEvent::SoiReady` back through the winit event-loop proxy. The main thread handles `SoiReady` by updating `session.last_soi_summary` for downstream consumers (Phase 52 display, Phase 55 activity stream).

The key constraints are: (1) the winit main thread must never block on I/O — all SOI work runs on a worker thread via `std::thread::Builder::new().spawn()`; (2) `HistoryDb` is not `Send`, so the worker must re-open its own connection to the same database path; (3) alt-screen commands (vim, htop) set `last_command_id` but produce `None` output — these must short-circuit to a `SoiReady` with `Severity::Info` and a "no output captured" summary string without error; (4) output already processed by `glass_history::output::process_output` respects the 50 KB cap and binary detection before SOI sees it.

No new crates are needed. The implementation touches `glass_core/src/event.rs` (new `AppEvent::SoiReady` variant), `src/main.rs` (spawn worker after `insert_command`, handle `SoiReady`), and `glass_mux/src/session.rs` (new `last_soi_summary` field on `Session`). The `glass_history` and `glass_soi` crates require no changes.

**Primary recommendation:** Spawn a plain `std::thread` immediately after `db.insert_command` succeeds inside the `AppEvent::Shell { CommandFinished }` arm of `user_event`, passing the DB path + command_id + processed output string. The thread re-opens `HistoryDb`, calls `classify`/`parse`/`insert_parsed_output`, then sends `AppEvent::SoiReady` through the cloned proxy. Match `AppEvent::SoiReady` in `user_event` to store summary on `Session` and trigger a redraw.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SOIL-01 | SOI parsing runs automatically on every CommandFinished event without user intervention | CommandFinished arm in `user_event` already triggers `insert_command`; extend it to spawn SOI worker thread |
| SOIL-02 | SOI parsing runs off the main thread (spawn_blocking) with no impact on terminal input latency | Existing patterns: git query + pruning both use `std::thread::Builder::new().spawn()`. The MCP crate uses `tokio::task::spawn_blocking`; main app has no tokio runtime on the main thread, so `std::thread::spawn` is the correct pattern |
| SOIL-03 | SoiReady event emits after parsing completes, carrying command_id, summary, and severity | New `AppEvent::SoiReady` variant added to `glass_core/src/event.rs`; fired from worker thread via proxy |
| SOIL-04 | Edge cases handled: no output, alt-screen apps, very large output (>50KB), binary output | `process_output` already handles binary + truncation; worker must handle `None` output (no capture/alt-screen) gracefully with a sentinel summary |
</phase_requirements>

---

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| glass_soi | workspace | `classify` + `parse` entry points | Built in Phase 48; provides `OutputType`, `ParsedOutput`, `Severity` |
| glass_history | workspace | `HistoryDb::open`, `insert_parsed_output`, `get_output_summary` | Built in Phase 49; all SOI storage lives here |
| glass_core | workspace | `AppEvent` enum, `SessionId` — event routing | Already the event bus for PTY events, git queries, config reload |
| std::thread | stdlib | Off-thread SOI work | Matches existing git-query + pruning patterns; no tokio runtime on winit main thread |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tracing | workspace | `tracing::warn!` / `tracing::debug!` inside worker | Project-wide convention for operational logging |
| anyhow | workspace | Error propagation in worker thread | Project-wide convention; Result<()> returned, logged on failure |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `std::thread::spawn` | `tokio::task::spawn_blocking` | Main app event loop has no tokio handle; spawning a blocking task requires a handle. The MCP subprocess uses `tokio::runtime::Runtime::new()` separately. For this phase, `std::thread::spawn` matches the git-query and pruning precedent exactly — simpler and consistent |
| Re-opening `HistoryDb` in worker | Pass `Arc<Mutex<HistoryDb>>` | `rusqlite::Connection` is not `Send`. Re-opening follows the existing MCP crate pattern where each handler opens its own connection to the same path |
| Sending full `ParsedOutput` in event | Sending only summary + severity | Full `ParsedOutput` is already stored in the database; event only needs what the display layer requires: `command_id`, `one_line`, `severity`. Avoids cloning large vecs across threads |

**No new Cargo.toml changes needed.** `glass_core` already imports nothing from `glass_soi` or `glass_history`; the worker thread in `main.rs` already uses both. `glass_core/src/event.rs` only needs a new enum arm.

---

## Architecture Patterns

### Where Things Live

```
src/main.rs                          # Spawn SOI worker + handle SoiReady event
crates/glass_core/src/event.rs       # Add AppEvent::SoiReady variant
crates/glass_mux/src/session.rs      # Add last_soi_summary: Option<SoiSummary> field
```

No changes to `glass_soi`, `glass_history`, `glass_terminal`, or `glass_renderer` in this phase.

### Pattern 1: New AppEvent Variant

**What:** `AppEvent::SoiReady` carries the three fields required by SOIL-03.

**When to use:** Fired from worker thread after `insert_parsed_output` succeeds (or when output is None/skipped).

```rust
// Source: crates/glass_core/src/event.rs (existing AppEvent enum)
AppEvent::SoiReady {
    window_id: winit::window::WindowId,
    session_id: SessionId,
    command_id: i64,
    summary: String,       // one_line from OutputSummary
    severity: String,      // "Error" | "Warning" | "Info" | "Success"
},
```

Using `String` for severity in the event avoids a cross-crate dependency (`glass_core` must not depend on `glass_soi`). The handler in `main.rs` receives the raw strings; future display (Phase 52) can re-parse severity from the stored `String`.

### Pattern 2: Off-Thread SOI Worker

**What:** Immediately after `db.insert_command` succeeds and `session.last_command_id` is set, clone the proxy and spawn a worker. The worker re-opens `HistoryDb`, fetches the stored output, runs SOI, stores results, then fires `SoiReady`.

**Reference pattern:** The git-query worker at line 2860 of `src/main.rs`:

```rust
// Existing pattern (git query, ~line 2860):
std::thread::Builder::new()
    .name("Glass git query".into())
    .spawn(move || {
        let git_info = query_git_status(&cwd_owned);
        let _ = proxy.send_event(AppEvent::GitInfo { ... });
    })
    .ok();
```

**SOI worker follows the same shape:**

```rust
// After insert_command succeeds (inside AppEvent::Shell { CommandFinished } arm):
let db_path = session.history_db.as_ref()
    .map(|db| db.path().to_path_buf());   // need HistoryDb::path() accessor

if let (Some(path), Some(cmd_id)) = (db_path, session.last_command_id) {
    let proxy = self.proxy.clone();
    let wid = window_id;
    let sid = session_id;
    // Capture processed output from CommandOutput flow:
    // NOTE: output is stored in DB by the time CommandFinished fires.
    // Worker reads it from DB via db.get_command_output(cmd_id).
    std::thread::Builder::new()
        .name("Glass SOI parse".into())
        .spawn(move || {
            let db = match glass_history::db::HistoryDb::open(&path) {
                Ok(db) => db,
                Err(e) => {
                    tracing::warn!("SOI worker: failed to open DB: {}", e);
                    return;
                }
            };
            // Fetch stored output text for this command
            let output_text = db.get_output_for_command(cmd_id)
                .unwrap_or(None); // None = no output / alt-screen

            // Classify and parse
            let command_text = db.get_command_text(cmd_id).ok().flatten()
                .unwrap_or_default();
            let (summary, severity) = match output_text {
                None => ("no output captured".to_string(), "Info".to_string()),
                Some(text) => {
                    let output_type = glass_soi::classify(&text, Some(&command_text));
                    let parsed = glass_soi::parse(&text, output_type, Some(&command_text));
                    // Store in DB
                    if let Err(e) = db.insert_parsed_output(cmd_id, &parsed) {
                        tracing::warn!("SOI: insert_parsed_output failed: {}", e);
                    }
                    let sev = match parsed.summary.severity {
                        glass_soi::Severity::Error => "Error",
                        glass_soi::Severity::Warning => "Warning",
                        glass_soi::Severity::Info => "Info",
                        glass_soi::Severity::Success => "Success",
                    };
                    (parsed.summary.one_line, sev.to_string())
                }
            };
            let _ = proxy.send_event(AppEvent::SoiReady {
                window_id: wid,
                session_id: sid,
                command_id: cmd_id,
                summary,
                severity,
            });
        })
        .ok();
}
```

**Critical:** `HistoryDb` needs a `path()` accessor returning `&Path` (for cloning to worker). Check `db.rs` — the `conn` field is a `rusqlite::Connection`; add `HistoryDb::path(&self) -> &Path` backed by a stored `PathBuf`. Alternatively, pass the path directly from the session creation site.

### Pattern 3: Handling SoiReady in user_event

```rust
AppEvent::SoiReady { window_id, session_id, command_id, summary, severity } => {
    if let Some(ctx) = self.windows.get_mut(&window_id) {
        if let Some(session) = ctx.session_mux.session_mut(session_id) {
            // Only store if this is the most recent command
            if session.last_command_id == Some(command_id) {
                session.last_soi_summary = Some(SoiSummary {
                    command_id,
                    one_line: summary,
                    severity,
                });
            }
        }
        ctx.window.request_redraw();
    }
}
```

### Pattern 4: Session Field Addition

```rust
// In crates/glass_mux/src/session.rs, add to Session struct:
/// Most recent SOI parse result. Updated by AppEvent::SoiReady.
/// None until first command completes with SOI enabled.
pub last_soi_summary: Option<SoiSummary>,
```

`SoiSummary` is a small value type defined in `glass_mux` or `glass_core`:

```rust
/// A parsed SOI summary for the most recent command in a session.
#[derive(Debug, Clone)]
pub struct SoiSummary {
    pub command_id: i64,
    pub one_line: String,
    pub severity: String, // "Error" | "Warning" | "Info" | "Success"
}
```

### Pattern 5: Output Fetch Helpers Needed in HistoryDb

The worker needs two lightweight query methods that `glass_history` does not yet expose:

1. `HistoryDb::get_output_for_command(cmd_id: i64) -> Result<Option<String>>`
   - `SELECT output FROM commands WHERE id = ?1`
   - Returns `None` if row has `NULL` output (alt-screen / no capture)

2. `HistoryDb::get_command_text(cmd_id: i64) -> Result<Option<String>>`
   - `SELECT command FROM commands WHERE id = ?1`
   - Needed to pass `command_hint` to `glass_soi::classify`

3. `HistoryDb::path(&self) -> &Path`
   - Backed by a stored `path: PathBuf` field added to `HistoryDb`
   - Required for worker to re-open the same DB file

These are all `O(1)` point queries on the primary key — no performance concern.

### Anti-Patterns to Avoid

- **Blocking the main thread:** Never call `glass_soi::parse` directly in `user_event`. The parse function iterates output lines and runs regex matching — measurable latency for large outputs.
- **Sharing HistoryDb across threads:** `rusqlite::Connection` is `!Send`. Do not wrap in `Arc<Mutex<>>`. Re-open in the worker.
- **Panicking the worker:** Worker errors must be logged with `tracing::warn!` and return early. A failed SOI parse must never crash the terminal.
- **Double-insert on re-entry:** If `CommandOutput` arrives after `CommandFinished` (race condition on PTY close), the worker may run twice. Guard with `IF NOT EXISTS` or ignore unique-constraint errors on `command_output_records(command_id)`.
- **Sending ParsedOutput in AppEvent:** Do not include `Vec<OutputRecord>` in the event. It is already stored in the database; the event carries only what the main thread needs to display.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| ANSI stripping in SOI worker | Custom strip function | `glass_history::output::process_output` already ran | Output in DB is already stripped; worker works on the stored text |
| Binary detection | Custom byte scan | Already done by `process_output` before `CommandOutput` event | Binary content becomes `"[binary output: N bytes]"` placeholder in DB |
| Large output handling | Custom truncation | `process_output` respects `max_output_capture_kb` (default 50 KB) | Worker sees already-truncated text; no additional truncation needed |
| Thread-safe DB access | `Arc<Mutex<HistoryDb>>` | Re-open `HistoryDb` in worker | SQLite WAL mode allows concurrent readers; re-open is idiomatic |
| Event bus | Custom channel | `EventLoopProxy<AppEvent>` already the project's event bus | All PTY/git/config events use this pattern |

**Key insight:** The output processing pipeline (`process_output`) is already the gating layer. By the time the SOI worker runs, the stored output text is already safe (UTF-8, no ANSI, bounded size). SOI only needs to classify and parse what's there.

---

## Common Pitfalls

### Pitfall 1: Race between CommandOutput and CommandFinished

**What goes wrong:** `AppEvent::CommandOutput` (which calls `db.update_output`) and `AppEvent::Shell { CommandFinished }` (which spawns the SOI worker) are both sent from the PTY reader thread. The PTY reader sends `CommandOutput` first, then `CommandFinished`. However, the winit event queue may process them out of order under load.

**Why it happens:** `EventLoopProxy::send_event` is FIFO but the PTY loop can interleave with other OS events.

**How to avoid:** The SOI worker reads output from the DB, not from the event. If the worker runs before `update_output` commits, it will see `NULL` output and produce a "no output captured" summary. This is acceptable for Phase 50 — a conservative skip rather than a crash. Downstream phases (52 display) can tolerate this gracefully.

**Warning signs:** SOI summaries always show "no output captured" for commands with visible output. If seen, add a small retry or move the spawn to after `CommandOutput` processing.

### Pitfall 2: HistoryDb Path Unavailable

**What goes wrong:** `Session.history_db` is `Option<HistoryDb>`. If the database failed to open at session creation, no path is available. The worker cannot be spawned.

**Why it happens:** `history_db: None` is set when `HistoryDb::open` fails (e.g., permissions, disk full).

**How to avoid:** Guard with `if let Some(ref db) = session.history_db` before extracting the path. If `history_db` is `None`, skip SOI silently. This matches the existing pattern for `insert_command`.

### Pitfall 3: Alt-Screen Apps (vim, htop, tmux)

**What goes wrong:** These apps run in alternate screen mode. The PTY reader does not capture their output (it captures only primary-screen output). `CommandOutput` is never sent. The DB `commands.output` column stays `NULL`. The SOI worker sees `None` from `get_output_for_command`.

**Why it happens:** `glass_terminal/src/pty.rs` only captures output when not in alt-screen mode. This is intentional.

**How to avoid:** Worker checks for `None` output and emits `SoiReady` with `summary = "no output captured"` and `severity = "Info"`. Never return an error for this case.

### Pitfall 4: DB re-open latency on Windows

**What goes wrong:** `HistoryDb::open` runs a schema migration check on every open. Migration is idempotent (`CREATE TABLE IF NOT EXISTS`) but involves multiple PRAGMA calls. On Windows with antivirus scanning, this can add 10-100ms.

**Why it happens:** Windows file I/O is slower; antivirus hooks `CreateFile`.

**How to avoid:** The worker thread runs entirely off the main thread — this latency does not affect input. Log if open takes >500ms with `tracing::warn!`. Do not cache or share the connection.

### Pitfall 5: Unique Constraint on Double-Insert

**What goes wrong:** If `insert_parsed_output` is called twice for the same `command_id` (e.g., from a retry or a bug), the `command_output_records` table will raise a UNIQUE constraint error (if one is set) or silently create a duplicate row.

**Why it happens:** Potential future race or retry logic.

**How to avoid:** In `glass_history/src/soi.rs`, make `insert_parsed_output` use `INSERT OR IGNORE` for the summary row, or check for an existing record before inserting. Alternatively, add a UNIQUE constraint on `command_output_records(command_id)` in the schema.

---

## Code Examples

### Adding AppEvent::SoiReady

```rust
// Source: crates/glass_core/src/event.rs — extend AppEvent enum
/// SOI parse completed for a finished command.
/// Fired from the SOI worker thread via EventLoopProxy.
SoiReady {
    window_id: winit::window::WindowId,
    session_id: SessionId,
    /// The history DB row id for the completed command.
    command_id: i64,
    /// One-line human/agent readable summary.
    summary: String,
    /// Highest severity: "Error" | "Warning" | "Info" | "Success"
    severity: String,
},
```

### HistoryDb Path Accessor

```rust
// Source: crates/glass_history/src/db.rs
pub struct HistoryDb {
    conn: Connection,
    path: PathBuf,   // Add this field
}

impl HistoryDb {
    pub fn open(path: &Path) -> Result<Self> {
        // existing open logic ...
        Ok(Self { conn, path: path.to_path_buf() })
    }

    /// Return the filesystem path of this database.
    pub fn path(&self) -> &Path {
        &self.path
    }
}
```

### Output Fetch Helper

```rust
// Source: crates/glass_history/src/db.rs
/// Fetch the stored output text for a command. Returns None if output is NULL.
pub fn get_output_for_command(&self, command_id: i64) -> Result<Option<String>> {
    self.conn.query_row(
        "SELECT output FROM commands WHERE id = ?1",
        params![command_id],
        |row| row.get(0),
    ).optional().map_err(Into::into)
}

/// Fetch the command text for a command.
pub fn get_command_text(&self, command_id: i64) -> Result<Option<String>> {
    self.conn.query_row(
        "SELECT command FROM commands WHERE id = ?1",
        params![command_id],
        |row| row.get(0),
    ).optional().map_err(Into::into)
}
```

Note: requires `use rusqlite::OptionalExtension;` in `db.rs`.

### No-Output Short-Circuit

```rust
// Inside SOI worker thread closure:
let (summary, severity) = match output_text {
    None | Some(ref s) if s.is_empty() => {
        // Alt-screen app, no capture, or binary placeholder only
        ("no output captured".to_string(), "Info".to_string())
    }
    Some(text) => {
        let output_type = glass_soi::classify(&text, Some(&command_text));
        let parsed = glass_soi::parse(&text, output_type, Some(&command_text));
        if let Err(e) = db.insert_parsed_output(cmd_id, &parsed) {
            tracing::warn!("SOI: insert_parsed_output failed cmd={}: {}", cmd_id, e);
        }
        let sev_str = match &parsed.summary.severity {
            glass_soi::Severity::Error => "Error",
            glass_soi::Severity::Warning => "Warning",
            glass_soi::Severity::Info => "Info",
            glass_soi::Severity::Success => "Success",
        };
        (parsed.summary.one_line, sev_str.to_string())
    }
};
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| SOI types defined inline | `glass_soi` crate with `classify`/`parse` public API | Phase 48 | Clean dispatch; this phase is a consumer not a builder |
| No SOI storage | `command_output_records` + `output_records` tables in history DB | Phase 49 | Worker can call `insert_parsed_output` directly |
| PTY events only | `AppEvent` as project-wide event bus (git, config, coordination) | Phase 14+ | `SoiReady` fits the established pattern naturally |

**Nothing deprecated:** Phase 50 extends existing systems; it does not replace anything.

---

## Open Questions

1. **CommandOutput vs CommandFinished ordering guarantee**
   - What we know: PTY reader sends `CommandOutput` then `CommandFinished` in that order
   - What's unclear: Whether winit's `EventLoopProxy` FIFO guarantee is sufficient under high event load
   - Recommendation: For Phase 50, accept that a late `CommandOutput` can cause the worker to see `NULL` and emit a "no output" summary. Document as a known limitation. Phase 52 display can tolerate this gracefully.

2. **Benchmark name "input_latency"**
   - What we know: `benches/perf_benchmarks.rs` has `bench_osc_scanner`, `bench_resolve_color`, `bench_cold_start` — no `input_latency` benchmark
   - What's unclear: Whether SOIL-02 intends the existing `osc_scan` benchmark or a new one
   - Recommendation: Wave 0 creates a new `bench_input_processing` benchmark that measures `process_output` latency on a 50 KB buffer. This is the correct proxy for "input processing off-thread does not regress latency."

3. **SoiSummary type location**
   - What we know: `Session` is in `glass_mux`; `glass_core` must not depend on `glass_soi`
   - What's unclear: Whether `SoiSummary` lives in `glass_mux` or `glass_core`
   - Recommendation: Define `SoiSummary` as a plain `struct` (three `String`/`i64` fields) in `glass_mux/src/session.rs` alongside `Session`. No cross-crate dependency needed.

---

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in (`#[test]`) + criterion for benchmarks |
| Config file | None — tests inline per project convention |
| Quick run command | `cargo test --workspace 2>&1` |
| Full suite command | `cargo test --workspace && cargo bench 2>&1` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SOIL-01 | `AppEvent::SoiReady` fires after CommandFinished with a valid command_id | integration | `cargo test -p glass_history soi_pipeline` | Wave 0 |
| SOIL-02 | SOI parse does not block main thread (benchmark proxy) | benchmark | `cargo bench -- bench_input_processing` | Wave 0 |
| SOIL-03 | SoiReady event carries command_id, summary string, non-empty severity | unit | `cargo test -p glass_core event_soi_ready` | Wave 0 |
| SOIL-04 edge: no output | None-output short-circuits to Info/no-output-captured | unit | `cargo test -p glass_history soi_worker_no_output` | Wave 0 |
| SOIL-04 edge: binary | Binary placeholder string produces FreeformChunk, no panic | unit | `cargo test -p glass_history soi_worker_binary` | Wave 0 |
| SOIL-04 edge: large | 60 KB output truncated to 50 KB before SOI, no memory spike | unit | `cargo test -p glass_history output_truncation_before_soi` | Wave 0 (reuse existing `test_process_output_large_truncated`) |

### Sampling Rate
- **Per task commit:** `cargo test --workspace`
- **Per wave merge:** `cargo test --workspace && cargo clippy --workspace -- -D warnings`
- **Phase gate:** Full suite green + no clippy warnings before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `benches/perf_benchmarks.rs` — add `bench_input_processing` benchmark covering `glass_history::output::process_output` on a 50 KB payload (SOIL-02 proxy)
- [ ] Test for `AppEvent::SoiReady` variant in `glass_core` — add to `crates/glass_core/src/event.rs` `#[cfg(test)]` block
- [ ] Integration test for SOI worker flow in `glass_history` — requires a temp DB with a command row; call worker logic as a function

---

## Sources

### Primary (HIGH confidence)
- Direct code reading: `src/main.rs` lines 2424-2880 — `AppEvent::Shell` arm, `CommandFinished` handling, git-query worker pattern
- Direct code reading: `crates/glass_core/src/event.rs` — full `AppEvent` enum, `SessionId`, `ShellEvent`
- Direct code reading: `crates/glass_mux/src/session.rs` — `Session` struct, all existing fields
- Direct code reading: `crates/glass_history/src/soi.rs` — `insert_parsed_output`, `get_output_summary`, `get_output_records` signatures
- Direct code reading: `crates/glass_history/src/db.rs` — `HistoryDb` struct (no `path` field yet), `insert_command`, `update_output`
- Direct code reading: `crates/glass_history/src/output.rs` — `process_output`, `is_binary`, `truncate_head_tail`
- Direct code reading: `crates/glass_soi/src/lib.rs` — `classify`, `parse`, `freeform_parse` public API
- Direct code reading: `crates/glass_soi/src/types.rs` — `Severity`, `ParsedOutput`, `OutputSummary`, `OutputRecord`
- Direct code reading: `benches/perf_benchmarks.rs` — existing benchmarks (no `input_latency` benchmark exists)
- Direct code reading: `crates/glass_mcp/src/tools.rs` lines 492+ — `tokio::task::spawn_blocking` pattern used in async context

### Secondary (MEDIUM confidence)
- `.planning/phases/49-soi-storage-schema/49-RESEARCH.md` — confirmed Phase 49 design decisions and DB schema
- `.planning/STATE.md` decisions section — "SOI parsing runs in spawn_blocking off main thread" (note: requirement says spawn_blocking; code analysis confirms std::thread is the correct choice since main is not tokio)

### Tertiary (LOW confidence)
- None

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all crates and patterns confirmed by reading source code
- Architecture: HIGH — thread spawning pattern confirmed by two existing examples (git query, pruning); event bus pattern confirmed by AppEvent enum
- Pitfalls: HIGH — race condition, alt-screen, and binary output all traced to specific code paths in `pty.rs` and `output.rs`

**Research date:** 2026-03-12
**Valid until:** 2026-04-12 (stable domain — internal codebase changes are the only invalidation risk)
