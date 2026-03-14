# Settings Overlay Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an in-app settings overlay (Ctrl+Shift+,) with three tabs: Settings (editable config), Shortcuts (cheatsheet), and About (version info).

**Architecture:** New `settings_overlay.rs` in glass_renderer with types and text generation. Rendering via existing FrameRenderer pattern (draw_settings_overlay). Config write-back via new `update_config_field()` in glass_core. Overlay state and hotkey wiring in main.rs.

**Tech Stack:** Rust, wgpu, glyphon, toml, winit

**Spec:** `docs/superpowers/specs/2026-03-13-settings-overlay-design.md`

---

## Chunk 1: Overlay Types, Shortcuts Tab, and About Tab

### Task 1: SettingsOverlay types and tab enum

**Files:**
- Create: `crates/glass_renderer/src/settings_overlay.rs`
- Modify: `crates/glass_renderer/src/lib.rs`

- [ ] **Step 1: Write tests for SettingsTab cycling**

Create `crates/glass_renderer/src/settings_overlay.rs` with the tab enum and tests:

```rust
//! SettingsOverlayRenderer: fullscreen overlay for settings, shortcuts, and about.
//!
//! Three-tab layout: Settings (sidebar + editable fields), Shortcuts (cheatsheet),
//! About (version info). Follows the same pattern as ActivityOverlayRenderer.

use alacritty_terminal::vte::ansi::Rgb;

use crate::rect_renderer::RectInstance;

/// Active tab in the settings overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    Settings,
    Shortcuts,
    About,
}

impl SettingsTab {
    pub fn next(self) -> Self {
        match self {
            Self::Settings => Self::Shortcuts,
            Self::Shortcuts => Self::About,
            Self::About => Self::Settings,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Settings => Self::About,
            Self::Shortcuts => Self::Settings,
            Self::About => Self::Shortcuts,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Settings => "Settings",
            Self::Shortcuts => "Shortcuts",
            Self::About => "About",
        }
    }
}

/// Text label for rendering in the settings overlay.
#[derive(Debug, Clone)]
pub struct SettingsOverlayTextLabel {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub color: Rgb,
}

/// Render data for the settings overlay.
#[derive(Debug)]
pub struct SettingsOverlayRenderData {
    pub tab: SettingsTab,
    /// Settings tab state
    pub section_index: usize,
    pub field_index: usize,
    pub editing: bool,
    pub edit_buffer: String,
    /// Current config values for the Settings tab
    pub config: SettingsConfigSnapshot,
    /// Shortcuts tab scroll offset
    pub shortcuts_scroll: usize,
}

/// Snapshot of current config values for display in the Settings tab.
/// Extracted from GlassConfig so the renderer doesn't depend on glass_core.
#[derive(Debug, Clone)]
pub struct SettingsConfigSnapshot {
    // Font
    pub font_family: String,
    pub font_size: f32,
    // Agent
    pub agent_enabled: bool,
    pub agent_mode: String,
    pub agent_budget: f64,
    pub agent_cooldown: u64,
    // SOI
    pub soi_enabled: bool,
    pub soi_shell_summary: bool,
    pub soi_min_lines: u32,
    // Snapshots
    pub snapshot_enabled: bool,
    pub snapshot_max_mb: u32,
    pub snapshot_retention_days: u32,
    // Pipes
    pub pipes_enabled: bool,
    pub pipes_auto_expand: bool,
    pub pipes_max_capture_mb: u32,
    // History
    pub history_max_output_kb: u32,
}

impl Default for SettingsConfigSnapshot {
    fn default() -> Self {
        Self {
            font_family: "Consolas".to_string(),
            font_size: 14.0,
            agent_enabled: false,
            agent_mode: "Off".to_string(),
            agent_budget: 1.0,
            agent_cooldown: 30,
            soi_enabled: true,
            soi_shell_summary: false,
            soi_min_lines: 0,
            snapshot_enabled: true,
            snapshot_max_mb: 500,
            snapshot_retention_days: 30,
            pipes_enabled: true,
            pipes_auto_expand: true,
            pipes_max_capture_mb: 10,
            history_max_output_kb: 50,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_tab_cycle_next() {
        assert_eq!(SettingsTab::Settings.next(), SettingsTab::Shortcuts);
        assert_eq!(SettingsTab::Shortcuts.next(), SettingsTab::About);
        assert_eq!(SettingsTab::About.next(), SettingsTab::Settings);
    }

    #[test]
    fn test_settings_tab_cycle_prev() {
        assert_eq!(SettingsTab::Settings.prev(), SettingsTab::About);
        assert_eq!(SettingsTab::Shortcuts.prev(), SettingsTab::Settings);
        assert_eq!(SettingsTab::About.prev(), SettingsTab::Shortcuts);
    }

    #[test]
    fn test_settings_tab_labels() {
        assert_eq!(SettingsTab::Settings.label(), "Settings");
        assert_eq!(SettingsTab::Shortcuts.label(), "Shortcuts");
        assert_eq!(SettingsTab::About.label(), "About");
    }
}
```

- [ ] **Step 2: Register module in lib.rs**

In `crates/glass_renderer/src/lib.rs`, add after `pub mod activity_overlay;`:

```rust
pub mod settings_overlay;
```

And add re-exports after the activity_overlay re-exports:

```rust
pub use settings_overlay::{
    SettingsOverlayRenderData, SettingsOverlayTextLabel, SettingsTab,
};
```

- [ ] **Step 3: Run tests**

