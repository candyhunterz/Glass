# Phase 2: Terminal Core - Research

**Researched:** 2026-03-04
**Domain:** GPU text rendering (glyphon), terminal grid rendering, keyboard input encoding, clipboard, scrollback, window resize/reflow
**Confidence:** HIGH

---

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| CORE-02 | Correct VT/ANSI escape sequence rendering (colors, formatting, cursor movement) | `Term::renderable_content()` provides `GridIterator<Cell>` with `fg`, `bg`, `flags`; Color resolution via `Colors[NamedColor]` to `Rgb{r,g,b}`; glyphon renders styled text per cell |
| CORE-03 | Keyboard with Ctrl, Alt, Shift modifiers (vim, fzf, tmux work) | winit `KeyEvent` provides `logical_key`, `physical_key`, `text`, `text_with_all_modifiers`; xterm modifier encoding: `Ctrl+letter = c & 0x1f`, `Alt+x = ESC + x`, arrows = `CSI 1;mod X`; `TermMode::APP_CURSOR` changes arrow encoding |
| CORE-04 | Bracketed paste mode (no accidental execution) | `TermMode::BRACKETED_PASTE` flag on Term; wrap pasted text with `\x1b[200~` ... `\x1b[201~`; check mode flag before wrapping |
| CORE-05 | Scrollback 10,000 lines without degradation | `Config { scrolling_history: 10000 }` passed to `Term::new()`; `Term::scroll_display(Scroll::Delta(n))`; `grid.display_offset()` for viewport; already uses ring buffer storage |
| CORE-06 | Copy with Ctrl+Shift+C, paste with Ctrl+Shift+V | `arboard` crate for clipboard; intercept Ctrl+Shift+C/V before PTY forwarding; read selection from `Term.selection` grid range |
| CORE-07 | Window resize causes terminal content reflow | `Term::resize()` + `PtyMsg::Resize(WindowSize)` already scaffolded; compute cell size from font metrics; `WindowSize { num_lines, num_cols, cell_width, cell_height }` |
| CORE-08 | UTF-8 renders correctly (no mojibake) | `SetConsoleCP(65001)` already in main.rs; `TERM=xterm-256color`, `COLORTERM=truecolor` already set; glyphon/cosmic-text handles Unicode shaping + font fallback |
| RNDR-02 | Truecolor (24-bit RGB) output from bat, delta, neovim | `Color::Spec(Rgb{r,g,b})` in cell fg/bg provides direct RGB; `COLORTERM=truecolor` env already set in PTY spawn; map to glyphon `Color::rgba(r, g, b, 255)` |
| RNDR-03 | Cursor renders in block, beam, underline shapes with optional blink | `RenderableCursor { shape: CursorShape, point: Point }` from `renderable_content()`; `CursorShape` enum: Block, Underline, Beam, Hidden; blink via `about_to_wait()` timer |
| RNDR-04 | Configurable font family and font size | `GlassConfig { font_family, font_size }` already defined; `FontSystem::new()` + `Metrics::new(font_size, line_height)` in glyphon; cell dimensions = font metrics |
</phase_requirements>

---

## Summary

Phase 2 transforms Glass from a scaffold (dark window + PTY running in background) into a functional terminal. The core challenge is the rendering pipeline: reading the `alacritty_terminal::Term` grid, resolving colors to RGB, and drawing styled text via glyphon onto the wgpu surface. This is the largest and most complex phase because it touches every layer simultaneously: input encoding, grid rendering, font management, clipboard, scrollback, and resize.

The primary technical risk is getting the glyphon integration correct. Glyphon 0.10.0 requires wgpu 28.0.0 (already locked in the workspace), uses cosmic-text for font shaping, and provides `TextRenderer::prepare()` + `render()` as the frame-level API. The key insight for terminal rendering is that each line of the terminal grid maps to a `cosmic_text::Buffer` with per-character `Attrs` (color, weight, style). Cell backgrounds are drawn as colored quads in a separate render pipeline, not by glyphon.

The keyboard input encoding is the second critical piece. Phase 1 only forwards `event.text` (ASCII printable). Phase 2 must encode Ctrl (letter & 0x1f), Alt (ESC prefix), arrow keys (CSI sequences with modifier parameters), function keys, Home/End/PageUp/PageDown, and special keys. The encoding depends on terminal mode flags (APP_CURSOR, APP_KEYPAD) which change how arrow and keypad keys are encoded.

**Primary recommendation:** Build in this order: (1) glyphon text rendering pipeline with static test text, (2) grid-to-glyphon bridge reading real Term cells, (3) keyboard input encoding for modifiers, (4) scrollback + clipboard + bracketed paste, (5) cursor rendering + resize reflow + font config.

