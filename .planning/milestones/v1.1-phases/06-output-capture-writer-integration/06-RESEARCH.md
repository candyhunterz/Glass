# Phase 6: Output Capture + Writer Integration - Research

**Researched:** 2026-03-05
**Domain:** PTY output capture, SQLite schema migration, scrollback rendering
**Confidence:** HIGH

## Summary

This phase has two independent workstreams: (1) capturing command output from the PTY data flow and persisting it to the history database, and (2) fixing the `display_offset` tech debt so block decorations scroll correctly. Both are well-scoped with clear integration points in the existing codebase.

The output capture challenge is straightforward because Glass already has shell integration (OSC 133 A/B/C/D) tracking command lifecycle through `BlockManager`, and the PTY reader thread already pre-scans all bytes through `OscScanner` before VTE parsing. The capture point is the `pty_read_with_scan` function in `pty.rs` -- bytes flow through there between OSC 133;C (CommandExecuted) and OSC 133;D (CommandFinished). Alternate-screen detection uses `TermMode::ALT_SCREEN` which alacritty_terminal 0.25.1 already tracks (bitflag `1 << 12`).

The `display_offset` fix is a two-line change in `frame.rs` (lines 115 and 169) replacing hardcoded `0` with `snapshot.display_offset`, which `GridSnapshot` already captures from the terminal state.

**Primary recommendation:** Implement output capture as a buffer in the PTY reader thread that accumulates bytes between CommandExecuted and CommandFinished events, then send accumulated output to main thread via a new `AppEvent` variant for async database write.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- Head + tail split when output exceeds the configured max: keep first half and last half with a `[...truncated N bytes...]` marker in between
- Default max output capture: 50KB, configurable via `max_output_capture_kb` in `[history]` TOML config section
- Binary output detection: if high ratio of non-printable bytes, store `[binary output: N bytes]` placeholder instead of raw content
- ANSI escape sequences stripped before storage -- store plain text only for cleaner search and smaller storage

### Claude's Discretion
- Output capture point in the PTY pipeline (where to tap bytes -- OscScanner level, BlockManager level, or separate buffer)
- Alternate-screen detection approach (how to detect vim/less/top and skip capture)
- History writer thread architecture (channel-based, shared buffer, etc.)
- display_offset wiring through frame.rs and block_renderer.rs
- Schema migration strategy for adding output column to commands table
- Capture scope: stdout+stderr interleaving, per-command accumulation, and flush-on-completion timing
- What to store for alternate-screen applications (empty string, placeholder, or null)
- Graceful handling when Glass exits mid-command (partial output storage vs discard)

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| HIST-02 | Command output is captured and stored (truncated to configurable max, default 50KB) | Output buffer in PTY reader thread, ANSI stripping, head+tail truncation, binary detection, schema migration adding `output TEXT` column |
| INFR-02 | Fix display_offset tech debt so block decorations scroll correctly | Replace hardcoded `display_offset = 0` in frame.rs lines 115 and 169 with `snapshot.display_offset` |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| alacritty_terminal | 0.25.1 | Terminal emulation, `TermMode::ALT_SCREEN` flag | Already in use, provides alt-screen detection |
| rusqlite | 0.38.0 | SQLite database, schema migration | Already in use for history database |
| strip-ansi-escapes | latest | Strip ANSI escape sequences from captured output | Standard crate for this exact purpose, avoids hand-rolling a regex |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| std::sync::mpsc | stdlib | Channel for sending captured output from PTY thread to main thread | Follows existing pattern (PtyMsg channel) |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| strip-ansi-escapes | Hand-rolled regex | strip-ansi-escapes handles all edge cases (partial sequences, 256-color, OSC) -- regex misses many |
| strip-ansi-escapes | vte parser to filter | More correct but significantly more code; the crate is ~100 lines and well-tested |

**Installation:**
```bash
cargo add strip-ansi-escapes
```

Note: Check if `strip-ansi-escapes` is needed as a dependency or if manual stripping via the existing VTE parser state is simpler. The crate adds minimal overhead. Alternatively, a simple byte-scanning loop that skips ESC sequences is sufficient since the output is stored as plain text and does not need to be perfectly parsed -- just "good enough" stripping.

