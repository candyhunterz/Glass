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
    // Orchestrator
    pub orchestrator_enabled: bool,
    pub orchestrator_max_iterations: u32,
    pub orchestrator_silence_secs: u64,
    pub orchestrator_prd_path: String,
    pub orchestrator_mode: String,
    pub orchestrator_verify_mode: String,
    pub orchestrator_feedback_llm: bool,
    pub orchestrator_max_prompt_hints: usize,
    pub orchestrator_ablation_enabled: bool,
    pub orchestrator_ablation_sweep_interval: u32,
}

impl Default for SettingsConfigSnapshot {
    fn default() -> Self {
        Self {
            font_family: {
                #[cfg(target_os = "windows")]
                {
                    "Consolas".to_string()
                }
                #[cfg(target_os = "macos")]
                {
                    "Menlo".to_string()
                }
                #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                {
                    "Monospace".to_string()
                }
            },
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
            orchestrator_enabled: false,
            orchestrator_max_iterations: 0,
            orchestrator_silence_secs: 60,
            orchestrator_prd_path: "PRD.md".to_string(),
            orchestrator_mode: "build".to_string(),
            orchestrator_verify_mode: "floor".to_string(),
            orchestrator_feedback_llm: false,
            orchestrator_max_prompt_hints: 10,
            orchestrator_ablation_enabled: true,
            orchestrator_ablation_sweep_interval: 20,
        }
    }
}

