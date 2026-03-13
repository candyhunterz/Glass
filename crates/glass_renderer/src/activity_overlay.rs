//! ActivityOverlayRenderer: fullscreen overlay showing agent activity stream.
//!
//! Two-column layout: agent cards (left) + event timeline (right).
//! Follows the same pattern as ConflictOverlay and SearchOverlayRenderer.

use alacritty_terminal::vte::ansi::Rgb;

use crate::rect_renderer::RectInstance;

/// Filter for which event categories to show in the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActivityViewFilter {
    #[default]
    All,
    Agents,
    Locks,
    Observations,
    Messages,
}

impl ActivityViewFilter {
    /// Cycle to the next filter tab.
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Agents,
            Self::Agents => Self::Locks,
            Self::Locks => Self::Observations,
            Self::Observations => Self::Messages,
            Self::Messages => Self::All,
        }
    }

    /// Cycle to the previous filter tab.
    pub fn prev(self) -> Self {
        match self {
            Self::All => Self::Messages,
            Self::Agents => Self::All,
            Self::Locks => Self::Agents,
            Self::Observations => Self::Locks,
            Self::Messages => Self::Observations,
        }
    }

    /// The category string this filter matches, or None for All.
    pub fn category(&self) -> Option<&str> {
        match self {
            Self::All => None,
            Self::Agents => Some("agent"),
            Self::Locks => Some("lock"),
            Self::Observations => Some("observe"),
            Self::Messages => Some("message"),
        }
    }

    /// Display label for the filter tab.
    pub fn label(&self) -> &str {
        match self {
            Self::All => "All",
            Self::Agents => "Agents",
            Self::Locks => "Locks",
            Self::Observations => "Observations",
            Self::Messages => "Messages",
        }
    }
}

/// Render data passed to the activity overlay.
#[derive(Debug)]
pub struct ActivityOverlayRenderData {
    pub agents: Vec<ActivityAgentCard>,
    pub events: Vec<ActivityTimelineEvent>,
    pub pinned: Vec<ActivityPinnedAlert>,
    pub filter: ActivityViewFilter,
    pub scroll_offset: usize,
    pub verbose: bool,
}

/// Agent card data for the left column.
#[derive(Debug, Clone)]
pub struct ActivityAgentCard {
    pub name: String,
    pub agent_type: String,
    pub status: String,
    pub task: Option<String>,
    pub locked_files: Vec<String>,
    pub is_idle: bool,
}

/// A single event for the timeline.
#[derive(Debug, Clone)]
pub struct ActivityTimelineEvent {
    pub timestamp: i64,
    pub agent_name: Option<String>,
    pub category: String,
    pub event_type: String,
    pub summary: String,
    pub pinned: bool,
}

/// A pinned alert for display below agent cards.
#[derive(Debug, Clone)]
pub struct ActivityPinnedAlert {
    pub id: i64,
    pub summary: String,
    pub timestamp: i64,
}

/// Text label for rendering in the overlay.
#[derive(Debug, Clone)]
pub struct ActivityOverlayTextLabel {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub color: Rgb,
}

/// Agent color palette — each agent gets a unique color by index.
const AGENT_COLORS: &[Rgb] = &[
    Rgb {
        r: 180,
        g: 140,
        b: 255,
    }, // purple
    Rgb {
        r: 100,
        g: 180,
        b: 246,
    }, // blue
    Rgb {
        r: 80,
        g: 200,
        b: 170,
    }, // teal
    Rgb {
        r: 220,
        g: 180,
        b: 100,
    }, // amber
    Rgb {
        r: 220,
        g: 120,
        b: 120,
    }, // coral
    Rgb {
        r: 140,
        g: 220,
        b: 140,
    }, // green
];

/// Get the color for an agent by index.
pub fn agent_color(index: usize) -> Rgb {
    AGENT_COLORS[index % AGENT_COLORS.len()]
}

/// Verb color based on event type.
pub fn verb_color(event_type: &str) -> Rgb {
    match event_type {
        "registered" | "status_changed" | "started" | "analyzed" => Rgb {
            r: 106,
            g: 166,
            b: 106,
        }, // green
        "acquired" | "locked" | "proposing" => Rgb {
            r: 220,
            g: 180,
            b: 100,
        }, // amber
        "conflict" | "error_noticed" | "heartbeat_lost" => Rgb {
            r: 255,
            g: 102,
            b: 102,
        }, // red
        "sent" | "broadcast" | "message" => Rgb {
            r: 100,
            g: 200,
            b: 255,
        }, // blue
        _ => Rgb {
            r: 136,
            g: 136,
            b: 136,
        }, // gray
    }
}