---

## Standard Stack

### Core (Phase 2 New Dependencies)

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `glyphon` | 0.10.0 | GPU text rendering via wgpu | The only maintained wgpu text renderer; wraps cosmic-text + etagere; requires `wgpu ^28.0.0` (exact match); used by COSMIC terminal |
| `cosmic-text` | 0.15.x | Font shaping, layout, fallback (transitive via glyphon) | Pure Rust text engine; handles Unicode, emoji, CJK; provides `FontSystem`, `Buffer`, `Attrs`, `SwashCache` |
| `arboard` | 3.x | System clipboard read/write | Standard Rust clipboard crate; maintained by 1Password; supports Windows, macOS, Linux |

### Core (Phase 1 Existing - Unchanged)

| Library | Version | Purpose |
|---------|---------|---------|
| `alacritty_terminal` | =0.25.1 | VTE parsing, terminal grid, ConPTY |
| `wgpu` | 28.0.0 | GPU rendering surface + pipelines |
| `winit` | 0.30.13 | Window management, keyboard events |
| `bytemuck` | 1.25.0 | GPU buffer byte casting |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `glyphon` | Custom glyph atlas + fontdue | 10x more code; no font fallback; no shaping; no atlas management |
| `arboard` | `clipboard-win` | Windows-only; arboard is cross-platform for future macOS/Linux |
| Per-cell Buffer | Per-line Buffer | Per-line is more efficient (fewer objects); per-cell wastes cosmic-text overhead |

**Installation (additions to workspace Cargo.toml):**
```toml
[workspace.dependencies]
glyphon       = "0.10.0"
arboard       = "3"
```

**Crate dependency additions:**
```toml
# crates/glass_renderer/Cargo.toml
[dependencies]
glyphon.workspace = true

# Root Cargo.toml (or glass_core)
[dependencies]
arboard.workspace = true
```

---

## Architecture Patterns

### Recommended Project Structure Additions

```
crates/
  glass_renderer/src/
    surface.rs          # (existing) wgpu device/queue/surface
    glyph_cache.rs      # NEW: FontSystem, TextAtlas, TextRenderer, SwashCache, Cache
    grid_renderer.rs    # NEW: walk Term grid -> Vec<TextArea> for glyphon
    rect_renderer.rs    # NEW: wgpu pipeline for colored quads (cell backgrounds, cursor, selections)
    frame.rs            # NEW: orchestrate full frame: clear -> rects -> text -> present
    lib.rs              # updated: re-export GlassRenderer with text rendering
  glass_terminal/src/
    input.rs            # NEW: keyboard event -> escape sequence encoding
    grid_snapshot.rs    # NEW: extract renderable data from locked Term
    lib.rs              # updated: export input encoding + grid snapshot
```

### Pattern 1: GridSnapshot for Lock-Minimizing Rendering

**What:** Copy renderable data from `Term` under a brief lock, then render without holding the lock.

**When to use:** Every frame in `RedrawRequested` handler.

```rust
// Source: alacritty_terminal 0.25.1 Term::renderable_content() verified in source
pub struct GridSnapshot {
    pub cells: Vec<RenderedCell>,
    pub cursor: RenderableCursor,
    pub display_offset: usize,
    pub mode: TermMode,
    pub columns: usize,
    pub screen_lines: usize,
}

pub struct RenderedCell {
    pub point: Point,           // (Line, Column) position
    pub c: char,                // character
    pub fg: Rgb,                // resolved foreground RGB
    pub bg: Rgb,                // resolved background RGB
    pub flags: Flags,           // BOLD, ITALIC, UNDERLINE, WIDE_CHAR, etc.
    pub zerowidth: Vec<char>,   // combining characters
}

pub fn snapshot_term(term: &Term<EventProxy>, default_colors: &DefaultColors) -> GridSnapshot {
    let content = term.renderable_content();
    let colors = content.colors;
    let mut cells = Vec::new();

    for cell in content.display_iter {
        let fg = resolve_color(cell.fg, colors, default_colors, cell.flags);
        let bg = resolve_color(cell.bg, colors, default_colors, cell.flags);
        cells.push(RenderedCell {
            point: cell.point,
            c: cell.c,
            fg, bg,
            flags: cell.flags,
            zerowidth: cell.zerowidth().map(|z| z.to_vec()).unwrap_or_default(),
        });
    }

    GridSnapshot {
        cells,
        cursor: content.cursor,
        display_offset: content.display_offset,
        mode: content.mode,
        columns: term.columns(),
        screen_lines: term.screen_lines(),
    }
}
```

