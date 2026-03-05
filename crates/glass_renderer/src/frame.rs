//! FrameRenderer: orchestrates the full rendering pipeline.
//!
//! Composites clear -> rect backgrounds -> text -> present for each frame.
//! Owns the GlyphCache, GridRenderer, and RectRenderer.

use alacritty_terminal::vte::ansi::Rgb;
use glyphon::{Buffer, Resolution};

use glass_terminal::GridSnapshot;

use crate::glyph_cache::GlyphCache;
use crate::grid_renderer::GridRenderer;
use crate::rect_renderer::RectRenderer;

/// Orchestrates the complete GPU rendering pipeline for terminal content.
///
/// Each frame: clear to background color, draw cell background rects,
/// draw text via glyphon, present. Owns all rendering sub-systems.
pub struct FrameRenderer {
    pub glyph_cache: GlyphCache,
    grid_renderer: GridRenderer,
    rect_renderer: RectRenderer,
    default_bg: Rgb,
    /// Reusable buffer storage to avoid per-frame allocation
    text_buffers: Vec<Buffer>,
}

impl FrameRenderer {
    /// Create the full rendering pipeline.
    ///
    /// Initializes GlyphCache (glyphon state), GridRenderer (cell metrics),
    /// and RectRenderer (instanced quad pipeline).
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        font_family: &str,
        font_size: f32,
        scale_factor: f32,
    ) -> Self {
        let mut glyph_cache = GlyphCache::new(device, queue, surface_format);
        let grid_renderer = GridRenderer::new(
            &mut glyph_cache.font_system,
            font_family,
            font_size,
            scale_factor,
        );
        let rect_renderer = RectRenderer::new(device, surface_format);
        let default_bg = Rgb { r: 26, g: 26, b: 26 };

        Self {
            glyph_cache,
            grid_renderer,
            rect_renderer,
            default_bg,
            text_buffers: Vec::new(),
        }
    }

    /// Returns (cell_width, cell_height) in physical pixels.
    pub fn cell_size(&self) -> (f32, f32) {
        self.grid_renderer.cell_size()
    }

    /// Draw a complete frame with terminal content.
    ///
    /// Pipeline: clear -> rect backgrounds/cursor -> text -> end pass.
    /// The caller is responsible for presenting the frame texture.
    pub fn draw_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        snapshot: &GridSnapshot,
    ) {
        // 1. Build rect instances (backgrounds + cursor)
        let rect_instances = self.grid_renderer.build_rects(snapshot, self.default_bg);
        let rect_count = rect_instances.len() as u32;

        // 2. Prepare rect renderer
        self.rect_renderer.prepare(device, queue, &rect_instances, width, height);

        // 3. Build text buffers and text areas
        self.grid_renderer.build_text_buffers(
            &mut self.glyph_cache.font_system,
            snapshot,
            &mut self.text_buffers,
        );
        let text_areas = self.grid_renderer.build_text_areas(
            &self.text_buffers,
            width,
            height,
        );

        // 4. Update viewport resolution
        self.glyph_cache.viewport.update(queue, Resolution { width, height });

        // 5. Prepare text renderer
        if let Err(e) = self.glyph_cache.text_renderer.prepare(
            device,
            queue,
            &mut self.glyph_cache.font_system,
            &mut self.glyph_cache.atlas,
            &self.glyph_cache.viewport,
            text_areas,
            &mut self.glyph_cache.swash_cache,
        ) {
            tracing::warn!("Text prepare error: {:?}", e);
        }

        // 6. Begin render pass with clear color
        let bg_r = self.default_bg.r as f64 / 255.0;
        let bg_g = self.default_bg.g as f64 / 255.0;
        let bg_b = self.default_bg.b as f64 / 255.0;

        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("terminal_frame_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: bg_r,
                            g: bg_g,
                            b: bg_b,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            // 7. Draw rect backgrounds first
            self.rect_renderer.render(&mut pass, rect_count);

            // 8. Draw text on top
            if let Err(e) = self.glyph_cache.text_renderer.render(
                &self.glyph_cache.atlas,
                &self.glyph_cache.viewport,
                &mut pass,
            ) {
                tracing::warn!("Text render error: {:?}", e);
            }
        }

        // 9. Submit (caller presents)
        queue.submit([encoder.finish()]);
    }

    /// Free unused glyph atlas space between frames.
    pub fn trim(&mut self) {
        self.glyph_cache.trim();
    }
}