## Architecture Patterns

### Recommended Data Flow
```
PTY bytes
  |
  v
pty_read_with_scan()  -- existing function in pty.rs
  |
  +-- OscScanner.scan(data)  -- detects CommandExecuted / CommandFinished
  +-- parser.advance()       -- feeds VTE parser
  +-- OutputBuffer.append(data)  -- NEW: accumulates bytes when capturing
  |
  v
On CommandFinished:
  OutputBuffer.finish() -> raw bytes
  |
  +-- Strip ANSI escapes
  +-- Detect binary content
  +-- Truncate if > max_output_capture_kb
  |
  v
  AppEvent::CommandOutput { output, ... }
  |
  v
Main thread: insert into HistoryDb
```

### Pattern 1: Output Buffer in PTY Reader Thread
**What:** A simple struct that accumulates bytes between CommandExecuted and CommandFinished events, living entirely within the PTY reader thread.
**When to use:** Always -- this is the only safe place to tap PTY bytes without adding locks.
**Example:**
```rust
// Lives in glass_terminal/src/output_capture.rs
pub struct OutputBuffer {
    buffer: Vec<u8>,
    capturing: bool,
    max_bytes: usize,
    total_seen: usize,
    alt_screen: bool,
}

impl OutputBuffer {
    pub fn new(max_bytes: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(max_bytes.min(65536)),
            capturing: false,
            max_bytes,
            total_seen: 0,
            alt_screen: false,
        }
    }

    pub fn start_capture(&mut self) {
        self.buffer.clear();
        self.capturing = true;
        self.total_seen = 0;
        self.alt_screen = false;
    }

    pub fn set_alt_screen(&mut self, active: bool) {
        self.alt_screen = active;
    }

    pub fn append(&mut self, data: &[u8]) {
        if !self.capturing || self.alt_screen {
            return;
        }
        self.total_seen += data.len();
        let remaining = self.max_bytes.saturating_sub(self.buffer.len());
        if remaining > 0 {
            let take = data.len().min(remaining);
            self.buffer.extend_from_slice(&data[..take]);
        }
    }

    pub fn finish(&mut self) -> Option<Vec<u8>> {
        if !self.capturing {
            return None;
        }
        self.capturing = false;
        if self.alt_screen {
            return None; // or Some(b"[alternate screen application]")
        }
        Some(std::mem::take(&mut self.buffer))
    }
}
```

### Pattern 2: Head+Tail Truncation
**What:** When output exceeds max, keep first half and last half with truncation marker.
**When to use:** Post-capture processing before database storage.
**Example:**
```rust
fn truncate_head_tail(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let half = max_bytes / 2;
    let skipped = text.len() - max_bytes;
    // Find safe UTF-8 boundaries
    let head_end = text.floor_char_boundary(half);
    let tail_start = text.ceil_char_boundary(text.len() - half);
    format!(
        "{}\n[...truncated {} bytes...]\n{}",
        &text[..head_end],
        skipped,
        &text[tail_start..],
    )
}
```

### Pattern 3: Binary Detection
**What:** Check ratio of non-printable bytes to determine if output is binary.
**When to use:** Before storing output, after ANSI stripping.
**Example:**
```rust
fn is_binary(data: &[u8]) -> bool {
    if data.is_empty() {
        return false;
    }
    // Sample first 8KB for efficiency
    let sample = &data[..data.len().min(8192)];
    let non_printable = sample.iter().filter(|&&b| {
        b < 0x20 && b != b'\n' && b != b'\r' && b != b'\t'
    }).count();
    // If >30% non-printable, treat as binary
    non_printable as f64 / sample.len() as f64 > 0.30
}
```

### Pattern 4: display_offset Fix
**What:** Replace hardcoded `display_offset = 0` with actual value from GridSnapshot.
**When to use:** Exactly two locations in frame.rs.
**Example:**
```rust
// In frame.rs draw_frame(), BEFORE:
let display_offset = 0; // TODO: wired in Plan 04 from scrollback offset

// AFTER:
let display_offset = snapshot.display_offset;
```