/// Renders the activity overlay visual elements.
pub struct ActivityOverlayRenderer {
    cell_width: f32,
    cell_height: f32,
}

impl ActivityOverlayRenderer {
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

    /// Build all text labels for the overlay.
    ///
    /// Returns labels for: header, filter tabs, agent cards, event timeline.
    pub fn build_overlay_text(
        &self,
        data: &ActivityOverlayRenderData,
        viewport_width: f32,
        viewport_height: f32,
    ) -> Vec<ActivityOverlayTextLabel> {
        let mut labels = Vec::new();
        let padding = self.cell_width;
        let header_y = self.cell_height;

        // Header: "Activity Stream"
        labels.push(ActivityOverlayTextLabel {
            text: "Activity Stream".to_string(),
            x: padding,
            y: header_y,
            color: Rgb {
                r: 180,
                g: 140,
                b: 255,
            },
        });

        // Filter tabs
        let filters = [
            ActivityViewFilter::All,
            ActivityViewFilter::Agents,
            ActivityViewFilter::Locks,
            ActivityViewFilter::Observations,
            ActivityViewFilter::Messages,
        ];
        let mut tab_x = viewport_width * 0.4;
        for f in &filters {
            let color = if *f == data.filter {
                Rgb {
                    r: 180,
                    g: 140,
                    b: 255,
                }
            } else {
                Rgb {
                    r: 136,
                    g: 136,
                    b: 136,
                }
            };
            labels.push(ActivityOverlayTextLabel {
                text: f.label().to_string(),
                x: tab_x,
                y: header_y,
                color,
            });
            tab_x += f.label().len() as f32 * self.cell_width + self.cell_width * 2.0;
        }

        // Close hint
        labels.push(ActivityOverlayTextLabel {
            text: "Esc to close".to_string(),
            x: viewport_width - 14.0 * self.cell_width,
            y: header_y,
            color: Rgb {
                r: 85,
                g: 85,
                b: 85,
            },
        });

        // Left column: Agent cards
        let left_width = 280.0_f32.min(viewport_width * 0.35);
        let mut card_y = self.cell_height * 3.0;

        // "Active Agents (N)" header
        labels.push(ActivityOverlayTextLabel {
            text: format!("Active Agents ({})", data.agents.len()),
            x: padding,
            y: card_y,
            color: Rgb {
                r: 102,
                g: 102,
                b: 102,
            },
        });
        card_y += self.cell_height * 1.5;

        for (i, agent) in data.agents.iter().enumerate() {
            let color = agent_color(i);
            // Agent name
            labels.push(ActivityOverlayTextLabel {
                text: agent.name.clone(),
                x: padding + self.cell_width,
                y: card_y,
                color,
            });

            let status_color = match agent.status.as_str() {
                "idle" => Rgb {
                    r: 136,
                    g: 136,
                    b: 136,
                },
                _ => Rgb {
                    r: 106,
                    g: 166,
                    b: 106,
                },
            };
            labels.push(ActivityOverlayTextLabel {
                text: agent.status.clone(),
                x: left_width - agent.status.len() as f32 * self.cell_width - padding,
                y: card_y,
                color: status_color,
            });
            card_y += self.cell_height;

            // Task
            if let Some(ref task) = agent.task {
                let truncated = if task.len() > 30 {
                    format!("{}...", &task[..27])
                } else {
                    task.clone()
                };
                labels.push(ActivityOverlayTextLabel {
                    text: format!("Task: {}", truncated),
                    x: padding + self.cell_width,
                    y: card_y,
                    color: Rgb {
                        r: 170,
                        g: 170,
                        b: 170,
                    },
                });
                card_y += self.cell_height;
            }

            // Locked files
            for file in &agent.locked_files {
                let short = file.rsplit('/').next().unwrap_or(file);
                labels.push(ActivityOverlayTextLabel {
                    text: format!("locked: {}", short),
                    x: padding + self.cell_width,
                    y: card_y,
                    color: Rgb {
                        r: 220,
                        g: 180,
                        b: 100,
                    },
                });
                card_y += self.cell_height;
            }

            card_y += self.cell_height * 0.5; // gap between cards
        }

        // Right column: Event timeline
        let timeline_x = left_width + padding * 2.0;
        let mut event_y = self.cell_height * 3.0;

        // "Event Timeline" header
        labels.push(ActivityOverlayTextLabel {
            text: "Event Timeline".to_string(),
            x: timeline_x,
            y: event_y,
            color: Rgb {
                r: 102,
                g: 102,
                b: 102,
            },
        });
        event_y += self.cell_height * 1.5;

        // Filter and paginate events
        let filtered: Vec<&ActivityTimelineEvent> = data
            .events
            .iter()
            .filter(|e| {
                if !data.verbose && e.event_type == "dismissed" {
                    return false;
                }
                match data.filter.category() {
                    Some(cat) => e.category == cat,
                    None => true,
                }
            })
            .collect();

        let max_visible = ((viewport_height - event_y) / self.cell_height) as usize;
        let visible = filtered.iter().skip(data.scroll_offset).take(max_visible);

        let mut last_minute: Option<i64> = None;

        for event in visible {
            // Minute group header
            let minute = event.timestamp / 60;
            if last_minute.map_or(true, |m| m != minute) {
                let time_str = format_timestamp_minute(event.timestamp);
                labels.push(ActivityOverlayTextLabel {
                    text: time_str,
                    x: timeline_x,
                    y: event_y,
                    color: Rgb {
                        r: 68,
                        g: 68,
                        b: 68,
                    },
                });
                event_y += self.cell_height;
                last_minute = Some(minute);
            }

            // Seconds
            let secs = format!(":{:02}", event.timestamp % 60);
            labels.push(ActivityOverlayTextLabel {
                text: secs,
                x: timeline_x,
                y: event_y,
                color: Rgb {
                    r: 68,
                    g: 68,
                    b: 68,
                },
            });

            // Agent name badge
            let badge_x = timeline_x + self.cell_width * 5.0;
            if let Some(ref name) = event.agent_name {
                let color_index =
                    name.bytes()
                        .fold(0usize, |acc, b| acc.wrapping_add(b as usize));
                labels.push(ActivityOverlayTextLabel {
                    text: name.clone(),
                    x: badge_x,
                    y: event_y,
                    color: agent_color(color_index),
                });
            }

            // Verb
            let verb_x = badge_x + self.cell_width * 14.0;
            labels.push(ActivityOverlayTextLabel {
                text: event.event_type.clone(),
                x: verb_x,
                y: event_y,
                color: verb_color(&event.event_type),
            });

            // Detail (summary)
            let detail_x = verb_x + self.cell_width * 12.0;
            labels.push(ActivityOverlayTextLabel {
                text: event.summary.clone(),
                x: detail_x,
                y: event_y,
                color: Rgb {
                    r: 204,
                    g: 204,
                    b: 204,
                },
            });

            event_y += self.cell_height;
        }

        labels
    }
}

