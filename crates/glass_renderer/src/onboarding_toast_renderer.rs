//! OnboardingToastRenderer: contextual hint toasts for feature discovery.
//!
//! Follows the same pattern as ProposalToastRenderer: positioned bottom-right
//! above the status bar. Each toast has an icon, description, and shortcut hint.
//! Auto-dismissed after 5 seconds (timer owned by main.rs).

use alacritty_terminal::vte::ansi::Rgb;
use glass_core::onboarding::HintId;

use crate::rect_renderer::RectInstance;

/// Data needed to render an onboarding toast.
pub struct OnboardingToastRenderData {
    pub hint_id: HintId,
    pub remaining_secs: u64,
    /// Number of pipe stages (only used for PipeViz hint).
    pub pipe_stages: Option<usize>,
}

/// Text label for rendering in the onboarding toast.
#[derive(Debug, Clone)]
pub struct OnboardingToastTextLabel {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub color: Rgb,
}

/// Stateless renderer for onboarding hint toasts.
pub struct OnboardingToastRenderer {
    cell_width: f32,
    cell_height: f32,
}

impl OnboardingToastRenderer {
    pub fn new(cell_width: f32, cell_height: f32) -> Self {
        Self {
            cell_width,
            cell_height,
        }
    }

    /// Build background rects for the toast. Returns a single semi-transparent rect.
    pub fn build_toast_rects(&self, viewport_w: f32, viewport_h: f32) -> Vec<RectInstance> {
        let toast_w = viewport_w * 0.45;
        let toast_h = self.cell_height * 2.5;
        let x = viewport_w - toast_w - self.cell_width;
        let y = viewport_h - toast_h - self.cell_height * 1.5; // above status bar

        vec![RectInstance {
            pos: [x, y, toast_w, toast_h],
            color: [0.02, 0.14, 0.20, 0.92], // dark teal
        }]
    }

    /// Build text labels for the toast content.
    pub fn build_toast_text(
        &self,
        data: &OnboardingToastRenderData,
        viewport_w: f32,
        viewport_h: f32,
    ) -> Vec<OnboardingToastTextLabel> {
        let toast_w = viewport_w * 0.45;
        let toast_h = self.cell_height * 2.5;
        let x = viewport_w - toast_w - self.cell_width;
        let y = viewport_h - toast_h - self.cell_height * 1.5;

        let (icon, description, shortcut) = hint_content(data.hint_id, data.pipe_stages);

        let mut labels = Vec::with_capacity(4);

        // Icon
        labels.push(OnboardingToastTextLabel {
            text: icon.to_string(),
            x: x + self.cell_width * 0.8,
            y: y + toast_h * 0.3,
            color: Rgb {
                r: 56,
                g: 189,
                b: 248,
            }, // cyan
        });

        // Description
        labels.push(OnboardingToastTextLabel {
            text: description,
            x: x + self.cell_width * 2.5,
            y: y + toast_h * 0.25,
            color: Rgb {
                r: 220,
                g: 220,
                b: 220,
            }, // light gray
        });

        // Shortcut hint
        labels.push(OnboardingToastTextLabel {
            text: shortcut,
            x: x + self.cell_width * 2.5,
            y: y + toast_h * 0.6,
            color: Rgb {
                r: 160,
                g: 180,
                b: 190,
            }, // muted cyan-gray
        });

        // Countdown
        labels.push(OnboardingToastTextLabel {
            text: format!("{}s", data.remaining_secs),
            x: x + toast_w - self.cell_width * 2.0,
            y: y + toast_h * 0.25,
            color: Rgb {
                r: 102,
                g: 102,
                b: 102,
            }, // dim gray
        });

        labels
    }
}

/// Returns (icon, description, shortcut hint) for a given hint ID.
fn hint_content(hint_id: HintId, pipe_stages: Option<usize>) -> (&'static str, String, String) {
    match hint_id {
        HintId::Undo => (
            "\u{27F2}", // ⟲
            "That command changed files on disk".to_string(),
            "Ctrl+Shift+Z to undo".to_string(),
        ),
        HintId::PipeViz => {
            let stages = pipe_stages.unwrap_or(2);
            (
                "\u{27F6}", // ⟶
                format!("Pipeline detected ({stages} stages)"),
                "Ctrl+Shift+P to inspect each stage".to_string(),
            )
        }
        HintId::HistorySearch => (
            "\u{2315}", // ⌕
            "You have 10+ commands in history".to_string(),
            "Ctrl+Shift+F to search".to_string(),
        ),
        HintId::Soi => (
            "\u{25C8}", // ◈
            "Glass parsed that output".to_string(),
            "Ctrl+Shift+F to query results later".to_string(),
        ),
        HintId::AgentProposals => (
            "\u{25C9}", // ◉
            "Agent has a proposal ready".to_string(),
            "Ctrl+Shift+A to review changes".to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_content_covers_all_variants() {
        let variants = [
            HintId::Undo,
            HintId::PipeViz,
            HintId::HistorySearch,
            HintId::Soi,
            HintId::AgentProposals,
        ];
        for hint in variants {
            let (icon, desc, shortcut) = hint_content(hint, Some(3));
            assert!(!icon.is_empty());
            assert!(!desc.is_empty());
            assert!(!shortcut.is_empty());
        }
    }

    #[test]
    fn pipe_hint_shows_stage_count() {
        let (_, desc, _) = hint_content(HintId::PipeViz, Some(5));
        assert!(desc.contains("5 stages"));
    }

    #[test]
    fn toast_rects_positioned_above_status_bar() {
        let renderer = OnboardingToastRenderer::new(8.0, 16.0);
        let rects = renderer.build_toast_rects(800.0, 600.0);
        assert_eq!(rects.len(), 1);
        let rect = &rects[0];
        // Should be in bottom-right area
        assert!(rect.pos[0] > 400.0); // right half
        assert!(rect.pos[1] > 400.0); // bottom area
    }
}