### Pattern 2: Color Resolution

**What:** Convert `alacritty_terminal::vte::ansi::Color` to `Rgb` for rendering.

**Verified from source:** `Color` has three variants: `Named(NamedColor)`, `Spec(Rgb)`, `Indexed(u8)`.

```rust
// Source: vte 0.15.0 ansi.rs (alacritty_terminal's vte dependency)
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};
use alacritty_terminal::term::color::Colors;

fn resolve_color(color: Color, colors: &Colors, defaults: &DefaultColors, flags: Flags) -> Rgb {
    match color {
        Color::Spec(rgb) => rgb,  // Direct 24-bit RGB — truecolor
        Color::Indexed(idx) => {
            // Lookup in 256-color palette: colors[idx] or fall back to default palette
            colors[idx as usize].unwrap_or(default_indexed_color(idx))
        }
        Color::Named(name) => {
            // Handle DIM/BOLD variants
            let name = if flags.contains(Flags::DIM) { name.to_dim() }
                       else if flags.contains(Flags::BOLD) { name.to_bright() }
                       else { name };
            colors[name].unwrap_or(defaults.named(name))
        }
    }
}
```

### Pattern 3: Glyphon Text Rendering Pipeline

**What:** Initialize glyphon once, prepare text areas per frame, render in the wgpu pass.

**Verified from:** glyphon docs.rs + hello-world example (GitHub).

```rust
// Source: glyphon 0.10.0 docs.rs + GitHub hello-world.rs
use glyphon::{
    Attrs, Buffer, Cache, Color as GlyphonColor, Family, FontSystem,
    Metrics, Resolution, Shaping, SwashCache, TextArea, TextAtlas,
    TextBounds, TextRenderer, Viewport,
};

// One-time initialization (in GlassRenderer::new or equivalent)
let mut font_system = FontSystem::new();  // discovers system fonts
let swash_cache = SwashCache::new();
let cache = Cache::new(&device);          // shared GPU pipelines/shaders
let mut atlas = TextAtlas::new(&device, &queue, &cache, surface_format);
let text_renderer = TextRenderer::new(&mut atlas, &device, MultisampleState::default(), None);
let viewport = Viewport::new(&device, &cache);

// Per-frame: build one Buffer per terminal line (not per cell)
for line_idx in 0..screen_lines {
    let mut buffer = Buffer::new(&mut font_system, Metrics::new(font_size, line_height));
    buffer.set_size(&mut font_system, Some(viewport_width), Some(line_height));
    // Set text with per-character attributes (colors, bold, italic)
    buffer.set_rich_text(
        &mut font_system,
        line_runs.iter().map(|run| (&*run.text, run.attrs)),
        Attrs::new().family(Family::Name(&config.font_family)),
        Shaping::Basic,  // Basic = no ligatures, correct for monospace
        None,
    );
    buffer.shape_until_scroll(&mut font_system, false);
    text_areas.push(TextArea {
        buffer: &buffer,
        left: 0.0,
        top: line_idx as f32 * line_height,
        scale: window.scale_factor() as f32,
        bounds: TextBounds { left: 0, top: 0, right: width as i32, bottom: height as i32 },
        default_color: GlyphonColor::rgba(255, 255, 255, 255),
        custom_glyphs: &[],
    });
}

// In render pass
viewport.update(&queue, Resolution { width, height });
text_renderer.prepare(&device, &queue, &mut font_system, &mut atlas,
                      &viewport, text_areas, &mut swash_cache)?;
text_renderer.render(&atlas, &viewport, &mut render_pass)?;
```

### Pattern 4: Keyboard Input Encoding

**What:** Translate winit `KeyEvent` to bytes sent to PTY. Encoding depends on terminal mode.

**xterm modifier encoding formula (verified):** modifier_param = 1 + bitmask where Shift=1, Alt=2, Ctrl=4, so Ctrl+Shift = 1+1+4 = 6.

