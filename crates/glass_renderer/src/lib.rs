//! glass_renderer — wgpu GPU surface and rendering

pub mod block_renderer;
pub mod frame;
pub mod glyph_cache;
pub mod grid_renderer;
pub mod rect_renderer;
pub mod status_bar;
pub mod surface;

pub use block_renderer::{BlockLabel, BlockRenderer};
pub use frame::FrameRenderer;
pub use glyph_cache::GlyphCache;
pub use grid_renderer::GridRenderer;
pub use rect_renderer::{RectInstance, RectRenderer};
pub use status_bar::{StatusBarRenderer, StatusLabel};
pub use surface::GlassRenderer;

/// Re-export FontSystem for parallel init in main.
pub use glyphon::FontSystem;
