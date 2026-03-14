# Glass Settings Overlay — Design Spec

## Overview

A single fullscreen overlay accessible via **Ctrl+Shift+,** (comma) that consolidates settings, keyboard shortcuts, and about info into three tabs. Follows the same rendering pattern as the existing activity overlay and conflict overlay.

## Motivation

Glass has accumulated 20+ keyboard shortcuts across 6 categories and a config file with 9 sections. Users cannot discover or remember these without consulting external documentation. An in-app settings overlay makes Glass self-documenting.

## Architecture

No new crates. All changes live in:

- `crates/glass_renderer/src/settings_overlay.rs` — new file: types, text generation, layout logic
- `crates/glass_renderer/src/lib.rs` — module registration and re-exports
- `crates/glass_renderer/src/frame.rs` — `draw_settings_overlay()` method on FrameRenderer
- `src/main.rs` — overlay state, hotkey handler, render wiring, config mutation

The overlay follows the exact same pattern as `activity_overlay.rs` and `draw_activity_overlay()` — types + text label generation in the overlay module, rendering via FrameRenderer using rect_renderer + glyphon text.

## Hotkey

**Ctrl+Shift+,** (comma) toggles the overlay. This matches the VS Code convention for settings. The logical key match is `Key::Character(c) if c.as_str() == ","`.

When the overlay is open, it consumes all keyboard input (same pattern as the activity overlay). Esc or Ctrl+Shift+, closes it.

## Tabs

Three top-level tabs displayed as a horizontal tab bar below the header:

1. **Settings** — editable config fields with sidebar navigation
2. **Shortcuts** — multi-column keyboard shortcut cheatsheet
3. **About** — version, platform, license info

Tab cycling: **Tab** moves to the next tab, **Shift+Tab** moves to the previous tab. The active tab is highlighted in purple (`Rgb { r: 180, g: 140, b: 255 }`).

## Tab 1: Settings

### Layout

Two-column layout:

- **Left sidebar** (fixed width ~140px): lists config sections. Arrow Up/Down navigates between sections. Active section highlighted with a background rect.
- **Right panel** (remaining width): shows the fields for the selected section.

### Sections and Fields

Each section shows its fields as labeled rows. Editable fields show a value that can be changed with keyboard input. Read-only fields show the current value dimmed.

| Section | Fields | Editable |
|---------|--------|----------|
| **Font** | font_family (text), font_size (number +/-) | Yes |
| **Agent Mode** | enabled (toggle), mode (cycle: Watch/Assist/Autonomous), max_budget_usd (number), cooldown_secs (number) | Yes |
| **SOI** | enabled (toggle), shell_summary (toggle), min_lines (number) | Yes |
| **Snapshots** | enabled (toggle), max_blob_store_mb (number), retention_days (number) | Yes |
| **Pipes** | enabled (toggle), auto_expand (toggle), max_capture_mb (number) | Yes |
| **History** | max_output_capture_kb (number) | Yes |

### Editing Interactions

- **Toggle fields** (enabled, shell_summary, auto_expand): Enter or Space cycles ON/OFF. Displayed as green "ON" or dim "OFF".
- **Cycle fields** (agent mode): Enter or Space cycles through the enum values.
- **Number fields** (font_size, max_budget_usd, etc.): Left/Right arrow or -/+ decrements/increments. Step sizes: font_size by 0.5, budget by 0.50, cooldown by 5, other integers by 1.
- **Text fields** (font_family): Enter activates inline edit mode. Type to replace. Enter confirms, Esc cancels.

### Persistence

When a field is changed, the overlay immediately writes the updated config to `~/.glass/config.toml`. The existing hot-reload watcher picks up the change and applies it (font changes trigger re-layout, agent changes restart the runtime, etc.). No separate "save" action is needed.

Writing config: read the current TOML file, parse it, update the changed field, serialize back, and write. Use the existing `toml` crate (already a dependency via `glass_core`). The write happens on the main thread since config changes are infrequent and the file is small.

### Footer

A dimmed text line at the bottom of the settings panel: "Advanced: edit ~/.glass/config.toml (hot-reloads automatically)"

## Tab 2: Shortcuts

### Layout

Multi-column cheatsheet layout — all shortcuts visible at once without scrolling (on a typical terminal window). Content is arranged in two columns with section headers inline.

### Sections and Shortcuts

| Section | Shortcuts |
|---------|-----------|
| **Core** | Copy (Ctrl+Shift+C), Paste (Ctrl+Shift+V), Search history (Ctrl+Shift+F), Undo (Ctrl+Shift+Z), Pipeline view (Ctrl+Shift+P), Check updates (Ctrl+Shift+U) |
| **Tabs** | New tab (Ctrl+Shift+T), Close tab (Ctrl+Shift+W), Next tab (Ctrl+Tab), Prev tab (Ctrl+Shift+Tab), Jump to 1-9 (Ctrl+1-9) |
| **Panes** | Split horizontal (Ctrl+Shift+D), Split vertical (Ctrl+Shift+E), Focus pane (Alt+Arrow), Resize pane (Alt+Shift+Arrow) |
| **Navigation** | Scroll up (Shift+PgUp), Scroll down (Shift+PgDn) |
| **Overlays** | Settings (Ctrl+Shift+,), Proposals (Ctrl+Shift+A), Activity stream (Ctrl+Shift+G) |
| **Agent Mode** | Accept proposal (Ctrl+Shift+Y, in review), Reject proposal (Ctrl+Shift+N, in review) |