```rust
// Source: xterm ctlseqs, WezTerm key encoding docs, Alacritty input/keyboard.rs patterns
use winit::keyboard::{Key, NamedKey, ModifiersState};

pub fn encode_key(key: &Key, modifiers: ModifiersState, mode: TermMode) -> Option<Vec<u8>> {
    match key {
        // Ctrl+letter: send ASCII control character (letter & 0x1f)
        Key::Character(c) if modifiers.control_key() => {
            let ch = c.chars().next()?;
            if ch.is_ascii_alphabetic() {
                Some(vec![(ch.to_ascii_lowercase() as u8) & 0x1f])
            } else {
                // Ctrl+[ = ESC, Ctrl+] = GS, etc.
                match ch {
                    '[' | '3' => Some(vec![0x1b]),      // ESC
                    '\\' | '4' => Some(vec![0x1c]),     // FS
                    ']' | '5' => Some(vec![0x1d]),      // GS
                    '6' => Some(vec![0x1e]),             // RS
                    '/' | '7' => Some(vec![0x1f]),       // US
                    '8' => Some(vec![0x7f]),             // DEL
                    _ => None,
                }
            }
        }
        // Alt+key: send ESC prefix then the character
        Key::Character(c) if modifiers.alt_key() => {
            let mut bytes = vec![0x1b]; // ESC prefix
            bytes.extend(c.as_bytes());
            Some(bytes)
        }
        // Named keys with modifier encoding
        Key::Named(named) => encode_named_key(*named, modifiers, mode),
        // Plain character
        Key::Character(c) => Some(c.as_bytes().to_vec()),
        _ => None,
    }
}

fn encode_named_key(key: NamedKey, mods: ModifiersState, mode: TermMode) -> Option<Vec<u8>> {
    let modifier_param = modifier_code(mods);
    let app_cursor = mode.contains(TermMode::APP_CURSOR);

    match key {
        // Arrow keys: CSI 1;mod A/B/C/D (normal) or SS3 A/B/C/D (app cursor, no mods)
        NamedKey::ArrowUp => Some(arrow_seq(b'A', modifier_param, app_cursor)),
        NamedKey::ArrowDown => Some(arrow_seq(b'B', modifier_param, app_cursor)),
        NamedKey::ArrowRight => Some(arrow_seq(b'C', modifier_param, app_cursor)),
        NamedKey::ArrowLeft => Some(arrow_seq(b'D', modifier_param, app_cursor)),
        // Enter, Tab, Backspace, Escape
        NamedKey::Enter => Some(vec![0x0d]),         // CR
        NamedKey::Tab => Some(vec![0x09]),            // HT
        NamedKey::Backspace => Some(vec![0x7f]),      // DEL
        NamedKey::Escape => Some(vec![0x1b]),
        // Home/End
        NamedKey::Home => Some(csi_tilde(1, modifier_param)),
        NamedKey::End => Some(csi_tilde(4, modifier_param)),
        // Page Up/Down
        NamedKey::PageUp => Some(csi_tilde(5, modifier_param)),
        NamedKey::PageDown => Some(csi_tilde(6, modifier_param)),
        // Insert/Delete
        NamedKey::Insert => Some(csi_tilde(2, modifier_param)),
        NamedKey::Delete => Some(csi_tilde(3, modifier_param)),
        // Function keys F1-F12
        NamedKey::F1 => Some(ss3_or_csi(b'P', 11, modifier_param)),
        NamedKey::F2 => Some(ss3_or_csi(b'Q', 12, modifier_param)),
        // ... F3-F12 follow the pattern with codes 13-24
        _ => None,
    }
}

fn modifier_code(mods: ModifiersState) -> u8 {
    let mut code: u8 = 0;
    if mods.shift_key() { code |= 1; }
    if mods.alt_key() { code |= 2; }
    if mods.control_key() { code |= 4; }
    if code > 0 { code + 1 } else { 0 }  // xterm: param = 1 + bitmask
}

fn arrow_seq(letter: u8, modifier: u8, app_cursor: bool) -> Vec<u8> {
    if modifier == 0 && app_cursor {
        vec![0x1b, b'O', letter]              // SS3 A (app cursor mode, no mods)
    } else if modifier == 0 {
        vec![0x1b, b'[', letter]              // CSI A (normal mode, no mods)
    } else {
        format!("\x1b[1;{}{}", modifier, letter as char).into_bytes()
    }
}

fn csi_tilde(code: u8, modifier: u8) -> Vec<u8> {
    if modifier == 0 {
        format!("\x1b[{}~", code).into_bytes()
    } else {
        format!("\x1b[{};{}~", code, modifier).into_bytes()
    }
}
```

### Pattern 5: Rect Renderer for Cell Backgrounds and Cursor

**What:** A separate wgpu pipeline that draws colored rectangles for cell backgrounds, cursor, and selection highlighting.

**When to use:** Every frame, rendered before text.

```rust
// Each rect is an instance: position (x, y, w, h) + color (r, g, b, a)
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct RectInstance {
    pos: [f32; 4],   // x, y, width, height in pixels
    color: [f32; 4], // RGBA normalized
}

// Build instances from GridSnapshot:
// 1. One rect per cell with non-default background
// 2. One rect for cursor position (shape determines size)
// 3. Rects for selection highlight (if any)
```

