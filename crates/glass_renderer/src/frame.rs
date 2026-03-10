//! FrameRenderer: orchestrates the full rendering pipeline.
//!
//! Composites clear -> rect backgrounds -> text -> present for each frame.
//! Owns the GlyphCache, GridRenderer, and RectRenderer.

use alacritty_terminal::vte::ansi::Rgb;
use glyphon::{
    Attrs, Buffer, Color as GlyphonColor, Family, Metrics, Resolution, Shaping, TextArea,
    TextBounds,
};

use glass_terminal::{Block, GridSnapshot, StatusState};

use crate::block_renderer::BlockRenderer;
use crate::glyph_cache::GlyphCache;
use crate::grid_renderer::GridRenderer;
use crate::rect_renderer::RectRenderer;
use crate::search_overlay_renderer::SearchOverlayRenderer;
use crate::status_bar::StatusBarRenderer;
use crate::tab_bar::TabBarRenderer;

/// Display data for the search overlay, extracted from SearchOverlay state.
/// Passed as Option to draw_frame to avoid borrow conflicts with WindowContext.
pub struct SearchOverlayRenderData {
    pub query: String,
    pub results: Vec<(String, Option<i32>, String, String)>, // (command, exit_code, timestamp, preview)
    pub selected: usize,
}

/// Orchestrates the complete GPU rendering pipeline for terminal content.
///
/// Each frame: clear to background color, draw cell background rects,
/// draw text via glyphon, present. Owns all rendering sub-systems.
pub struct FrameRenderer {
    pub glyph_cache: GlyphCache,
    grid_renderer: GridRenderer,
    rect_renderer: RectRenderer,
    block_renderer: BlockRenderer,
    search_overlay_renderer: SearchOverlayRenderer,
    status_bar: StatusBarRenderer,
    tab_bar: TabBarRenderer,
    default_bg: Rgb,
    /// Reusable buffer storage to avoid per-frame allocation
    text_buffers: Vec<Buffer>,
    /// Reusable position storage for per-cell grid rendering
    cell_positions: Vec<(usize, i32)>,
    /// Reusable buffer storage for overlay text (block labels, status bar)
    overlay_buffers: Vec<Buffer>,
    /// Reusable buffer storage for pipeline overlay text (drawn after overlay rects)
    pipeline_buffers: Vec<Buffer>,
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
        Self::with_font_system(
            glyphon::FontSystem::new(),
            device,
            queue,
            surface_format,
            font_family,
            font_size,
            scale_factor,
        )
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
        let mut glyph_cache =
            GlyphCache::with_font_system(font_system, device, queue, surface_format);
        let grid_renderer = GridRenderer::new(
            &mut glyph_cache.font_system,
            font_family,
            font_size,
            scale_factor,
        );
        let rect_renderer = RectRenderer::new(device, surface_format);
        let (cell_width, cell_height) = grid_renderer.cell_size();
        let block_renderer = BlockRenderer::new(cell_width, cell_height);
        let search_overlay_renderer = SearchOverlayRenderer::new(cell_width, cell_height);
        let status_bar = StatusBarRenderer::new(cell_height);
        let tab_bar = TabBarRenderer::new(cell_width, cell_height);
        let default_bg = Rgb {
            r: 26,
            g: 26,
            b: 26,
        };