### Rendering

Each shortcut row: action name (left-aligned, light gray) and key badge (right-aligned, purple text on dark background rect). Section headers in purple uppercase. The layout uses two columns with a gap, distributing sections to balance vertical height.

Arrow Up/Down scrolls if content exceeds viewport height (unlikely but handled).

## Tab 3: About

### Content

Static text, no interaction:

- **Header**: "Glass" in large purple text
- **Version**: "v2.5.0" (read from `env!("CARGO_PKG_VERSION")`)
- **Description**: "GPU-accelerated terminal emulator"
- **Link**: "github.com/candyhunterz/Glass"
- **License**: "MIT License"
- **Platform**: runtime OS + architecture (e.g., "Windows 11 x86_64")
- **Renderer**: wgpu backend info if available, otherwise "wgpu"

## Overlay State (Processor fields)

```rust
/// Whether the settings overlay is visible.
settings_overlay_visible: bool,
/// Active tab in the settings overlay (0=Settings, 1=Shortcuts, 2=About).
settings_overlay_tab: u8,
/// Selected sidebar section index in the Settings tab.
settings_section_index: usize,
/// Selected field index within the current section.
settings_field_index: usize,
/// Whether a text field is in inline edit mode.
settings_editing: bool,
/// Buffer for inline text editing.
settings_edit_buffer: String,
/// Scroll offset for the Shortcuts tab.
settings_shortcuts_scroll: usize,
```

All initialized to defaults (false, 0, empty string) in the Processor constructor.

## Rendering

### SettingsOverlayRenderer

New struct in `settings_overlay.rs`:

```rust
pub struct SettingsOverlayRenderer {
    cell_width: f32,
    cell_height: f32,
}
```

Methods:

- `build_backdrop_rect()` — full-viewport dark backdrop (same as activity overlay)
- `build_settings_text()` — generates `Vec<SettingsOverlayTextLabel>` for the Settings tab given current config, section index, field index
- `build_shortcuts_text()` — generates `Vec<SettingsOverlayTextLabel>` for the Shortcuts tab
- `build_about_text()` — generates `Vec<SettingsOverlayTextLabel>` for the About tab

### FrameRenderer::draw_settings_overlay()

Follows the exact same pattern as `draw_activity_overlay()`:

1. Build backdrop rect
2. Generate text labels from the appropriate tab method
3. Create per-label glyphon buffers
4. Prepare text renderer
5. Render pass with LoadOp::Load (overlay on existing frame)

Called in main.rs redraw section after the conflict overlay, before `frame.present()`.

## Config Write-Back

New function in `glass_core::config`:

```rust
pub fn update_config_field(path: &Path, section: &str, key: &str, value: &str) -> Result<(), ConfigError>
```

This function:
1. Reads the existing TOML file
2. Parses as `toml::Value` (preserving structure)
3. Updates the specific field
4. Serializes back to string
5. Writes to disk

The hot-reload watcher detects the write and triggers `AppEvent::ConfigReloaded`, applying the change.

## Visual Style

Consistent with existing Glass overlays:

- Backdrop: `[0.03, 0.03, 0.06, 0.95]` (near-black, 95% opacity)
- Header: "Glass Settings" in purple (`Rgb { r: 180, g: 140, b: 255 }`)
- Tab bar: active tab in purple with underline, inactive in gray
- Section headers: purple uppercase
- Field labels: light gray (`Rgb { r: 170, g: 170, b: 170 }`)
- Field values: white (`Rgb { r: 255, g: 255, b: 255 }`)
- Toggle ON: green (`Rgb { r: 106, g: 166, b: 106 }`)
- Toggle OFF: dim gray (`Rgb { r: 102, g: 102, b: 102 }`)
- Key badges: purple text on dark background
- "Esc to close" hint: dim gray, right-aligned in header
- Footer text: dim gray (`Rgb { r: 85, g: 85, b: 85 }`)

## Testing

- Unit tests in `settings_overlay.rs` for:
  - Tab cycling (next/prev wrapping)
  - Shortcut list completeness (all categories present)
  - Text label generation (non-empty, correct positioning)
  - About text includes version string
- Build + clippy verification after each task
- Full `cargo test --workspace` at the end

## Out of Scope

- Custom keybinding configuration (shortcuts are read-only display)
- Theme/color customization
- Config file syntax validation beyond what `toml` crate provides
- Multi-window support (settings overlay is per-window, same as other overlays)