### Anti-Patterns to Avoid

- **Holding Term lock during GPU operations:** Lock briefly for snapshot, release immediately. GPU draw calls can take 1-5ms.
- **Re-creating glyphon Buffer objects every frame:** Create once per line, update only when line content changes (damage tracking).
- **One glyphon Buffer per cell:** Huge overhead. Use one Buffer per terminal line with per-character `Attrs`.
- **Rebuilding glyph atlas every frame:** Atlas persists across frames; only rasterizes on cache miss.
- **Encoding all keyboard input as `event.text`:** Phase 1 approach. Ctrl, Alt, arrows, function keys have NO `text` value; they must be encoded as escape sequences from `logical_key`.

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| GPU text rendering | Custom glyph rasterizer + atlas | glyphon 0.10.0 | Font discovery, shaping, atlas management, wgpu integration; 1000s of lines saved |
| Font fallback (emoji, CJK) | Manual font loading | cosmic-text `FontSystem` (via glyphon) | Automatic system font discovery + fallback chain |
| Clipboard I/O | Win32 API calls | `arboard` 3.x | Thread safety, cross-platform, maintained by 1Password |
| Terminal color palette | Hardcoded RGB table | `alacritty_terminal::term::color::Colors` | 269-color palette with dim/bright variants; updates via OSC 4 |
| Scrollback storage | Custom ring buffer | `alacritty_terminal::grid::Storage` | Ring buffer with history; lazy allocation; resize-aware |
| Keyboard escape encoding | Full xterm encoder | Adapt from Alacritty's `input/keyboard.rs` | 500+ lines of mode-dependent encoding; CSI u, modifyOtherKeys, Kitty protocol |
| Wide character width | `unicode-width` lookups | `Cell.flags.contains(Flags::WIDE_CHAR)` | alacritty_terminal already computes this per cell |

**Key insight:** Phase 2 is an integration phase, not an implementation phase. The heavy lifting (VTE parsing, grid management, font shaping, GPU rendering) is done by existing libraries. The work is wiring them together correctly.

---

## Common Pitfalls

### Pitfall 1: Color Resolution Missing Dim/Bright Variants

**What goes wrong:** Named colors render without DIM/BOLD adjustments. `ls --color` output looks wrong -- bold directories show the same color as non-bold files.

**Why it happens:** `Color::Named(NamedColor::Blue)` with `Flags::BOLD` should resolve to BrightBlue, not Blue. The `NamedColor` enum has `to_bright()` and `to_dim()` methods for this.

**How to avoid:** Check `Flags::BOLD` and `Flags::DIM` when resolving `Color::Named` variants. Use `name.to_bright()` for BOLD and `name.to_dim()` for DIM.

**Warning signs:** All blue text is the same shade regardless of bold attribute; dim text is same brightness as normal.

### Pitfall 2: INVERSE Flag Not Applied

**What goes wrong:** Programs using reverse video (e.g., vim status line, fzf selection highlight) don't show inverted colors.

**Why it happens:** The `Flags::INVERSE` flag swaps fg and bg. Forgetting to check this flag results in selection highlights being invisible.

**How to avoid:** After resolving fg and bg colors, check `if flags.contains(Flags::INVERSE) { std::mem::swap(&mut fg, &mut bg); }`.

### Pitfall 3: WIDE_CHAR_SPACER Cells Rendered as Visible Characters

**What goes wrong:** CJK characters show a duplicate or garbage character in the second cell. Grid alignment breaks for all subsequent characters on the line.

**Why it happens:** Wide characters occupy 2 cells. The second cell has `Flags::WIDE_CHAR_SPACER` set and should NOT be rendered as a separate character.

**How to avoid:** Skip cells with `Flags::WIDE_CHAR_SPACER` during rendering. Render `WIDE_CHAR` cells at double width.

### Pitfall 4: Keyboard Ctrl+C/V Intercepted Instead of Forwarded

**What goes wrong:** Ctrl+C doesn't send SIGINT to running processes; Ctrl+V doesn't enter literal-insert mode in vim.

**Why it happens:** Glass intercepts Ctrl+C/V for copy/paste instead of forwarding to PTY. The convention for terminal emulators is Ctrl+**Shift**+C/V for clipboard operations.

**How to avoid:** Only intercept Ctrl+Shift+C and Ctrl+Shift+V. Plain Ctrl+C/V (without Shift) MUST be forwarded as control characters to the PTY.

### Pitfall 5: Bracketed Paste Only Checked at Paste Time

