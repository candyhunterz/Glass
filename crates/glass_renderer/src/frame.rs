//! FrameRenderer: orchestrates the full rendering pipeline.
//!
//! Composites clear -> rect backgrounds -> text -> present for each frame.
//! Owns the GlyphCache, GridRenderer, and RectRenderer.

use alacritty_terminal::vte::ansi::Rgb;
use glyphon::{Attrs, Buffer, Color as GlyphonColor, Family, Metrics, Resolution, Shaping, TextArea, TextBounds};

use glass_terminal::{Block, GridSnapshot, StatusState};

use crate::block_renderer::BlockRenderer;
use crate::glyph_cache::GlyphCache;
use crate::grid_renderer::GridRenderer;
use crate::rect_renderer::RectRenderer;
use crate::status_bar::StatusBarRenderer;

/// Orchestrates the complete GPU rendering pipeline for terminal content.
///
/// Each frame: clear to background color, draw cell background rects,
/// draw text via glyphon, present. Owns all rendering sub-systems.
pub struct FrameRenderer {
    pub glyph_cache: GlyphCache,
    grid_renderer: GridRenderer,
    rect_renderer: RectRenderer,
    block_renderer: BlockRenderer,
    status_bar: StatusBarRenderer,
    default_bg: Rgb,
    /// Reusable buffer storage to avoid per-frame allocation
    text_buffers: Vec<Buffer>,
    /// Reusable buffer storage for overlay text (block labels, status bar)
    overlay_buffers: Vec<Buffer>,
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
        Self::with_font_system(glyphon::FontSystem::new(), device, queue, surface_format, font_family, font_size, scale_factor)
    }

    /// Create the rendering pipeline with a pre-created FontSystem (for parallel init).
    pub fn with_font_system(
        font_system: glyphon::FontSystem,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        font_family: &str,
        font_size: f32,
        scale_factor: f32,
    ) -> Self {
        let mut glyph_cache = GlyphCache::with_font_system(font_system, device, queue, surface_format);
        let grid_renderer = GridRenderer::new(
            &mut glyph_cache.font_system,
            font_family,
            font_size,
            scale_factor,
        );
        let rect_renderer = RectRenderer::new(device, surface_format);
        let (cell_width, cell_height) = grid_renderer.cell_size();
        let block_renderer = BlockRenderer::new(cell_width, cell_height);
        let status_bar = StatusBarRenderer::new(cell_height);
        let default_bg = Rgb { r: 26, g: 26, b: 26 };

        Self {
            glyph_cache,
            grid_renderer,
            rect_renderer,
            block_renderer,
            status_bar,
            default_bg,
            text_buffers: Vec::new(),
            overlay_buffers: Vec::new(),
        }
    }

    /// Returns (cell_width, cell_height) in physical pixels.
    pub fn cell_size(&self) -> (f32, f32) {
        self.grid_renderer.cell_size()
    }

    /// Draw a complete frame with terminal content.
    ///
    /// Pipeline: clear -> rect backgrounds/cursor/block decorations/status bar -> text -> end pass.
    /// The caller is responsible for presenting the frame texture.
    ///
    /// When `blocks` is empty and `status` is None, this behaves identically to Phase 2.
    pub fn draw_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        snapshot: &GridSnapshot,
        blocks: &[&Block],
        status: Option<&StatusState>,
    ) {
        let w = width as f32;
        let h = height as f32;

        // 1. Build rect instances (backgrounds + cursor)
        let mut rect_instances = self.grid_renderer.build_rects(snapshot, self.default_bg);

        // 1b. Append block decoration rects (separators, badges)
        if !blocks.is_empty() {
            let display_offset = snapshot.display_offset;
            let block_rects = self.block_renderer.build_block_rects(
                blocks,
                display_offset,
                snapshot.screen_lines,
                w,
            );
            rect_instances.extend(block_rects);
        }

        // 1c. Append status bar background rect
        if status.is_some() {
            let status_rects = self.status_bar.build_status_rects(w, h);
            rect_instances.extend(status_rects);
        }

        let rect_count = rect_instances.len() as u32;

        // 2. Prepare rect renderer
        self.rect_renderer.prepare(device, queue, &rect_instances, width, height);

        // 3. Build text buffers and text areas for grid content
        self.grid_renderer.build_text_buffers(
            &mut self.glyph_cache.font_system,
            snapshot,
            &mut self.text_buffers,
        );
        let mut text_areas: Vec<TextArea<'_>> = self.grid_renderer.build_text_areas(
            &self.text_buffers,
            width,
            height,
        );

        // 3b. Build overlay text buffers for block labels and status bar.
        // Two-phase approach: first build all buffers (mutable), then create
        // text areas (immutable borrows) to satisfy the borrow checker.
        self.overlay_buffers.clear();

        let physical_font_size = self.grid_renderer.font_size * self.grid_renderer.scale_factor;
        let (cell_width, cell_height) = self.grid_renderer.cell_size();
        let metrics = Metrics::new(physical_font_size, cell_height);
        let font_family = &self.grid_renderer.font_family;

        // Track overlay layout metadata: (left, top, default_color) per buffer
        struct OverlayMeta {
            left: f32,
            top: f32,
            color: GlyphonColor,
        }
        let mut overlay_metas: Vec<OverlayMeta> = Vec::new();

        // Phase A: Build all overlay buffers
        // Block label buffers
        if !blocks.is_empty() {
            let display_offset = snapshot.display_offset;
            let block_labels = self.block_renderer.build_block_text(
                blocks,
                display_offset,
                snapshot.screen_lines,
                w,
            );
            for label in &block_labels {
                let mut buffer = Buffer::new(&mut self.glyph_cache.font_system, metrics);
                buffer.set_size(
                    &mut self.glyph_cache.font_system,
                    Some(w - label.x),
                    Some(cell_height),
                );
                buffer.set_text(
                    &mut self.glyph_cache.font_system,
                    &label.text,
                    &Attrs::new()
                        .family(Family::Name(font_family))
                        .color(GlyphonColor::rgba(label.color.r, label.color.g, label.color.b, 255)),
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.glyph_cache.font_system, false);
                self.overlay_buffers.push(buffer);
                overlay_metas.push(OverlayMeta {
                    left: label.x,
                    top: label.y,
                    color: GlyphonColor::rgba(label.color.r, label.color.g, label.color.b, 255),
                });
            }
        }

        // Status bar text buffers
        if let Some(status_state) = status {
            let status_label = self.status_bar.build_status_text(
                status_state.cwd(),
                status_state.git_info(),
                h,
            );

            // Left text (CWD)
            {
                let mut buffer = Buffer::new(&mut self.glyph_cache.font_system, metrics);
                buffer.set_size(
                    &mut self.glyph_cache.font_system,
                    Some(w),
                    Some(cell_height),
                );
                buffer.set_text(
                    &mut self.glyph_cache.font_system,
                    &status_label.left_text,
                    &Attrs::new()
                        .family(Family::Name(font_family))
                        .color(GlyphonColor::rgba(
                            status_label.left_color.r,
                            status_label.left_color.g,
                            status_label.left_color.b,
                            255,
                        )),
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.glyph_cache.font_system, false);
                self.overlay_buffers.push(buffer);
                overlay_metas.push(OverlayMeta {
                    left: cell_width * 0.5,
                    top: status_label.y,
                    color: GlyphonColor::rgba(
                        status_label.left_color.r,
                        status_label.left_color.g,
                        status_label.left_color.b,
                        255,
                    ),
                });
            }

            // Right text (git info)
            if let Some(ref right_text) = status_label.right_text {
                let right_text_width = right_text.len() as f32 * cell_width;
                let mut buffer = Buffer::new(&mut self.glyph_cache.font_system, metrics);
                buffer.set_size(
                    &mut self.glyph_cache.font_system,
                    Some(w),
                    Some(cell_height),
                );
                buffer.set_text(
                    &mut self.glyph_cache.font_system,
                    right_text,
                    &Attrs::new()
                        .family(Family::Name(font_family))
                        .color(GlyphonColor::rgba(
                            status_label.right_color.r,
                            status_label.right_color.g,
                            status_label.right_color.b,
                            255,
                        )),
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.glyph_cache.font_system, false);
                self.overlay_buffers.push(buffer);
                overlay_metas.push(OverlayMeta {
                    left: w - right_text_width - cell_width * 0.5,
                    top: status_label.y,
                    color: GlyphonColor::rgba(
                        status_label.right_color.r,
                        status_label.right_color.g,
                        status_label.right_color.b,
                        255,
                    ),
                });
            }
        }

        // Phase B: Create TextAreas from overlay buffers (immutable borrows only)
        for (i, meta) in overlay_metas.iter().enumerate() {
            text_areas.push(TextArea {
                buffer: &self.overlay_buffers[i],
                left: meta.left,
                top: meta.top,
                scale: 1.0,
                bounds: TextBounds {
                    left: 0,
                    top: 0,
                    right: width as i32,
                    bottom: height as i32,
                },
                default_color: meta.color,
                custom_glyphs: &[],
            });
        }

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

            // 7. Draw rect backgrounds first (grid + block decorations + status bar)
            self.rect_renderer.render(&mut pass, rect_count);

            // 8. Draw text on top (grid + block labels + status bar text)
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