### Anti-Patterns to Avoid
- **Capturing at the OscScanner level:** The scanner only sees raw bytes before VTE parsing. It works for OSC detection but would require duplicating buffer management that already exists in the PTY read loop.
- **Separate capture thread:** Adding another thread just to accumulate bytes would require shared state with the PTY reader -- unnecessary complexity since the PTY reader thread already has access to the bytes.
- **Storing ANSI escapes:** Wastes storage, makes search useless, and causes display corruption if ever shown in a non-terminal context.
- **Blocking the PTY reader for database writes:** Database writes are I/O-bound and could stall the PTY reader. Send captured output to the main thread via AppEvent and write from there (or a dedicated writer thread).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| ANSI escape stripping | Custom regex or byte scanner | `strip-ansi-escapes` crate OR vte-based filter | ANSI has ~20+ escape sequence types (CSI, OSC, DCS, PM, APC, SOS); a regex misses many |
| SQLite schema migration | Conditional ALTER TABLE | `CREATE TABLE IF NOT EXISTS` + `PRAGMA user_version` migration pattern | Robust against partial migrations, concurrent access |
| UTF-8 boundary detection | Manual byte scanning | `str::floor_char_boundary` / `str::ceil_char_boundary` (stable since 1.80) | Avoids panics from splitting inside multi-byte sequences |

**Key insight:** ANSI stripping looks trivial but has dozens of edge cases (256-color sequences, OSC hyperlinks, DCS sequences). Use the crate or accept "good enough" stripping with a simple state machine.

## Common Pitfalls

### Pitfall 1: Capturing Alt-Screen Output
**What goes wrong:** vim/less/top output fills the capture buffer with screen-painting escape sequences that are meaningless as "command output."
**Why it happens:** Alt-screen apps send their own screen buffer content through the same PTY stream.
**How to avoid:** Check `TermMode::ALT_SCREEN` flag on the terminal. When alt-screen is active, pause capture. The flag is already tracked by alacritty_terminal 0.25.1 (`1 << 12`). However, accessing it requires locking the terminal mutex.
**Warning signs:** Captured output contains `\x1b[?1049h` (DECSET alternate screen) or massive escape-heavy content.
**Recommended approach:** Rather than locking the terminal to check ALT_SCREEN on every PTY read, detect the escape sequences `\x1b[?1049h` (enter alt screen) and `\x1b[?1049l` (leave alt screen) in the raw byte stream alongside the OscScanner. This avoids the lock entirely and is more performant.

### Pitfall 2: UTF-8 Boundary Splits
**What goes wrong:** Truncating bytes mid-UTF-8 sequence creates invalid strings.
**Why it happens:** Head+tail split cuts at byte offset, not character boundary.
**How to avoid:** Use `str::floor_char_boundary` and `str::ceil_char_boundary` for safe splitting. Convert bytes to String first with lossy conversion, then truncate.
**Warning signs:** Invalid UTF-8 panics or replacement characters in stored output.