/// Format a unix timestamp's minute portion as "HH:MM".
fn format_timestamp_minute(timestamp: i64) -> String {
    let secs = timestamp % 86400;
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    format!("{:02}:{:02}", hours, minutes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_activity_view_filter_cycle() {
        let f = ActivityViewFilter::All;
        assert_eq!(f.next(), ActivityViewFilter::Agents);
        assert_eq!(f.next().next(), ActivityViewFilter::Locks);
        assert_eq!(ActivityViewFilter::Messages.next(), ActivityViewFilter::All);
    }

    #[test]
    fn test_activity_view_filter_prev() {
        assert_eq!(ActivityViewFilter::All.prev(), ActivityViewFilter::Messages);
        assert_eq!(ActivityViewFilter::Agents.prev(), ActivityViewFilter::All);
    }

    #[test]
    fn test_activity_view_filter_category() {
        assert_eq!(ActivityViewFilter::All.category(), None);
        assert_eq!(ActivityViewFilter::Agents.category(), Some("agent"));
        assert_eq!(ActivityViewFilter::Locks.category(), Some("lock"));
    }

    #[test]
    fn test_agent_color_wraps() {
        let c1 = agent_color(0);
        let c2 = agent_color(AGENT_COLORS.len());
        assert_eq!(c1, c2);
    }

    #[test]
    fn test_verb_color_categories() {
        let green = verb_color("registered");
        assert_eq!(green.g, 166);
        let red = verb_color("conflict");
        assert_eq!(red.r, 255);
        let gray = verb_color("unknown_type");
        assert_eq!(gray.r, 136);
    }

    #[test]
    fn test_backdrop_rect_covers_viewport() {
        let r = ActivityOverlayRenderer::new(10.0, 20.0);
        let rect = r.build_backdrop_rect(800.0, 600.0);
        assert_eq!(rect.pos[0], 0.0);
        assert_eq!(rect.pos[1], 0.0);
        assert_eq!(rect.pos[2], 800.0);
        assert_eq!(rect.pos[3], 600.0);
    }

    #[test]
    fn test_format_timestamp_minute() {
        // 10:34 = 10*3600 + 34*60 = 38040
        assert_eq!(format_timestamp_minute(38040), "10:34");
        assert_eq!(format_timestamp_minute(38042), "10:34");
    }
}
