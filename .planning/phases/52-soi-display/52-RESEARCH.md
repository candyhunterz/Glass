# Phase 52: SOI Display - Research

**Researched:** 2026-03-13
**Domain:** Rust GPU rendering (wgpu/glyphon), PTY write injection, TOML config extension, glass_history compress API
**Confidence:** HIGH

## Summary

Phase 52 is the first user-visible SOI phase. It has two distinct output surfaces: (1) a muted decoration line rendered by the GPU renderer on each completed command block — never touching the PTY stream — and (2) an opt-in hint line that IS injected into the PTY stream so that AI agents using the Bash tool can read SOI data. A third deliverable is the `[soi]` config section with hot-reload support.

The compression engine from Phase 51 (`HistoryDb::compress_output`) is already the correct API for getting the one-line summary. The block renderer (`BlockRenderer` in `glass_renderer`) already knows the pattern for adding muted text decorations to blocks — `build_block_text` produces `BlockLabel` structs that `draw_frame` renders via glyphon. Adding an SOI decoration follows the exact same path. The `SoiReady` event already fires after every classified command and stores the result in `session.last_soi_summary`; Phase 52 just needs to plumb that summary into a new field on `Block` so `BlockRenderer` can render it, and conditionally inject a hint line via `session.pty_sender.send(PtyMsg::Input(...))`.

The key architectural decision from the STATE.md log is pre-confirmed: "SOI summaries rendered as block decorations (NOT injected into PTY stream) to avoid OSC 133 race condition." The shell_summary hint line injection is a separate, opt-in feature gated behind the config section, and it must not affect the block decoration path.

**Primary recommendation:** (1) Add `soi_summary: Option<String>` and `soi_severity: Option<String>` fields to `Block` in glass_terminal. (2) When `AppEvent::SoiReady` fires in main.rs, find the block whose `started_epoch` matches the command and populate those fields. (3) Extend `BlockRenderer::build_block_text` to emit a `BlockLabel` for SOI summary text. (4) In the same `SoiReady` handler, if `config.soi.shell_summary == true` and the summary is non-empty, inject an ANSI-muted hint line via `pty_sender`. (5) Add `SoiSection` to `GlassConfig` with three fields: `enabled`, `shell_summary`, `format`.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SOID-01 | SOI one-line summary renders as block decoration on completed command blocks | `BlockRenderer::build_block_text` already produces `BlockLabel` structs for other decorations (badge, duration, undo); same pattern adds an SOI label anchored to the block separator line. `Block` needs two new fields: `soi_summary: Option<String>` and `soi_severity: Option<String>`. The `SoiReady` handler finds the matching block by command_id correlation via `last_command_id`. |
| SOID-02 | Shell summary hint line injected into PTY output stream for agent Bash tool discovery (configurable, respects min-lines threshold) | `PtyMsg::Input(Cow::Owned(bytes))` is the established mechanism for sending content to the PTY (used for shell integration injection and clipboard paste). The hint line is written in the `SoiReady` handler if `config.soi.shell_summary == true` and output line count exceeds `min_lines`. Format: ANSI dim text on its own line, ending with `\r\n`. Must not include OSC sequences to avoid triggering OSC 133 parsing. |
| SOID-03 | SOI display configurable via [soi] config section (enabled, shell_summary, format) | `GlassConfig` uses `Option<SoiSection>` pattern for all subsystem sections (history, snapshot, pipes). Add `SoiSection` with `enabled: bool` (default true), `shell_summary: bool` (default false), `format: String` (default "oneline"). Config hot-reload via existing `ConfigReloaded` event path — no new mechanism needed. |
</phase_requirements>

