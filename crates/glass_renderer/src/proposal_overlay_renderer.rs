//! ProposalOverlayRenderer: generates visual elements for the agent proposal review overlay.
//!
//! Produces a semi-transparent backdrop, a centered panel, selected-item highlight rects,
//! header/proposal/diff-preview/footer text labels. Display-only; does not intercept
//! keyboard input (hotkeys handled by event loop).

use alacritty_terminal::vte::ansi::Rgb;

use crate::rect_renderer::RectInstance;

/// Data transferred from Processor to the renderer for the proposal overlay.
#[derive(Debug, Clone)]
pub struct ProposalOverlayRenderData {
    /// List of (description, action) proposal tuples.
    pub proposals: Vec<(String, String)>,
    /// Index of the currently selected proposal.
    pub selected: usize,
    /// Diff preview text (lines joined with '\n').
    pub diff_preview: String,
}

/// A text label to be rendered in the proposal overlay.
#[derive(Debug, Clone)]
pub struct ProposalOverlayTextLabel {
    /// Text content
    pub text: String,
    /// X position in pixels
    pub x: f32,
    /// Y position in pixels
    pub y: f32,
    /// Text color
    pub color: Rgb,
}

/// Renders proposal review overlay visual elements (backdrop + panel + text).
///
/// Stateless helper that converts ProposalOverlayRenderData into RectInstances and text
/// labels for the GPU rendering pipeline. Follows the SearchOverlayRenderer pattern.
pub struct ProposalOverlayRenderer {
    cell_width: f32,
    cell_height: f32,
}

impl ProposalOverlayRenderer {
    /// Create a new ProposalOverlayRenderer with the given cell dimensions.
    pub fn new(cell_width: f32, cell_height: f32) -> Self {
        Self {
            cell_width,
            cell_height,
        }
    }

    /// Build the overlay rectangles.
    ///
    /// Returns:
    /// - Backdrop rect (full viewport, semi-transparent dark [0.03, 0.03, 0.03, 0.88])
    /// - Panel rect (80% width centered, full height minus 2*cell_height for tab/status bars)
    /// - Selected row highlight rect (if proposals are non-empty)
    pub fn build_overlay_rects(
        &self,
        viewport_w: f32,
        viewport_h: f32,
        data: &ProposalOverlayRenderData,
    ) -> Vec<RectInstance> {
        let mut rects = Vec::new();

        // Backdrop: full viewport, semi-transparent
        rects.push(RectInstance {
            pos: [0.0, 0.0, viewport_w, viewport_h],
            color: [0.03, 0.03, 0.03, 0.88],
        });

        // Panel: 80% width centered, full height minus tab+status bars
        let panel_w = viewport_w * 0.8;
        let panel_x = (viewport_w - panel_w) / 2.0;
        let panel_y = self.cell_height; // below tab bar
        let panel_h = viewport_h - self.cell_height * 2.0; // minus tab + status bars
        rects.push(RectInstance {
            pos: [panel_x, panel_y, panel_w, panel_h],
            color: [0.08, 0.12, 0.15, 1.0],
        });

        // Selected row highlight
        if !data.proposals.is_empty() {
            let selected = data.selected.min(data.proposals.len() - 1);
            // Header is row 0, proposals start at row 1 (each 1 cell_height tall)
            let header_rows = 2; // header + separator gap
            let row_y = panel_y + (header_rows + selected) as f32 * self.cell_height;
            rects.push(RectInstance {
                pos: [panel_x, row_y, panel_w, self.cell_height],
                color: [0.10, 0.30, 0.45, 0.85],
            });
        }

        rects
    }

