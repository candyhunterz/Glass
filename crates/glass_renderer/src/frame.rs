//! FrameRenderer: orchestrates the full rendering pipeline.
//!
//! Composites clear -> rect backgrounds -> text -> present for each frame.
//! Owns the GlyphCache, GridRenderer, and RectRenderer.

use alacritty_terminal::vte::ansi::Rgb;
use glyphon::{
    Attrs, Buffer, Color as GlyphonColor, Family, Metrics, Resolution, Shaping, TextArea,
    TextBounds,
};

use glass_core::config::ThemeConfig;
use glass_terminal::{Block, GridSnapshot, StatusState};

use crate::block_renderer::BlockRenderer;
use crate::glyph_cache::GlyphCache;
use crate::grid_renderer::GridRenderer;
use crate::onboarding_toast_renderer::{OnboardingToastRenderData, OnboardingToastRenderer};
use crate::proposal_overlay_renderer::{ProposalOverlayRenderData, ProposalOverlayRenderer};
use crate::proposal_toast_renderer::{ProposalToastRenderData, ProposalToastRenderer};
use crate::rect_renderer::RectRenderer;
use crate::scrollbar::ScrollbarRenderer;
use crate::search_overlay_renderer::SearchOverlayRenderer;
use crate::status_bar::StatusBarRenderer;
use crate::tab_bar::TabBarRenderer;
use crate::welcome_overlay::{WelcomeOverlayRenderData, WelcomeOverlayRenderer};

/// Soft purple used for agent activity stream text in the status bar.
const ACTIVITY_STREAM_PURPLE: GlyphonColor = GlyphonColor::rgba(180, 140, 255, 255);
/// Dim gray used for keyboard shortcut hint text.
const HINT_TEXT_GRAY: GlyphonColor = GlyphonColor::rgba(85, 85, 85, 255);
/// Pure white used for general text rendering.
const TEXT_WHITE: GlyphonColor = GlyphonColor::rgba(255, 255, 255, 255);

