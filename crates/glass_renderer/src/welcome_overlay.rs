//! WelcomeOverlayRenderer: 3-step first-run welcome wizard.
//!
//! Follows the same rendering pattern as SettingsOverlayRenderer:
//! fullscreen backdrop + text labels + single GPU pass with LoadOp::Load.

use alacritty_terminal::vte::ansi::Rgb;
use glass_core::onboarding::ProviderStatus;

use crate::rect_renderer::RectInstance;

/// Which step of the welcome wizard is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WelcomeStep {
    #[default]
    Providers,
    Orchestrator,
    QuickRef,
}

impl WelcomeStep {
    pub fn index(&self) -> usize {
        match self {
            Self::Providers => 0,
            Self::Orchestrator => 1,
            Self::QuickRef => 2,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Providers => Self::Orchestrator,
            Self::Orchestrator => Self::QuickRef,
            Self::QuickRef => Self::QuickRef, // no wrap
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Providers => Self::Providers, // no wrap
            Self::Orchestrator => Self::Providers,
            Self::QuickRef => Self::Orchestrator,
        }
    }
}

/// Text label for the welcome overlay.
#[derive(Debug, Clone)]
pub struct WelcomeOverlayTextLabel {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub color: Rgb,
}

/// Render data for the welcome overlay (passed from main.rs).
pub struct WelcomeOverlayRenderData {
    pub step: WelcomeStep,
    pub providers: Vec<ProviderStatus>,
}

/// Stateless renderer for the welcome overlay.
pub struct WelcomeOverlayRenderer {
    cell_width: f32,
    cell_height: f32,
}

impl WelcomeOverlayRenderer {
    pub fn new(cell_width: f32, cell_height: f32) -> Self {
        Self {
            cell_width,
            cell_height,
        }
    }

    /// Build the fullscreen backdrop rect.
    pub fn build_backdrop_rect(&self, viewport_w: f32, viewport_h: f32) -> RectInstance {
        RectInstance {
            pos: [0.0, 0.0, viewport_w, viewport_h],
            color: [0.03, 0.03, 0.06, 0.95],
        }
    }

    /// Build a centered content panel rect.
    fn build_panel_rect(&self, viewport_w: f32, viewport_h: f32) -> RectInstance {
        let panel_w = (viewport_w * 0.5).min(self.cell_width * 60.0);
        let panel_h = viewport_h * 0.6;
        let x = (viewport_w - panel_w) / 2.0;
        let y = (viewport_h - panel_h) / 2.0;

        RectInstance {
            pos: [x, y, panel_w, panel_h],
            color: [0.06, 0.06, 0.10, 0.9],
        }
    }

    /// Build all rects for the overlay (backdrop + panel).
    pub fn build_rects(&self, viewport_w: f32, viewport_h: f32) -> Vec<RectInstance> {
        vec![
            self.build_backdrop_rect(viewport_w, viewport_h),
            self.build_panel_rect(viewport_w, viewport_h),
        ]
    }

