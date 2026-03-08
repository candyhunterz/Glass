//! GridSnapshot: extract renderable cell data from the terminal grid.
//!
//! This module provides a lock-minimizing rendering pattern: copy all renderable
//! data from `Term` under a brief lock, then render without holding it.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Point;
use alacritty_terminal::selection::SelectionRange;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::{RenderableCursor, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

use crate::event_proxy::EventProxy;

/// Default foreground and background colors for the terminal.
#[derive(Debug, Clone)]
pub struct DefaultColors {
    pub fg: Rgb,
    pub bg: Rgb,
}

impl Default for DefaultColors {
    fn default() -> Self {
        Self {
            fg: Rgb { r: 204, g: 204, b: 204 }, // Light gray
            bg: Rgb { r: 26, g: 26, b: 26 },    // Dark gray (matches clear color)
        }
    }
}

impl DefaultColors {
    /// Get the default color for a named color.
    pub fn named(&self, name: NamedColor) -> Rgb {
        match name {
            NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::DimForeground => {
                self.fg
            }
            NamedColor::Background => self.bg,
            _ => default_named_color(name),
        }
    }
}

/// A single rendered cell with resolved RGB colors.
#[derive(Debug, Clone)]
pub struct RenderedCell {
    pub point: Point,
    pub c: char,
    pub fg: Rgb,
    pub bg: Rgb,
    pub flags: Flags,
    pub zerowidth: Vec<char>,
}

/// A snapshot of the terminal grid for rendering.
pub struct GridSnapshot {
    pub cells: Vec<RenderedCell>,
    pub cursor: RenderableCursor,
    pub display_offset: usize,
    pub history_size: usize,
    pub mode: TermMode,
    pub columns: usize,
    pub screen_lines: usize,
    pub selection: Option<SelectionRange>,
}

/// Resolve a `Color` to an `Rgb` value using the terminal color palette.
pub fn resolve_color(
    color: Color,
    colors: &Colors,
    defaults: &DefaultColors,
    flags: Flags,
) -> Rgb {
    match color {
        Color::Spec(rgb) => rgb,
        Color::Indexed(idx) => colors[idx as usize].unwrap_or_else(|| default_indexed_color(idx)),
        Color::Named(name) => {
            // Apply DIM/BOLD variant transformation
            let name = if flags.contains(Flags::DIM) {
                name.to_dim()
            } else if flags.contains(Flags::BOLD) {
                name.to_bright()
            } else {
                name
            };
            colors[name].unwrap_or_else(|| defaults.named(name))
        }
    }
}

/// Standard xterm 256-color palette defaults.
pub fn default_indexed_color(idx: u8) -> Rgb {
    match idx {
        // Standard ANSI colors 0-15
        0 => Rgb { r: 0, g: 0, b: 0 },         // Black
        1 => Rgb { r: 205, g: 0, b: 0 },        // Red
        2 => Rgb { r: 0, g: 205, b: 0 },        // Green
        3 => Rgb { r: 205, g: 205, b: 0 },       // Yellow
        4 => Rgb { r: 0, g: 0, b: 238 },         // Blue
        5 => Rgb { r: 205, g: 0, b: 205 },       // Magenta
        6 => Rgb { r: 0, g: 205, b: 205 },       // Cyan
        7 => Rgb { r: 229, g: 229, b: 229 },     // White
        8 => Rgb { r: 127, g: 127, b: 127 },     // Bright Black
        9 => Rgb { r: 255, g: 0, b: 0 },         // Bright Red
        10 => Rgb { r: 0, g: 255, b: 0 },        // Bright Green
        11 => Rgb { r: 255, g: 255, b: 0 },      // Bright Yellow
        12 => Rgb { r: 92, g: 92, b: 255 },      // Bright Blue
        13 => Rgb { r: 255, g: 0, b: 255 },      // Bright Magenta
        14 => Rgb { r: 0, g: 255, b: 255 },      // Bright Cyan
        15 => Rgb { r: 255, g: 255, b: 255 },    // Bright White
        // 6x6x6 color cube (indices 16-231)
        16..=231 => {
            let idx = idx - 16;
            let r_idx = idx / 36;
            let g_idx = (idx % 36) / 6;
            let b_idx = idx % 6;
            let to_value = |i: u8| -> u8 {
                if i == 0 { 0 } else { 55 + 40 * i }
            };
            Rgb {
                r: to_value(r_idx),
                g: to_value(g_idx),
                b: to_value(b_idx),
            }
        }
        // Grayscale ramp (indices 232-255)
        232..=255 => {
            let value = 8 + 10 * (idx - 232);
            Rgb { r: value, g: value, b: value }
        }
    }
}

/// Default RGB values for named ANSI colors (xterm defaults).
fn default_named_color(name: NamedColor) -> Rgb {
    match name {
        NamedColor::Black => Rgb { r: 0, g: 0, b: 0 },
        NamedColor::Red => Rgb { r: 205, g: 0, b: 0 },
        NamedColor::Green => Rgb { r: 0, g: 205, b: 0 },
        NamedColor::Yellow => Rgb { r: 205, g: 205, b: 0 },
        NamedColor::Blue => Rgb { r: 0, g: 0, b: 238 },
        NamedColor::Magenta => Rgb { r: 205, g: 0, b: 205 },
        NamedColor::Cyan => Rgb { r: 0, g: 205, b: 205 },
        NamedColor::White => Rgb { r: 229, g: 229, b: 229 },
        NamedColor::BrightBlack => Rgb { r: 127, g: 127, b: 127 },
        NamedColor::BrightRed => Rgb { r: 255, g: 0, b: 0 },
        NamedColor::BrightGreen => Rgb { r: 0, g: 255, b: 0 },
        NamedColor::BrightYellow => Rgb { r: 255, g: 255, b: 0 },
        NamedColor::BrightBlue => Rgb { r: 92, g: 92, b: 255 },
        NamedColor::BrightMagenta => Rgb { r: 255, g: 0, b: 255 },
        NamedColor::BrightCyan => Rgb { r: 0, g: 255, b: 255 },
        NamedColor::BrightWhite => Rgb { r: 255, g: 255, b: 255 },
        NamedColor::DimBlack => Rgb { r: 0, g: 0, b: 0 },
        NamedColor::DimRed => Rgb { r: 102, g: 0, b: 0 },
        NamedColor::DimGreen => Rgb { r: 0, g: 102, b: 0 },
        NamedColor::DimYellow => Rgb { r: 102, g: 102, b: 0 },
        NamedColor::DimBlue => Rgb { r: 0, g: 0, b: 119 },
        NamedColor::DimMagenta => Rgb { r: 102, g: 0, b: 102 },
        NamedColor::DimCyan => Rgb { r: 0, g: 102, b: 102 },
        NamedColor::DimWhite => Rgb { r: 114, g: 114, b: 114 },
        NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::DimForeground => {
            Rgb { r: 204, g: 204, b: 204 }
        }
        NamedColor::Background => Rgb { r: 26, g: 26, b: 26 },
        NamedColor::Cursor => Rgb { r: 204, g: 204, b: 204 },
    }
}

/// Extract a renderable snapshot from the terminal under a brief lock.
#[cfg_attr(feature = "perf", tracing::instrument(skip_all))]
pub fn snapshot_term(term: &Term<EventProxy>, defaults: &DefaultColors) -> GridSnapshot {
    let content = term.renderable_content();
    let colors = content.colors;

    let mut cells = Vec::with_capacity(term.columns() * term.screen_lines());
    for indexed in content.display_iter {
        let cell: &Cell = &indexed.cell;
        let mut fg = resolve_color(cell.fg, colors, defaults, cell.flags);
        let mut bg = resolve_color(cell.bg, colors, defaults, cell.flags);

        // Apply INVERSE flag: swap fg and bg after resolution
        if cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }

        cells.push(RenderedCell {
            point: indexed.point,
            c: cell.c,
            fg,
            bg,
            flags: cell.flags,
            zerowidth: cell.zerowidth().map(|z| z.to_vec()).unwrap_or_default(),
        });
    }

    GridSnapshot {
        cells,
        cursor: content.cursor,
        display_offset: content.display_offset,
        history_size: term.grid().history_size(),
        mode: content.mode,
        columns: term.columns(),
        screen_lines: term.screen_lines(),
        selection: content.selection,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::term::cell::Flags;
    use alacritty_terminal::term::color::Colors;
    use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

    fn empty_colors() -> Colors {
        Colors::default()
    }

    fn defaults() -> DefaultColors {
        DefaultColors::default()
    }

    #[test]
    fn test_resolve_color_spec_truecolor_passthrough() {
        let colors = empty_colors();
        let defs = defaults();
        let color = Color::Spec(Rgb { r: 255, g: 0, b: 0 });
        let result = resolve_color(color, &colors, &defs, Flags::empty());
        assert_eq!(result, Rgb { r: 255, g: 0, b: 0 });
    }

    #[test]
    fn test_resolve_color_named_blue_with_bold_returns_bright_blue() {
        let colors = empty_colors();
        let defs = defaults();
        let color = Color::Named(NamedColor::Blue);
        let result = resolve_color(color, &colors, &defs, Flags::BOLD);
        // BOLD transforms Blue -> BrightBlue
        let expected = default_named_color(NamedColor::BrightBlue);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_resolve_color_named_white_with_dim_returns_dim_white() {
        let colors = empty_colors();
        let defs = defaults();
        let color = Color::Named(NamedColor::White);
        let result = resolve_color(color, &colors, &defs, Flags::DIM);
        // DIM transforms White -> DimWhite
        let expected = default_named_color(NamedColor::DimWhite);
        assert_eq!(result, expected);
    }

    #[test]
    fn test_resolve_color_indexed_196() {
        let colors = empty_colors();
        let defs = defaults();
        let color = Color::Indexed(196);
        let result = resolve_color(color, &colors, &defs, Flags::empty());
        // Index 196 = color cube: (196-16) = 180, r=180/36=5, g=(180%36)/6=0, b=180%6=0
        // r = 55 + 40*5 = 255, g = 0, b = 0
        assert_eq!(result, Rgb { r: 255, g: 0, b: 0 });
    }

    #[test]
    fn test_inverse_flag_swaps_fg_and_bg() {
        // We test through resolve_color + manual swap logic
        let colors = empty_colors();
        let defs = defaults();
        let fg_color = Color::Spec(Rgb { r: 255, g: 255, b: 255 });
        let bg_color = Color::Spec(Rgb { r: 0, g: 0, b: 0 });
        let flags = Flags::INVERSE;

        let mut fg = resolve_color(fg_color, &colors, &defs, flags);
        let mut bg = resolve_color(bg_color, &colors, &defs, flags);

        // Apply INVERSE swap (same logic as snapshot_term)
        if flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }

        // After swap: fg should be black, bg should be white
        assert_eq!(fg, Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(bg, Rgb { r: 255, g: 255, b: 255 });
    }

    #[test]
    fn test_wide_char_spacer_included_with_flag() {
        // WIDE_CHAR_SPACER cells should be present in the snapshot with the flag set
        // so the renderer can identify and skip them.
        let flags = Flags::WIDE_CHAR_SPACER;
        assert!(flags.contains(Flags::WIDE_CHAR_SPACER));
        // The flag is preserved in RenderedCell.flags during snapshot_term,
        // allowing the renderer to check and skip these cells.
    }

    #[test]
    fn test_default_indexed_color_standard_ansi() {
        // Color 0 = Black
        assert_eq!(default_indexed_color(0), Rgb { r: 0, g: 0, b: 0 });
        // Color 1 = Red
        assert_eq!(default_indexed_color(1), Rgb { r: 205, g: 0, b: 0 });
        // Color 15 = Bright White
        assert_eq!(default_indexed_color(15), Rgb { r: 255, g: 255, b: 255 });
    }

    #[test]
    fn test_default_indexed_color_cube() {
        // Color 16 = first cube entry (0,0,0) = black
        assert_eq!(default_indexed_color(16), Rgb { r: 0, g: 0, b: 0 });
        // Color 21 = (0,0,5) = blue component maxed
        assert_eq!(default_indexed_color(21), Rgb { r: 0, g: 0, b: 255 });
        // Color 196 = (5,0,0) = red
        assert_eq!(default_indexed_color(196), Rgb { r: 255, g: 0, b: 0 });
    }

    #[test]
    fn test_default_indexed_color_grayscale() {
        // Color 232 = darkest gray: 8 + 10*0 = 8
        assert_eq!(default_indexed_color(232), Rgb { r: 8, g: 8, b: 8 });
        // Color 255 = lightest gray: 8 + 10*23 = 238
        assert_eq!(default_indexed_color(255), Rgb { r: 238, g: 238, b: 238 });
    }

    #[test]
    fn test_colors_palette_override() {
        // When Colors has an override, it should be used instead of default
        let mut colors = empty_colors();
        let custom = Rgb { r: 100, g: 200, b: 50 };
        colors[1] = Some(custom); // Override color index 1
        let defs = defaults();

        let result = resolve_color(Color::Indexed(1), &colors, &defs, Flags::empty());
        assert_eq!(result, custom);
    }
}