**What goes wrong:** Bracketed paste wrapping is applied even when the shell doesn't support it, or not applied when it does.

**Why it happens:** The terminal mode flag `TermMode::BRACKETED_PASTE` is set/cleared by the shell via escape sequences. Checking a static config instead of the live mode flag causes mismatches.

**How to avoid:** Check `term.mode().contains(TermMode::BRACKETED_PASTE)` at paste time. If set, wrap with `\x1b[200~` ... `\x1b[201~`. If not set, paste raw.

### Pitfall 6: Scrollback Doesn't Reset on New Output

**What goes wrong:** User scrolls up, then a command produces output, but the viewport stays scrolled up. User misses new output.

**Why it happens:** `display_offset` is not reset to 0 when new content arrives while scrolled.

**How to avoid:** On `AppEvent::TerminalDirty`, if `display_offset > 0`, reset to bottom with `term.scroll_display(Scroll::Bottom)` or respect `ALTERNATE_SCROLL` mode.

### Pitfall 7: Font Size Change Doesn't Recompute Cell Dimensions

**What goes wrong:** Changing font size makes text overlap or leaves gaps. Terminal reports wrong size to PTY.

**Why it happens:** Cell width/height are computed once at startup. Font size change doesn't trigger WindowSize update to PTY.

**How to avoid:** When font size changes: (1) recompute cell_width/cell_height from new font metrics, (2) recompute num_cols/num_lines from window size / cell size, (3) send `PtyMsg::Resize(new_window_size)` to PTY, (4) call `term.resize()`.

### Pitfall 8: DPI Scaling Ignored in Font Rendering

**What goes wrong:** Text appears blurry on high-DPI displays or oversized on low-DPI.

**Why it happens:** glyphon `TextArea.scale` and `Metrics` need physical pixel dimensions, not logical.

**How to avoid:** Use `window.scale_factor()` to convert logical to physical pixels. Pass physical dimensions to surface config and font metrics. `Metrics::new(font_size * scale_factor, line_height * scale_factor)`.

---

## Code Examples

### Terminal Config with Scrollback History

```rust
// Source: alacritty_terminal 0.25.1 term/mod.rs line 334-361 (verified in cargo registry source)
use alacritty_terminal::term::Config as TermConfig;
use alacritty_terminal::vte::ansi::CursorStyle;

let term_config = TermConfig {
    scrolling_history: 10_000,  // CORE-05: configurable scrollback
    default_cursor_style: CursorStyle {
        shape: CursorShape::Block,
        blinking: false,
    },
    vi_mode_cursor_style: None,
    semantic_escape_chars: String::from(",│`|:\"' ()[]{}<>\t"),
    kitty_keyboard: false,
    osc52: Osc52::default(),
};
```

### alacritty_terminal Cell Access

```rust
// Source: alacritty_terminal 0.25.1 term/cell.rs (verified in cargo registry source)
// Cell struct:
// pub struct Cell {
//     pub c: char,                        // character content
//     pub fg: Color,                      // Color::Named | Color::Spec(Rgb) | Color::Indexed(u8)
//     pub bg: Color,
//     pub flags: Flags,                   // BOLD, ITALIC, UNDERLINE, WIDE_CHAR, etc.
//     pub extra: Option<Arc<CellExtra>>,  // zerowidth chars, underline color, hyperlink
// }
//
// Color variants (from vte 0.15.0 ansi.rs):
// Color::Named(NamedColor)  -> resolve via Colors palette
// Color::Spec(Rgb { r, g, b })  -> direct 24-bit truecolor
// Color::Indexed(u8)  -> 256-color palette lookup
```

### Clipboard Operations (Copy/Paste)

```rust
// Source: arboard crate docs
use arboard::Clipboard;

fn clipboard_copy(text: &str) {
    if let Ok(mut clipboard) = Clipboard::new() {
        let _ = clipboard.set_text(text);
    }
}

fn clipboard_paste() -> Option<String> {
    Clipboard::new().ok()?.get_text().ok()
}

// Bracketed paste wrapping (CORE-04)
fn paste_to_pty(text: &str, mode: TermMode, pty_sender: &EventLoopSender) {
    let bytes = if mode.contains(TermMode::BRACKETED_PASTE) {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"\x1b[200~");  // bracket start
        buf.extend_from_slice(text.as_bytes());
        buf.extend_from_slice(b"\x1b[201~");  // bracket end
        buf
    } else {
        text.as_bytes().to_vec()
    };
    let _ = pty_sender.send(PtyMsg::Input(Cow::Owned(bytes)));
}
```

### Scrollback Interaction

```rust
// Source: alacritty_terminal 0.25.1 grid/mod.rs (Scroll enum verified)
use alacritty_terminal::grid::Scroll;