Run: `cargo test --package glass_renderer settings_overlay`
Expected: All 3 tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/glass_renderer/src/settings_overlay.rs crates/glass_renderer/src/lib.rs
git commit -m "feat(renderer): add settings overlay types and tab enum"
```

---

### Task 2: Shortcuts tab text generation

**Files:**
- Modify: `crates/glass_renderer/src/settings_overlay.rs`

- [ ] **Step 1: Write test for shortcuts text generation**

Add to the test module in `settings_overlay.rs`:

```rust
    #[test]
    fn test_shortcuts_text_has_all_categories() {
        let renderer = SettingsOverlayRenderer::new(10.0, 20.0);
        let labels = renderer.build_shortcuts_text(800.0, 600.0, 0);
        let text: Vec<&str> = labels.iter().map(|l| l.text.as_str()).collect();
        // All category headers present
        assert!(text.contains(&"CORE"));
        assert!(text.contains(&"TABS"));
        assert!(text.contains(&"PANES"));
        assert!(text.contains(&"NAVIGATION"));
        assert!(text.contains(&"OVERLAYS"));
        // Some specific shortcuts present
        assert!(text.iter().any(|t| t.contains("Ctrl+Shift+C")));
        assert!(text.iter().any(|t| t.contains("Ctrl+Shift+T")));
        assert!(text.iter().any(|t| t.contains("Ctrl+Shift+,")));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --package glass_renderer test_shortcuts_text_has_all_categories`
Expected: FAIL — `SettingsOverlayRenderer` does not exist yet

- [ ] **Step 3: Add SettingsOverlayRenderer and build_shortcuts_text**

Add to `settings_overlay.rs`, after the `SettingsConfigSnapshot` impl and before the test module:

```rust
/// Section names for the settings sidebar.
pub const SETTINGS_SECTIONS: &[&str] = &[
    "Font",
    "Agent Mode",
    "SOI",
    "Snapshots",
    "Pipes",
    "History",
];

/// A single shortcut entry for display.
struct ShortcutEntry {
    action: &'static str,
    keys: &'static str,
}

/// A category of shortcuts.
struct ShortcutCategory {
    name: &'static str,
    entries: &'static [ShortcutEntry],
}

const SHORTCUT_DATA: &[ShortcutCategory] = &[
    ShortcutCategory {
        name: "CORE",
        entries: &[
            ShortcutEntry { action: "Copy", keys: "Ctrl+Shift+C" },
            ShortcutEntry { action: "Paste", keys: "Ctrl+Shift+V" },
            ShortcutEntry { action: "Search history", keys: "Ctrl+Shift+F" },
            ShortcutEntry { action: "Undo last command", keys: "Ctrl+Shift+Z" },
            ShortcutEntry { action: "Toggle pipeline view", keys: "Ctrl+Shift+P" },
            ShortcutEntry { action: "Check for updates", keys: "Ctrl+Shift+U" },
        ],
    },
    ShortcutCategory {
        name: "TABS",
        entries: &[
            ShortcutEntry { action: "New tab", keys: "Ctrl+Shift+T" },
            ShortcutEntry { action: "Close tab/pane", keys: "Ctrl+Shift+W" },
            ShortcutEntry { action: "Next tab", keys: "Ctrl+Tab" },
            ShortcutEntry { action: "Previous tab", keys: "Ctrl+Shift+Tab" },
            ShortcutEntry { action: "Jump to tab 1-9", keys: "Ctrl+1-9" },
        ],
    },
    ShortcutCategory {
        name: "PANES",
        entries: &[
            ShortcutEntry { action: "Split horizontal", keys: "Ctrl+Shift+D" },
            ShortcutEntry { action: "Split vertical", keys: "Ctrl+Shift+E" },
            ShortcutEntry { action: "Focus pane", keys: "Alt+Arrow" },
            ShortcutEntry { action: "Resize pane", keys: "Alt+Shift+Arrow" },
        ],
    },
    ShortcutCategory {
        name: "NAVIGATION",
        entries: &[
            ShortcutEntry { action: "Scroll up", keys: "Shift+PgUp" },
            ShortcutEntry { action: "Scroll down", keys: "Shift+PgDn" },
        ],
    },
    ShortcutCategory {
        name: "OVERLAYS",
        entries: &[
            ShortcutEntry { action: "Settings", keys: "Ctrl+Shift+," },
            ShortcutEntry { action: "Review proposals", keys: "Ctrl+Shift+A" },
            ShortcutEntry { action: "Activity stream", keys: "Ctrl+Shift+G" },
        ],
    },
    ShortcutCategory {
        name: "AGENT MODE",
        entries: &[
            ShortcutEntry { action: "Accept proposal", keys: "Ctrl+Shift+Y" },
            ShortcutEntry { action: "Reject proposal", keys: "Ctrl+Shift+N" },
        ],
    },
];

/// Renders the settings overlay visual elements.
pub struct SettingsOverlayRenderer {
    cell_width: f32,
    cell_height: f32,
}

impl SettingsOverlayRenderer {
    pub fn new(cell_width: f32, cell_height: f32) -> Self {
        Self { cell_width, cell_height }
    }

    /// Build backdrop rectangle (semi-transparent dark overlay).
    pub fn build_backdrop_rect(&self, viewport_width: f32, viewport_height: f32) -> RectInstance {
        RectInstance {
            pos: [0.0, 0.0, viewport_width, viewport_height],
            color: [0.03, 0.03, 0.06, 0.95],
        }
    }

    /// Build the common header and tab bar labels shared by all tabs.
    pub fn build_header_text(
        &self,
        active_tab: SettingsTab,
        viewport_width: f32,
    ) -> Vec<SettingsOverlayTextLabel> {
        let mut labels = Vec::new();
        let padding = self.cell_width;
        let header_y = self.cell_height;

        // Title
        labels.push(SettingsOverlayTextLabel {
            text: "Glass Settings".to_string(),
            x: padding,
            y: header_y,
            color: Rgb { r: 180, g: 140, b: 255 },
        });

        // Close hint
        labels.push(SettingsOverlayTextLabel {
            text: "Esc to close".to_string(),
            x: viewport_width - 14.0 * self.cell_width,
            y: header_y,
            color: Rgb { r: 85, g: 85, b: 85 },
        });

        // Tab bar
        let tabs = [SettingsTab::Settings, SettingsTab::Shortcuts, SettingsTab::About];
        let tab_y = self.cell_height * 2.5;
        let mut tab_x = padding;
        for tab in &tabs {
            let color = if *tab == active_tab {
                Rgb { r: 180, g: 140, b: 255 }
            } else {
                Rgb { r: 136, g: 136, b: 136 }
            };
            labels.push(SettingsOverlayTextLabel {
                text: tab.label().to_string(),
                x: tab_x,
                y: tab_y,
                color,
            });
            tab_x += tab.label().len() as f32 * self.cell_width + self.cell_width * 3.0;
        }

        labels
    }

    /// Build text labels for the Shortcuts tab (multi-column cheatsheet).
    pub fn build_shortcuts_text(
        &self,
        viewport_width: f32,
        _viewport_height: f32,
        _scroll_offset: usize,
    ) -> Vec<SettingsOverlayTextLabel> {
        let mut labels = Vec::new();
        let padding = self.cell_width;
        let content_y = self.cell_height * 4.5;

        // Two-column layout: split categories across columns
        let col_width = (viewport_width - padding * 3.0) / 2.0;
        let col1_x = padding;
        let col2_x = padding + col_width + padding;

        // Distribute categories: first half left, second half right
        let mid = (SHORTCUT_DATA.len() + 1) / 2;
        let mut y = content_y;

        for (i, category) in SHORTCUT_DATA.iter().enumerate() {
            let (col_x, col_y) = if i < mid {
                (col1_x, &mut y)
            } else {
                // For the right column, we need a separate y tracker
                // We'll handle this by computing offsets
                (col2_x, &mut y)
            };

            // Reset y for right column start
            if i == mid {
                y = content_y;
            }

            // Category header
            labels.push(SettingsOverlayTextLabel {
                text: category.name.to_string(),
                x: col_x,
                y: *col_y,
                color: Rgb { r: 180, g: 140, b: 255 },
            });
            *col_y += self.cell_height * 1.2;

            // Shortcut entries
            for entry in category.entries {
                // Action name
                labels.push(SettingsOverlayTextLabel {
                    text: entry.action.to_string(),
                    x: col_x + self.cell_width,
                    y: *col_y,
                    color: Rgb { r: 204, g: 204, b: 204 },
                });
                // Key badge
                labels.push(SettingsOverlayTextLabel {
                    text: entry.keys.to_string(),
                    x: col_x + col_width - entry.keys.len() as f32 * self.cell_width - self.cell_width,
                    y: *col_y,
                    color: Rgb { r: 180, g: 140, b: 255 },
                });
                *col_y += self.cell_height;
            }
            *col_y += self.cell_height * 0.5; // gap between categories
        }

        labels
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --package glass_renderer settings_overlay`
Expected: All 4 tests pass (3 tab tests + shortcuts test)

- [ ] **Step 5: Commit**

```bash
git add crates/glass_renderer/src/settings_overlay.rs
git commit -m "feat(renderer): add shortcuts tab text generation"
```

---

### Task 3: About tab text generation

**Files:**
- Modify: `crates/glass_renderer/src/settings_overlay.rs`

- [ ] **Step 1: Write test for about text**

Add to the test module:

```rust
    #[test]
    fn test_about_text_has_version() {
        let renderer = SettingsOverlayRenderer::new(10.0, 20.0);
        let labels = renderer.build_about_text(800.0, 600.0);
        let text: Vec<&str> = labels.iter().map(|l| l.text.as_str()).collect();
        assert!(text.iter().any(|t| t.contains("Glass")));
        assert!(text.iter().any(|t| t.contains("GPU-accelerated")));
        assert!(text.iter().any(|t| t.contains("MIT")));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --package glass_renderer test_about_text_has_version`
Expected: FAIL — `build_about_text` does not exist yet

- [ ] **Step 3: Add build_about_text method**

Add to the `impl SettingsOverlayRenderer` block:

```rust
    /// Build text labels for the About tab.
    pub fn build_about_text(
        &self,
        viewport_width: f32,
        _viewport_height: f32,
    ) -> Vec<SettingsOverlayTextLabel> {
        let mut labels = Vec::new();
        let center_x = viewport_width * 0.3;
        let mut y = self.cell_height * 6.0;

        // Glass name
        labels.push(SettingsOverlayTextLabel {
            text: "Glass".to_string(),
            x: center_x,
            y,
            color: Rgb { r: 180, g: 140, b: 255 },
        });
        y += self.cell_height * 2.0;

        // Version
        labels.push(SettingsOverlayTextLabel {
            text: format!("v{}", env!("CARGO_PKG_VERSION")),
            x: center_x,
            y,
            color: Rgb { r: 204, g: 204, b: 204 },
        });
        y += self.cell_height * 1.5;

        // Description
        labels.push(SettingsOverlayTextLabel {
            text: "GPU-accelerated terminal emulator".to_string(),
            x: center_x,
            y,
            color: Rgb { r: 170, g: 170, b: 170 },
        });
        y += self.cell_height * 2.0;

        // GitHub
        labels.push(SettingsOverlayTextLabel {
            text: "github.com/candyhunterz/Glass".to_string(),
            x: center_x,
            y,
            color: Rgb { r: 100, g: 180, b: 246 },
        });
        y += self.cell_height * 1.5;

        // License
        labels.push(SettingsOverlayTextLabel {
            text: "MIT License".to_string(),
            x: center_x,
            y,
            color: Rgb { r: 170, g: 170, b: 170 },
        });
        y += self.cell_height * 2.0;

        // Platform
        labels.push(SettingsOverlayTextLabel {
            text: format!("Platform: {} {}", std::env::consts::OS, std::env::consts::ARCH),
            x: center_x,
            y,
            color: Rgb { r: 102, g: 102, b: 102 },
        });
        y += self.cell_height;

        // Renderer
        labels.push(SettingsOverlayTextLabel {
            text: "Renderer: wgpu".to_string(),
            x: center_x,
            y,
            color: Rgb { r: 102, g: 102, b: 102 },
        });

        labels
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test --package glass_renderer settings_overlay`
Expected: All 5 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/glass_renderer/src/settings_overlay.rs
git commit -m "feat(renderer): add about tab text generation"
```

---

### Task 4: Settings tab text generation (sidebar + fields)

**Files:**
- Modify: `crates/glass_renderer/src/settings_overlay.rs`

- [ ] **Step 1: Write test for settings text**

Add to the test module:

```rust
    #[test]
    fn test_settings_text_has_sections_and_fields() {
        let renderer = SettingsOverlayRenderer::new(10.0, 20.0);
        let config = SettingsConfigSnapshot::default();
        let labels = renderer.build_settings_text(800.0, 600.0, &config, 0, 0, false, "");
        let text: Vec<&str> = labels.iter().map(|l| l.text.as_str()).collect();
        // Sidebar sections present
        assert!(text.contains(&"Font"));
        assert!(text.contains(&"Agent Mode"));
        assert!(text.contains(&"SOI"));
        // Font section fields present (section_index=0 is Font)
        assert!(text.iter().any(|t| t.contains("Font Family")));
        assert!(text.iter().any(|t| t.contains("Font Size")));
    }

    #[test]
    fn test_settings_text_agent_section() {
        let renderer = SettingsOverlayRenderer::new(10.0, 20.0);
        let config = SettingsConfigSnapshot::default();
        // section_index=1 is Agent Mode
        let labels = renderer.build_settings_text(800.0, 600.0, &config, 1, 0, false, "");
        let text: Vec<&str> = labels.iter().map(|l| l.text.as_str()).collect();
        assert!(text.iter().any(|t| t.contains("Enabled")));
        assert!(text.iter().any(|t| t.contains("Mode")));
        assert!(text.iter().any(|t| t.contains("Budget")));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --package glass_renderer test_settings_text`
Expected: FAIL — `build_settings_text` does not exist yet

- [ ] **Step 3: Add build_settings_text method**

Add to the `impl SettingsOverlayRenderer` block:

```rust
    /// Build text labels for the Settings tab (sidebar + fields for selected section).
    #[allow(clippy::too_many_arguments)]
    pub fn build_settings_text(
        &self,
        viewport_width: f32,
        viewport_height: f32,
        config: &SettingsConfigSnapshot,
        section_index: usize,
        field_index: usize,
        editing: bool,
        edit_buffer: &str,
    ) -> Vec<SettingsOverlayTextLabel> {
        let mut labels = Vec::new();
        let padding = self.cell_width;
        let content_y = self.cell_height * 4.5;
        let sidebar_width = 16.0 * self.cell_width;

        // Sidebar: section list
        labels.push(SettingsOverlayTextLabel {
            text: "SECTIONS".to_string(),
            x: padding,
            y: content_y,
            color: Rgb { r: 180, g: 140, b: 255 },
        });

        let mut sidebar_y = content_y + self.cell_height * 1.5;
        for (i, section) in SETTINGS_SECTIONS.iter().enumerate() {
            let color = if i == section_index {
                Rgb { r: 255, g: 255, b: 255 }
            } else {
                Rgb { r: 136, g: 136, b: 136 }
            };
            labels.push(SettingsOverlayTextLabel {
                text: section.to_string(),
                x: padding + self.cell_width,
                y: sidebar_y,
                color,
            });
            sidebar_y += self.cell_height * 1.3;
        }

        // Right panel: fields for selected section
        let panel_x = sidebar_width + padding * 2.0;
        let section_name = SETTINGS_SECTIONS.get(section_index).copied().unwrap_or("Font");

        labels.push(SettingsOverlayTextLabel {
            text: section_name.to_uppercase(),
            x: panel_x,
            y: content_y,
            color: Rgb { r: 102, g: 102, b: 102 },
        });

        let mut field_y = content_y + self.cell_height * 1.5;
        let fields = self.fields_for_section(section_index, config, editing, edit_buffer);

        for (i, (label, value, is_toggle)) in fields.iter().enumerate() {
            let is_selected = i == field_index;
            let label_color = if is_selected {
                Rgb { r: 255, g: 255, b: 255 }
            } else {
                Rgb { r: 170, g: 170, b: 170 }
            };

            // Field label
            labels.push(SettingsOverlayTextLabel {
                text: label.to_string(),
                x: panel_x + self.cell_width,
                y: field_y,
                color: label_color,
            });

            // Field value
            let value_color = if *is_toggle {
                if value == "ON" {
                    Rgb { r: 106, g: 166, b: 106 }
                } else {
                    Rgb { r: 102, g: 102, b: 102 }
                }
            } else if is_selected {
                Rgb { r: 180, g: 140, b: 255 }
            } else {
                Rgb { r: 204, g: 204, b: 204 }
            };

            let indicator = if is_selected { "> " } else { "  " };
            labels.push(SettingsOverlayTextLabel {
                text: format!("{}{}", indicator, value),
                x: panel_x + 22.0 * self.cell_width,
                y: field_y,
                color: value_color,
            });

            field_y += self.cell_height * 1.3;
        }

        // Footer
        labels.push(SettingsOverlayTextLabel {
            text: "Advanced: edit ~/.glass/config.toml (hot-reloads automatically)".to_string(),
            x: panel_x,
            y: viewport_height - self.cell_height * 3.0,
            color: Rgb { r: 85, g: 85, b: 85 },
        });

        labels
    }

    /// Get field labels and values for a given section index.
    fn fields_for_section(
        &self,
        section_index: usize,
        config: &SettingsConfigSnapshot,
        editing: bool,
        edit_buffer: &str,
    ) -> Vec<(&'static str, String, bool)> {
        match section_index {
            0 => vec![
                // Font
                ("Font Family", if editing { edit_buffer.to_string() } else { config.font_family.clone() }, false),
                ("Font Size", format!("{:.1}", config.font_size), false),
            ],
            1 => vec![
                // Agent Mode
                ("Enabled", if config.agent_enabled { "ON".to_string() } else { "OFF".to_string() }, true),
                ("Mode", config.agent_mode.clone(), false),
                ("Budget (USD)", format!("${:.2}", config.agent_budget), false),
                ("Cooldown (sec)", format!("{}", config.agent_cooldown), false),
            ],
            2 => vec![
                // SOI
                ("Enabled", if config.soi_enabled { "ON".to_string() } else { "OFF".to_string() }, true),
                ("Shell Summary", if config.soi_shell_summary { "ON".to_string() } else { "OFF".to_string() }, true),
                ("Min Lines", format!("{}", config.soi_min_lines), false),
            ],
            3 => vec![
                // Snapshots
                ("Enabled", if config.snapshot_enabled { "ON".to_string() } else { "OFF".to_string() }, true),
                ("Max Storage (MB)", format!("{}", config.snapshot_max_mb), false),
                ("Retention (days)", format!("{}", config.snapshot_retention_days), false),
            ],
            4 => vec![
                // Pipes
                ("Enabled", if config.pipes_enabled { "ON".to_string() } else { "OFF".to_string() }, true),
                ("Auto Expand", if config.pipes_auto_expand { "ON".to_string() } else { "OFF".to_string() }, true),
                ("Max Capture (MB)", format!("{}", config.pipes_max_capture_mb), false),
            ],
            5 => vec![
                // History
                ("Max Output (KB)", format!("{}", config.history_max_output_kb), false),
            ],
            _ => vec![],
        }
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test --package glass_renderer settings_overlay`
Expected: All 7 tests pass

- [ ] **Step 5: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`
Expected: No warnings

- [ ] **Step 6: Commit**

```bash
git add crates/glass_renderer/src/settings_overlay.rs
git commit -m "feat(renderer): add settings tab text generation with sidebar and fields"
```

---

### Task 5: draw_settings_overlay in FrameRenderer

**Files:**
- Modify: `crates/glass_renderer/src/frame.rs`

- [ ] **Step 1: Add draw_settings_overlay method**

In `crates/glass_renderer/src/frame.rs`, add before `trim()` (same location pattern as `draw_activity_overlay`). Follow the exact same structure as `draw_activity_overlay`:

```rust
    /// Draw the settings overlay (fullscreen, on top of everything).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_settings_overlay(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        data: &crate::settings_overlay::SettingsOverlayRenderData,
    ) {
        let (cell_width, cell_height) = self.grid_renderer.cell_size();
        let overlay = crate::settings_overlay::SettingsOverlayRenderer::new(cell_width, cell_height);

        // 1. Backdrop rect
        let backdrop = overlay.build_backdrop_rect(width as f32, height as f32);
        self.rect_renderer.prepare(device, queue, &[backdrop], width, height);

        // 2. Build text labels: header + active tab content
        let mut all_labels = overlay.build_header_text(data.tab, width as f32);
        match data.tab {
            crate::settings_overlay::SettingsTab::Settings => {
                all_labels.extend(overlay.build_settings_text(
                    width as f32,
                    height as f32,
                    &data.config,
                    data.section_index,
                    data.field_index,
                    data.editing,
                    &data.edit_buffer,
                ));
            }
            crate::settings_overlay::SettingsTab::Shortcuts => {
                all_labels.extend(overlay.build_shortcuts_text(
                    width as f32,
                    height as f32,
                    data.shortcuts_scroll,
                ));
            }
            crate::settings_overlay::SettingsTab::About => {
                all_labels.extend(overlay.build_about_text(width as f32, height as f32));
            }
        }

        // 3-6. Build buffers, prepare, render (identical to draw_activity_overlay)
        let physical_font_size = self.grid_renderer.font_size * self.grid_renderer.scale_factor;
        let metrics = Metrics::new(physical_font_size, cell_height);
        let font_family = &self.grid_renderer.font_family;

        let mut settings_buffers: Vec<Buffer> = Vec::with_capacity(all_labels.len());
        for label in &all_labels {
            let mut buffer = Buffer::new(&mut self.glyph_cache.font_system, metrics);
            buffer.set_size(
                &mut self.glyph_cache.font_system,
                Some(width as f32 - label.x),
                Some(cell_height),
            );
            buffer.set_text(
                &mut self.glyph_cache.font_system,
                &label.text,
                &Attrs::new()
                    .family(Family::Name(font_family))
                    .color(GlyphonColor::rgba(label.color.r, label.color.g, label.color.b, 255)),
                Shaping::Advanced,
                None,
            );
            buffer.shape_until_scroll(&mut self.glyph_cache.font_system, false);
            settings_buffers.push(buffer);
        }

        let text_areas: Vec<TextArea<'_>> = all_labels
            .iter()
            .zip(settings_buffers.iter())
            .map(|(label, buffer)| TextArea {
                buffer,
                left: label.x,
                top: label.y,
                scale: 1.0,
                bounds: TextBounds {
                    left: 0,
                    top: 0,
                    right: width as i32,
                    bottom: height as i32,
                },
                default_color: GlyphonColor::rgba(label.color.r, label.color.g, label.color.b, 255),
                custom_glyphs: &[],
            })
            .collect();

        self.glyph_cache.viewport.update(queue, Resolution { width, height });

        if let Err(e) = self.glyph_cache.text_renderer.prepare(
            device, queue, &mut self.glyph_cache.font_system,
            &mut self.glyph_cache.atlas, &self.glyph_cache.viewport,
            text_areas, &mut self.glyph_cache.swash_cache,
        ) {
            tracing::warn!("Settings overlay text prepare error: {:?}", e);
        }

        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("settings_overlay_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.rect_renderer.render(&mut pass, 1);
            if let Err(e) = self.glyph_cache.text_renderer.render(
                &self.glyph_cache.atlas, &self.glyph_cache.viewport, &mut pass,
            ) {
                tracing::warn!("Settings overlay text render error: {:?}", e);
            }
        }
        queue.submit([encoder.finish()]);
    }
```

- [ ] **Step 2: Run build**

Run: `cargo build --package glass_renderer`
Expected: Compiles without errors

- [ ] **Step 3: Commit**

```bash
git add crates/glass_renderer/src/frame.rs
git commit -m "feat(renderer): add draw_settings_overlay to FrameRenderer"
```

---

## Chunk 2: Hotkey Wiring, Keyboard Navigation, and Config Write-Back

### Task 6: Overlay state and Ctrl+Shift+, hotkey in main.rs

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add overlay state fields to Processor**

In `src/main.rs`, add to the `Processor` struct (after `activity_verbose`):

```rust
    /// Whether the settings overlay is visible.
    settings_overlay_visible: bool,
    /// Active tab in the settings overlay.
    settings_overlay_tab: glass_renderer::SettingsTab,
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

- [ ] **Step 2: Initialize fields in Processor constructor**

In the `Processor { ... }` block (around line 5070), add:

```rust
                settings_overlay_visible: false,
                settings_overlay_tab: Default::default(),
                settings_section_index: 0,
                settings_field_index: 0,
                settings_editing: false,
                settings_edit_buffer: String::new(),
                settings_shortcuts_scroll: 0,
```

- [ ] **Step 3: Add Ctrl+Shift+, hotkey handler**

In the Ctrl+Shift key handler section (after the Ctrl+Shift+G block), add:

```rust
                            // Ctrl+Shift+,: Toggle settings overlay.
                            Key::Character(c) if c.as_str() == "<" || c.as_str() == "," => {
                                self.settings_overlay_visible = !self.settings_overlay_visible;
                                if !self.settings_overlay_visible {
                                    self.settings_overlay_tab = Default::default();
                                    self.settings_section_index = 0;
                                    self.settings_field_index = 0;
                                    self.settings_editing = false;
                                    self.settings_edit_buffer.clear();
                                    self.settings_shortcuts_scroll = 0;
                                }
                                ctx.window.request_redraw();
                                return;
                            }
```

Note: On US keyboards, Shift+, produces `<`. We match both `<` and `,` for robustness. The key is `Ctrl+Shift+,` which on some layouts sends `<` as the logical key.

- [ ] **Step 4: Add settings overlay keyboard handlers**

Add the overlay key handler block (before the activity overlay handler block):

```rust
                    // When the settings overlay is open, intercept all navigation keys.
                    if self.settings_overlay_visible && event.state == ElementState::Pressed {
                        match &event.logical_key {
                            Key::Named(NamedKey::Escape) => {
                                if self.settings_editing {
                                    // Cancel inline edit
                                    self.settings_editing = false;
                                    self.settings_edit_buffer.clear();
                                } else {
                                    self.settings_overlay_visible = false;
                                    self.settings_overlay_tab = Default::default();
                                    self.settings_section_index = 0;
                                    self.settings_field_index = 0;
                                    self.settings_shortcuts_scroll = 0;
                                }
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::Tab) if modifiers.shift_key() => {
                                self.settings_overlay_tab = self.settings_overlay_tab.prev();
                                self.settings_field_index = 0;
                                self.settings_shortcuts_scroll = 0;
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::Tab) => {
                                self.settings_overlay_tab = self.settings_overlay_tab.next();
                                self.settings_field_index = 0;
                                self.settings_shortcuts_scroll = 0;
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::ArrowUp) => {
                                match self.settings_overlay_tab {
                                    glass_renderer::SettingsTab::Settings => {
                                        if self.settings_field_index > 0 {
                                            self.settings_field_index -= 1;
                                        } else if self.settings_section_index > 0 {
                                            self.settings_section_index -= 1;
                                            self.settings_field_index = 0;
                                        }
                                    }
                                    glass_renderer::SettingsTab::Shortcuts => {
                                        self.settings_shortcuts_scroll =
                                            self.settings_shortcuts_scroll.saturating_sub(1);
                                    }
                                    _ => {}
                                }
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::ArrowDown) => {
                                match self.settings_overlay_tab {
                                    glass_renderer::SettingsTab::Settings => {
                                        self.settings_field_index += 1;
                                        // Clamping happens in renderer (fields_for_section length)
                                    }
                                    glass_renderer::SettingsTab::Shortcuts => {
                                        self.settings_shortcuts_scroll += 1;
                                    }
                                    _ => {}
                                }
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::ArrowLeft) => {
                                if self.settings_section_index > 0 {
                                    self.settings_section_index -= 1;
                                    self.settings_field_index = 0;
                                }
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::ArrowRight) => {
                                if self.settings_section_index < 5 {
                                    self.settings_section_index += 1;
                                    self.settings_field_index = 0;
                                }
                                ctx.window.request_redraw();
                                return;
                            }
                            _ => {
                                // Consume all keys when overlay is visible
                                return;
                            }
                        }
                    }
```

- [ ] **Step 5: Wire up rendering in redraw section**

In the redraw section, after the activity overlay block and before `frame.present();`, add:

```rust
                // Settings overlay (fullscreen, on top of everything)
                if self.settings_overlay_visible {
                    let config_snapshot = glass_renderer::settings_overlay::SettingsConfigSnapshot {
                        font_family: self.config.font_family.clone(),
                        font_size: self.config.font_size,
                        agent_enabled: self.config.agent.as_ref().map(|a| a.mode != glass_core::agent_runtime::AgentMode::Off).unwrap_or(false),
                        agent_mode: self.config.agent.as_ref().map(|a| format!("{:?}", a.mode)).unwrap_or_else(|| "Off".to_string()),
                        agent_budget: self.config.agent.as_ref().map(|a| a.max_budget_usd).unwrap_or(1.0),
                        agent_cooldown: self.config.agent.as_ref().map(|a| a.cooldown_secs).unwrap_or(30),
                        soi_enabled: self.config.soi.as_ref().map(|s| s.enabled).unwrap_or(true),
                        soi_shell_summary: self.config.soi.as_ref().map(|s| s.shell_summary).unwrap_or(false),
                        soi_min_lines: self.config.soi.as_ref().map(|s| s.min_lines).unwrap_or(0),
                        snapshot_enabled: self.config.snapshot.as_ref().map(|s| s.enabled).unwrap_or(true),
                        snapshot_max_mb: self.config.snapshot.as_ref().map(|s| s.max_size_mb).unwrap_or(500),
                        snapshot_retention_days: self.config.snapshot.as_ref().map(|s| s.retention_days).unwrap_or(30),
                        pipes_enabled: self.config.pipes.as_ref().map(|p| p.enabled).unwrap_or(true),
                        pipes_auto_expand: self.config.pipes.as_ref().map(|p| p.auto_expand).unwrap_or(true),
                        pipes_max_capture_mb: self.config.pipes.as_ref().map(|p| p.max_capture_mb).unwrap_or(10),
                        history_max_output_kb: self.config.history.as_ref().map(|h| h.max_output_capture_kb).unwrap_or(50),
                    };

                    let render_data = glass_renderer::SettingsOverlayRenderData {
                        tab: self.settings_overlay_tab,
                        section_index: self.settings_section_index,
                        field_index: self.settings_field_index,
                        editing: self.settings_editing,
                        edit_buffer: self.settings_edit_buffer.clone(),
                        config: config_snapshot,
                        shortcuts_scroll: self.settings_shortcuts_scroll,
                    };

                    ctx.frame_renderer.draw_settings_overlay(
                        ctx.renderer.device(),
                        ctx.renderer.queue(),
                        &view,
                        sc.width,
                        sc.height,
                        &render_data,
                    );
                }
```

- [ ] **Step 6: Build and clippy**

Run: `cargo build && cargo clippy --workspace -- -D warnings`
Expected: Compiles and passes clippy

- [ ] **Step 7: Commit**

```bash
git add src/main.rs
git commit -m "feat: wire settings overlay with Ctrl+Shift+, hotkey and navigation"
```

---

### Task 7: Config write-back (update_config_field)

**Files:**
- Modify: `crates/glass_core/src/config.rs`

- [ ] **Step 1: Write test for config write-back**

Add to the test module in `config.rs` (create one if needed):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_config_field_creates_section() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "font_size = 14.0\n").unwrap();

        update_config_field(&path, None, "font_size", "16.0").unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("16.0"));
    }

    #[test]
    fn test_update_config_field_nested_section() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[soi]\nenabled = true\n").unwrap();

        update_config_field(&path, Some("soi"), "enabled", "false").unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("enabled = false"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --package glass_core test_update_config_field`
Expected: FAIL — function does not exist

- [ ] **Step 3: Implement update_config_field**

Add to `crates/glass_core/src/config.rs`:

```rust
/// Update a single field in a TOML config file.
///
/// If `section` is None, updates a top-level key. If `section` is Some,
/// updates a key within that `[section]`. Creates the section if it doesn't
/// exist. The hot-reload watcher will detect the file change.
pub fn update_config_field(
    path: &std::path::Path,
    section: Option<&str>,
    key: &str,
    value: &str,
) -> Result<(), ConfigError> {
    let content = std::fs::read_to_string(path).unwrap_or_default();
    let mut doc: toml::Value = content.parse().unwrap_or(toml::Value::Table(toml::map::Map::new()));

    let table = doc.as_table_mut().ok_or_else(|| ConfigError {
        message: "Config file is not a TOML table".to_string(),
        line: None,
        column: None,
        snippet: None,
    })?;

    // Parse the value string into a TOML value
    let parsed_value: toml::Value = value.parse().unwrap_or(toml::Value::String(value.to_string()));

    if let Some(section_name) = section {
        let section_table = table
            .entry(section_name)
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        if let Some(t) = section_table.as_table_mut() {
            t.insert(key.to_string(), parsed_value);
        }
    } else {
        table.insert(key.to_string(), parsed_value);
    }

    let output = toml::to_string_pretty(&doc).map_err(|e| ConfigError {
        message: format!("Failed to serialize config: {}", e),
        line: None,
        column: None,
        snippet: None,
    })?;

    std::fs::write(path, output).map_err(|e| ConfigError {
        message: format!("Failed to write config: {}", e),
        line: None,
        column: None,
        snippet: None,
    })?;

    Ok(())
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --package glass_core test_update_config_field`
Expected: Both tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/glass_core/src/config.rs
git commit -m "feat(core): add update_config_field for settings overlay write-back"
```

---

### Task 8: Settings field editing in main.rs (Enter/Space toggles, +/- numbers)

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add Enter/Space handler for settings fields**

Inside the settings overlay key handler (in the `_ =>` catch-all arm, replace it with specific handlers):

Replace the `_ => { return; }` arm with:

```rust
                            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                                if matches!(self.settings_overlay_tab, glass_renderer::SettingsTab::Settings) {
                                    self.handle_settings_field_activate();
                                    ctx.window.request_redraw();
                                }
                                return;
                            }
                            Key::Character(c) if c.as_str() == "+" || c.as_str() == "=" => {
                                if matches!(self.settings_overlay_tab, glass_renderer::SettingsTab::Settings) {
                                    self.handle_settings_field_increment(true);
                                    ctx.window.request_redraw();
                                }
                                return;
                            }
                            Key::Character(c) if c.as_str() == "-" => {
                                if matches!(self.settings_overlay_tab, glass_renderer::SettingsTab::Settings) {
                                    self.handle_settings_field_increment(false);
                                    ctx.window.request_redraw();
                                }
                                return;
                            }
                            _ => {
                                return; // Consume all other keys
                            }
```

- [ ] **Step 2: Add helper methods to Processor**

Since `Processor` only has `impl ApplicationHandler<AppEvent> for Processor`, add the helpers as standalone functions that take the necessary state:

```rust
/// Handle Enter/Space on a settings field: toggles booleans, cycles enums.
fn handle_settings_activate(
    config: &glass_core::config::GlassConfig,
    section_index: usize,
    field_index: usize,
) -> Option<(Option<&'static str>, &'static str, String)> {
    // Returns (section, key, new_value) if a config write is needed
    match (section_index, field_index) {
        // Agent Mode: enabled (toggle mode Off <-> Watch)
        (1, 0) => {
            let current = config.agent.as_ref().map(|a| &a.mode).cloned().unwrap_or_default();
            let new_mode = if current == glass_core::agent_runtime::AgentMode::Off {
                "\"Watch\""
            } else {
                "\"Off\""
            };
            Some((Some("agent"), "mode", new_mode.to_string()))
        }
        // Agent Mode: mode (cycle Watch -> Assist -> Autonomous -> Off)
        (1, 1) => {
            let current = config.agent.as_ref().map(|a| &a.mode).cloned().unwrap_or_default();
            let new_mode = match current {
                glass_core::agent_runtime::AgentMode::Off => "\"Watch\"",
                glass_core::agent_runtime::AgentMode::Watch => "\"Assist\"",
                glass_core::agent_runtime::AgentMode::Assist => "\"Autonomous\"",
                glass_core::agent_runtime::AgentMode::Autonomous => "\"Off\"",
            };
            Some((Some("agent"), "mode", new_mode.to_string()))
        }
        // SOI: enabled
        (2, 0) => {
            let current = config.soi.as_ref().map(|s| s.enabled).unwrap_or(true);
            Some((Some("soi"), "enabled", (!current).to_string()))
        }
        // SOI: shell_summary
        (2, 1) => {
            let current = config.soi.as_ref().map(|s| s.shell_summary).unwrap_or(false);
            Some((Some("soi"), "shell_summary", (!current).to_string()))
        }
        // Snapshots: enabled
        (3, 0) => {
            let current = config.snapshot.as_ref().map(|s| s.enabled).unwrap_or(true);
            Some((Some("snapshot"), "enabled", (!current).to_string()))
        }
        // Pipes: enabled
        (4, 0) => {
            let current = config.pipes.as_ref().map(|p| p.enabled).unwrap_or(true);
            Some((Some("pipes"), "enabled", (!current).to_string()))
        }
        // Pipes: auto_expand
        (4, 1) => {
            let current = config.pipes.as_ref().map(|p| p.auto_expand).unwrap_or(true);
            Some((Some("pipes"), "auto_expand", (!current).to_string()))
        }
        _ => None,
    }
}

/// Handle +/- on a settings number field.
fn handle_settings_increment(
    config: &glass_core::config::GlassConfig,
    section_index: usize,
    field_index: usize,
    increment: bool,
) -> Option<(Option<&'static str>, &'static str, String)> {
    let delta = if increment { 1 } else { -1 };
    match (section_index, field_index) {
        // Font size: step 0.5
        (0, 1) => {
            let current = config.font_size;
            let new_val = (current + delta as f32 * 0.5).max(6.0).min(72.0);
            Some((None, "font_size", format!("{:.1}", new_val)))
        }
        // Agent budget: step 0.50
        (1, 2) => {
            let current = config.agent.as_ref().map(|a| a.max_budget_usd).unwrap_or(1.0);
            let new_val = (current + delta as f64 * 0.5).max(0.0);
            Some((Some("agent"), "max_budget_usd", format!("{:.2}", new_val)))
        }
        // Agent cooldown: step 5
        (1, 3) => {
            let current = config.agent.as_ref().map(|a| a.cooldown_secs).unwrap_or(30) as i64;
            let new_val = (current + delta as i64 * 5).max(0);
            Some((Some("agent"), "cooldown_secs", new_val.to_string()))
        }
        // SOI min_lines: step 1
        (2, 2) => {
            let current = config.soi.as_ref().map(|s| s.min_lines).unwrap_or(0) as i64;
            let new_val = (current + delta as i64).max(0);
            Some((Some("soi"), "min_lines", new_val.to_string()))
        }
        // Snapshot max_mb: step 100
        (3, 1) => {
            let current = config.snapshot.as_ref().map(|s| s.max_size_mb).unwrap_or(500) as i64;
            let new_val = (current + delta as i64 * 100).max(100);
            Some((Some("snapshot"), "max_size_mb", new_val.to_string()))
        }
        // Snapshot retention_days: step 1
        (3, 2) => {
            let current = config.snapshot.as_ref().map(|s| s.retention_days).unwrap_or(30) as i64;
            let new_val = (current + delta as i64).max(1);
            Some((Some("snapshot"), "retention_days", new_val.to_string()))
        }
        // Pipes max_capture_mb: step 1
        (4, 2) => {
            let current = config.pipes.as_ref().map(|p| p.max_capture_mb).unwrap_or(10) as i64;
            let new_val = (current + delta as i64).max(1);
            Some((Some("pipes"), "max_capture_mb", new_val.to_string()))
        }
        // History max_output_kb: step 10
        (5, 0) => {
            let current = config.history.as_ref().map(|h| h.max_output_capture_kb).unwrap_or(50) as i64;
            let new_val = (current + delta as i64 * 10).max(10);
            Some((Some("history"), "max_output_capture_kb", new_val.to_string()))
        }
        _ => None,
    }
}
```

- [ ] **Step 3: Wire the helpers into the key handlers**

Update `handle_settings_field_activate` and `handle_settings_field_increment` calls in the match arms to call the helpers and write to config:

```rust
// In the Enter/Space handler:
if let Some((section, key, value)) =
    handle_settings_activate(&self.config, self.settings_section_index, self.settings_field_index)
{
    if let Some(config_path) = glass_core::config::GlassConfig::config_path() {
        if let Err(e) = glass_core::config::update_config_field(&config_path, section, key, &value) {
            tracing::warn!("Settings: failed to write config: {}", e);
        }
    }
}

// In the +/= handler:
if let Some((section, key, value)) =
    handle_settings_increment(&self.config, self.settings_section_index, self.settings_field_index, true)
{
    if let Some(config_path) = glass_core::config::GlassConfig::config_path() {
        if let Err(e) = glass_core::config::update_config_field(&config_path, section, key, &value) {
            tracing::warn!("Settings: failed to write config: {}", e);
        }
    }
}

// In the - handler (same but false for increment):
if let Some((section, key, value)) =
    handle_settings_increment(&self.config, self.settings_section_index, self.settings_field_index, false)
{ /* same pattern */ }
```

- [ ] **Step 4: Build and clippy**

Run: `cargo build && cargo clippy --workspace -- -D warnings`
Expected: Compiles and passes clippy

- [ ] **Step 5: Run full test suite**

Run: `cargo test --workspace`
Expected: All tests pass

- [ ] **Step 6: Run fmt check**

Run: `cargo fmt --all -- --check`
Expected: No formatting issues

- [ ] **Step 7: Commit**

```bash
git add src/main.rs crates/glass_core/src/config.rs
git commit -m "feat: add settings field editing with config write-back"
```