---

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| glass_history | workspace | `HistoryDb::compress_output(cmd_id, TokenBudget::OneLine)` — get the one-line text | Already implemented in Phase 51; no new DB queries needed |
| glass_terminal (Block) | workspace | `Block` struct — add `soi_summary` and `soi_severity` fields | Block is the rendering unit; decorations are keyed on block fields |
| glass_renderer (BlockRenderer) | workspace | `build_block_text` extended to emit SOI label | Established pattern for all block text overlays |
| glass_core (GlassConfig) | workspace | `SoiSection` added to config; hot-reload via `ConfigReloaded` | All subsystem configs live here; pattern identical to `PipesSection` |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| alacritty_terminal::vte::ansi::Rgb | =0.25.1 | Color type for `BlockLabel` | Required by BlockLabel.color field |
| glyphon | workspace | Text buffer and text area — already used in `draw_frame` overlay path | Used in the overlay_buffers/overlay_metas pattern in frame.rs |
| serde / toml | workspace | `#[derive(Deserialize)]` for `SoiSection` | Same as all other config sections |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Storing summary on `Block` | Keeping it only on `session.last_soi_summary` | `last_soi_summary` is session-level (only the most recent command). If the user scrolls back to older commands, the decoration must still render — requires the summary to live on the `Block` itself. |
| `PtyMsg::Input` for hint line | A separate OSC sequence | `PtyMsg::Input` is the correct mechanism (already used for shell integration injection). A new OSC sequence would require changes to `OscScanner` and would create a feedback loop with shell integration's own OSC 133 parsing. |
| Separate module for SOI rendering | Extending `BlockRenderer` | `BlockRenderer` is stateless and already handles all block text/rect overlays. The SOI decoration is just another label — no new renderer type needed. |

**No new Cargo.toml changes needed.** All dependencies already exist in the workspace.

---

## Architecture Patterns

### Where Things Live

```
crates/glass_terminal/src/block_manager.rs  # Add soi_summary / soi_severity fields to Block
crates/glass_renderer/src/block_renderer.rs # Add SOI label to build_block_text()
crates/glass_core/src/config.rs             # Add SoiSection, update GlassConfig
src/main.rs                                  # SoiReady handler: populate block fields + inject hint
```

No new files needed. No new crates. No new Cargo dependencies.

### Pattern 1: Block Fields for SOI Decoration (SOID-01)

**What:** Two new `Option` fields on `Block` — populated from the `SoiReady` event handler.

**When to use:** Block decoration rendering path reads these fields; renderer never touches the session.

```rust
// Source: crates/glass_terminal/src/block_manager.rs — Block struct
/// One-line SOI summary text, set after SoiReady fires. None until classified.
pub soi_summary: Option<String>,
/// Highest severity string from SOI parse: "Error" | "Warning" | "Info" | "Success".
pub soi_severity: Option<String>,
```

Both fields default to `None` in `Block::new()`. The `BlockRenderer` checks for `Some` before emitting a label.

### Pattern 2: BlockLabel for SOI Decoration in build_block_text() (SOID-01)

**What:** New label emitted at the end of `build_block_text()` — appears on the separator line of completed blocks that have an SOI summary.

**When to use:** `build_block_text` already iterates blocks and emits labels; SOI label follows the same structure as the duration and undo labels.

```rust
// Source: crates/glass_renderer/src/block_renderer.rs — build_block_text()
// After the existing duration and [undo] label logic:
if let Some(ref soi_text) = block.soi_summary {
    // Position: left side of separator line, with a small left margin
    let soi_color = soi_color_for_severity(block.soi_severity.as_deref());
    labels.push(BlockLabel {
        x: self.cell_width * 1.0, // one cell indent from left edge
        y,
        text: soi_text.clone(),
        color: soi_color,
    });
}
```

**Color mapping by severity** — muted/dim palette, not the same as exit code badges:

```rust
// Local helper in block_renderer.rs
fn soi_color_for_severity(severity: Option<&str>) -> Rgb {
    match severity {
        Some("Error")   => Rgb { r: 200, g: 80,  b: 80  }, // muted red
        Some("Warning") => Rgb { r: 200, g: 160, b: 60  }, // muted amber
        Some("Info")    => Rgb { r: 100, g: 160, b: 200 }, // muted blue
        Some("Success") => Rgb { r: 80,  g: 160, b: 80  }, // muted green
        _               => Rgb { r: 140, g: 140, b: 140 }, // neutral gray
    }
}
```

### Pattern 3: SoiReady Handler — Populating Block Fields (SOID-01)

**What:** In the `AppEvent::SoiReady` handler in `main.rs`, after storing `session.last_soi_summary`, find the most-recent complete block and set its SOI fields.

**When to use:** This is the only place block SOI fields are populated; happens exactly once per completed command (if SOI parsing succeeds).

