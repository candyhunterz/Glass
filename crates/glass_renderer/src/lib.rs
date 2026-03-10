//! glass_renderer — wgpu GPU surface and rendering

pub mod block_renderer;
pub mod config_error_overlay;
pub mod conflict_overlay;
pub mod frame;
pub mod glyph_cache;
pub mod grid_renderer;
pub mod rect_renderer;
pub mod search_overlay_renderer;
pub mod status_bar;
pub mod surface;
pub mod tab_bar;

pub use block_renderer::{BlockLabel, BlockRenderer};
pub use config_error_overlay::{ConfigErrorOverlay, ConfigErrorTextLabel};
pub use conflict_overlay::{ConflictOverlay, ConflictTextLabel};
pub use frame::{DividerRect, FrameRenderer, PaneViewport};
pub use glyph_cache::GlyphCache;
pub use grid_renderer::GridRenderer;
pub use rect_renderer::{RectInstance, RectRenderer};
pub use search_overlay_renderer::{SearchOverlayRenderer, SearchOverlayTextLabel};
pub use status_bar::{StatusBarRenderer, StatusLabel};
pub use surface::GlassRenderer;
pub use tab_bar::{TabBarRenderer, TabDisplayInfo, TabLabel};

/// Re-export FontSystem for parallel init in main.
pub use glyphon::FontSystem;