// Mouse wheel scrolling
fn handle_scroll(term: &mut Term<EventProxy>, delta: i32) {
    term.scroll_display(Scroll::Delta(delta));
    // Triggers Wakeup via EventProxy -> request_redraw()
}

// Keyboard scrollback (Shift+PageUp/Down)
fn handle_page_scroll(term: &mut Term<EventProxy>, up: bool) {
    if up {
        term.scroll_display(Scroll::PageUp);
    } else {
        term.scroll_display(Scroll::PageDown);
    }
}
```

### Damage-Aware Rendering

```rust
// Source: alacritty_terminal 0.25.1 term/mod.rs line 458 (verified)
// Term::damage() returns TermDamage iterator over LineDamageBounds
// This allows skipping unchanged lines during rendering

let damage = term.damage();
for line_damage in damage {
    // line_damage.line: usize — which line changed
    // line_damage.left: usize — leftmost changed column
    // line_damage.right: usize — rightmost changed column
    // Only re-layout and re-render this line's Buffer
}
term.reset_damage();
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `wgpu_glyph` for text | `glyphon` 0.10 | 2023 | wgpu_glyph unmaintained; glyphon is the ecosystem replacement |
| Custom 256-color palette | `alacritty_terminal::term::color::Colors` | N/A | 269-color palette with dim/bright/cursor colors; OSC 4 updates |
| Manual xterm key tables | Kitty keyboard protocol | 2022+ | Modern terminals support CSI u / Kitty protocol; for now Glass uses xterm compat |
| Per-cell glyph atlas lookup | Per-line cosmic-text Buffer | 2024 | cosmic-text batches shaping per line; much more efficient than per-cell |

**Deprecated/outdated:**
- `wgpu_glyph`: unmaintained since 2023, incompatible with wgpu 28. Use glyphon.
- `rusttype` / `glyph_brush`: deprecated, cosmic-text is the successor.
- Manual `wcwidth` lookups: `alacritty_terminal` already handles this via `unicode-width`.

---

## Open Questions

1. **glyphon per-line Buffer lifecycle**
   - What we know: Creating a `Buffer::new()` per line per frame works but may allocate
   - What's unclear: Whether Buffers should be cached and only re-shaped on damage
   - Recommendation: Start with create-per-frame for correctness; optimize with damage-based caching once rendering works. Profile before optimizing.

2. **Rect renderer pipeline implementation**
   - What we know: Cell backgrounds need colored quads drawn before text
   - What's unclear: Whether to use wgpu instanced rendering or a simple vertex buffer approach
   - Recommendation: Use instanced rendering with `RectInstance` buffer. Single draw call for all backgrounds. This is the approach Alacritty and COSMIC terminal use.

3. **Ctrl+Shift+C/V vs. winit modifier state**
   - What we know: winit provides `ModifiersState` with shift/ctrl/alt flags
   - What's unclear: Whether `text_with_all_modifiers` returns text for Ctrl+Shift+C
   - Recommendation: Check `logical_key` for `Key::Character("c")` with both ctrl and shift modifiers active, NOT `text` which may be empty or wrong for modified keys.

4. **Selection (for copy)**
   - What we know: `alacritty_terminal` has a `Selection` type and `Term.selection`
   - What's unclear: Whether Glass should implement mouse-based selection in Phase 2 or defer
   - Recommendation: Implement basic selection for copy (CORE-06 requires it). Use `Term.selection` API. Mouse drag selection is complex but needed for practical copy.

---

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | `cargo test` (built-in) |
| Config file | None |
| Quick run command | `cargo test --workspace` |
| Full suite command | `cargo test --workspace --all-targets` |