```rust
// Source: src/main.rs — AppEvent::SoiReady handler
AppEvent::SoiReady { window_id, session_id, command_id, summary, severity } => {
    if let Some(ctx) = self.windows.get_mut(&window_id) {
        if let Some(session) = ctx.session_mux.session_mut(session_id) {
            if session.last_command_id == Some(command_id) {
                session.last_soi_summary = Some(SoiSummary {
                    command_id,
                    one_line: summary.clone(),
                    severity: severity.clone(),
                });
                // Populate the most-recent complete block's SOI fields
                // (last block in blocks() that is Complete)
                if let Some(block) = session.block_manager.blocks_mut()
                    .iter_mut()
                    .rev()
                    .find(|b| b.state == glass_terminal::BlockState::Complete)
                {
                    block.soi_summary = Some(summary.clone());
                    block.soi_severity = Some(severity.clone());
                }
                // Shell summary hint injection (SOID-02, if enabled)
                // ... see Pattern 4
            }
        }
        ctx.window.request_redraw();
    }
}
```

**Why reverse-search for Complete blocks:** The SOI worker fires after `CommandFinished`, which transitions the current block to `Complete`. `self.current` in `BlockManager` still points to the completed block, but the next `PromptStart` will advance it. Using `rev().find(Complete)` is robust across timing variations.

### Pattern 4: Shell Summary Hint Line Injection (SOID-02)

