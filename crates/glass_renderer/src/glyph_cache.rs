//! GlyphCache: wraps all glyphon state needed for GPU text rendering.
//!
//! Initializes FontSystem (system font discovery), TextAtlas (glyph texture),
//! TextRenderer (GPU draw pipeline), SwashCache (glyph rasterization),
//! Cache (shared GPU pipelines), and Viewport (resolution-aware rendering).

use glyphon::{Cache, FontSystem, SwashCache, TextAtlas, TextRenderer, Viewport};
use wgpu::MultisampleState;

/// All glyphon state needed for text rendering, bundled for convenience.
///
/// Created once per window/renderer, reused across frames. The `FontSystem`
/// discovers system fonts automatically. The `TextAtlas` manages a GPU texture
/// atlas for rasterized glyphs. The `TextRenderer` provides the wgpu render
/// pipeline for drawing text.
pub struct GlyphCache {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub cache: Cache,
    pub atlas: TextAtlas,
    pub text_renderer: TextRenderer,
    pub viewport: Viewport,
}

impl GlyphCache {
    /// Initialize all glyphon state for text rendering.
    ///
    /// - `device`: wgpu device for GPU resource creation
    /// - `queue`: wgpu queue for texture uploads
    /// - `surface_format`: texture format of the render target (e.g., Bgra8UnormSrgb)
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        Self::with_font_system(FontSystem::new(), device, queue, surface_format)
    }

    /// Initialize with a pre-created FontSystem (for parallel init).
    pub fn with_font_system(
        font_system: FontSystem,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        // Glyph rasterization cache (swash is the rasterizer used by cosmic-text)
        let swash_cache = SwashCache::new();

        // Shared GPU pipelines and shaders for text rendering
        let cache = Cache::new(device);

        // GPU texture atlas for rasterized glyphs
        let atlas = TextAtlas::new(device, queue, &cache, surface_format);

        // Text draw pipeline
        let mut atlas_for_renderer = atlas;
        let text_renderer = TextRenderer::new(
            &mut atlas_for_renderer,
            device,
            MultisampleState::default(),
            None,
        );

        // Resolution-aware viewport
        let viewport = Viewport::new(device, &cache);

        Self {
            font_system,
            swash_cache,
            cache,
            atlas: atlas_for_renderer,
            text_renderer,
            viewport,
        }
    }

    /// Free unused atlas space between frames.
    ///
    /// Call this after presenting each frame to reclaim GPU memory
    /// from glyphs that are no longer visible.
    pub fn trim(&mut self) {
        self.atlas.trim();
    }
}