/// Half-cell horizontal padding used for status bar edge margins.
const HALF_CELL_GAP: f32 = 0.5;
/// Two-cell horizontal gap between status bar sections.
const SECTION_PADDING_CELLS: f32 = 2.0;

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
    scrollbar: ScrollbarRenderer,
    status_bar: StatusBarRenderer,
    tab_bar: TabBarRenderer,
    default_bg: Rgb,
    /// Active theme for chrome colors.
    theme: ThemeConfig,
    /// Reusable buffer storage to avoid per-frame allocation
    text_buffers: Vec<Buffer>,
    /// Reusable position storage for per-cell grid rendering
    cell_positions: Vec<(usize, i32)>,
    /// Reusable buffer storage for overlay text (block labels, status bar)
    overlay_buffers: Vec<Buffer>,
    /// Reusable buffer storage for pipeline overlay text (drawn after overlay rects)
    pipeline_buffers: Vec<Buffer>,
    /// Last rendered GridSnapshot generation (single-pane path).
    /// Initialized to `u64::MAX` to force the first render.
    last_rendered_generation: u64,
    /// Cached pane buffer/position ranges for multi-pane rebuild skipping.
    cached_pane_ranges: Vec<(usize, usize, usize, usize)>,
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
        let theme = ThemeConfig::default();
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
        let scrollbar = ScrollbarRenderer::new();
        let status_bar = StatusBarRenderer::new(cell_width, cell_height);
        let tab_bar = TabBarRenderer::new(cell_width, cell_height);
        let default_bg = Rgb {
            r: theme.terminal_bg[0],
            g: theme.terminal_bg[1],
            b: theme.terminal_bg[2],
        };

        Self {
            glyph_cache,
            grid_renderer,
            rect_renderer,
            block_renderer,
            scrollbar,
            search_overlay_renderer,
            status_bar,
            tab_bar,
            default_bg,
            theme,
            text_buffers: Vec::new(),
            cell_positions: Vec::new(),
            overlay_buffers: Vec::new(),
            pipeline_buffers: Vec::new(),
            last_rendered_generation: u64::MAX,
            cached_pane_ranges: Vec::new(),
        }
    }

    /// Returns (cell_width, cell_height) in physical pixels.
    pub fn cell_size(&self) -> (f32, f32) {
        self.grid_renderer.cell_size()
    }

    /// Invalidate the cached generation so the next render forces a full buffer rebuild.
    /// Call this after window resize or scale-factor changes to ensure all layout
    /// calculations use the new viewport dimensions.
    pub fn invalidate_generation(&mut self) {
        self.last_rendered_generation = u64::MAX;
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
        self.status_bar = StatusBarRenderer::new(cell_width, cell_height);
        self.tab_bar = TabBarRenderer::new(cell_width, cell_height);
    }

    /// Returns a reference to the scrollbar renderer (for hit testing).
    pub fn scrollbar(&self) -> &ScrollbarRenderer {
        &self.scrollbar
    }

    /// Returns a reference to the tab bar renderer (for hit testing).
    pub fn tab_bar(&self) -> &TabBarRenderer {
        &self.tab_bar
    }

    /// Returns a mutable reference to the tab bar renderer (for scroll offset).
    pub fn tab_bar_mut(&mut self) -> &mut TabBarRenderer {
        &mut self.tab_bar
    }

    /// Update the active theme (called on config hot-reload).
    pub fn update_theme(&mut self, theme: ThemeConfig) {
        self.default_bg = Rgb {
            r: theme.terminal_bg[0],
            g: theme.terminal_bg[1],
            b: theme.terminal_bg[2],
        };
        self.block_renderer.update_theme(theme.clone());
        self.search_overlay_renderer.update_theme(theme.clone());
        self.status_bar.update_theme(theme.clone());
        self.tab_bar.update_theme(theme.clone());
        self.theme = theme;
    }

    /// Returns a reference to the active theme.
    pub fn theme(&self) -> &ThemeConfig {
        &self.theme
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
        hovered_tab: Option<usize>,
        drop_index: Option<usize>,
        update_text: Option<&str>,
        coordination_text: Option<&str>,
        agent_cost_text: Option<&str>,
        agent_paused: bool,
        scrollbar_hovered: bool,
        scrollbar_dragging: bool,
        agent_mode_text: Option<&str>,
        proposal_count_text: Option<&str>,
        proposal_toast: Option<&ProposalToastRenderData>,
        proposal_overlay: Option<&ProposalOverlayRenderData>,
        agent_activity_line: Option<&str>,
        orchestrating: bool,
        onboarding_toast: Option<&OnboardingToastRenderData>,
        welcome_overlay: Option<&WelcomeOverlayRenderData>,
    ) {
        let w = width as f32;
        let h = height as f32;
        let two_line_status = agent_activity_line.is_some();

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

        // 1c. Append status bar background rect
        if status.is_some() {
            let status_rects = if two_line_status {
                self.status_bar
                    .build_status_rects_two_line(w, h, orchestrating)
            } else {
                self.status_bar.build_status_rects(w, h, orchestrating)
            };
            rect_instances.extend(status_rects);
        }

        // 1c2. Append tab bar rects (at top of viewport)
        if let Some(tabs) = tab_bar_info {
            let tab_rects = self
                .tab_bar
                .build_tab_rects(tabs, w, hovered_tab, drop_index);
            rect_instances.extend(tab_rects);
        }

        // 1c3. Append scrollbar rects (right edge of pane, between tab bar and status bar)
        {
            let status_bar_h_sb = if status.is_some() {
                self.status_bar.height(two_line_status)
            } else {
                0.0
            };
            let scrollbar_rects = self.scrollbar.build_scrollbar_rects(
                w,
                grid_y_offset,
                h - grid_y_offset - status_bar_h_sb,
                snapshot.display_offset,
                snapshot.history_size,
                snapshot.screen_lines,
                scrollbar_hovered,
                scrollbar_dragging,
            );
            rect_instances.extend(scrollbar_rects);
        }

        // 1d. Append search overlay rects (backdrop, input box, result rows)
        // These are bg-layer rects; the backdrop covers terminal cell backgrounds.
        // When search is active, grid text is suppressed (see step 3 below)
        // so the backdrop is visible and only search text renders on top.
        if let Some(overlay) = search_overlay {
            let overlay_rects = self.search_overlay_renderer.build_overlay_rects(
                overlay.results.len(),
                overlay.selected,
                w,
                h,
            );
            rect_instances.extend(overlay_rects);
        }

        // 1d2. Proposal overlay rects (full-screen backdrop + panel) -- drawn before pipeline
        if let Some(overlay_data) = proposal_overlay {
            let (cell_w_po, cell_h_po) = self.grid_renderer.cell_size();
            let overlay_renderer = ProposalOverlayRenderer::new(cell_w_po, cell_h_po);
            let overlay_rects = overlay_renderer.build_overlay_rects(w, h, overlay_data);
            rect_instances.extend(overlay_rects);
        }

        // 1d3. Proposal toast rect (above status bar, right-aligned)
        if let Some(_toast_data) = proposal_toast {
            let (cell_w_pt, cell_h_pt) = self.grid_renderer.cell_size();
            let toast_renderer = ProposalToastRenderer::new(cell_w_pt, cell_h_pt);
            let toast_rects = toast_renderer.build_toast_rects(w, h);
            rect_instances.extend(toast_rects);
        }

        // 1d4. Onboarding toast rect (above status bar, right-aligned)
        if let Some(_onb_toast) = onboarding_toast {
            let (cell_w_ot, cell_h_ot) = self.grid_renderer.cell_size();
            let onb_renderer = OnboardingToastRenderer::new(cell_w_ot, cell_h_ot);
            let onb_rects = onb_renderer.build_toast_rects(w, h);
            rect_instances.extend(onb_rects);
        }

        // Record where background rects end (overlay rects rendered after text come next)
        let bg_rect_count = rect_instances.len() as u32;

        // 1b. Block decoration rects (separators, badges) — rendered AFTER grid text
        // so the dark background covers terminal text and labels are always readable.
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

        // 1e. Pipeline panel rects (bottom of viewport, above status bar)
        let status_bar_h = if status.is_some() {
            self.status_bar.height(two_line_status)
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
        // When search overlay is active, suppress grid text so the backdrop is visible.
        let mut text_areas: Vec<TextArea<'_>> = if search_overlay.is_some() {
            Vec::new()
        } else {
            // PERF-R01: Skip expensive font shaping when terminal content unchanged.
            if snapshot.generation != self.last_rendered_generation {
                self.text_buffers.clear();
                self.cell_positions.clear();
                self.grid_renderer.build_cell_buffers(
                    &mut self.glyph_cache.font_system,
                    snapshot,
                    &mut self.text_buffers,
                    &mut self.cell_positions,
                );
                self.last_rendered_generation = snapshot.generation;
            }
            self.grid_renderer.build_cell_text_areas_offset(
                &self.text_buffers,
                &self.cell_positions,
                width,
                height,
                0.0,
                grid_y_offset,
            )
        };

        // 3b. Build overlay text buffers for block labels and status bar.
        // Two-phase approach: first build all buffers (mutable), then create
        // text areas (immutable borrows) to satisfy the borrow checker.
        // TODO(PERF-R04): Cache overlay buffers when content is unchanged.
        // The overlay text (status bar, tab bar, block labels, search overlay,
        // proposal toast/overlay) depends on many inputs; caching would require
        // hashing all inputs to detect staleness. Deferred to a future pass.
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

        // Phase A: Build all overlay buffers (status bar, tabs, search, proposals)

        // Status bar text buffers
        if let Some(status_state) = status {
            let status_label = self.status_bar.build_status_text(
                status_state.cwd(),
                status_state.git_info(),
                update_text,
                coordination_text,
                agent_cost_text,
                agent_paused,
                agent_mode_text,
                proposal_count_text,
                w,
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
                    left: cell_width * HALF_CELL_GAP,
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
                    left: w - right_text_width - cell_width * HALF_CELL_GAP,
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
                    cell_width * SECTION_PADDING_CELLS
                } else {
                    cell_width * HALF_CELL_GAP
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

            // Agent cost text -- positioned left of coordination_text
            if let Some(ref agent_text) = status_label.agent_cost_text {
                let right_text_chars = status_label
                    .right_text
                    .as_ref()
                    .map(|t| t.len())
                    .unwrap_or(0);
                let coord_text_chars = status_label
                    .coordination_text
                    .as_ref()
                    .map(|t| t.len())
                    .unwrap_or(0);
                let coord_gap = if coord_text_chars > 0 {
                    cell_width * SECTION_PADDING_CELLS
                } else {
                    0.0
                };
                let right_gap = if right_text_chars > 0 {
                    cell_width * SECTION_PADDING_CELLS
                } else {
                    cell_width * HALF_CELL_GAP
                };
                let agent_text_width = agent_text.len() as f32 * cell_width;
                let agent_x = w
                    - (right_text_chars as f32 * cell_width)
                    - right_gap
                    - (coord_text_chars as f32 * cell_width)
                    - coord_gap
                    - cell_width  // gap between agent and coordination
                    - agent_text_width;
                let mut buffer = Buffer::new(&mut self.glyph_cache.font_system, metrics);
                buffer.set_size(
                    &mut self.glyph_cache.font_system,
                    Some(w),
                    Some(cell_height),
                );
                buffer.set_text(
                    &mut self.glyph_cache.font_system,
                    agent_text,
                    &Attrs::new()
                        .family(Family::Name(font_family))
                        .color(GlyphonColor::rgba(
                            status_label.agent_cost_color.r,
                            status_label.agent_cost_color.g,
                            status_label.agent_cost_color.b,
                            255,
                        )),
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.glyph_cache.font_system, false);
                self.overlay_buffers.push(buffer);
                overlay_metas.push(OverlayMeta {
                    left: agent_x,
                    top: status_label.y,
                    color: GlyphonColor::rgba(
                        status_label.agent_cost_color.r,
                        status_label.agent_cost_color.g,
                        status_label.agent_cost_color.b,
                        255,
                    ),
                });
            }

            // Agent mode text -- positioned left of agent_cost_text
            if let Some(ref mode_text) = status_label.agent_mode_text {
                let right_text_chars = status_label
                    .right_text
                    .as_ref()
                    .map(|t| t.len())
                    .unwrap_or(0);
                let coord_text_chars = status_label
                    .coordination_text
                    .as_ref()
                    .map(|t| t.len())
                    .unwrap_or(0);
                let agent_cost_chars = status_label
                    .agent_cost_text
                    .as_ref()
                    .map(|t| t.len())
                    .unwrap_or(0);
                let right_gap = if right_text_chars > 0 {
                    cell_width * SECTION_PADDING_CELLS
                } else {
                    cell_width * HALF_CELL_GAP
                };
                let coord_gap = if coord_text_chars > 0 {
                    cell_width * SECTION_PADDING_CELLS
                } else {
                    0.0
                };
                let cost_gap = if agent_cost_chars > 0 {
                    cell_width
                } else {
                    0.0
                };
                let mode_text_width = mode_text.len() as f32 * cell_width;
                let mode_x = w
                    - (right_text_chars as f32 * cell_width)
                    - right_gap
                    - (coord_text_chars as f32 * cell_width)
                    - coord_gap
                    - (agent_cost_chars as f32 * cell_width)
                    - cost_gap
                    - cell_width // gap between mode and cost
                    - mode_text_width;
                let mut buffer = Buffer::new(&mut self.glyph_cache.font_system, metrics);
                buffer.set_size(
                    &mut self.glyph_cache.font_system,
                    Some(w),
                    Some(cell_height),
                );
                buffer.set_text(
                    &mut self.glyph_cache.font_system,
                    mode_text,
                    &Attrs::new()
                        .family(Family::Name(font_family))
                        .color(GlyphonColor::rgba(
                            status_label.agent_mode_color.r,
                            status_label.agent_mode_color.g,
                            status_label.agent_mode_color.b,
                            255,
                        )),
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.glyph_cache.font_system, false);
                self.overlay_buffers.push(buffer);
                overlay_metas.push(OverlayMeta {
                    left: mode_x,
                    top: status_label.y,
                    color: GlyphonColor::rgba(
                        status_label.agent_mode_color.r,
                        status_label.agent_mode_color.g,
                        status_label.agent_mode_color.b,
                        255,
                    ),
                });
            }

            // Proposal count text -- positioned left of agent_mode_text
            if let Some(ref proposal_text) = status_label.proposal_count_text {
                let right_text_chars = status_label
                    .right_text
                    .as_ref()
                    .map(|t| t.len())
                    .unwrap_or(0);
                let coord_text_chars = status_label
                    .coordination_text
                    .as_ref()
                    .map(|t| t.len())
                    .unwrap_or(0);
                let agent_cost_chars = status_label
                    .agent_cost_text
                    .as_ref()
                    .map(|t| t.len())
                    .unwrap_or(0);
                let mode_chars = status_label
                    .agent_mode_text
                    .as_ref()
                    .map(|t| t.len())
                    .unwrap_or(0);
                let right_gap = if right_text_chars > 0 {
                    cell_width * SECTION_PADDING_CELLS
                } else {
                    cell_width * HALF_CELL_GAP
                };
                let coord_gap = if coord_text_chars > 0 {
                    cell_width * SECTION_PADDING_CELLS
                } else {
                    0.0
                };
                let cost_gap = if agent_cost_chars > 0 {
                    cell_width
                } else {
                    0.0
                };
                let mode_gap = if mode_chars > 0 { cell_width } else { 0.0 };
                let proposal_text_width = proposal_text.len() as f32 * cell_width;
                let proposal_x = w
                    - (right_text_chars as f32 * cell_width)
                    - right_gap
                    - (coord_text_chars as f32 * cell_width)
                    - coord_gap
                    - (agent_cost_chars as f32 * cell_width)
                    - cost_gap
                    - (mode_chars as f32 * cell_width)
                    - mode_gap
                    - cell_width // gap between proposal and mode
                    - proposal_text_width;
                let mut buffer = Buffer::new(&mut self.glyph_cache.font_system, metrics);
                buffer.set_size(
                    &mut self.glyph_cache.font_system,
                    Some(w),
                    Some(cell_height),
                );
                buffer.set_text(
                    &mut self.glyph_cache.font_system,
                    proposal_text,
                    &Attrs::new()
                        .family(Family::Name(font_family))
                        .color(GlyphonColor::rgba(
                            status_label.proposal_count_color.r,
                            status_label.proposal_count_color.g,
                            status_label.proposal_count_color.b,
                            255,
                        )),
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.glyph_cache.font_system, false);
                self.overlay_buffers.push(buffer);
                overlay_metas.push(OverlayMeta {
                    left: proposal_x,
                    top: status_label.y,
                    color: GlyphonColor::rgba(
                        status_label.proposal_count_color.r,
                        status_label.proposal_count_color.g,
                        status_label.proposal_count_color.b,
                        255,
                    ),
                });
            }

            // Center text (update notification / onboarding tip)
            // Only show if there is enough horizontal space between left CWD and right-side items.
            if let Some(ref center_text) = status_label.center_text {
                let left_text_width = status_label.left_text.len() as f32 * cell_width + cell_width;
                let right_side_width = {
                    let mut rw = 0.0f32;
                    if let Some(ref rt) = status_label.right_text {
                        rw += rt.len() as f32 * cell_width + cell_width * SECTION_PADDING_CELLS;
                    }
                    if let Some(ref ct) = status_label.coordination_text {
                        rw += ct.len() as f32 * cell_width + cell_width * SECTION_PADDING_CELLS;
                    }
                    if let Some(ref at) = status_label.agent_cost_text {
                        rw += at.len() as f32 * cell_width + cell_width;
                    }
                    if let Some(ref mt) = status_label.agent_mode_text {
                        rw += mt.len() as f32 * cell_width + cell_width;
                    }
                    if let Some(ref pt) = status_label.proposal_count_text {
                        rw += pt.len() as f32 * cell_width + cell_width;
                    }
                    rw
                };
                let center_text_width = center_text.len() as f32 * cell_width;
                let center_x = (w - center_text_width) / 2.0;
                let right_items_start = w - right_side_width;
                // Check actual pixel positions: center text must not overlap left OR right items
                if center_x > left_text_width && center_x + center_text_width < right_items_start {
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

            // Agent activity line (top row of two-line status bar)
            if let Some(activity_text) = agent_activity_line {
                let activity_y = status_label.y - cell_height;
                let activity_color = ACTIVITY_STREAM_PURPLE;
                let mut buffer = Buffer::new(&mut self.glyph_cache.font_system, metrics);
                buffer.set_size(
                    &mut self.glyph_cache.font_system,
                    Some(w),
                    Some(cell_height),
                );
                buffer.set_text(
                    &mut self.glyph_cache.font_system,
                    activity_text,
                    &Attrs::new()
                        .family(Family::Name(font_family))
                        .color(activity_color),
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.glyph_cache.font_system, false);
                self.overlay_buffers.push(buffer);
                overlay_metas.push(OverlayMeta {
                    left: cell_width * HALF_CELL_GAP,
                    top: activity_y,
                    color: activity_color,
                });

                // Expand hint at far right of agent line
                let hint = "Ctrl+Shift+G";
                let hint_width = hint.len() as f32 * cell_width;
                let hint_color = HINT_TEXT_GRAY;
                let mut hint_buf = Buffer::new(&mut self.glyph_cache.font_system, metrics);
                hint_buf.set_size(
                    &mut self.glyph_cache.font_system,
                    Some(w),
                    Some(cell_height),
                );
                hint_buf.set_text(
                    &mut self.glyph_cache.font_system,
                    hint,
                    &Attrs::new()
                        .family(Family::Name(font_family))
                        .color(hint_color),
                    Shaping::Advanced,
                    None,
                );
                hint_buf.shape_until_scroll(&mut self.glyph_cache.font_system, false);
                self.overlay_buffers.push(hint_buf);
                overlay_metas.push(OverlayMeta {
                    left: w - hint_width - cell_width * HALF_CELL_GAP,
                    top: activity_y,
                    color: hint_color,
                });
            }
        }

        // Tab bar text buffers
        if let Some(tabs) = tab_bar_info {
            let tab_labels = self.tab_bar.build_tab_text(tabs, w, hovered_tab);
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

        // Proposal overlay text buffers
        if let Some(overlay_data) = proposal_overlay {
            let (cell_w_po, cell_h_po) = self.grid_renderer.cell_size();
            let overlay_renderer = ProposalOverlayRenderer::new(cell_w_po, cell_h_po);
            let overlay_labels = overlay_renderer.build_overlay_text(w, h, overlay_data);
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

        // Proposal toast text buffers
        if let Some(toast_data) = proposal_toast {
            let (cell_w_pt, cell_h_pt) = self.grid_renderer.cell_size();
            let toast_renderer = ProposalToastRenderer::new(cell_w_pt, cell_h_pt);
            let toast_labels = toast_renderer.build_toast_text(toast_data, w, h);
            for label in &toast_labels {
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

        // Onboarding toast text buffers
        if let Some(onb_data) = onboarding_toast {
            let (cell_w_ot, cell_h_ot) = self.grid_renderer.cell_size();
            let onb_renderer = OnboardingToastRenderer::new(cell_w_ot, cell_h_ot);
            let onb_labels = onb_renderer.build_toast_text(onb_data, w, h);
            for label in &onb_labels {
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

        // Build block decoration + pipeline label buffers (rendered in second pass,
        // after grid text, so decorations are always readable over terminal content)
        self.pipeline_buffers.clear();
        let mut pipeline_metas: Vec<OverlayMeta> = Vec::new();

        // Block label text (duration, [undo], OK/X badge, SOI summary)
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
                self.pipeline_buffers.push(buffer);
                pipeline_metas.push(OverlayMeta {
                    left: label.x,
                    top: label.y + grid_y_offset,
                    color: GlyphonColor::rgba(label.color.r, label.color.g, label.color.b, 255),
                });
            }
        }

        // Pipeline panel text
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

        // 5. Prepare text renderer (grid + status bar — NO block labels or pipeline labels)
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

            // 7. Draw background rects (grid bg + status bar + tab bar + overlays)
            self.rect_renderer.render(&mut pass, bg_rect_count);

            // 8. Draw text (grid + status bar — no block labels)
            if let Err(e) = self.glyph_cache.text_renderer.render(
                &self.glyph_cache.atlas,
                &self.glyph_cache.viewport,
                &mut pass,
            ) {
                tracing::warn!("Text render error: {:?}", e);
            }

            // 9. Draw block decoration + pipeline overlay rects ON TOP of grid text
            self.rect_renderer
                .render_range(&mut pass, bg_rect_count, total_rect_count);
        }

        // Submit pass 1
        queue.submit([encoder.finish()]);

        // 10. Second pass for block decoration + pipeline label text (on top of overlay rects)
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

        // Welcome overlay: separate render pass on top of everything (like settings/activity overlays)
        if let Some(welcome_data) = welcome_overlay {
            self.draw_welcome_overlay(device, queue, view, width, height, welcome_data);
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
        hovered_tab: Option<usize>,
        drop_index: Option<usize>,
        update_text: Option<&str>,
        coordination_text: Option<&str>,
        agent_cost_text: Option<&str>,
        agent_paused: bool,
        scrollbar_state: &[(bool, bool)],
        agent_mode_text: Option<&str>,
        proposal_count_text: Option<&str>,
        proposal_toast: Option<&ProposalToastRenderData>,
        proposal_overlay: Option<&ProposalOverlayRenderData>,
        agent_activity_line: Option<&str>,
        orchestrating: bool,
        onboarding_toast: Option<&OnboardingToastRenderData>,
        welcome_overlay: Option<&WelcomeOverlayRenderData>,
    ) {
        let w = width as f32;
        let h = height as f32;
        let two_line_status = agent_activity_line.is_some();

        // 1. Build rect instances for all panes (with viewport offsets)
        let mut rect_instances: Vec<crate::rect_renderer::RectInstance> = Vec::new();

        for (i, (viewport, snapshot, _blocks, is_focused)) in panes.iter().enumerate() {
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

            // Scrollbar rects for this pane
            {
                let (sb_hovered, sb_dragging) =
                    scrollbar_state.get(i).copied().unwrap_or((false, false));
                let scrollbar_rects = self.scrollbar.build_scrollbar_rects(
                    (viewport.x + viewport.width) as f32,
                    viewport.y as f32,
                    viewport.height as f32,
                    snapshot.display_offset,
                    snapshot.history_size,
                    snapshot.screen_lines,
                    sb_hovered,
                    sb_dragging,
                );
                rect_instances.extend(scrollbar_rects);
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
            let status_rects = if two_line_status {
                self.status_bar
                    .build_status_rects_two_line(w, h, orchestrating)
            } else {
                self.status_bar.build_status_rects(w, h, orchestrating)
            };
            rect_instances.extend(status_rects);
        }

        // Tab bar rects
        if let Some(tabs) = tab_bar_info {
            let tab_rects = self
                .tab_bar
                .build_tab_rects(tabs, w, hovered_tab, drop_index);
            rect_instances.extend(tab_rects);
        }

        // Proposal overlay rects (window-global, rendered once after all panes)
        if let Some(overlay_data) = proposal_overlay {
            let (cell_w_po, cell_h_po) = self.grid_renderer.cell_size();
            let overlay_renderer = ProposalOverlayRenderer::new(cell_w_po, cell_h_po);
            let overlay_rects = overlay_renderer.build_overlay_rects(w, h, overlay_data);
            rect_instances.extend(overlay_rects);
        }

        // Proposal toast rect (window-global, above status bar)
        if let Some(_toast_data) = proposal_toast {
            let (cell_w_pt, cell_h_pt) = self.grid_renderer.cell_size();
            let toast_renderer = ProposalToastRenderer::new(cell_w_pt, cell_h_pt);
            let toast_rects = toast_renderer.build_toast_rects(w, h);
            rect_instances.extend(toast_rects);
        }

        // Onboarding toast rect (window-global, above status bar)
        if let Some(_onb_toast) = onboarding_toast {
            let (cell_w_ot, cell_h_ot) = self.grid_renderer.cell_size();
            let onb_renderer = OnboardingToastRenderer::new(cell_w_ot, cell_h_ot);
            let onb_rects = onb_renderer.build_toast_rects(w, h);
            rect_instances.extend(onb_rects);
        }

        let total_rect_count = rect_instances.len() as u32;

        // 2. Prepare rect renderer
        self.rect_renderer
            .prepare(device, queue, &rect_instances, width, height);

        // 3. Build per-cell text buffers for all panes
        // We need separate buffer storage per pane since they have different offsets
        // PERF-R01: Skip rebuild if no pane snapshot has changed since last render.
        let max_generation = panes
            .iter()
            .map(|(_, snap, _, _)| snap.generation)
            .max()
            .unwrap_or(0);
        if max_generation != self.last_rendered_generation {
            self.text_buffers.clear();
            self.cell_positions.clear();
            self.cached_pane_ranges.clear();
            for (_viewport, snapshot, _blocks, _is_focused) in panes.iter() {
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
                self.cached_pane_ranges
                    .push((buf_start, buf_end, pos_start, pos_end));
            }
            self.last_rendered_generation = max_generation;
        }
        let mut text_areas: Vec<TextArea<'_>> = Vec::new();
        let pane_ranges = &self.cached_pane_ranges;

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
                agent_cost_text,
                agent_paused,
                agent_mode_text,
                proposal_count_text,
                w,
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
                    left: cell_width * HALF_CELL_GAP,
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
                    left: w - right_text_width - cell_width * HALF_CELL_GAP,
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
                    cell_width * SECTION_PADDING_CELLS
                } else {
                    cell_width * HALF_CELL_GAP
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

            // Agent cost text -- positioned left of coordination_text (multi-pane)
            if let Some(ref agent_text) = status_label.agent_cost_text {
                let right_text_chars = status_label
                    .right_text
                    .as_ref()
                    .map(|t| t.len())
                    .unwrap_or(0);
                let coord_text_chars = status_label
                    .coordination_text
                    .as_ref()
                    .map(|t| t.len())
                    .unwrap_or(0);
                let coord_gap = if coord_text_chars > 0 {
                    cell_width * SECTION_PADDING_CELLS
                } else {
                    0.0
                };
                let right_gap = if right_text_chars > 0 {
                    cell_width * SECTION_PADDING_CELLS
                } else {
                    cell_width * HALF_CELL_GAP
                };
                let agent_text_width = agent_text.len() as f32 * cell_width;
                let agent_x = w
                    - (right_text_chars as f32 * cell_width)
                    - right_gap
                    - (coord_text_chars as f32 * cell_width)
                    - coord_gap
                    - cell_width
                    - agent_text_width;
                let mut buffer = Buffer::new(&mut self.glyph_cache.font_system, metrics);
                buffer.set_size(
                    &mut self.glyph_cache.font_system,
                    Some(w),
                    Some(cell_height),
                );
                buffer.set_text(
                    &mut self.glyph_cache.font_system,
                    agent_text,
                    &Attrs::new()
                        .family(Family::Name(font_family))
                        .color(GlyphonColor::rgba(
                            status_label.agent_cost_color.r,
                            status_label.agent_cost_color.g,
                            status_label.agent_cost_color.b,
                            255,
                        )),
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.glyph_cache.font_system, false);
                self.overlay_buffers.push(buffer);
                overlay_metas.push(OverlayMeta {
                    left: agent_x,
                    top: status_label.y,
                    color: GlyphonColor::rgba(
                        status_label.agent_cost_color.r,
                        status_label.agent_cost_color.g,
                        status_label.agent_cost_color.b,
                        255,
                    ),
                });
            }

            // Agent mode text -- positioned left of agent_cost_text (multi-pane)
            if let Some(ref mode_text) = status_label.agent_mode_text {
                let right_text_chars = status_label
                    .right_text
                    .as_ref()
                    .map(|t| t.len())
                    .unwrap_or(0);
                let coord_text_chars = status_label
                    .coordination_text
                    .as_ref()
                    .map(|t| t.len())
                    .unwrap_or(0);
                let agent_cost_chars = status_label
                    .agent_cost_text
                    .as_ref()
                    .map(|t| t.len())
                    .unwrap_or(0);
                let right_gap = if right_text_chars > 0 {
                    cell_width * SECTION_PADDING_CELLS
                } else {
                    cell_width * HALF_CELL_GAP
                };
                let coord_gap = if coord_text_chars > 0 {
                    cell_width * SECTION_PADDING_CELLS
                } else {
                    0.0
                };
                let cost_gap = if agent_cost_chars > 0 {
                    cell_width
                } else {
                    0.0
                };
                let mode_text_width = mode_text.len() as f32 * cell_width;
                let mode_x = w
                    - (right_text_chars as f32 * cell_width)
                    - right_gap
                    - (coord_text_chars as f32 * cell_width)
                    - coord_gap
                    - (agent_cost_chars as f32 * cell_width)
                    - cost_gap
                    - cell_width
                    - mode_text_width;
                let mut buffer = Buffer::new(&mut self.glyph_cache.font_system, metrics);
                buffer.set_size(
                    &mut self.glyph_cache.font_system,
                    Some(w),
                    Some(cell_height),
                );
                buffer.set_text(
                    &mut self.glyph_cache.font_system,
                    mode_text,
                    &Attrs::new()
                        .family(Family::Name(font_family))
                        .color(GlyphonColor::rgba(
                            status_label.agent_mode_color.r,
                            status_label.agent_mode_color.g,
                            status_label.agent_mode_color.b,
                            255,
                        )),
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.glyph_cache.font_system, false);
                self.overlay_buffers.push(buffer);
                overlay_metas.push(OverlayMeta {
                    left: mode_x,
                    top: status_label.y,
                    color: GlyphonColor::rgba(
                        status_label.agent_mode_color.r,
                        status_label.agent_mode_color.g,
                        status_label.agent_mode_color.b,
                        255,
                    ),
                });
            }

            // Proposal count text -- positioned left of agent_mode_text (multi-pane)
            if let Some(ref proposal_text) = status_label.proposal_count_text {
                let right_text_chars = status_label
                    .right_text
                    .as_ref()
                    .map(|t| t.len())
                    .unwrap_or(0);
                let coord_text_chars = status_label
                    .coordination_text
                    .as_ref()
                    .map(|t| t.len())
                    .unwrap_or(0);
                let agent_cost_chars = status_label
                    .agent_cost_text
                    .as_ref()
                    .map(|t| t.len())
                    .unwrap_or(0);
                let mode_chars = status_label
                    .agent_mode_text
                    .as_ref()
                    .map(|t| t.len())
                    .unwrap_or(0);
                let right_gap = if right_text_chars > 0 {
                    cell_width * SECTION_PADDING_CELLS
                } else {
                    cell_width * HALF_CELL_GAP
                };
                let coord_gap = if coord_text_chars > 0 {
                    cell_width * SECTION_PADDING_CELLS
                } else {
                    0.0
                };
                let cost_gap = if agent_cost_chars > 0 {
                    cell_width
                } else {
                    0.0
                };
                let mode_gap = if mode_chars > 0 { cell_width } else { 0.0 };
                let proposal_text_width = proposal_text.len() as f32 * cell_width;
                let proposal_x = w
                    - (right_text_chars as f32 * cell_width)
                    - right_gap
                    - (coord_text_chars as f32 * cell_width)
                    - coord_gap
                    - (agent_cost_chars as f32 * cell_width)
                    - cost_gap
                    - (mode_chars as f32 * cell_width)
                    - mode_gap
                    - cell_width
                    - proposal_text_width;
                let mut buffer = Buffer::new(&mut self.glyph_cache.font_system, metrics);
                buffer.set_size(
                    &mut self.glyph_cache.font_system,
                    Some(w),
                    Some(cell_height),
                );
                buffer.set_text(
                    &mut self.glyph_cache.font_system,
                    proposal_text,
                    &Attrs::new()
                        .family(Family::Name(font_family))
                        .color(GlyphonColor::rgba(
                            status_label.proposal_count_color.r,
                            status_label.proposal_count_color.g,
                            status_label.proposal_count_color.b,
                            255,
                        )),
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.glyph_cache.font_system, false);
                self.overlay_buffers.push(buffer);
                overlay_metas.push(OverlayMeta {
                    left: proposal_x,
                    top: status_label.y,
                    color: GlyphonColor::rgba(
                        status_label.proposal_count_color.r,
                        status_label.proposal_count_color.g,
                        status_label.proposal_count_color.b,
                        255,
                    ),
                });
            }

            // Center text (update notification / onboarding tip) -- multi-pane path
            // Only show if there is enough horizontal space between left CWD and right-side items.
            if let Some(ref center_text) = status_label.center_text {
                let left_text_width = status_label.left_text.len() as f32 * cell_width + cell_width;
                let right_side_width = {
                    let mut rw = 0.0f32;
                    if let Some(ref rt) = status_label.right_text {
                        rw += rt.len() as f32 * cell_width + cell_width * SECTION_PADDING_CELLS;
                    }
                    if let Some(ref ct) = status_label.coordination_text {
                        rw += ct.len() as f32 * cell_width + cell_width * SECTION_PADDING_CELLS;
                    }
                    if let Some(ref at) = status_label.agent_cost_text {
                        rw += at.len() as f32 * cell_width + cell_width;
                    }
                    if let Some(ref mt) = status_label.agent_mode_text {
                        rw += mt.len() as f32 * cell_width + cell_width;
                    }
                    if let Some(ref pt) = status_label.proposal_count_text {
                        rw += pt.len() as f32 * cell_width + cell_width;
                    }
                    rw
                };
                let center_text_width = center_text.len() as f32 * cell_width;
                let center_x = (w - center_text_width) / 2.0;
                let right_items_start = w - right_side_width;
                // Check actual pixel positions: center text must not overlap left OR right items
                if center_x > left_text_width && center_x + center_text_width < right_items_start {
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

            // Agent activity line (top row of two-line status bar)
            if let Some(activity_text) = agent_activity_line {
                let activity_y = status_label.y - cell_height;
                let activity_color = ACTIVITY_STREAM_PURPLE;
                let mut buffer = Buffer::new(&mut self.glyph_cache.font_system, metrics);
                buffer.set_size(
                    &mut self.glyph_cache.font_system,
                    Some(w),
                    Some(cell_height),
                );
                buffer.set_text(
                    &mut self.glyph_cache.font_system,
                    activity_text,
                    &Attrs::new()
                        .family(Family::Name(font_family))
                        .color(activity_color),
                    Shaping::Advanced,
                    None,
                );
                buffer.shape_until_scroll(&mut self.glyph_cache.font_system, false);
                self.overlay_buffers.push(buffer);
                overlay_metas.push(OverlayMeta {
                    left: cell_width * HALF_CELL_GAP,
                    top: activity_y,
                    color: activity_color,
                });

                // Expand hint
                let hint = "Ctrl+Shift+G";
                let hint_width = hint.len() as f32 * cell_width;
                let hint_color = HINT_TEXT_GRAY;
                let mut hint_buf = Buffer::new(&mut self.glyph_cache.font_system, metrics);
                hint_buf.set_size(
                    &mut self.glyph_cache.font_system,
                    Some(w),
                    Some(cell_height),
                );
                hint_buf.set_text(
                    &mut self.glyph_cache.font_system,
                    hint,
                    &Attrs::new()
                        .family(Family::Name(font_family))
                        .color(hint_color),
                    Shaping::Advanced,
                    None,
                );
                hint_buf.shape_until_scroll(&mut self.glyph_cache.font_system, false);
                self.overlay_buffers.push(hint_buf);
                overlay_metas.push(OverlayMeta {
                    left: w - hint_width - cell_width * HALF_CELL_GAP,
                    top: activity_y,
                    color: hint_color,
                });
            }
        }

        // Tab bar text
        if let Some(tabs) = tab_bar_info {
            let tab_labels = self.tab_bar.build_tab_text(tabs, w, hovered_tab);
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

        // Proposal overlay text buffers (window-global, after tab bar)
        if let Some(overlay_data) = proposal_overlay {
            let (cell_w_po, cell_h_po) = self.grid_renderer.cell_size();
            let overlay_renderer = ProposalOverlayRenderer::new(cell_w_po, cell_h_po);
            let overlay_labels = overlay_renderer.build_overlay_text(w, h, overlay_data);
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

        // Proposal toast text buffers (window-global, after tab bar)
        if let Some(toast_data) = proposal_toast {
            let (cell_w_pt, cell_h_pt) = self.grid_renderer.cell_size();
            let toast_renderer = ProposalToastRenderer::new(cell_w_pt, cell_h_pt);
            let toast_labels = toast_renderer.build_toast_text(toast_data, w, h);
            for label in &toast_labels {
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

        // Onboarding toast text buffers (window-global, after tab bar)
        if let Some(onb_data) = onboarding_toast {
            let (cell_w_ot, cell_h_ot) = self.grid_renderer.cell_size();
            let onb_renderer = OnboardingToastRenderer::new(cell_w_ot, cell_h_ot);
            let onb_labels = onb_renderer.build_toast_text(onb_data, w, h);
            for label in &onb_labels {
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

        // Welcome overlay: separate render pass on top of everything (like settings/activity overlays)
        if let Some(welcome_data) = welcome_overlay {
            self.draw_welcome_overlay(device, queue, view, width, height, welcome_data);
        }
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

    /// Draw the activity stream overlay (fullscreen, on top of everything).
    ///
    /// Uses LoadOp::Load to preserve the existing frame content underneath.
    /// Must be called AFTER draw_frame/draw_multi_pane_frame (reuses rect_renderer).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_activity_overlay(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        data: &crate::activity_overlay::ActivityOverlayRenderData,
    ) {
        let (cell_width, cell_height) = self.grid_renderer.cell_size();
        let overlay =
            crate::activity_overlay::ActivityOverlayRenderer::new(cell_width, cell_height);

        // 1. Backdrop rect
        let backdrop = overlay.build_backdrop_rect(width as f32, height as f32);
        self.rect_renderer
            .prepare(device, queue, &[backdrop], width, height);

        // 2. Text labels
        let labels = if data.filter == crate::activity_overlay::ActivityViewFilter::Orchestrator {
            overlay.build_orchestrator_text(data, width as f32, height as f32)
        } else {
            overlay.build_overlay_text(data, width as f32, height as f32)
        };

        // 3. Build per-label text buffers
        let physical_font_size = self.grid_renderer.font_size * self.grid_renderer.scale_factor;
        let metrics = Metrics::new(physical_font_size, cell_height);
        let font_family = &self.grid_renderer.font_family;

        let mut activity_buffers: Vec<Buffer> = Vec::with_capacity(labels.len());
        for label in &labels {
            let mut buffer = Buffer::new(&mut self.glyph_cache.font_system, metrics);
            buffer.set_size(
                &mut self.glyph_cache.font_system,
                Some(width as f32 - label.x),
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
            activity_buffers.push(buffer);
        }

        // 4. Build text areas referencing the buffers
        let text_areas: Vec<TextArea<'_>> = labels
            .iter()
            .zip(activity_buffers.iter())
            .map(|(label, buffer)| TextArea {
                buffer,
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

        // 5. Prepare text renderer
        self.glyph_cache
            .viewport
            .update(queue, Resolution { width, height });

        if let Err(e) = self.glyph_cache.text_renderer.prepare(
            device,
            queue,
            &mut self.glyph_cache.font_system,
            &mut self.glyph_cache.atlas,
            &self.glyph_cache.viewport,
            text_areas,
            &mut self.glyph_cache.swash_cache,
        ) {
            tracing::warn!("Activity overlay text prepare error: {:?}", e);
        }

        // 6. Render pass: rects then text
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("activity_overlay_pass"),
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
            self.rect_renderer.render(&mut pass, 1);
            if let Err(e) = self.glyph_cache.text_renderer.render(
                &self.glyph_cache.atlas,
                &self.glyph_cache.viewport,
                &mut pass,
            ) {
                tracing::warn!("Activity overlay text render error: {:?}", e);
            }
        }
        queue.submit([encoder.finish()]);
    }

    /// Draw the settings overlay (fullscreen, on top of everything).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_settings_overlay(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        data: &crate::settings_overlay::SettingsOverlayRenderData,
    ) {
        let (cell_width, cell_height) = self.grid_renderer.cell_size();
        let overlay =
            crate::settings_overlay::SettingsOverlayRenderer::new(cell_width, cell_height);

        // 1. Backdrop rect
        let backdrop = overlay.build_backdrop_rect(width as f32, height as f32);
        self.rect_renderer
            .prepare(device, queue, &[backdrop], width, height);

        // 2. Build text labels: header + active tab content
        let mut all_labels = overlay.build_header_text(data.tab, width as f32);
        match data.tab {
            crate::settings_overlay::SettingsTab::Settings => {
                all_labels.extend(overlay.build_settings_text(
                    width as f32,
                    height as f32,
                    &data.config,
                    data.section_index,
                    data.field_index,
                    data.editing,
                    &data.edit_buffer,
                ));
            }
            crate::settings_overlay::SettingsTab::Shortcuts => {
                all_labels.extend(overlay.build_shortcuts_text(
                    width as f32,
                    height as f32,
                    data.shortcuts_scroll,
                ));
            }
            crate::settings_overlay::SettingsTab::About => {
                all_labels.extend(overlay.build_about_text(width as f32, height as f32));
            }
        }

        // 3-6. Build buffers, prepare, render (identical to draw_activity_overlay)
        let physical_font_size = self.grid_renderer.font_size * self.grid_renderer.scale_factor;
        let metrics = Metrics::new(physical_font_size, cell_height);
        let font_family = &self.grid_renderer.font_family;

        let mut settings_buffers: Vec<Buffer> = Vec::with_capacity(all_labels.len());
        for label in &all_labels {
            let mut buffer = Buffer::new(&mut self.glyph_cache.font_system, metrics);
            buffer.set_size(
                &mut self.glyph_cache.font_system,
                Some(width as f32 - label.x),
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
            settings_buffers.push(buffer);
        }

        let text_areas: Vec<TextArea<'_>> = all_labels
            .iter()
            .zip(settings_buffers.iter())
            .map(|(label, buffer)| TextArea {
                buffer,
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
            text_areas,
            &mut self.glyph_cache.swash_cache,
        ) {
            tracing::warn!("Settings overlay text prepare error: {:?}", e);
        }

        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("settings_overlay_pass"),
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
            self.rect_renderer.render(&mut pass, 1);
            if let Err(e) = self.glyph_cache.text_renderer.render(
                &self.glyph_cache.atlas,
                &self.glyph_cache.viewport,
                &mut pass,
            ) {
                tracing::warn!("Settings overlay text render error: {:?}", e);
            }
        }
        queue.submit([encoder.finish()]);
    }

    /// Draw the welcome overlay (fullscreen, on top of everything).
    ///
    /// Uses LoadOp::Load to preserve the existing frame content underneath.
    /// Must be called AFTER draw_frame/draw_multi_pane_frame (reuses rect_renderer).
    fn draw_welcome_overlay(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        data: &WelcomeOverlayRenderData,
    ) {
        let (cell_width, cell_height) = self.grid_renderer.cell_size();
        let overlay = WelcomeOverlayRenderer::new(cell_width, cell_height);

        // 1. Build rects (backdrop + panel)
        let rects = overlay.build_rects(width as f32, height as f32);
        let rect_count = rects.len() as u32;
        self.rect_renderer
            .prepare(device, queue, &rects, width, height);

        // 2. Build text labels
        let labels = overlay.build_text(data, width as f32, height as f32);

        // 3. Build per-label text buffers
        let physical_font_size = self.grid_renderer.font_size * self.grid_renderer.scale_factor;
        let metrics = Metrics::new(physical_font_size, cell_height);
        let font_family = &self.grid_renderer.font_family;

        let mut welcome_buffers: Vec<Buffer> = Vec::with_capacity(labels.len());
        for label in &labels {
            let mut buffer = Buffer::new(&mut self.glyph_cache.font_system, metrics);
            buffer.set_size(
                &mut self.glyph_cache.font_system,
                Some(width as f32 - label.x),
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
            welcome_buffers.push(buffer);
        }

        // 4. Build text areas referencing the buffers
        let text_areas: Vec<TextArea<'_>> = labels
            .iter()
            .zip(welcome_buffers.iter())
            .map(|(label, buffer)| TextArea {
                buffer,
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

        // 5. Prepare text renderer
        self.glyph_cache
            .viewport
            .update(queue, Resolution { width, height });

        if let Err(e) = self.glyph_cache.text_renderer.prepare(
            device,
            queue,
            &mut self.glyph_cache.font_system,
            &mut self.glyph_cache.atlas,
            &self.glyph_cache.viewport,
            text_areas,
            &mut self.glyph_cache.swash_cache,
        ) {
            tracing::warn!("Welcome overlay text prepare error: {:?}", e);
        }

        // 6. Render pass: rects then text
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("welcome_overlay_pass"),
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
            self.rect_renderer.render(&mut pass, rect_count);
            if let Err(e) = self.glyph_cache.text_renderer.render(
                &self.glyph_cache.atlas,
                &self.glyph_cache.viewport,
                &mut pass,
            ) {
                tracing::warn!("Welcome overlay text render error: {:?}", e);
            }
        }
        queue.submit([encoder.finish()]);
    }

    /// Draw a centered toast notification on top of existing frame content.
    ///
    /// Renders a small dark rect centered on the viewport with the given text.
    /// Uses LoadOp::Load to preserve the existing frame content underneath.
    /// Must be called AFTER draw_frame/draw_multi_pane_frame (reuses rect_renderer).
    pub fn draw_centered_toast(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view: &wgpu::TextureView,
        width: u32,
        height: u32,
        text: &str,
    ) {
        let (cell_width, cell_height) = self.grid_renderer.cell_size();

        // Calculate box dimensions: text width + 3 cells padding X, 0.5 cells padding Y
        let text_px_width = text.len() as f32 * cell_width;
        let box_w = text_px_width + cell_width * 3.0;
        let box_h = cell_height + cell_height * 0.5;

        // Center on screen
        let box_x = (width as f32 - box_w) * 0.5;
        let box_y = (height as f32 - box_h) * 0.5;

        let toast_rect = crate::rect_renderer::RectInstance {
            pos: [box_x, box_y, box_w, box_h],
            color: [0.1, 0.1, 0.1, 0.85],
        };

        self.rect_renderer
            .prepare(device, queue, &[toast_rect], width, height);

        // Build text buffer
        let physical_font_size = self.grid_renderer.font_size * self.grid_renderer.scale_factor;
        let metrics = Metrics::new(physical_font_size, cell_height);
        let font_family = &self.grid_renderer.font_family;

        let mut toast_buffer = Buffer::new(&mut self.glyph_cache.font_system, metrics);
        toast_buffer.set_size(&mut self.glyph_cache.font_system, Some(box_w), Some(box_h));
        toast_buffer.set_text(
            &mut self.glyph_cache.font_system,
            text,
            &Attrs::new()
                .family(Family::Name(font_family))
                .color(TEXT_WHITE),
            Shaping::Advanced,
            None,
        );
        toast_buffer.shape_until_scroll(&mut self.glyph_cache.font_system, false);

        let text_x = box_x + cell_width * 1.5;
        let text_y = box_y + cell_height * 0.25;

        let toast_text_areas = vec![TextArea {
            buffer: &toast_buffer,
            left: text_x,
            top: text_y,
            scale: 1.0,
            bounds: TextBounds {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            },
            default_color: TEXT_WHITE,
            custom_glyphs: &[],
        }];

        self.glyph_cache
            .viewport
            .update(queue, Resolution { width, height });

        if let Err(e) = self.glyph_cache.text_renderer.prepare(
            device,
            queue,
            &mut self.glyph_cache.font_system,
            &mut self.glyph_cache.atlas,
            &self.glyph_cache.viewport,
            toast_text_areas,
            &mut self.glyph_cache.swash_cache,
        ) {
            tracing::warn!("Centered toast text prepare error: {:?}", e);
        }

        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("centered_toast_pass"),
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
            self.rect_renderer.render(&mut pass, 1);
            if let Err(e) = self.glyph_cache.text_renderer.render(
                &self.glyph_cache.atlas,
                &self.glyph_cache.viewport,
                &mut pass,
            ) {
                tracing::warn!("Centered toast text render error: {:?}", e);
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