/// Section names for the settings sidebar.
pub const SETTINGS_SECTIONS: &[&str] = &[
    "Font",
    "Agent Mode",
    "SOI",
    "Snapshots",
    "Pipes",
    "History",
    "Orchestrator",
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
            ShortcutEntry {
                action: "Copy",
                keys: "Ctrl+Shift+C",
            },
            ShortcutEntry {
                action: "Paste",
                keys: "Ctrl+Shift+V",
            },
            ShortcutEntry {
                action: "Search history",
                keys: "Ctrl+Shift+F",
            },
            ShortcutEntry {
                action: "Undo last command",
                keys: "Ctrl+Shift+Z",
            },
            ShortcutEntry {
                action: "Toggle pipeline view",
                keys: "Ctrl+Shift+P",
            },
            ShortcutEntry {
                action: "Check for updates",
                keys: "Ctrl+Shift+U",
            },
        ],
    },
    ShortcutCategory {
        name: "TABS",
        entries: &[
            ShortcutEntry {
                action: "New tab",
                keys: "Ctrl+Shift+T",
            },
            ShortcutEntry {
                action: "Close tab/pane",
                keys: "Ctrl+Shift+W",
            },
            ShortcutEntry {
                action: "Next tab",
                keys: "Ctrl+Tab",
            },
            ShortcutEntry {
                action: "Previous tab",
                keys: "Ctrl+Shift+Tab",
            },
            ShortcutEntry {
                action: "Jump to tab 1-9",
                keys: "Ctrl+1-9",
            },
        ],
    },
    ShortcutCategory {
        name: "PANES",
        entries: &[
            ShortcutEntry {
                action: "Split Down (horizontal)",
                keys: "Ctrl+Shift+D",
            },
            ShortcutEntry {
                action: "Split East/right (vertical)",
                keys: "Ctrl+Shift+E",
            },
            ShortcutEntry {
                action: "Focus pane",
                keys: "Alt+Arrow",
            },
            ShortcutEntry {
                action: "Resize pane",
                keys: "Alt+Shift+Arrow",
            },
        ],
    },
    ShortcutCategory {
        name: "NAVIGATION",
        entries: &[
            ShortcutEntry {
                action: "Scroll up",
                keys: "Shift+PgUp",
            },
            ShortcutEntry {
                action: "Scroll down",
                keys: "Shift+PgDn",
            },
        ],
    },
    ShortcutCategory {
        name: "OVERLAYS",
        entries: &[
            ShortcutEntry {
                action: "Settings",
                keys: "Ctrl+Shift+,",
            },
            ShortcutEntry {
                action: "Review proposals",
                keys: "Ctrl+Shift+A",
            },
            ShortcutEntry {
                action: "Activity stream",
                keys: "Ctrl+Shift+G",
            },
        ],
    },
    ShortcutCategory {
        name: "AGENT MODE",
        entries: &[
            ShortcutEntry {
                action: "Accept proposal",
                keys: "Ctrl+Shift+Y",
            },
            ShortcutEntry {
                action: "Reject proposal",
                keys: "Ctrl+Shift+N",
            },
            ShortcutEntry {
                action: "Toggle orchestrator",
                keys: "Ctrl+Shift+O",
            },
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
        Self {
            cell_width,
            cell_height,
        }
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
            color: Rgb {
                r: 180,
                g: 140,
                b: 255,
            },
        });

        // Close hint
        labels.push(SettingsOverlayTextLabel {
            text: "Esc to close".to_string(),
            x: viewport_width - 14.0 * self.cell_width,
            y: header_y,
            color: Rgb {
                r: 85,
                g: 85,
                b: 85,
            },
        });

        // Tab bar
        let tabs = [
            SettingsTab::Settings,
            SettingsTab::Shortcuts,
            SettingsTab::About,
        ];
        let tab_y = self.cell_height * 2.5;
        let mut tab_x = padding;
        for tab in &tabs {
            let is_active = *tab == active_tab;
            let color = if is_active {
                Rgb {
                    r: 180,
                    g: 140,
                    b: 255,
                }
            } else {
                Rgb {
                    r: 102,
                    g: 102,
                    b: 102,
                }
            };
            // Show brackets around active tab: [ Settings ]
            let text = if is_active {
                format!("[ {} ]", tab.label())
            } else {
                format!("  {}  ", tab.label())
            };
            labels.push(SettingsOverlayTextLabel {
                text,
                x: tab_x,
                y: tab_y,
                color,
            });
            tab_x += (tab.label().len() + 4) as f32 * self.cell_width + self.cell_width * 2.0;
        }

        // Tab switch hint
        labels.push(SettingsOverlayTextLabel {
            text: "Tab / Shift+Tab to switch".to_string(),
            x: tab_x + self.cell_width * 2.0,
            y: tab_y,
            color: Rgb {
                r: 110,
                g: 110,
                b: 120,
            },
        });

        labels
    }

    /// Build text labels for the Shortcuts tab (multi-column cheatsheet).
    pub fn build_shortcuts_text(
        &self,
        viewport_width: f32,
        viewport_height: f32,
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
        let mid = SHORTCUT_DATA.len().div_ceil(2);
        let mut y_left = content_y;
        let mut y_right = content_y;

        for (i, category) in SHORTCUT_DATA.iter().enumerate() {
            let (col_x, col_y) = if i < mid {
                (col1_x, &mut y_left)
            } else {
                (col2_x, &mut y_right)
            };

            // Category header
            labels.push(SettingsOverlayTextLabel {
                text: category.name.to_string(),
                x: col_x,
                y: *col_y,
                color: Rgb {
                    r: 180,
                    g: 140,
                    b: 255,
                },
            });
            *col_y += self.cell_height * 1.2;

            // Shortcut entries
            for entry in category.entries {
                // Action name
                labels.push(SettingsOverlayTextLabel {
                    text: entry.action.to_string(),
                    x: col_x + self.cell_width,
                    y: *col_y,
                    color: Rgb {
                        r: 204,
                        g: 204,
                        b: 204,
                    },
                });
                // Key badge
                labels.push(SettingsOverlayTextLabel {
                    text: entry.keys.to_string(),
                    x: col_x + col_width
                        - entry.keys.len() as f32 * self.cell_width
                        - self.cell_width,
                    y: *col_y,
                    color: Rgb {
                        r: 180,
                        g: 140,
                        b: 255,
                    },
                });
                *col_y += self.cell_height;
            }
            *col_y += self.cell_height * 0.5; // gap between categories
        }

        // Footer hint
        labels.push(SettingsOverlayTextLabel {
            text: "Esc  close    Tab  switch tab".to_string(),
            x: self.cell_width,
            y: viewport_height - self.cell_height * 2.0,
            color: Rgb {
                r: 110,
                g: 110,
                b: 120,
            },
        });

        labels
    }

    /// Build text labels for the About tab.
    pub fn build_about_text(
        &self,
        viewport_width: f32,
        viewport_height: f32,
    ) -> Vec<SettingsOverlayTextLabel> {
        let mut labels = Vec::new();
        let center_x = viewport_width * 0.3;
        let mut y = self.cell_height * 6.0;

        // Glass name
        labels.push(SettingsOverlayTextLabel {
            text: "Glass".to_string(),
            x: center_x,
            y,
            color: Rgb {
                r: 180,
                g: 140,
                b: 255,
            },
        });
        y += self.cell_height * 2.0;

        // Version
        labels.push(SettingsOverlayTextLabel {
            text: format!("v{}", env!("CARGO_PKG_VERSION")),
            x: center_x,
            y,
            color: Rgb {
                r: 204,
                g: 204,
                b: 204,
            },
        });
        y += self.cell_height * 1.5;

        // Description
        labels.push(SettingsOverlayTextLabel {
            text: "GPU-accelerated terminal emulator".to_string(),
            x: center_x,
            y,
            color: Rgb {
                r: 170,
                g: 170,
                b: 170,
            },
        });
        y += self.cell_height * 2.0;

        // GitHub
        labels.push(SettingsOverlayTextLabel {
            text: "github.com/candyhunterz/Glass".to_string(),
            x: center_x,
            y,
            color: Rgb {
                r: 100,
                g: 180,
                b: 246,
            },
        });
        y += self.cell_height * 1.5;

        // License
        labels.push(SettingsOverlayTextLabel {
            text: "MIT License".to_string(),
            x: center_x,
            y,
            color: Rgb {
                r: 170,
                g: 170,
                b: 170,
            },
        });
        y += self.cell_height * 2.0;

        // Platform
        labels.push(SettingsOverlayTextLabel {
            text: format!(
                "Platform: {} {}",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
            x: center_x,
            y,
            color: Rgb {
                r: 102,
                g: 102,
                b: 102,
            },
        });
        y += self.cell_height;

        // Renderer
        labels.push(SettingsOverlayTextLabel {
            text: "Renderer: wgpu".to_string(),
            x: center_x,
            y,
            color: Rgb {
                r: 102,
                g: 102,
                b: 102,
            },
        });

        // Footer hint
        labels.push(SettingsOverlayTextLabel {
            text: "Esc  close    Tab  switch tab".to_string(),
            x: self.cell_width,
            y: viewport_height - self.cell_height * 2.0,
            color: Rgb {
                r: 110,
                g: 110,
                b: 120,
            },
        });

        labels
    }

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
            color: Rgb {
                r: 180,
                g: 140,
                b: 255,
            },
        });

        let mut sidebar_y = content_y + self.cell_height * 1.5;
        for (i, section) in SETTINGS_SECTIONS.iter().enumerate() {
            let color = if i == section_index {
                Rgb {
                    r: 255,
                    g: 255,
                    b: 255,
                }
            } else {
                Rgb {
                    r: 136,
                    g: 136,
                    b: 136,
                }
            };
            let prefix = if i == section_index { "> " } else { "  " };
            labels.push(SettingsOverlayTextLabel {
                text: format!("{}{}", prefix, section),
                x: padding,
                y: sidebar_y,
                color,
            });
            sidebar_y += self.cell_height * 1.3;
        }

        // Right panel: fields for selected section
        let panel_x = sidebar_width + padding * 2.0;
        let section_name = SETTINGS_SECTIONS
            .get(section_index)
            .copied()
            .unwrap_or("Font");

        labels.push(SettingsOverlayTextLabel {
            text: section_name.to_uppercase(),
            x: panel_x,
            y: content_y,
            color: Rgb {
                r: 102,
                g: 102,
                b: 102,
            },
        });

        let mut field_y = content_y + self.cell_height * 1.5;
        let fields = self.fields_for_section(section_index, config, editing, edit_buffer);

        for (i, (label, value, is_toggle, is_display_only)) in fields.iter().enumerate() {
            let is_selected = i == field_index;
            let label_color = if *is_display_only {
                Rgb {
                    r: 100,
                    g: 100,
                    b: 100,
                }
            } else if is_selected {
                Rgb {
                    r: 255,
                    g: 255,
                    b: 255,
                }
            } else {
                Rgb {
                    r: 170,
                    g: 170,
                    b: 170,
                }
            };

            // Field label
            labels.push(SettingsOverlayTextLabel {
                text: label.to_string(),
                x: panel_x + self.cell_width,
                y: field_y,
                color: label_color,
            });

            // Field value
            let value_color = if *is_display_only {
                Rgb {
                    r: 100,
                    g: 100,
                    b: 100,
                }
            } else if *is_toggle {
                if value == "ON" {
                    Rgb {
                        r: 106,
                        g: 166,
                        b: 106,
                    }
                } else {
                    Rgb {
                        r: 102,
                        g: 102,
                        b: 102,
                    }
                }
            } else if is_selected {
                Rgb {
                    r: 180,
                    g: 140,
                    b: 255,
                }
            } else {
                Rgb {
                    r: 204,
                    g: 204,
                    b: 204,
                }
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

        // Navigation footer
        let footer_y = viewport_height - self.cell_height * 3.5;
        labels.push(SettingsOverlayTextLabel {
            text:
                "Left/Right  section    Up/Down  field    Enter/Space  toggle    +/-  adjust value"
                    .to_string(),
            x: padding,
            y: footer_y,
            color: Rgb {
                r: 110,
                g: 110,
                b: 120,
            },
        });
        labels.push(SettingsOverlayTextLabel {
            text: "Esc  close    Tab  switch tab    Advanced: ~/.glass/config.toml".to_string(),
            x: padding,
            y: footer_y + self.cell_height * 1.2,
            color: Rgb {
                r: 110,
                g: 110,
                b: 120,
            },
        });

        // Suppress unused variable warnings
        let _ = viewport_width;

        labels
    }

    /// Get field labels and values for a given section index.
    fn fields_for_section(
        &self,
        section_index: usize,
        config: &SettingsConfigSnapshot,
        editing: bool,
        edit_buffer: &str,
    ) -> Vec<(&'static str, String, bool, bool)> {
        match section_index {
            0 => vec![
                // Font
                (
                    "Font Family",
                    if editing {
                        edit_buffer.to_string()
                    } else {
                        config.font_family.clone()
                    },
                    false,
                    false,
                ),
                (
                    "Font Size",
                    format!("{:.1}", config.font_size),
                    false,
                    false,
                ),
            ],
            1 => vec![
                // Agent Mode
                (
                    "Enabled",
                    if config.agent_enabled {
                        "ON".to_string()
                    } else {
                        "OFF".to_string()
                    },
                    true,
                    false,
                ),
                ("Mode", config.agent_mode.clone(), false, false),
                (
                    "Budget (USD)",
                    format!("${:.2}", config.agent_budget),
                    false,
                    false,
                ),
                (
                    "Cooldown (sec)",
                    format!("{}", config.agent_cooldown),
                    false,
                    false,
                ),
            ],
            2 => vec![
                // SOI
                (
                    "Enabled",
                    if config.soi_enabled {
                        "ON".to_string()
                    } else {
                        "OFF".to_string()
                    },
                    true,
                    false,
                ),
                (
                    "Shell Summary",
                    if config.soi_shell_summary {
                        "ON".to_string()
                    } else {
                        "OFF".to_string()
                    },
                    true,
                    false,
                ),
                (
                    "Min Lines",
                    format!("{}", config.soi_min_lines),
                    false,
                    false,
                ),
            ],
            3 => vec![
                // Snapshots
                (
                    "Enabled",
                    if config.snapshot_enabled {
                        "ON".to_string()
                    } else {
                        "OFF".to_string()
                    },
                    true,
                    false,
                ),
                (
                    "Max Storage (MB)",
                    format!("{}", config.snapshot_max_mb),
                    false,
                    false,
                ),
                (
                    "Retention (days)",
                    format!("{}", config.snapshot_retention_days),
                    false,
                    false,
                ),
            ],
            4 => vec![
                // Pipes
                (
                    "Enabled",
                    if config.pipes_enabled {
                        "ON".to_string()
                    } else {
                        "OFF".to_string()
                    },
                    true,
                    false,
                ),
                (
                    "Auto Expand",
                    if config.pipes_auto_expand {
                        "ON".to_string()
                    } else {
                        "OFF".to_string()
                    },
                    true,
                    false,
                ),
                (
                    "Max Capture (MB)",
                    format!("{}", config.pipes_max_capture_mb),
                    false,
                    false,
                ),
            ],
            5 => vec![
                // History
                (
                    "Max Output (KB)",
                    format!("{}", config.history_max_output_kb),
                    false,
                    false,
                ),
            ],
            6 => vec![
                // Orchestrator — 3 editable + 3 display-only
                (
                    "Enabled",
                    if config.orchestrator_enabled {
                        "ON".to_string()
                    } else {
                        "OFF".to_string()
                    },
                    true,
                    false,
                ),
                (
                    "Max Iterations",
                    if config.orchestrator_max_iterations == 0 {
                        "unlimited".to_string()
                    } else {
                        format!("{}", config.orchestrator_max_iterations)
                    },
                    false,
                    false,
                ),
                (
                    "Silence Timeout (sec)",
                    format!("{}", config.orchestrator_silence_secs),
                    false,
                    false,
                ),
                // Display-only fields (dimmed, not editable)
                (
                    "PRD Path",
                    config.orchestrator_prd_path.clone(),
                    false,
                    true,
                ),
                ("Mode", config.orchestrator_mode.clone(), false, true),
                (
                    "Verify Mode",
                    config.orchestrator_verify_mode.clone(),
                    false,
                    true,
                ),
                (
                    "Feedback LLM",
                    if config.orchestrator_feedback_llm {
                        "ON".to_string()
                    } else {
                        "OFF".to_string()
                    },
                    true,
                    false,
                ),
                (
                    "Max Prompt Hints",
                    format!("{}", config.orchestrator_max_prompt_hints),
                    false,
                    false,
                ),
                (
                    "Ablation Testing",
                    if config.orchestrator_ablation_enabled {
                        "ON".to_string()
                    } else {
                        "OFF".to_string()
                    },
                    true,
                    false,
                ),
                (
                    "Ablation Sweep Interval",
                    format!("{}", config.orchestrator_ablation_sweep_interval),
                    false,
                    false,
                ),
            ],
            _ => vec![],
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

    #[test]
    fn test_about_text_has_version() {
        let renderer = SettingsOverlayRenderer::new(10.0, 20.0);
        let labels = renderer.build_about_text(800.0, 600.0);
        let text: Vec<&str> = labels.iter().map(|l| l.text.as_str()).collect();
        assert!(text.iter().any(|t| t.contains("Glass")));
        assert!(text.iter().any(|t| t.contains("GPU-accelerated")));
        assert!(text.iter().any(|t| t.contains("MIT")));
    }

    #[test]
    fn test_settings_text_has_sections_and_fields() {
        let renderer = SettingsOverlayRenderer::new(10.0, 20.0);
        let config = SettingsConfigSnapshot::default();
        let labels = renderer.build_settings_text(800.0, 600.0, &config, 0, 0, false, "");
        let text: Vec<&str> = labels.iter().map(|l| l.text.as_str()).collect();
        // Sidebar sections present (prefixed with "> " or "  ")
        assert!(text
            .iter()
            .any(|t| t.contains("Font") && !t.contains("Font Family") && !t.contains("Font Size")));
        assert!(text.iter().any(|t| t.contains("Agent Mode")));
        assert!(text.iter().any(|t| t.contains("SOI")));
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
}