### Phase Requirements -> Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| CORE-02 | Truecolor VT/ANSI rendering | Integration | `cargo test -p glass_terminal -- color_resolution` | No -- Wave 0 |
| CORE-03 | Ctrl/Alt/Shift modifiers produce correct escape sequences | Unit | `cargo test -p glass_terminal -- input_encoding` | No -- Wave 0 |
| CORE-04 | Bracketed paste wraps text with ESC[200~ / ESC[201~ | Unit | `cargo test -p glass_terminal -- bracketed_paste` | No -- Wave 0 |
| CORE-05 | 10,000 line scrollback works | Integration | `cargo test -p glass_terminal -- scrollback` | No -- Wave 0 |
| CORE-06 | Copy/paste via Ctrl+Shift+C/V | Manual | Run Glass, select text, Ctrl+Shift+C, Ctrl+Shift+V | N/A |
| CORE-07 | Resize reflows terminal content | Integration | `cargo test -p glass_terminal -- resize_reflow` | No -- Wave 0 |
| CORE-08 | UTF-8 renders without mojibake | Smoke (manual) | Run `Write-Output "cafe\u0301 \u{1F980}"` in Glass | N/A |
| RNDR-02 | Truecolor from bat/delta/neovim | Smoke (manual) | Run `bat --color=always Cargo.toml` in Glass | N/A |
| RNDR-03 | Cursor shapes (block, beam, underline) | Smoke (manual) | Open neovim in Glass, verify cursor changes between modes | N/A |
| RNDR-04 | Font family/size configurable | Smoke (manual) | Change `GlassConfig.font_size`, verify text re-renders | N/A |

### Sampling Rate

- **Per task commit:** `cargo build --workspace` + `cargo test --workspace` (~15s)
- **Per wave merge:** `cargo test --workspace --all-targets`
- **Phase gate:** Full suite green + manual smoke tests (truecolor, keyboard, scrollback, clipboard, resize)

### Wave 0 Gaps

- [ ] `crates/glass_terminal/src/input.rs` + tests -- keyboard escape sequence encoding unit tests (CORE-03)
- [ ] `crates/glass_terminal/src/grid_snapshot.rs` + tests -- color resolution tests (CORE-02)
- [ ] Bracketed paste unit test in `glass_terminal` (CORE-04)
- [ ] Scrollback configuration test verifying `Config { scrolling_history: 10000 }` (CORE-05)
- [ ] Add `glyphon = "0.10.0"` and `arboard = "3"` to workspace dependencies
- [ ] Add `glyphon.workspace = true` to `glass_renderer/Cargo.toml`

---

## Sources

### Primary (HIGH confidence)

- `alacritty_terminal` 0.25.1 source code (cargo registry) -- verified `Cell`, `Flags`, `Color`, `RenderableContent`, `RenderableCursor`, `Config`, `TermMode`, `Scroll` types directly from source files
- `vte` 0.15.0 source code (cargo registry) -- verified `Color` enum variants (`Named`, `Spec(Rgb)`, `Indexed`), `Rgb { r, g, b }` struct
- [glyphon docs.rs](https://docs.rs/glyphon/latest/glyphon/) -- `TextRenderer`, `TextAtlas`, `Cache`, `Viewport`, method signatures for `new()`, `prepare()`, `render()`
- [glyphon hello-world.rs](https://github.com/grovesNL/glyphon/blob/main/examples/hello-world.rs) -- complete initialization and rendering example
- [alacritty_terminal cell docs](https://docs.rs/alacritty_terminal/latest/alacritty_terminal/term/cell/) -- Cell struct, CellExtra, Flags bitflags
- [alacritty_terminal TermMode flags](https://docs.rs/alacritty_terminal/0.25.1/alacritty_terminal/term/struct.TermMode.html) -- all 27 mode flags including BRACKETED_PASTE, APP_CURSOR, ALT_SCREEN

### Secondary (MEDIUM confidence)

- [WezTerm key encoding docs](https://wezterm.org/config/key-encoding.html) -- xterm modifier encoding conventions
- [xterm ctlseqs](https://invisible-island.net/xterm/ctlseqs/ctlseqs.html) -- authoritative escape sequence reference
- [arboard GitHub](https://github.com/1Password/arboard) -- clipboard API, Windows threading considerations
- [Bracketed paste mode](https://cirw.in/blog/bracketed-paste) -- ESC[200~ / ESC[201~ wrapping protocol

### Tertiary (LOW confidence -- needs validation during implementation)

- glyphon `Buffer` per-line caching strategy -- no authoritative source; based on cosmic-text architecture inference
- Rect renderer instanced rendering pattern -- based on Alacritty/COSMIC terminal approach but not verified against Glass's wgpu version

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- glyphon 0.10.0 requires wgpu ^28.0.0 (verified on crates.io); arboard is mature
- Architecture: HIGH -- grid snapshot pattern verified from alacritty_terminal source; glyphon API verified from docs + examples
- Pitfalls: HIGH -- color resolution, WIDE_CHAR handling, keyboard encoding all verified from alacritty_terminal source
- Keyboard encoding: MEDIUM -- xterm conventions are well-documented but implementation completeness (all keys, all modes) needs runtime testing

**Research date:** 2026-03-04
**Valid until:** 2026-05-01 (stable ecosystem; glyphon/wgpu release cadence is quarterly)