**What:** After populating the block, optionally inject a hint line into the PTY stream that shell tools (e.g., Claude Code's Bash tool) will see in their output.

**When to use:** Only when `config.soi.shell_summary == true` AND `config.soi.enabled == true` AND the summary is non-empty.

```rust
// Source: src/main.rs — inside AppEvent::SoiReady handler, after block population
let soi_cfg = self.config.soi.as_ref();
let shell_summary_enabled = soi_cfg.map(|s| s.enabled && s.shell_summary).unwrap_or(false);
if shell_summary_enabled && !summary.is_empty() {
    // ANSI dim/italic hint line — muted so it is visually distinct from command output.
    // No OSC sequences — would be parsed by OscScanner and cause side effects.
    // Format: "\x1b[2m[soi] {summary}\x1b[0m\r\n"
    let hint = format!("\x1b[2m[glass-soi] {}\x1b[0m\r\n", summary);
    let _ = session.pty_sender.send(PtyMsg::Input(
        std::borrow::Cow::Owned(hint.into_bytes())
    ));
}
```

**Min-lines threshold:** SOID-02 mentions "respects min-lines threshold." The `SoiSummary` stored in `session.last_soi_summary` does not carry line count, but the `CommandOutputSummaryRow.raw_line_count` in the DB does. For Phase 52, a simple check is sufficient: only inject if the command produced output at all (summary not equal to the empty-output fallback). A `min_lines` field in `SoiSection` with default `0` (disabled) allows config control without requiring a DB fetch in the hot path.

### Pattern 5: SoiSection Config (SOID-03)

**What:** New config section following the exact same pattern as `PipesSection`.

```rust
// Source: crates/glass_core/src/config.rs
/// SOI display configuration in the `[soi]` TOML section.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct SoiSection {
    /// Whether SOI decorations and hint lines are enabled. Default true.
    #[serde(default = "default_soi_enabled")]
    pub enabled: bool,
    /// Whether to inject a hint line into the PTY stream after each classified
    /// command. Off by default; intended for AI agent environments. Default false.
    #[serde(default = "default_soi_shell_summary")]
    pub shell_summary: bool,
    /// Summary format for the block decoration. Currently only "oneline" is
    /// implemented; reserved for future formats. Default "oneline".
    #[serde(default = "default_soi_format")]
    pub format: String,
    /// Minimum output line count before injecting a shell summary hint.
    /// 0 = inject for all classified commands. Default 0.
    #[serde(default)]
    pub min_lines: u32,
}
fn default_soi_enabled() -> bool { true }
fn default_soi_shell_summary() -> bool { false }
fn default_soi_format() -> String { "oneline".to_string() }
```

Add `pub soi: Option<SoiSection>` to `GlassConfig` (field + `Default` impl setting it to `None`).

**Hot-reload:** `ConfigReloaded` event already propagates the new config to `self.config` in main.rs. The `SoiReady` handler reads `self.config.soi` at event time, so hot-reload is automatically respected — no additional wiring needed.

**Disabling without restart (SOID-03 criterion 3):** Setting `soi.enabled = false` causes `shell_summary_enabled` (Pattern 4) to be false (no new hint lines), and `build_block_text` should skip the SOI label when `block.soi_summary.is_some()` but `config.soi.enabled == false`. Since `draw_frame` doesn't currently pass config, the simplest approach is: only set `block.soi_summary` if `config.soi.enabled == true` at `SoiReady` time. If the user disables SOI mid-session, existing block labels remain (they are already committed), but new commands will not get labels. This is acceptable behavior — the requirement says "without requiring a restart," not "removes labels retroactively."

### Anti-Patterns to Avoid

- **Injecting OSC sequences in the hint line:** The hint text goes through `OscScanner` in the PTY reader thread. Any `\x1b]` (OSC start) would be parsed as a shell integration event. Use only SGR sequences (`\x1b[...m`) for styling.
- **Fetching from HistoryDb in draw_frame:** The GPU rendering path must not do DB I/O. All SOI data must already be on `Block` when `draw_frame` is called.
- **Storing the full CompressedOutput on Block:** Block is cloned for rendering. Store only the display-ready `String` (and severity string), not the full `CompressedOutput` with record_ids.
- **Positioning SOI label to overlap exit code badge:** The exit code badge occupies the right edge. Duration is left of the badge. Undo is left of duration. SOI label is at the left edge of the separator line (x = 1 cell width) — the opposite end — avoiding all collisions.
- **Re-fetching summary from HistoryDb in SoiReady handler:** The `summary` field in `AppEvent::SoiReady` is already the one-line text produced by Phase 50's SOI worker. No additional DB fetch is needed for SOID-01. Phase 53 MCP tools will do full `compress_output()` calls on demand.
- **Using `current_block_mut()` blindly:** `session.block_manager.current` points to the block that was active when `CommandFinished` fired. If a new `PromptStart` fires between `CommandFinished` and `SoiReady` (race condition on fast shells), `current` already points to the new block. Using `rev().find(Complete)` avoids this race.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| One-line summary text | Re-query HistoryDb from SoiReady handler | `AppEvent::SoiReady.summary` field (already contains the text) | The SOI worker already computed it; the event carries it directly |
| Severity color mapping | Dynamic color calculation from string | Static `soi_color_for_severity()` local function | Five cases; static match is zero-overhead and always correct |
| Config hot-reload for soi section | New watcher or polling | Existing `ConfigReloaded` event path | Already wired for all other sections; free behavior |
| Muted styling for hint line | Custom ANSI escape builder | `format!("\x1b[2m...\x1b[0m\r\n", text)` | SGR 2 (dim) is universally supported; no need for a crate |

**Key insight:** The SOI summary text and severity are already computed and delivered by Phase 50's infrastructure. Phase 52 is entirely a display/routing concern — no new computation, no new DB schema, no new crates.

---

## Common Pitfalls

### Pitfall 1: Block SOI Fields Not Populated When SoiReady Is Late

**What goes wrong:** The SOI worker runs off-thread and may complete after the next `PromptStart` event has fired. If the `SoiReady` handler only writes to `current_block_mut()`, it will write to the new (wrong) block.

**Why it happens:** Shell integration sequences arrive quickly; fast shells (especially fish) emit the next `PromptStart` within milliseconds of `CommandFinished`. The SOI worker takes longer.

**How to avoid:** In the `SoiReady` handler, search for the last `Complete` block, not the current block. The check `if session.last_command_id == Some(command_id)` (already in place) guards against writing to a completely wrong session, but not against the within-session ordering issue. Adding `rev().find(|b| b.state == Complete)` is the correct guard.

**Warning signs:** SOI decorations appear on the wrong block (on the new prompt separator instead of the completed command separator).

### Pitfall 2: Hint Line Appears as Shell Input, Not Output

**What goes wrong:** Writing to the PTY via `PtyMsg::Input` while the shell is at a prompt will cause the hint line to appear as if the user typed it, possibly corrupting the prompt line display.

**Why it happens:** `PtyMsg::Input` sends bytes to the PTY's stdin. The shell echoes typed characters and renders them in the prompt line. A `\r\n` terminates a command, which the shell would try to execute.

**How to avoid:** The hint line must only be injected when the shell is NOT at an interactive prompt — specifically, it should be sent immediately after the command's output ends (during the `SoiReady` event). At this point, the shell has finished the command but has NOT yet printed the next prompt. The PTY is in "output mode" — bytes written appear as terminal output, not as interactive input. This window is correct; no additional timing guard is needed.

**Warning signs:** Shell executes `[glass-soi] 3 errors in src/main.rs` as a command.

**Important nuance:** This pitfall is avoided by the timing — `SoiReady` fires after `CommandFinished` but before the shell prints the next prompt. The SOI worker takes on the order of milliseconds; the shell takes at least one round trip to print the next prompt. The race is extremely unlikely but acknowledged.

### Pitfall 3: SOI Label Overlaps with Other Block Labels at Right Edge

**What goes wrong:** If the SOI summary text is long (e.g., "47 errors in crates/glass_terminal/src/block_manager.rs"), it extends rightward and overlaps the duration or badge labels.

**Why it happens:** `draw_frame` renders all `BlockLabel`s as independent `TextArea` entries; there is no collision detection.

**How to avoid:** Position the SOI label at `x = cell_width * 1.0` (left edge, one-cell indent). Long text will be clipped by the `TextArea`'s width bound (`w - label.x` in `draw_frame`). Truncate the summary to a safe maximum character count before setting `block.soi_summary` — or set a max width in pixels when creating the TextArea buffer (the `buffer.set_size(Some(max_width), ...)` call in frame.rs already sets `Some(w - label.x)` for each overlay buffer). Left-alignment avoids all right-side collisions.

**Warning signs:** Text visually overlapping badge or duration at the right edge of the block separator.

### Pitfall 4: GlassConfig Default Does Not Include SoiSection

**What goes wrong:** Adding a field without updating `Default` will cause a compile error (`missing field`).

**Why it happens:** `GlassConfig` has a manual `Default` implementation (not `#[derive(Default)]`) because of the platform-conditional `font_family`.

**How to avoid:** Add `soi: None` to the `Default` impl body in `config.rs`. This is the same pattern as `history: None`, `snapshot: None`, `pipes: None`.

### Pitfall 5: hint_line min_lines Check Requires Extra DB Fetch

**What goes wrong:** SOID-02 says the hint respects a "min-lines threshold." If this check requires reading `raw_line_count` from `command_output_records`, it forces a synchronous DB read in the `SoiReady` handler (main thread event loop).

**Why it happens:** The `AppEvent::SoiReady` payload only carries `command_id`, `summary`, and `severity`. It does not carry `raw_line_count`.

**How to avoid:** Two options:
1. Add `raw_line_count: i64` to the `AppEvent::SoiReady` payload — the SOI worker already has access to `CommandOutputSummaryRow.raw_line_count` after parsing.
2. Only inject the hint if summary is non-trivial (i.e., the SOI worker produced actual records, not just a FreeformChunk fallback).

Option 1 is cleaner. The SOI worker in `main.rs` (around line 2933) has the `OutputSummary` returned from `glass_soi::parse()`, which contains `raw_line_count`. Add this to the event and use it in the main thread without a DB round-trip.

---

## Code Examples

Verified patterns from existing codebase sources:

### Adding a Field to Block (glass_terminal/src/block_manager.rs)

```rust
// Source: crates/glass_terminal/src/block_manager.rs — Block struct
// Existing fields for reference:
pub has_snapshot: bool,          // bool field, defaults false in Block::new()
pub pipeline_expanded: bool,     // bool field, defaults false in Block::new()
// New pattern (same structure):
pub soi_summary: Option<String>, // None in Block::new()
pub soi_severity: Option<String>, // None in Block::new()
```

```rust
// Source: crates/glass_terminal/src/block_manager.rs — Block::new()
// Existing:
has_snapshot: false,
pipeline_expanded: false,
// Add:
soi_summary: None,
soi_severity: None,
```

### Adding a Label in build_block_text (glass_renderer/src/block_renderer.rs)

```rust
// Source: crates/glass_renderer/src/block_renderer.rs — build_block_text()
// Existing [undo] label pattern:
if block.has_snapshot && block.state == BlockState::Complete {
    let undo_text = "[undo]";
    // ... compute x position ...
    labels.push(BlockLabel { x: undo_x, y, text: undo_text.to_string(), color: ... });
}
// New SOI label pattern (same structure, left-anchored):
if let Some(ref soi_text) = block.soi_summary {
    labels.push(BlockLabel {
        x: self.cell_width * 1.0,
        y,
        text: soi_text.clone(),
        color: soi_color_for_severity(block.soi_severity.as_deref()),
    });
}
```

### PtyMsg::Input Injection Pattern (src/main.rs)

```rust
// Source: src/main.rs line ~445 — shell integration injection
let _ = pty_sender.send(PtyMsg::Input(Cow::Owned(inject_cmd.into_bytes())));

// For hint line injection in SoiReady handler:
let hint = format!("\x1b[2m[glass-soi] {}\x1b[0m\r\n", summary);
let _ = session.pty_sender.send(PtyMsg::Input(
    std::borrow::Cow::Owned(hint.into_bytes())
));
```

### SoiSection Config Pattern (glass_core/src/config.rs)

```rust
// Source: crates/glass_core/src/config.rs — PipesSection (exact pattern to follow)
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PipesSection {
    #[serde(default = "default_pipes_enabled")]
    pub enabled: bool,
    // ... other fields
}
fn default_pipes_enabled() -> bool { true }

// Add to GlassConfig struct:
pub soi: Option<SoiSection>,

// Add to GlassConfig::Default impl:
soi: None,
```

### AppEvent::SoiReady — Adding raw_line_count (glass_core/src/event.rs)

```rust
// Source: crates/glass_core/src/event.rs — AppEvent::SoiReady
// Current:
SoiReady {
    window_id: winit::window::WindowId,
    session_id: SessionId,
    command_id: i64,
    summary: String,
    severity: String,
}
// Proposed addition for min_lines support:
SoiReady {
    // ... existing fields ...
    /// Raw output line count, for min_lines threshold check.
    raw_line_count: i64,
}
```

The SOI worker at line ~2933 in main.rs already calls `glass_soi::parse()` which returns `ParsedOutput` containing `OutputSummary` with `raw_line_count`. This field can be extracted and added to the event without any additional DB fetch.

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| No SOI display — raw `session.last_soi_summary` stored but never rendered | Block decoration + optional hint line | Phase 52 | First user-visible SOI output |
| Block decorations only on right side (badge, duration, undo) | Add left-side SOI summary label | Phase 52 | Left edge of separator becomes the classification surface |
| No `[soi]` config section | `SoiSection` with `enabled`, `shell_summary`, `format`, `min_lines` | Phase 52 | Users can disable all SOI UI without restarting |

**Nothing deprecated:** Phase 52 adds new fields and behaviors without changing existing rendering paths.

---

## Open Questions

1. **Should the hint line prefix be `[glass-soi]` or something shorter?**
   - What we know: The agent needs to identify the hint line in output
   - What's unclear: Whether the prefix should be user-configurable via `format` field
   - Recommendation: Use `[glass-soi]` as a stable prefix (easy for agents to grep); make it a constant. The `format` field is reserved for future use.

2. **What happens to block SOI fields on terminal scroll/history trim?**
   - What we know: `BlockManager.blocks` grows without bound until a session ends; history trimming affects the terminal's scrollback grid but not `blocks`
   - What's unclear: Whether there is a block pruning mechanism that would lose SOI fields
   - Recommendation: No change needed — blocks are not pruned mid-session. SOI fields persist as long as the block is in memory.

3. **Should draw_frame receive config to conditionally suppress SOI labels?**
   - What we know: Currently, `draw_frame` does not receive any config. All block fields are render-on-presence (if field is Some, render it). SOID-03 says disabling should suppress decorations.
   - Recommendation: The simpler approach: only populate `block.soi_summary` when `config.soi.enabled == true` at SoiReady time. Don't pass config into the renderer. Accept that toggling enabled mid-session won't retroactively clear existing block decorations — new commands will not get decorations while disabled.

---

## Validation Architecture

> `workflow.nyquist_validation` is `true` in `.planning/config.json` — section included.

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in (`#[test]`) |
| Config file | None — tests inline per project convention |
| Quick run command | `cargo test -p glass_renderer -- block_renderer` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SOID-01 | `build_block_text` emits a SOI label for a block with `soi_summary = Some(...)` | unit | `cargo test -p glass_renderer -- block_renderer::test_soi_label_emitted_for_complete_block` | Wave 0 |
| SOID-01 | `build_block_text` does NOT emit SOI label for a block with `soi_summary = None` | unit | `cargo test -p glass_renderer -- block_renderer::test_soi_label_absent_when_no_summary` | Wave 0 |
| SOID-01 | SOI label color is muted red for Error severity | unit | `cargo test -p glass_renderer -- block_renderer::test_soi_label_color_error` | Wave 0 |
| SOID-01 | SOI label is positioned at x = cell_width (left edge) | unit | `cargo test -p glass_renderer -- block_renderer::test_soi_label_left_anchored` | Wave 0 |
| SOID-02 | Hint line format is `\x1b[2m[glass-soi] {summary}\x1b[0m\r\n` | unit | `cargo test -p glass_core -- config::test_soi_section_defaults` | Wave 0 |
| SOID-03 | `SoiSection` defaults: `enabled=true`, `shell_summary=false`, `format="oneline"`, `min_lines=0` | unit | `cargo test -p glass_core -- config::test_soi_section_defaults` | Wave 0 |
| SOID-03 | `GlassConfig::load_from_str` parses `[soi]` section correctly | unit | `cargo test -p glass_core -- config::test_soi_section_roundtrip` | Wave 0 |
| SOID-03 | `GlassConfig` with no `[soi]` section uses all defaults | unit | `cargo test -p glass_core -- config::test_soi_section_absent_uses_defaults` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_renderer -- block_renderer && cargo test -p glass_core -- config`
- **Per wave merge:** `cargo test --workspace && cargo clippy --workspace -- -D warnings`
- **Phase gate:** Full suite green + no clippy warnings before `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `crates/glass_terminal/src/block_manager.rs` — add `soi_summary: Option<String>`, `soi_severity: Option<String>` fields to `Block` and `Block::new()`
- [ ] `crates/glass_renderer/src/block_renderer.rs` — add `soi_color_for_severity()` helper and SOI label emission in `build_block_text()`; add 4 unit tests
- [ ] `crates/glass_core/src/config.rs` — add `SoiSection` struct, default functions, field on `GlassConfig`, `Default` update; add 3 unit tests
- [ ] `crates/glass_core/src/event.rs` — add `raw_line_count: i64` to `AppEvent::SoiReady` variant; update test
- [ ] `src/main.rs` — update `SoiReady` handler to populate block fields and inject hint line; update constructor for `AppEvent::SoiReady` in the SOI worker

*(No new test framework needed — Rust built-in tests already in use project-wide.)*

---

## Sources

### Primary (HIGH confidence)
- Direct code reading: `crates/glass_renderer/src/block_renderer.rs` — `build_block_text()` method, `BlockLabel` struct, existing label patterns for badge/duration/undo
- Direct code reading: `crates/glass_renderer/src/frame.rs` — `draw_frame()` overlay_buffers/overlay_metas pipeline, confirmed no config passed to renderer
- Direct code reading: `crates/glass_terminal/src/block_manager.rs` — `Block` struct fields, `Block::new()` defaults, `visible_blocks()`, `blocks_mut()`
- Direct code reading: `crates/glass_core/src/config.rs` — `PipesSection` pattern, `GlassConfig` struct and `Default` impl, `load_from_str`
- Direct code reading: `crates/glass_core/src/event.rs` — `AppEvent::SoiReady` current payload, `SoiSummary` shape
- Direct code reading: `crates/glass_mux/src/session.rs` — `Session` fields, `last_soi_summary: Option<SoiSummary>`, `last_command_id`
- Direct code reading: `src/main.rs` (lines 3097–3121) — current `SoiReady` handler; (lines 425–445) — PTY injection pattern for shell integration; (lines 1694–1695) — `PtyMsg::Input` for keyboard input
- Direct code reading: `crates/glass_history/src/compress.rs` — `compress()`, `CompressedOutput`, `TokenBudget`
- Direct code reading: `crates/glass_history/src/db.rs` — `HistoryDb::compress_output()` signature
- Direct code reading: `.planning/STATE.md` decisions — "SOI summaries rendered as block decorations (NOT injected into PTY stream) to avoid OSC 133 race condition"

### Secondary (MEDIUM confidence)
- `.planning/phases/51-soi-compression-engine/51-01-SUMMARY.md` and `51-02-SUMMARY.md` — confirmed Phase 51 complete, `compress_output` delegation exists, `DiffSummary`/`RecordFingerprint` available

### Tertiary (LOW confidence)
- None — all findings verified by direct source reading

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all types, methods, and patterns verified by direct codebase reading
- Architecture: HIGH — extension points (Block fields, BlockRenderer label emission, config sections, PtyMsg injection) all verified against existing implementations of the same patterns
- Pitfalls: HIGH — timing race for SoiReady verified by reading block manager state transitions; OSC injection pitfall verified by reading OscScanner behavior; config Default gap verified by reading existing manual Default impl

**Research date:** 2026-03-13
**Valid until:** 2026-04-13 (stable internal domain — only invalidated if Block/BlockRenderer/config structures change)
