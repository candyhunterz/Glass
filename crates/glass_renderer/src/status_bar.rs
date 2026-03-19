//! StatusBarRenderer: bottom-pinned status bar with CWD and git info.
//!
//! Produces a background rectangle and text labels for the status bar
//! that sits at the bottom of the terminal viewport.

use alacritty_terminal::vte::ansi::Rgb;

use glass_terminal::GitInfo;

use crate::rect_renderer::RectInstance;

/// Text content for the status bar.
#[derive(Debug, Clone)]
pub struct StatusLabel {
    /// Left-aligned text (CWD path)
    pub left_text: String,
    /// Right-aligned text (git branch + dirty count)
    pub right_text: Option<String>,
    /// Center-aligned text (update notification)
    pub center_text: Option<String>,
    /// Coordination status text (agent/lock counts)
    pub coordination_text: Option<String>,
    /// Agent cost text (e.g. "$0.0012" or "PAUSED")
    pub agent_cost_text: Option<String>,
    /// Agent mode text (e.g. "[agent: watch]")
    pub agent_mode_text: Option<String>,
    /// Proposal count text (e.g. "2 proposals")
    pub proposal_count_text: Option<String>,
    /// Y position in pixels
    pub y: f32,
    /// Color for left text (CWD)
    pub left_color: Rgb,
    /// Color for right text (git info)
    pub right_color: Rgb,
    /// Color for center text (update notification)
    pub center_color: Rgb,
    /// Color for coordination text (soft purple)
    pub coordination_color: Rgb,
    /// Color for agent cost text (green active, red paused)
    pub agent_cost_color: Rgb,
    /// Color for agent mode text (soft cyan)
    pub agent_mode_color: Rgb,
    /// Color for proposal count text (soft yellow)
    pub proposal_count_color: Rgb,
}

/// Build the agent activity line text for the two-line status bar.
///
/// Format: "name status task  |  name status  |  N locks      Ctrl+Shift+G"
/// If more than 2 agents, shows first 2 + "+N more".
pub fn build_agent_activity_line(
    agents: &[(String, String, Option<String>)],
    lock_count: usize,
    _max_chars: usize,
) -> String {
    let mut parts = Vec::new();
    let show_count = agents.len().min(2);

    for (name, status, task) in agents.iter().take(show_count) {
        let entry = if let Some(t) = task {
            let truncated = if t.len() > 20 {
                format!("{}...", &t[..17])
            } else {
                t.clone()
            };
            format!("{} {} {}", name, status, truncated)
        } else {
            format!("{} {}", name, status)
        };
        parts.push(entry);
    }

    if agents.len() > 2 {
        parts.push(format!("+{} more", agents.len() - 2));
    }

    let mut line = parts.join("  |  ");

    if lock_count > 0 {
        line.push_str(&format!("  |  {} locks", lock_count));
    }

    line
}

/// Renders the bottom-pinned status bar.
///
/// Produces a background rectangle and text labels showing the current
/// working directory and optional git branch/dirty information.
pub struct StatusBarRenderer {
    cell_width: f32,
    cell_height: f32,
}

impl StatusBarRenderer {
    /// Create a new StatusBarRenderer with the given cell dimensions.
    pub fn new(cell_width: f32, cell_height: f32) -> Self {
        Self {
            cell_width,
            cell_height,
        }
    }

    /// Build the status bar background rectangle.
    ///
    /// Returns a single full-width rect at the bottom of the viewport,
    /// 1 cell_height tall, slightly lighter than terminal background.
    /// When `orchestrating` is true, adds a colored accent line at the top.
    pub fn build_status_rects(
        &self,
        viewport_width: f32,
        viewport_height: f32,
        orchestrating: bool,
    ) -> Vec<RectInstance> {
        let y = viewport_height - self.cell_height;
        let mut rects = vec![RectInstance {
            pos: [0.0, y, viewport_width, self.cell_height],
            color: if orchestrating {
                // Dark teal tint when orchestrating
                [15.0 / 255.0, 45.0 / 255.0, 40.0 / 255.0, 1.0]
            } else {
                [38.0 / 255.0, 38.0 / 255.0, 38.0 / 255.0, 1.0]
            },
        }];
        if orchestrating {
            // 2px accent line at top of status bar
            rects.push(RectInstance {
                pos: [0.0, y, viewport_width, 2.0],
                color: [0.0, 200.0 / 255.0, 120.0 / 255.0, 1.0], // green accent
            });
        }
        rects
    }