        Self {
            glyph_cache,
            grid_renderer,
            rect_renderer,
            block_renderer,
            search_overlay_renderer,
            status_bar,
            tab_bar,
            default_bg,
            text_buffers: Vec::new(),
            cell_positions: Vec::new(),
            overlay_buffers: Vec::new(),
            pipeline_buffers: Vec::new(),
        }
    }

    /// Returns (cell_width, cell_height) in physical pixels.
    pub fn cell_size(&self) -> (f32, f32) {
        self.grid_renderer.cell_size()
    }

    /// Rebuild font metrics and all dependent sub-renderers after a font change.
    ///
    /// Called when the user changes font_family or font_size in config.toml.
    /// Rebuilds GridRenderer (cell metrics), then updates BlockRenderer,
    /// SearchOverlayRenderer, StatusBarRenderer, and TabBarRenderer.
    pub fn update_font(&mut self, font_family: &str, font_size: f32, scale_factor: f32) {
        self.grid_renderer = GridRenderer::new(
            &mut self.glyph_cache.font_system,
            font_family,
            font_size,
            scale_factor,
        );
        let (cell_width, cell_height) = self.grid_renderer.cell_size();
        self.block_renderer = BlockRenderer::new(cell_width, cell_height);
        self.search_overlay_renderer = SearchOverlayRenderer::new(cell_width, cell_height);
        self.status_bar = StatusBarRenderer::new(cell_height);
        self.tab_bar = TabBarRenderer::new(cell_width, cell_height);
    }

    /// Returns a reference to the tab bar renderer (for hit testing).
    pub fn tab_bar(&self) -> &TabBarRenderer {
        &self.tab_bar
    }

    /// Draw a complete frame with terminal content.
    ///
    /// Pipeline: clear -> rect backgrounds/cursor/block decorations/status bar -> text -> end pass.
    /// The caller is responsible for presenting the frame texture.
    ///
    /// When `blocks` is empty and `status` is None, this behaves identically to Phase 2.
    #[cfg_attr(feature = "perf", tracing::instrument(skip_all))]
    #[allow(clippy::too_many_arguments)]
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
        search_overlay: Option<&SearchOverlayRenderData>,
        tab_bar_info: Option<&[crate::tab_bar::TabDisplayInfo]>,
        update_text: Option<&str>,
        coordination_text: Option<&str>,
    ) {
        let w = width as f32;
        let h = height as f32;

        // Compute grid y-offset: shift content below tab bar when present
        let grid_y_offset = if tab_bar_info.is_some() {
            let (_, cell_height) = self.grid_renderer.cell_size();
            cell_height
        } else {
            0.0
        };

        // 1. Build rect instances (backgrounds + cursor)
        let mut rect_instances = if grid_y_offset > 0.0 {
            self.grid_renderer
                .build_rects_offset(snapshot, self.default_bg, 0.0, grid_y_offset)
        } else {
            self.grid_renderer.build_rects(snapshot, self.default_bg)
        };

        // 1a2. Append selection highlight rects
        if let Some(ref sel) = snapshot.selection {
            let mut sel_rects = self.grid_renderer.build_selection_rects(
                sel,
                snapshot.display_offset,
                snapshot.columns,
            );
            if grid_y_offset > 0.0 {
                for rect in &mut sel_rects {
                    rect.pos[1] += grid_y_offset;
                }
            }
            rect_instances.extend(sel_rects);
        }

        // 1a3. Append text decoration rects (underline, strikethrough)
        {
            let mut deco_rects = self.grid_renderer.build_decoration_rects(snapshot);
            if grid_y_offset > 0.0 {
                for rect in &mut deco_rects {
                    rect.pos[1] += grid_y_offset;
                }
            }
            rect_instances.extend(deco_rects);
        }

        // 1b. Append block decoration rects (separators, badges)
        // Block lines are absolute; convert viewport start to absolute coords.
        if !blocks.is_empty() {
            let viewport_abs_start = snapshot
                .history_size
                .saturating_sub(snapshot.display_offset);
            let mut block_rects = self.block_renderer.build_block_rects(
                blocks,
                viewport_abs_start,
                snapshot.screen_lines,
                w,
            );
            if grid_y_offset > 0.0 {
                for rect in &mut block_rects {
                    rect.pos[1] += grid_y_offset;
                }
            }
            rect_instances.extend(block_rects);
        }

        // 1c. Append status bar background rect
        if status.is_some() {
            let status_rects = self.status_bar.build_status_rects(w, h);
            rect_instances.extend(status_rects);
        }

        // 1c2. Append tab bar rects (at top of viewport)
        if let Some(tabs) = tab_bar_info {
            let tab_rects = self.tab_bar.build_tab_rects(tabs, w);
            rect_instances.extend(tab_rects);
        }

        // 1d. Append search overlay rects (backdrop, input box, result rows)
        if let Some(overlay) = search_overlay {
            let overlay_rects = self.search_overlay_renderer.build_overlay_rects(
                overlay.results.len(),
                overlay.selected,
                w,
                h,
            );
            rect_instances.extend(overlay_rects);
        }

        // Record where background rects end (pipeline overlay rects come after)
        let bg_rect_count = rect_instances.len() as u32;

        // 1e. Pipeline panel rects (bottom of viewport, above status bar)
        let (_, cell_height_early) = self.grid_renderer.cell_size();
        let status_bar_h = if status.is_some() {
            cell_height_early
        } else {
            0.0
        };
        if !blocks.is_empty() {
            let pipeline_rects =
                self.block_renderer
                    .build_pipeline_rects(blocks, w, h, status_bar_h);
            rect_instances.extend(pipeline_rects);
        }

        let total_rect_count = rect_instances.len() as u32;

        // 2. Prepare rect renderer (all rects in one buffer, drawn in two passes)
        self.rect_renderer
            .prepare(device, queue, &rect_instances, width, height);

        // 3. Build per-cell text buffers and text areas for grid content
        self.text_buffers.clear();
        self.cell_positions.clear();
        self.grid_renderer.build_cell_buffers(
            &mut self.glyph_cache.font_system,
            snapshot,
            &mut self.text_buffers,
            &mut self.cell_positions,
        );
        let mut text_areas: Vec<TextArea<'_>> = self.grid_renderer.build_cell_text_areas_offset(
            &self.text_buffers,
            &self.cell_positions,
            width,
            height,
            0.0,
            grid_y_offset,
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
            let viewport_abs_start = snapshot
                .history_size
                .saturating_sub(snapshot.display_offset);
            let block_labels = self.block_renderer.build_block_text(
                blocks,
                viewport_abs_start,
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
                        .color(GlyphonColor::rgba(
                            label.color.r,
                            label.color.g,
                            label.color.b,
                            255,
                        )),
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.glyph_cache.font_system, false);
                self.overlay_buffers.push(buffer);
                overlay_metas.push(OverlayMeta {
                    left: label.x,
                    top: label.y + grid_y_offset,
                    color: GlyphonColor::rgba(label.color.r, label.color.g, label.color.b, 255),
                });
            }
        }

        // Status bar text buffers
        if let Some(status_state) = status {
            let status_label = self.status_bar.build_status_text(
                status_state.cwd(),
                status_state.git_info(),
                update_text,
                coordination_text,
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

            // Coordination text (agent/lock counts) -- positioned left of git info
            if let Some(ref coord_text) = status_label.coordination_text {
                // Position: right-aligned but offset further left than git info
                let right_text_chars = status_label
                    .right_text
                    .as_ref()
                    .map(|t| t.len())
                    .unwrap_or(0);
                let coord_text_width = coord_text.len() as f32 * cell_width;
                let gap = if right_text_chars > 0 {
                    cell_width * 2.0
                } else {
                    cell_width * 0.5
                };
                let coord_x = w - (right_text_chars as f32 * cell_width) - gap - coord_text_width;
                let mut buffer = Buffer::new(&mut self.glyph_cache.font_system, metrics);
                buffer.set_size(
                    &mut self.glyph_cache.font_system,
                    Some(w),
                    Some(cell_height),
                );
                buffer.set_text(
                    &mut self.glyph_cache.font_system,
                    coord_text,
                    &Attrs::new()
                        .family(Family::Name(font_family))
                        .color(GlyphonColor::rgba(
                            status_label.coordination_color.r,
                            status_label.coordination_color.g,
                            status_label.coordination_color.b,
                            255,
                        )),
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.glyph_cache.font_system, false);
                self.overlay_buffers.push(buffer);
                overlay_metas.push(OverlayMeta {
                    left: coord_x,
                    top: status_label.y,
                    color: GlyphonColor::rgba(
                        status_label.coordination_color.r,
                        status_label.coordination_color.g,
                        status_label.coordination_color.b,
                        255,
                    ),
                });
            }

            // Center text (update notification)
            if let Some(ref center_text) = status_label.center_text {
                let center_text_width = center_text.len() as f32 * cell_width;
                let center_x = (w - center_text_width) / 2.0;
                let mut buffer = Buffer::new(&mut self.glyph_cache.font_system, metrics);
                buffer.set_size(
                    &mut self.glyph_cache.font_system,
                    Some(w),
                    Some(cell_height),
                );
                buffer.set_text(
                    &mut self.glyph_cache.font_system,
                    center_text,
                    &Attrs::new()
                        .family(Family::Name(font_family))
                        .color(GlyphonColor::rgba(
                            status_label.center_color.r,
                            status_label.center_color.g,
                            status_label.center_color.b,
                            255,
                        )),
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.glyph_cache.font_system, false);
                self.overlay_buffers.push(buffer);
                overlay_metas.push(OverlayMeta {
                    left: center_x,
                    top: status_label.y,
                    color: GlyphonColor::rgba(
                        status_label.center_color.r,
                        status_label.center_color.g,
                        status_label.center_color.b,
                        255,
                    ),
                });
            }
        }

        // Tab bar text buffers
        if let Some(tabs) = tab_bar_info {
            let tab_labels = self.tab_bar.build_tab_text(tabs, w);
            for label in &tab_labels {
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
                        .color(GlyphonColor::rgba(
                            label.color.r,
                            label.color.g,
                            label.color.b,
                            255,
                        )),
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

        // Search overlay text buffers
        if let Some(overlay) = search_overlay {
            let overlay_labels = self.search_overlay_renderer.build_overlay_text(
                &overlay.query,
                &overlay.results,
                overlay.selected,
                w,
                h,
            );
            for label in &overlay_labels {
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
                        .color(GlyphonColor::rgba(
                            label.color.r,
                            label.color.g,
                            label.color.b,
                            255,
                        )),
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

        // Build pipeline label buffers separately (rendered in second pass)
        self.pipeline_buffers.clear();
        let mut pipeline_metas: Vec<OverlayMeta> = Vec::new();

        if !blocks.is_empty() {
            let pipeline_labels =
                self.block_renderer
                    .build_pipeline_text(blocks, w, h, status_bar_h);
            for label in &pipeline_labels {
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
                        .color(GlyphonColor::rgba(
                            label.color.r,
                            label.color.g,
                            label.color.b,
                            255,
                        )),
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.glyph_cache.font_system, false);
                self.pipeline_buffers.push(buffer);
                pipeline_metas.push(OverlayMeta {
                    left: label.x,
                    top: label.y,
                    color: GlyphonColor::rgba(label.color.r, label.color.g, label.color.b, 255),
                });
            }
        }

        let has_pipeline_overlay = !pipeline_metas.is_empty();

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
        self.glyph_cache
            .viewport
            .update(queue, Resolution { width, height });

        // 5. Prepare text renderer (grid + block labels + status bar — NO pipeline labels)
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

            // 7. Draw background rects (grid + block decorations + status bar)
            self.rect_renderer.render(&mut pass, bg_rect_count);

            // 8. Draw text (grid + block labels + status bar)
            if let Err(e) = self.glyph_cache.text_renderer.render(
                &self.glyph_cache.atlas,
                &self.glyph_cache.viewport,
                &mut pass,
            ) {
                tracing::warn!("Text render error: {:?}", e);
            }

            // 9. Draw pipeline overlay rects on top of text
            self.rect_renderer
                .render_range(&mut pass, bg_rect_count, total_rect_count);
        }

        // Submit pass 1
        queue.submit([encoder.finish()]);

        // 10. Second pass for pipeline label text (on top of overlay rects)
        if has_pipeline_overlay {
            let pipeline_text_areas: Vec<TextArea<'_>> = pipeline_metas
                .iter()
                .enumerate()
                .map(|(i, meta)| TextArea {
                    buffer: &self.pipeline_buffers[i],
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
                })
                .collect();

            if let Err(e) = self.glyph_cache.text_renderer.prepare(
                device,
                queue,
                &mut self.glyph_cache.font_system,
                &mut self.glyph_cache.atlas,
                &self.glyph_cache.viewport,
                pipeline_text_areas,
                &mut self.glyph_cache.swash_cache,
            ) {
                tracing::warn!("Pipeline text prepare error: {:?}", e);
            }

            let mut encoder2 = device.create_command_encoder(&Default::default());
            {
                let mut pass2 = encoder2.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("pipeline_overlay_pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load, // preserve previous content
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });

                if let Err(e) = self.glyph_cache.text_renderer.render(
                    &self.glyph_cache.atlas,
                    &self.glyph_cache.viewport,
                    &mut pass2,
                ) {
                    tracing::warn!("Pipeline text render error: {:?}", e);
                }
            }
            queue.submit([encoder2.finish()]);
        }
    }

    /// Draw a complete frame with multiple split panes.
    ///
    /// Each pane's grid content is rendered at its viewport offset with TextBounds
    /// clipping. Dividers are drawn between panes. Status bar and tab bar are
    /// drawn globally. The focused pane gets a subtle accent border.
    ///
    /// `panes`: Vec of (viewport, snapshot, blocks, is_focused) for each pane.
    #[cfg_attr(feature = "perf", tracing::instrument(skip_all))]
    #[allow(clippy::too_many_arguments)]
    pub fn draw_multi_pane_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        panes: &[(PaneViewport, &GridSnapshot, &[&Block], bool)],
        dividers: &[DividerRect],
        status: Option<&StatusState>,
        tab_bar_info: Option<&[crate::tab_bar::TabDisplayInfo]>,
        update_text: Option<&str>,
        coordination_text: Option<&str>,
    ) {
        let w = width as f32;
        let h = height as f32;

        // 1. Build rect instances for all panes (with viewport offsets)
        let mut rect_instances: Vec<crate::rect_renderer::RectInstance> = Vec::new();

        for (viewport, snapshot, _blocks, is_focused) in panes {
            let pane_rects = self.grid_renderer.build_rects_offset(
                snapshot,
                self.default_bg,
                viewport.x as f32,
                viewport.y as f32,
            );
            rect_instances.extend(pane_rects);

            // Selection highlight rects
            if let Some(ref sel) = snapshot.selection {
                let mut sel_rects = self.grid_renderer.build_selection_rects(
                    sel,
                    snapshot.display_offset,
                    snapshot.columns,
                );
                for rect in &mut sel_rects {
                    rect.pos[0] += viewport.x as f32;
                    rect.pos[1] += viewport.y as f32;
                }
                rect_instances.extend(sel_rects);
            }

            // Text decoration rects (underline, strikethrough)
            {
                let mut deco_rects = self.grid_renderer.build_decoration_rects(snapshot);
                for rect in &mut deco_rects {
                    rect.pos[0] += viewport.x as f32;
                    rect.pos[1] += viewport.y as f32;
                }
                rect_instances.extend(deco_rects);
            }

            // Focused pane accent border (1px cornflower blue)
            if *is_focused && panes.len() > 1 {
                let bx = viewport.x as f32;
                let by = viewport.y as f32;
                let bw = viewport.width as f32;
                let bh = viewport.height as f32;
                let border_color = [100.0 / 255.0, 149.0 / 255.0, 237.0 / 255.0, 1.0];
                let t = 1.0;
                // Top
                rect_instances.push(crate::rect_renderer::RectInstance {
                    pos: [bx, by, bw, t],
                    color: border_color,
                });
                // Bottom
                rect_instances.push(crate::rect_renderer::RectInstance {
                    pos: [bx, by + bh - t, bw, t],
                    color: border_color,
                });
                // Left
                rect_instances.push(crate::rect_renderer::RectInstance {
                    pos: [bx, by, t, bh],
                    color: border_color,
                });
                // Right
                rect_instances.push(crate::rect_renderer::RectInstance {
                    pos: [bx + bw - t, by, t, bh],
                    color: border_color,
                });
            }
        }

        // Divider rects between panes
        for div in dividers {
            rect_instances.push(crate::rect_renderer::RectInstance {
                pos: [
                    div.x as f32,
                    div.y as f32,
                    div.width as f32,
                    div.height as f32,
                ],
                color: [80.0 / 255.0, 80.0 / 255.0, 80.0 / 255.0, 1.0],
            });
        }

        // Status bar background rect
        if status.is_some() {
            let status_rects = self.status_bar.build_status_rects(w, h);
            rect_instances.extend(status_rects);
        }

        // Tab bar rects
        if let Some(tabs) = tab_bar_info {
            let tab_rects = self.tab_bar.build_tab_rects(tabs, w);
            rect_instances.extend(tab_rects);
        }

        let total_rect_count = rect_instances.len() as u32;

        // 2. Prepare rect renderer
        self.rect_renderer
            .prepare(device, queue, &rect_instances, width, height);

        // 3. Build per-cell text buffers for all panes
        // We need separate buffer storage per pane since they have different offsets
        self.text_buffers.clear();
        self.cell_positions.clear();
        let mut text_areas: Vec<TextArea<'_>> = Vec::new();
        let mut pane_ranges: Vec<(usize, usize, usize, usize)> = Vec::new();

        for (_viewport, snapshot, _blocks, _is_focused) in panes {
            let buf_start = self.text_buffers.len();
            let pos_start = self.cell_positions.len();
            self.grid_renderer.build_cell_buffers(
                &mut self.glyph_cache.font_system,
                snapshot,
                &mut self.text_buffers,
                &mut self.cell_positions,
            );
            let buf_end = self.text_buffers.len();
            let pos_end = self.cell_positions.len();
            pane_ranges.push((buf_start, buf_end, pos_start, pos_end));
        }

        // Build text areas with offsets for each pane
        for (i, (viewport, _snapshot, _blocks, _is_focused)) in panes.iter().enumerate() {
            let (buf_start, buf_end, pos_start, pos_end) = pane_ranges[i];
            let pane_buffers = &self.text_buffers[buf_start..buf_end];
            let pane_positions = &self.cell_positions[pos_start..pos_end];
            let areas = self.grid_renderer.build_cell_text_areas_offset(
                pane_buffers,
                pane_positions,
                viewport.width,
                viewport.height,
                viewport.x as f32,
                viewport.y as f32,
            );
            text_areas.extend(areas);
        }

        // 3b. Build overlay text (status bar + tab bar)
        self.overlay_buffers.clear();
        let physical_font_size = self.grid_renderer.font_size * self.grid_renderer.scale_factor;
        let (_cell_width, cell_height) = self.grid_renderer.cell_size();
        let cell_width = _cell_width;
        let metrics = Metrics::new(physical_font_size, cell_height);
        let font_family = &self.grid_renderer.font_family;

        struct OverlayMeta {
            left: f32,
            top: f32,
            color: GlyphonColor,
        }
        let mut overlay_metas: Vec<OverlayMeta> = Vec::new();

        // Status bar text
        if let Some(status_state) = status {
            let status_label = self.status_bar.build_status_text(
                status_state.cwd(),
                status_state.git_info(),
                update_text,
                coordination_text,
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

            // Coordination text (agent/lock counts) -- positioned left of git info
            if let Some(ref coord_text) = status_label.coordination_text {
                let right_text_chars = status_label
                    .right_text
                    .as_ref()
                    .map(|t| t.len())
                    .unwrap_or(0);
                let coord_text_width = coord_text.len() as f32 * cell_width;
                let gap = if right_text_chars > 0 {
                    cell_width * 2.0
                } else {
                    cell_width * 0.5
                };
                let coord_x = w - (right_text_chars as f32 * cell_width) - gap - coord_text_width;
                let mut buffer = Buffer::new(&mut self.glyph_cache.font_system, metrics);
                buffer.set_size(
                    &mut self.glyph_cache.font_system,
                    Some(w),
                    Some(cell_height),
                );
                buffer.set_text(
                    &mut self.glyph_cache.font_system,
                    coord_text,
                    &Attrs::new()
                        .family(Family::Name(font_family))
                        .color(GlyphonColor::rgba(
                            status_label.coordination_color.r,
                            status_label.coordination_color.g,
                            status_label.coordination_color.b,
                            255,
                        )),
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.glyph_cache.font_system, false);
                self.overlay_buffers.push(buffer);
                overlay_metas.push(OverlayMeta {
                    left: coord_x,
                    top: status_label.y,
                    color: GlyphonColor::rgba(
                        status_label.coordination_color.r,
                        status_label.coordination_color.g,
                        status_label.coordination_color.b,
                        255,
                    ),
                });
            }

            // Center text (update notification)
            if let Some(ref center_text) = status_label.center_text {
                let center_text_width = center_text.len() as f32 * cell_width;
                let center_x = (w - center_text_width) / 2.0;
                let mut buffer = Buffer::new(&mut self.glyph_cache.font_system, metrics);
                buffer.set_size(
                    &mut self.glyph_cache.font_system,
                    Some(w),
                    Some(cell_height),
                );
                buffer.set_text(
                    &mut self.glyph_cache.font_system,
                    center_text,
                    &Attrs::new()
                        .family(Family::Name(font_family))
                        .color(GlyphonColor::rgba(
                            status_label.center_color.r,
                            status_label.center_color.g,
                            status_label.center_color.b,
                            255,
                        )),
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.glyph_cache.font_system, false);
                self.overlay_buffers.push(buffer);
                overlay_metas.push(OverlayMeta {
                    left: center_x,
                    top: status_label.y,
                    color: GlyphonColor::rgba(
                        status_label.center_color.r,
                        status_label.center_color.g,
                        status_label.center_color.b,
                        255,
                    ),
                });
            }
        }

        // Tab bar text
        if let Some(tabs) = tab_bar_info {
            let tab_labels = self.tab_bar.build_tab_text(tabs, w);
            for label in &tab_labels {
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
                        .color(GlyphonColor::rgba(
                            label.color.r,
                            label.color.g,
                            label.color.b,
                            255,
                        )),
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

        // Create TextAreas from overlay buffers
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
        self.glyph_cache
            .viewport
            .update(queue, Resolution { width, height });

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
            tracing::warn!("Multi-pane text prepare error: {:?}", e);
        }

        // 6. Render pass
        let bg_r = self.default_bg.r as f64 / 255.0;
        let bg_g = self.default_bg.g as f64 / 255.0;
        let bg_b = self.default_bg.b as f64 / 255.0;

        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("multi_pane_frame_pass"),
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

            // Draw all rects
            self.rect_renderer.render(&mut pass, total_rect_count);

            // Draw all text
            if let Err(e) = self.glyph_cache.text_renderer.render(
                &self.glyph_cache.atlas,
                &self.glyph_cache.viewport,
                &mut pass,
            ) {
                tracing::warn!("Multi-pane text render error: {:?}", e);
            }
        }
        queue.submit([encoder.finish()]);
    }

    /// Draw a config error overlay banner on top of existing frame content.
    ///
    /// Renders a dark red rect at the top of the viewport with the error message.
    /// Uses LoadOp::Load to preserve the existing frame content underneath.
    /// Must be called AFTER draw_frame/draw_multi_pane_frame (reuses rect_renderer).
    pub fn draw_config_error_overlay(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        error: &glass_core::config::ConfigError,
    ) {
        let (cell_width, cell_height) = self.grid_renderer.cell_size();
        let overlay = crate::config_error_overlay::ConfigErrorOverlay::new(cell_width, cell_height);
        let error_rects = overlay.build_error_rects(width as f32);
        let error_labels = overlay.build_error_text(error, width as f32);

        // Reuse self.rect_renderer -- safe because this runs after the main draw
        self.rect_renderer
            .prepare(device, queue, &error_rects, width, height);

        // Build error text buffer
        let physical_font_size = self.grid_renderer.font_size * self.grid_renderer.scale_factor;
        let metrics = Metrics::new(physical_font_size, cell_height);
        let font_family = &self.grid_renderer.font_family;

        let mut error_buffer = Buffer::new(&mut self.glyph_cache.font_system, metrics);
        if let Some(label) = error_labels.first() {
            error_buffer.set_size(
                &mut self.glyph_cache.font_system,
                Some(width as f32),
                Some(cell_height),
            );
            error_buffer.set_text(
                &mut self.glyph_cache.font_system,
                &label.text,
                &Attrs::new()
                    .family(Family::Name(font_family))
                    .color(GlyphonColor::rgba(
                        label.color.r,
                        label.color.g,
                        label.color.b,
                        255,
                    )),
                Shaping::Advanced,
                None,
            );
            error_buffer.shape_until_scroll(&mut self.glyph_cache.font_system, false);
        }

        let error_text_areas: Vec<TextArea<'_>> = error_labels
            .iter()
            .take(1)
            .map(|label| TextArea {
                buffer: &error_buffer,
                left: label.x,
                top: label.y,
                scale: 1.0,
                bounds: TextBounds {
                    left: 0,
                    top: 0,
                    right: width as i32,
                    bottom: height as i32,
                },
                default_color: GlyphonColor::rgba(label.color.r, label.color.g, label.color.b, 255),
                custom_glyphs: &[],
            })
            .collect();

        self.glyph_cache
            .viewport
            .update(queue, Resolution { width, height });

        if let Err(e) = self.glyph_cache.text_renderer.prepare(
            device,
            queue,
            &mut self.glyph_cache.font_system,
            &mut self.glyph_cache.atlas,
            &self.glyph_cache.viewport,
            error_text_areas,
            &mut self.glyph_cache.swash_cache,
        ) {
            tracing::warn!("Config error overlay text prepare error: {:?}", e);
        }

        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("config_error_overlay_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.rect_renderer
                .render(&mut pass, error_rects.len() as u32);
            if let Err(e) = self.glyph_cache.text_renderer.render(
                &self.glyph_cache.atlas,
                &self.glyph_cache.viewport,
                &mut pass,
            ) {
                tracing::warn!("Config error overlay text render error: {:?}", e);
            }
        }
        queue.submit([encoder.finish()]);
    }

    /// Render the conflict warning overlay (amber banner at the bottom of the viewport).
    ///
    /// Uses LoadOp::Load to preserve the existing frame content underneath.
    /// Must be called AFTER draw_frame/draw_multi_pane_frame (reuses rect_renderer).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_conflict_overlay(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        agent_count: usize,
        lock_count: usize,
    ) {
        let (cell_width, cell_height) = self.grid_renderer.cell_size();
        let overlay = crate::conflict_overlay::ConflictOverlay::new(cell_width, cell_height);
        let warning_rects = overlay.build_warning_rects(width as f32, height as f32, 1);
        let warning_labels = overlay.build_warning_text(agent_count, lock_count, height as f32);

        // Reuse self.rect_renderer -- safe because this runs after the main draw
        self.rect_renderer
            .prepare(device, queue, &warning_rects, width, height);

        // Build warning text buffer
        let physical_font_size = self.grid_renderer.font_size * self.grid_renderer.scale_factor;
        let metrics = Metrics::new(physical_font_size, cell_height);
        let font_family = &self.grid_renderer.font_family;

        let mut warning_buffer = Buffer::new(&mut self.glyph_cache.font_system, metrics);
        if let Some(label) = warning_labels.first() {
            warning_buffer.set_size(
                &mut self.glyph_cache.font_system,
                Some(width as f32),
                Some(cell_height),
            );
            warning_buffer.set_text(
                &mut self.glyph_cache.font_system,
                &label.text,
                &Attrs::new()
                    .family(Family::Name(font_family))
                    .color(GlyphonColor::rgba(
                        label.color.r,
                        label.color.g,
                        label.color.b,
                        255,
                    )),
                Shaping::Advanced,
                None,
            );
            warning_buffer.shape_until_scroll(&mut self.glyph_cache.font_system, false);
        }

        let warning_text_areas: Vec<TextArea<'_>> = warning_labels
            .iter()
            .take(1)
            .map(|label| TextArea {
                buffer: &warning_buffer,
                left: label.x,
                top: label.y,
                scale: 1.0,
                bounds: TextBounds {
                    left: 0,
                    top: 0,
                    right: width as i32,
                    bottom: height as i32,
                },
                default_color: GlyphonColor::rgba(label.color.r, label.color.g, label.color.b, 255),
                custom_glyphs: &[],
            })
            .collect();

        self.glyph_cache
            .viewport
            .update(queue, Resolution { width, height });

        if let Err(e) = self.glyph_cache.text_renderer.prepare(
            device,
            queue,
            &mut self.glyph_cache.font_system,
            &mut self.glyph_cache.atlas,
            &self.glyph_cache.viewport,
            warning_text_areas,
            &mut self.glyph_cache.swash_cache,
        ) {
            tracing::warn!("Conflict overlay text prepare error: {:?}", e);
        }

        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("conflict_overlay_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.rect_renderer
                .render(&mut pass, warning_rects.len() as u32);
            if let Err(e) = self.glyph_cache.text_renderer.render(
                &self.glyph_cache.atlas,
                &self.glyph_cache.viewport,
                &mut pass,
            ) {
                tracing::warn!("Conflict overlay text render error: {:?}", e);
            }
        }
        queue.submit([encoder.finish()]);
    }

    /// Free unused glyph atlas space between frames.
    pub fn trim(&mut self) {
        self.glyph_cache.trim();
    }
}

/// Viewport position and size for a single pane within a multi-pane layout.
pub struct PaneViewport {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// A divider rectangle between split panes.
pub struct DividerRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