    /// Build all text labels for the current step.
    pub fn build_text(
        &self,
        data: &WelcomeOverlayRenderData,
        viewport_w: f32,
        viewport_h: f32,
    ) -> Vec<WelcomeOverlayTextLabel> {
        let panel_w = (viewport_w * 0.5).min(self.cell_width * 60.0);
        let panel_h = viewport_h * 0.6;
        let panel_x = (viewport_w - panel_w) / 2.0;
        let panel_y = (viewport_h - panel_h) / 2.0;

        let white = Rgb { r: 255, g: 255, b: 255 };
        let gray = Rgb { r: 136, g: 136, b: 136 };
        let cyan = Rgb { r: 56, g: 189, b: 248 };
        let green = Rgb { r: 74, g: 222, b: 128 };
        let red = Rgb { r: 248, g: 113, b: 113 };
        let purple = Rgb { r: 167, g: 139, b: 250 };

        let cx = panel_x + panel_w / 2.0;
        let mut labels = Vec::new();

        match data.step {
            WelcomeStep::Providers => {
                // Title
                labels.push(WelcomeOverlayTextLabel {
                    text: "Welcome to Glass".to_string(),
                    x: cx - self.cell_width * 8.0,
                    y: panel_y + self.cell_height * 2.0,
                    color: white,
                });
                labels.push(WelcomeOverlayTextLabel {
                    text: "AI-assisted terminal emulator".to_string(),
                    x: cx - self.cell_width * 14.0,
                    y: panel_y + self.cell_height * 3.5,
                    color: gray,
                });

                // Provider header
                labels.push(WelcomeOverlayTextLabel {
                    text: "LLM Providers Detected".to_string(),
                    x: panel_x + self.cell_width * 4.0,
                    y: panel_y + self.cell_height * 6.0,
                    color: gray,
                });

                // Provider list
                for (i, provider) in data.providers.iter().enumerate() {
                    let y_pos = panel_y + self.cell_height * (7.5 + i as f32 * 1.5);
                    let (icon, icon_color) = if provider.available {
                        ("\u{2713}", green) // checkmark
                    } else {
                        ("\u{2717}", red) // x mark
                    };
                    let name_color = if provider.available { white } else { gray };

                    labels.push(WelcomeOverlayTextLabel {
                        text: icon.to_string(),
                        x: panel_x + self.cell_width * 5.0,
                        y: y_pos,
                        color: icon_color,
                    });
                    labels.push(WelcomeOverlayTextLabel {
                        text: provider.name.to_string(),
                        x: panel_x + self.cell_width * 7.0,
                        y: y_pos,
                        color: name_color,
                    });
                    labels.push(WelcomeOverlayTextLabel {
                        text: provider.detail.clone(),
                        x: panel_x + panel_w - self.cell_width * 12.0,
                        y: y_pos,
                        color: gray,
                    });
                }
            }

            WelcomeStep::Orchestrator => {
                labels.push(WelcomeOverlayTextLabel {
                    text: "Start Building".to_string(),
                    x: cx - self.cell_width * 7.0,
                    y: panel_y + self.cell_height * 2.0,
                    color: white,
                });
                labels.push(WelcomeOverlayTextLabel {
                    text: "The shortcut you came here for".to_string(),
                    x: cx - self.cell_width * 15.0,
                    y: panel_y + self.cell_height * 3.5,
                    color: gray,
                });
                labels.push(WelcomeOverlayTextLabel {
                    text: "Ctrl+Shift+O".to_string(),
                    x: cx - self.cell_width * 6.0,
                    y: panel_y + self.cell_height * 6.5,
                    color: cyan,
                });
                labels.push(WelcomeOverlayTextLabel {
                    text: "Starts the Orchestrator \u{2014} autonomous".to_string(),
                    x: cx - self.cell_width * 18.0,
                    y: panel_y + self.cell_height * 8.5,
                    color: Rgb { r: 204, g: 204, b: 204 },
                });
                labels.push(WelcomeOverlayTextLabel {
                    text: "TDD-driven building from a PRD.".to_string(),
                    x: cx - self.cell_width * 15.0,
                    y: panel_y + self.cell_height * 10.0,
                    color: Rgb { r: 204, g: 204, b: 204 },
                });
                labels.push(WelcomeOverlayTextLabel {
                    text: "Write a PRD.md, press the shortcut, walk away.".to_string(),
                    x: cx - self.cell_width * 23.0,
                    y: panel_y + self.cell_height * 11.5,
                    color: gray,
                });
                labels.push(WelcomeOverlayTextLabel {
                    text: "Configure provider in Ctrl+Shift+, \u{2192} Agent Mode".to_string(),
                    x: cx - self.cell_width * 23.0,
                    y: panel_y + self.cell_height * 14.0,
                    color: purple,
                });
            }

            WelcomeStep::QuickRef => {
                labels.push(WelcomeOverlayTextLabel {
                    text: "You're Ready".to_string(),
                    x: cx - self.cell_width * 6.0,
                    y: panel_y + self.cell_height * 2.0,
                    color: white,
                });
                labels.push(WelcomeOverlayTextLabel {
                    text: "Glass will teach you the rest as you go".to_string(),
                    x: cx - self.cell_width * 20.0,
                    y: panel_y + self.cell_height * 3.5,
                    color: gray,
                });

                labels.push(WelcomeOverlayTextLabel {
                    text: "SHORTCUTS YOU'LL DISCOVER".to_string(),
                    x: panel_x + self.cell_width * 4.0,
                    y: panel_y + self.cell_height * 6.0,
                    color: gray,
                });

                let shortcuts = [
                    ("Ctrl+Shift+Z", "Undo"),
                    ("Ctrl+Shift+P", "Pipes"),
                    ("Ctrl+Shift+F", "Search"),
                    ("Ctrl+Shift+A", "Agent"),
                ];
                for (i, (key, label)) in shortcuts.iter().enumerate() {
                    let col = if i < 2 { 0.0 } else { panel_w * 0.5 };
                    let row = (i % 2) as f32;
                    labels.push(WelcomeOverlayTextLabel {
                        text: key.to_string(),
                        x: panel_x + self.cell_width * 5.0 + col,
                        y: panel_y + self.cell_height * (7.5 + row * 1.5),
                        color: cyan,
                    });
                    labels.push(WelcomeOverlayTextLabel {
                        text: label.to_string(),
                        x: panel_x + self.cell_width * 19.0 + col,
                        y: panel_y + self.cell_height * (7.5 + row * 1.5),
                        color: gray,
                    });
                }

                labels.push(WelcomeOverlayTextLabel {
                    text: "Ctrl+Shift+, for all settings & shortcuts".to_string(),
                    x: cx - self.cell_width * 20.0,
                    y: panel_y + self.cell_height * 12.0,
                    color: purple,
                });

                labels.push(WelcomeOverlayTextLabel {
                    text: "Press Enter or Esc to start".to_string(),
                    x: cx - self.cell_width * 13.0,
                    y: panel_y + self.cell_height * 15.0,
                    color: cyan,
                });
            }
        }

        // Step indicator (all steps)
        let step_idx = data.step.index();
        let dots: String = (0..3)
            .map(|i| if i == step_idx { "\u{25CF}" } else { "\u{25CB}" })
            .collect::<Vec<_>>()
            .join("  ");
        labels.push(WelcomeOverlayTextLabel {
            text: format!("Step {} of 3   {}", step_idx + 1, dots),
            x: cx - self.cell_width * 8.0,
            y: panel_y + panel_h - self.cell_height * 2.0,
            color: gray,
        });

        // Navigation hint
        let nav_hint = match data.step {
            WelcomeStep::Providers => "\u{2192} to continue",
            WelcomeStep::Orchestrator => "\u{2190} back  \u{2192} continue",
            WelcomeStep::QuickRef => "\u{2190} back  Enter/Esc to start",
        };
        labels.push(WelcomeOverlayTextLabel {
            text: nav_hint.to_string(),
            x: cx - self.cell_width * 10.0,
            y: panel_y + panel_h - self.cell_height * 1.0,
            color: gray,
        });

        labels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_navigation_no_wrap() {
        assert_eq!(WelcomeStep::Providers.prev(), WelcomeStep::Providers);
        assert_eq!(WelcomeStep::QuickRef.next(), WelcomeStep::QuickRef);
    }

    #[test]
    fn step_navigation_forward() {
        assert_eq!(WelcomeStep::Providers.next(), WelcomeStep::Orchestrator);
        assert_eq!(WelcomeStep::Orchestrator.next(), WelcomeStep::QuickRef);
    }

    #[test]
    fn step_navigation_backward() {
        assert_eq!(WelcomeStep::QuickRef.prev(), WelcomeStep::Orchestrator);
        assert_eq!(WelcomeStep::Orchestrator.prev(), WelcomeStep::Providers);
    }

    #[test]
    fn build_rects_returns_backdrop_and_panel() {
        let renderer = WelcomeOverlayRenderer::new(8.0, 16.0);
        let rects = renderer.build_rects(800.0, 600.0);
        assert_eq!(rects.len(), 2);
        // First rect is fullscreen backdrop
        assert_eq!(rects[0].pos, [0.0, 0.0, 800.0, 600.0]);
    }

    #[test]
    fn build_text_returns_labels_for_each_step() {
        let renderer = WelcomeOverlayRenderer::new(8.0, 16.0);
        let providers = vec![
            ProviderStatus {
                name: "Claude CLI",
                available: true,
                detail: "found".to_string(),
            },
        ];

        for step in [WelcomeStep::Providers, WelcomeStep::Orchestrator, WelcomeStep::QuickRef] {
            let data = WelcomeOverlayRenderData {
                step,
                providers: providers.clone(),
            };
            let labels = renderer.build_text(&data, 800.0, 600.0);
            assert!(!labels.is_empty(), "Step {:?} should produce labels", step);
        }
    }
}