    /// Build status bar background rectangles for two-line mode.
    ///
    /// Returns a rect that is 2 * cell_height tall when agents are active.
    /// When `orchestrating` is true, adds a colored accent line at the top.
    pub fn build_status_rects_two_line(
        &self,
        viewport_width: f32,
        viewport_height: f32,
        orchestrating: bool,
    ) -> Vec<RectInstance> {
        let height = self.cell_height * 2.0;
        let y = viewport_height - height;
        let mut rects = vec![RectInstance {
            pos: [0.0, y, viewport_width, height],
            color: if orchestrating {
                [15.0 / 255.0, 45.0 / 255.0, 40.0 / 255.0, 1.0]
            } else {
                [38.0 / 255.0, 38.0 / 255.0, 38.0 / 255.0, 1.0]
            },
        }];
        if orchestrating {
            rects.push(RectInstance {
                pos: [0.0, y, viewport_width, 2.0],
                color: [0.0, 200.0 / 255.0, 120.0 / 255.0, 1.0],
            });
        }
        rects
    }

    /// Get the status bar height in pixels (1 or 2 lines).
    pub fn height(&self, two_line: bool) -> f32 {
        if two_line {
            self.cell_height * 2.0
        } else {
            self.cell_height
        }
    }

    /// Build text content for the status bar.
    ///
    /// Left side: CWD path (truncated if needed).
    /// Center: update notification (if available).
    /// Right side: git branch name + dirty count if available.
    /// Agent cost text: shown when agent is active (green) or paused (red).
    /// Agent mode text: shows current agent mode (soft cyan).
    /// Proposal count text: shows pending proposals (soft yellow).
    #[allow(clippy::too_many_arguments)]
    pub fn build_status_text(
        &self,
        cwd: &str,
        git_info: Option<&GitInfo>,
        update_text: Option<&str>,
        coordination_text: Option<&str>,
        agent_cost_text: Option<&str>,
        agent_paused: bool,
        agent_mode_text: Option<&str>,
        proposal_count_text: Option<&str>,
        viewport_width: f32,
        viewport_height: f32,
    ) -> StatusLabel {
        let y = viewport_height - self.cell_height;

        // Dynamic CWD truncation: use roughly half the viewport width for CWD,
        // leaving room for right-side elements (git branch, agent cost, etc.).
        let max_cwd_chars = ((viewport_width / self.cell_width) as usize / 2).max(20);
        // Safe truncation for Unicode paths
        let left_text = if cwd.chars().count() > max_cwd_chars {
            let skip = cwd.chars().count() - (max_cwd_chars - 3);
            format!("...{}", cwd.chars().skip(skip).collect::<String>())
        } else {
            cwd.to_string()
        };

        let right_text = git_info.map(|info| {
            if info.dirty_count > 0 {
                format!("{} +{}", info.branch, info.dirty_count)
            } else {
                info.branch.clone()
            }
        });

        let center_text = update_text.map(|t| t.to_string());
        let coordination_text = coordination_text.map(|t| t.to_string());
        let agent_cost_text = agent_cost_text.map(|t| t.to_string());
        let agent_mode_text = agent_mode_text.map(|t| t.to_string());
        let proposal_count_text = proposal_count_text.map(|t| t.to_string());

        // Git branch color: cyan if clean, with yellow dirty count appended
        // For simplicity, use cyan as the base right_color
        let right_color = Rgb {
            r: 80,
            g: 200,
            b: 200,
        };

        // Bright yellow-gold for update notification visibility
        let center_color = Rgb {
            r: 255,
            g: 200,
            b: 50,
        };

        // Soft purple for coordination info
        let coordination_color = Rgb {
            r: 180,
            g: 140,
            b: 255,
        };

        // Green when active, red when paused (AGTR-07)
        let agent_cost_color = if agent_paused {
            Rgb {
                r: 255,
                g: 80,
                b: 80,
            }
        } else {
            Rgb {
                r: 80,
                g: 220,
                b: 120,
            }
        };

        // Soft cyan for agent mode indicator
        let agent_mode_color = Rgb {
            r: 100,
            g: 180,
            b: 200,
        };

        // Soft yellow for proposal count
        let proposal_count_color = Rgb {
            r: 220,
            g: 200,
            b: 100,
        };

        StatusLabel {
            left_text,
            right_text,
            center_text,
            coordination_text,
            agent_cost_text,
            agent_mode_text,
            proposal_count_text,
            y,
            left_color: Rgb {
                r: 204,
                g: 204,
                b: 204,
            },
            right_color,
            center_color,
            coordination_color,
            agent_cost_color,
            agent_mode_color,
            proposal_count_color,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer() -> StatusBarRenderer {
        StatusBarRenderer::new(10.0, 20.0)
    }

    fn make_label(renderer: &StatusBarRenderer) -> StatusLabel {
        renderer.build_status_text(
            "/home/user/project",
            None,
            None,
            None,
            None,
            false,
            None,
            None,
            800.0,
            600.0,
        )
    }

    #[test]
    fn test_status_bar_cwd_left_text() {
        let r = renderer();
        let label = make_label(&r);
        assert_eq!(label.left_text, "/home/user/project");
    }

    #[test]
    fn test_status_bar_cwd_truncation() {
        let r = renderer();
        let long_cwd = "/".repeat(80);
        // With cell_width=10.0 and viewport_width=800.0, max_cwd_chars = (800/10)/2 = 40
        let label = r.build_status_text(
            &long_cwd, None, None, None, None, false, None, None, 800.0, 600.0,
        );
        assert!(
            label.left_text.chars().count() <= 40,
            "Truncated CWD should be at most max_cwd_chars (40), got {}",
            label.left_text.chars().count()
        );
        assert!(label.left_text.starts_with("..."));
    }

    #[test]
    fn test_status_bar_y_position() {
        let r = renderer();
        let label = make_label(&r);
        // y = viewport_height - cell_height = 600 - 20 = 580
        assert_eq!(label.y, 580.0);
    }

    #[test]
    fn test_status_bar_none_fields_when_no_optional_data() {
        let r = renderer();
        let label = make_label(&r);
        assert!(label.right_text.is_none());
        assert!(label.center_text.is_none());
        assert!(label.coordination_text.is_none());
        assert!(label.agent_cost_text.is_none());
        assert!(label.agent_mode_text.is_none());
        assert!(label.proposal_count_text.is_none());
    }

    #[test]
    fn test_status_bar_agent_mode_and_proposals() {
        let r = renderer();
        let label = r.build_status_text(
            "/home/user",
            None,
            None,
            None,
            None,
            false,
            Some("[agent: watch]"),
            Some("2 proposals"),
            800.0,
            600.0,
        );
        assert_eq!(
            label.agent_mode_text.as_deref(),
            Some("[agent: watch]"),
            "agent_mode_text should be set"
        );
        assert_eq!(
            label.proposal_count_text.as_deref(),
            Some("2 proposals"),
            "proposal_count_text should be set"
        );
        // Verify colors
        assert_eq!(
            label.agent_mode_color,
            Rgb {
                r: 100,
                g: 180,
                b: 200
            }
        );
        assert_eq!(
            label.proposal_count_color,
            Rgb {
                r: 220,
                g: 200,
                b: 100
            }
        );
    }

    #[test]
    fn test_status_bar_agent_paused_color() {
        let r = renderer();
        let label = r.build_status_text(
            "/home/user",
            None,
            None,
            None,
            Some("PAUSED $1.00"),
            true,
            None,
            None,
            800.0,
            600.0,
        );
        assert_eq!(
            label.agent_cost_color,
            Rgb {
                r: 255,
                g: 80,
                b: 80
            }
        );
    }

    #[test]
    fn test_agent_activity_line_two_agents() {
        let agents = vec![
            (
                "claude-code".to_string(),
                "editing".to_string(),
                Some("main.rs".to_string()),
            ),
            ("cursor".to_string(), "idle".to_string(), None),
        ];
        let line = build_agent_activity_line(&agents, 2, 100);
        assert!(line.contains("claude-code"));
        assert!(line.contains("editing"));
        assert!(line.contains("cursor"));
        assert!(line.contains("idle"));
    }

    #[test]
    fn test_agent_activity_line_overflow() {
        let agents: Vec<_> = (0..5)
            .map(|i| (format!("agent-{}", i), "active".to_string(), None))
            .collect();
        let line = build_agent_activity_line(&agents, 0, 80);
        assert!(line.contains("+3 more"));
    }

    #[test]
    fn test_status_bar_agent_active_color() {
        let r = renderer();
        let label = r.build_status_text(
            "/home/user",
            None,
            None,
            None,
            Some("agent: $0.0012"),
            false,
            None,
            None,
            800.0,
            600.0,
        );
        assert_eq!(
            label.agent_cost_color,
            Rgb {
                r: 80,
                g: 220,
                b: 120
            }
        );
    }
}