### Pitfall 3: PTY Thread Blocked by Database Write
**What goes wrong:** SQLite INSERT blocks the PTY reader thread, causing visible lag during output.
**Why it happens:** Writing to database on the PTY thread blocks the read loop.
**How to avoid:** Send captured output via `AppEvent` to the main thread. The main thread can write to the database on the event loop (it's a quick INSERT) or spawn a one-shot thread. The existing `AppEvent::Shell` pattern is the model.
**Warning signs:** Terminal freezes briefly after commands with large output.

### Pitfall 4: Output Buffer Growing Unbounded
**What goes wrong:** A command that outputs gigabytes of data fills memory before CommandFinished fires.
**Why it happens:** No cap on the buffer during capture.
**How to avoid:** Cap the buffer at `max_output_capture_kb` bytes during accumulation, not just at truncation time. Stop appending once full. Track `total_seen` for the truncation marker.
**Warning signs:** Memory usage spikes during `find /` or `cat large_file`.

### Pitfall 5: Schema Migration on Existing Databases
**What goes wrong:** Existing databases from Phase 5 don't have the `output` column.
**Why it happens:** `CREATE TABLE IF NOT EXISTS` won't add columns to existing tables.
**How to avoid:** Use `PRAGMA user_version` to track schema version. On open, check version and apply migrations. Version 0 (Phase 5) -> Version 1 (Phase 6) adds `ALTER TABLE commands ADD COLUMN output TEXT`.
**Warning signs:** "no such column: output" errors on upgrade.

### Pitfall 6: display_offset Direction Convention
**What goes wrong:** Block decorations scroll in the wrong direction.
**Why it happens:** `display_offset` in alacritty_terminal counts lines FROM THE BOTTOM of scrollback. Larger values mean further back in history. The `BlockRenderer` uses it as "lines from top" in its visibility check.
**How to avoid:** Understand the convention: `display_offset = 0` means "showing the latest content" (bottom of scrollback). The `BlockManager.visible_blocks()` already takes `display_offset` as a parameter and its filter logic uses it correctly -- it was designed for this. The fix in frame.rs just passes the real value through.
**Warning signs:** Decorations appear at wrong lines during scroll, or all decorations disappear when scrolling up.

## Code Examples

### Schema Migration Pattern
```rust
// In glass_history/src/db.rs
const SCHEMA_VERSION: i64 = 1; // Bump from 0 (Phase 5) to 1 (Phase 6)

fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

    if version < 1 {
        // Phase 6: add output column
        conn.execute_batch(
            "ALTER TABLE commands ADD COLUMN output TEXT;
             PRAGMA user_version = 1;"
        )?;
    }

    Ok(())
}
```

### New AppEvent Variant for Output
```rust
// In glass_core/src/event.rs
pub enum AppEvent {
    // ... existing variants ...

    /// Captured command output ready for database storage.
    CommandOutput {
        window_id: WindowId,
        /// The processed output text (ANSI-stripped, truncated, or binary placeholder)
        output: String,
    },
}
```

### HistoryConfig Extension
```rust
// In glass_history/src/config.rs
pub struct HistoryConfig {
    pub max_age_days: u32,
    pub max_size_bytes: u64,
    pub max_output_capture_kb: u32, // NEW: default 50
}
```

### Alt-Screen Detection via Byte Scanning
```rust
// Detect DECSET/DECRST 1049 in raw PTY bytes without locking terminal
// These are CSI sequences: \x1b[?1049h (enter) and \x1b[?1049l (leave)
// The OscScanner already handles ESC], extend concept for ESC[ detection
// or simply track a boolean flag in OutputBuffer

impl OutputBuffer {
    pub fn check_alt_screen(&mut self, data: &[u8]) {
        // Simple substring search -- these sequences are rarely split across buffers
        // but could be. For robustness, track partial matches.
        if data.windows(8).any(|w| w == b"\x1b[?1049h") {
            self.alt_screen = true;
        }
        if data.windows(8).any(|w| w == b"\x1b[?1049l") {
            self.alt_screen = false;
        }
    }
}
```

### CommandRecord with Output
```rust
// Updated struct in glass_history/src/db.rs
pub struct CommandRecord {
    pub id: Option<i64>,
    pub command: String,
    pub cwd: String,
    pub exit_code: Option<i32>,
    pub started_at: i64,
    pub finished_at: i64,
    pub duration_ms: i64,
    pub output: Option<String>, // NEW
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Hardcoded `display_offset = 0` | Pass `snapshot.display_offset` through | Phase 6 (now) | Block decorations finally scroll correctly |
| No output capture | Buffer in PTY thread + async DB write | Phase 6 (now) | HIST-02 fulfilled |
| `commands` table without output | Schema migration adds `output TEXT` | Phase 6 (now) | Backward-compatible upgrade |

**Deprecated/outdated:**
- Nothing deprecated in this phase; all changes are additive.

## Open Questions

1. **ANSI stripping approach**
   - What we know: `strip-ansi-escapes` crate exists and handles standard cases. Alternatively, a simple state machine skipping ESC sequences works for "good enough" stripping.
   - What's unclear: Whether adding a dependency is worth it vs. ~40 lines of code.
   - Recommendation: Use the crate if it compiles cleanly. Otherwise, hand-roll a simple ESC-skip loop since perfect ANSI parsing is not critical for stored plain text.

2. **When to write to database**
   - What we know: History DB writes currently don't happen at all (Phase 5 built the DB but didn't wire it into the terminal loop). Phase 6 needs to establish the write pattern.
   - What's unclear: Whether the main thread event loop is the right place for DB writes, or if a dedicated writer thread is better.
   - Recommendation: Start with main thread writes via `AppEvent`. A single INSERT with output takes <1ms even for 50KB. If profiling shows issues, add a writer thread later.

3. **Partial output on early exit**
   - What we know: If Glass window is closed mid-command, the PTY reader thread exits without CommandFinished.
   - What's unclear: Whether partial output should be stored or discarded.
   - Recommendation: Discard partial output for v1.1. The command record itself (without output) is still valuable. Partial output capture adds complexity with unclear user value.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test framework (cargo test) |
| Config file | Cargo.toml (workspace) |
| Quick run command | `cargo test -p glass_history` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| HIST-02a | Output buffer accumulates bytes between CommandExecuted and CommandFinished | unit | `cargo test -p glass_terminal output_buffer` | No - Wave 0 |
| HIST-02b | Alt-screen detection pauses capture | unit | `cargo test -p glass_terminal alt_screen` | No - Wave 0 |
| HIST-02c | Binary detection returns placeholder | unit | `cargo test -p glass_history binary_detect` | No - Wave 0 |
| HIST-02d | Head+tail truncation preserves first/last halves | unit | `cargo test -p glass_history truncate` | No - Wave 0 |
| HIST-02e | ANSI stripping produces plain text | unit | `cargo test -p glass_history ansi_strip` | No - Wave 0 |
| HIST-02f | Schema migration adds output column to existing DB | unit | `cargo test -p glass_history migration` | No - Wave 0 |
| HIST-02g | CommandRecord with output inserts and retrieves correctly | unit | `cargo test -p glass_history output_roundtrip` | No - Wave 0 |
| HIST-02h | max_output_capture_kb config parsing | unit | `cargo test -p glass_history config` | Partial (config.rs) |
| INFR-02a | display_offset wired through frame.rs | integration | Manual verification (visual) | No - manual-only |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_terminal -p glass_history`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_terminal/src/output_capture.rs` -- OutputBuffer struct and tests (HIST-02a, HIST-02b)
- [ ] `crates/glass_history/src/output.rs` or additions to `db.rs` -- truncation, binary detection, ANSI stripping tests (HIST-02c, HIST-02d, HIST-02e)
- [ ] Schema migration tests in `db.rs` (HIST-02f)
- [ ] Updated `insert_command` / `get_command` tests with output field (HIST-02g)
- [ ] `max_output_capture_kb` in HistoryConfig (HIST-02h)

## Sources

### Primary (HIGH confidence)
- Codebase inspection: `crates/glass_terminal/src/pty.rs` -- PTY reader thread architecture, OscScanner integration point
- Codebase inspection: `crates/glass_terminal/src/block_manager.rs` -- command lifecycle states
- Codebase inspection: `crates/glass_history/src/db.rs` -- current schema, CommandRecord struct
- Codebase inspection: `crates/glass_renderer/src/frame.rs` -- lines 115, 169 with hardcoded `display_offset = 0`
- Codebase inspection: `crates/glass_renderer/src/block_renderer.rs` -- accepts `display_offset` parameter already
- alacritty_terminal 0.25.1 source: `TermMode::ALT_SCREEN = 1 << 12` confirmed in term/mod.rs line 69
- alacritty_terminal 0.25.1 source: `\x1b[?1049h/l` for DECSET/DECRST alternate screen buffer

### Secondary (MEDIUM confidence)
- `str::floor_char_boundary` / `str::ceil_char_boundary` -- stable since Rust 1.80 (verify project MSRV)
- `strip-ansi-escapes` crate -- well-maintained, standard solution for ANSI stripping

### Tertiary (LOW confidence)
- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all code is in the local codebase and verified
- Architecture: HIGH -- data flow path clearly traced through pty.rs, block_manager.rs, frame.rs
- Pitfalls: HIGH -- based on direct code inspection of existing patterns and alacritty_terminal internals

**Research date:** 2026-03-05
**Valid until:** 2026-04-05 (stable codebase, no external API dependencies)