    /// Build the text labels for the proposal overlay.
    ///
    /// Returns:
    /// - Header label: "Agent Proposals (N pending)"
    /// - Proposal description labels (selected marked with ">", others with " ")
    /// - Diff preview lines (max 50 lines)
    /// - Footer hint label with Ctrl+Shift+Y/N/A keys
    pub fn build_overlay_text(
        &self,
        viewport_w: f32,
        viewport_h: f32,
        data: &ProposalOverlayRenderData,
    ) -> Vec<ProposalOverlayTextLabel> {
        let mut labels = Vec::new();

        let panel_w = viewport_w * 0.8;
        let panel_x = (viewport_w - panel_w) / 2.0;
        let panel_y = self.cell_height;
        let text_x = panel_x + self.cell_width;

        // Header: "Agent Proposals (N pending)"
        let header_color = Rgb {
            r: 100,
            g: 200,
            b: 220,
        };
        labels.push(ProposalOverlayTextLabel {
            text: format!("Agent Proposals ({} pending)", data.proposals.len()),
            x: text_x,
            y: panel_y + self.cell_height * 0.25,
            color: header_color,
        });

        // Proposal list rows (header_rows = 2 for header + gap row)
        let header_rows = 2;
        let selected = if data.proposals.is_empty() {
            0
        } else {
            data.selected.min(data.proposals.len() - 1)
        };

        for (i, (desc, _action)) in data.proposals.iter().enumerate() {
            let prefix = if i == selected { "> " } else { "  " };
            let color = if i == selected {
                Rgb {
                    r: 220,
                    g: 220,
                    b: 220,
                }
            } else {
                Rgb {
                    r: 160,
                    g: 160,
                    b: 160,
                }
            };
            labels.push(ProposalOverlayTextLabel {
                text: format!("{}{}", prefix, desc),
                x: text_x,
                y: panel_y + (header_rows + i) as f32 * self.cell_height,
                color,
            });
        }

        // Diff preview section
        // Start after proposal list + a gap row
        let diff_start_row = header_rows + data.proposals.len() + 1;
        let diff_lines: Vec<&str> = data.diff_preview.lines().collect();
        let truncated_lines: Vec<&str> = diff_lines.iter().copied().take(50).collect();

        for (i, line) in truncated_lines.iter().enumerate() {
            let color = if line.starts_with('+') {
                Rgb {
                    r: 100,
                    g: 200,
                    b: 100,
                }
            } else if line.starts_with('-') {
                Rgb {
                    r: 200,
                    g: 100,
                    b: 100,
                }
            } else {
                Rgb {
                    r: 140,
                    g: 140,
                    b: 140,
                }
            };
            labels.push(ProposalOverlayTextLabel {
                text: line.to_string(),
                x: text_x,
                y: panel_y + (diff_start_row + i) as f32 * self.cell_height,
                color,
            });
        }

        // Footer hint
        let footer_y = viewport_h - self.cell_height * 1.5;
        labels.push(ProposalOverlayTextLabel {
            text: "[Ctrl+Shift+Y: approve] [Ctrl+Shift+N: reject] [Ctrl+Shift+A: toggle overlay]"
                .to_string(),
            x: text_x,
            y: footer_y,
            color: Rgb {
                r: 120,
                g: 140,
                b: 150,
            },
        });

        labels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer() -> ProposalOverlayRenderer {
        ProposalOverlayRenderer::new(10.0, 20.0)
    }

    fn sample_data() -> ProposalOverlayRenderData {
        ProposalOverlayRenderData {
            proposals: vec![
                ("Create auth module".to_string(), "write".to_string()),
                ("Update config file".to_string(), "write".to_string()),
            ],
            selected: 0,
            diff_preview: "+ added line\n- removed line\n  context line".to_string(),
        }
    }

    #[test]
    fn test_build_overlay_rects_has_backdrop() {
        let r = renderer();
        let data = sample_data();
        let rects = r.build_overlay_rects(800.0, 600.0, &data);
        // First rect must be the backdrop (full viewport)
        assert!(!rects.is_empty(), "Should have at least one rect");
        let backdrop = &rects[0];
        assert_eq!(backdrop.pos[0], 0.0, "Backdrop x should be 0");
        assert_eq!(backdrop.pos[1], 0.0, "Backdrop y should be 0");
        assert_eq!(
            backdrop.pos[2], 800.0,
            "Backdrop width should be full viewport"
        );
        assert_eq!(
            backdrop.pos[3], 600.0,
            "Backdrop height should be full viewport"
        );
        assert!(
            (backdrop.color[3] - 0.88).abs() < 0.01,
            "Backdrop alpha should be 0.88"
        );
    }

    #[test]
    fn test_build_overlay_rects_backdrop_color() {
        let r = renderer();
        let data = sample_data();
        let rects = r.build_overlay_rects(800.0, 600.0, &data);
        let c = rects[0].color;
        assert!((c[0] - 0.03).abs() < 0.01);
        assert!((c[1] - 0.03).abs() < 0.01);
        assert!((c[2] - 0.03).abs() < 0.01);
    }

    #[test]
    fn test_build_overlay_rects_panel_sizing() {
        let r = renderer();
        let data = sample_data();
        let rects = r.build_overlay_rects(800.0, 600.0, &data);
        // Panel is 2nd rect
        assert!(rects.len() >= 2, "Should have backdrop + panel");
        let panel = &rects[1];
        // 80% of 800 = 640
        assert_eq!(panel.pos[2], 640.0, "Panel width should be 80% of viewport");
        // centered: x = (800-640)/2 = 80
        assert_eq!(panel.pos[0], 80.0, "Panel should be horizontally centered");
        // y = cell_height (below tab bar) = 20
        assert_eq!(panel.pos[1], 20.0, "Panel should start below tab bar");
        // height = 600 - 20*2 = 560
        assert_eq!(
            panel.pos[3], 560.0,
            "Panel height should exclude tab+status bars"
        );
    }

    #[test]
    fn test_build_overlay_rects_selected_highlight() {
        let r = renderer();
        let data = sample_data();
        let rects = r.build_overlay_rects(800.0, 600.0, &data);
        // With proposals, should have backdrop + panel + highlight = 3 rects
        assert_eq!(rects.len(), 3, "Should have backdrop + panel + highlight");
        // Highlight matches panel x and width
        let panel = &rects[1];
        let highlight = &rects[2];
        assert_eq!(
            highlight.pos[0], panel.pos[0],
            "Highlight x should match panel x"
        );
        assert_eq!(
            highlight.pos[2], panel.pos[2],
            "Highlight width should match panel width"
        );
    }

    #[test]
    fn test_build_overlay_rects_no_highlight_empty_proposals() {
        let r = renderer();
        let data = ProposalOverlayRenderData {
            proposals: vec![],
            selected: 0,
            diff_preview: String::new(),
        };
        let rects = r.build_overlay_rects(800.0, 600.0, &data);
        // Only backdrop + panel (no highlight for empty proposals)
        assert_eq!(rects.len(), 2, "No highlight rect when proposals is empty");
    }

    #[test]
    fn test_build_overlay_text_header() {
        let r = renderer();
        let data = sample_data();
        let labels = r.build_overlay_text(800.0, 600.0, &data);
        assert!(!labels.is_empty(), "Should produce text labels");
        assert!(
            labels[0].text.contains("Agent Proposals"),
            "First label should be the header"
        );
        assert!(
            labels[0].text.contains("2 pending"),
            "Header should show proposal count"
        );
    }

    #[test]
    fn test_build_overlay_text_selected_marked() {
        let r = renderer();
        let data = sample_data();
        let labels = r.build_overlay_text(800.0, 600.0, &data);
        // Find the proposal labels (after header)
        let proposal_labels: Vec<&ProposalOverlayTextLabel> = labels
            .iter()
            .filter(|l| l.text.starts_with("> ") || l.text.starts_with("  "))
            .collect();
        assert!(!proposal_labels.is_empty(), "Should have proposal labels");
        assert!(
            proposal_labels[0].text.starts_with("> "),
            "Selected proposal should be prefixed with >"
        );
        assert!(
            proposal_labels[1].text.starts_with("  "),
            "Non-selected proposals should be prefixed with spaces"
        );
    }

    #[test]
    fn test_build_overlay_text_diff_preview_lines() {
        let r = renderer();
        let data = sample_data();
        let labels = r.build_overlay_text(800.0, 600.0, &data);
        // Count diff preview labels (the sample has 3 lines)
        let diff_labels: Vec<&ProposalOverlayTextLabel> = labels
            .iter()
            .filter(|l| {
                l.text.starts_with('+')
                    || l.text.starts_with('-')
                    || (l.text.starts_with(' ') && l.text != " " && !l.text.starts_with("  "))
            })
            .collect();
        assert!(!diff_labels.is_empty(), "Should include diff preview lines");
    }

    #[test]
    fn test_build_overlay_text_diff_truncation_at_50_lines() {
        let r = renderer();
        // Create 100 diff lines
        let diff: String = (0..100)
            .map(|i| format!("+ line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let data = ProposalOverlayRenderData {
            proposals: vec![("Test proposal".to_string(), "write".to_string())],
            selected: 0,
            diff_preview: diff,
        };
        let labels = r.build_overlay_text(800.0, 600.0, &data);
        // Count labels with "+" prefix (diff lines)
        let diff_label_count = labels
            .iter()
            .filter(|l| l.text.starts_with("+ line"))
            .count();
        assert_eq!(
            diff_label_count, 50,
            "Diff preview should be truncated to 50 lines"
        );
    }

    #[test]
    fn test_build_overlay_text_footer_hint() {
        let r = renderer();
        let data = sample_data();
        let labels = r.build_overlay_text(800.0, 600.0, &data);
        let footer = labels.last().expect("Should have at least one label");
        assert!(
            footer.text.contains("Ctrl+Shift+Y"),
            "Footer should mention approve key"
        );
        assert!(
            footer.text.contains("Ctrl+Shift+N"),
            "Footer should mention reject key"
        );
    }
}
