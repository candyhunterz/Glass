mod history;

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Term, TermMode};
use clap::{Parser, Subcommand};
use glass_core::config::GlassConfig;
use glass_core::event::{AppEvent, GitStatus, SessionId, ShellEvent};
use glass_history::{resolve_db_path, db::{HistoryDb, CommandRecord}};
use glass_mux::{Session, SessionMux, SearchOverlay};
use glass_renderer::{FontSystem, FrameRenderer, GlassRenderer};
use glass_terminal::{
    BlockManager, DefaultColors, EventProxy, OscEvent, PipelineHit, PtyMsg, PtySender, StatusState,
    encode_key, query_git_status, snapshot_term,
};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

// ---------------------------------------------------------------------------
// CLI definition (clap derive)
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "glass", version, about = "GPU-accelerated terminal emulator")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug, PartialEq)]
enum Commands {
    /// Query command history
    History {
        #[command(subcommand)]
        action: Option<HistoryAction>,
    },
    /// MCP server commands
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
    /// Undo a specific command's file changes
    Undo {
        /// The command ID to undo
        command_id: i64,
    },
}

#[derive(Subcommand, Debug, PartialEq)]
enum HistoryAction {
    /// Search command history by text
    Search {
        /// Search term (FTS5 query)
        query: String,
        #[command(flatten)]
        filters: HistoryFilters,
    },
    /// List recent commands
    List {
        #[command(flatten)]
        filters: HistoryFilters,
    },
}

#[derive(clap::Args, Debug, PartialEq, Default)]
struct HistoryFilters {
    /// Filter by exit code
    #[arg(long)]
    exit: Option<i32>,
    /// Only show commands after this time (e.g. 1h, 2d, 2024-01-15)
    #[arg(long)]
    after: Option<String>,
    /// Only show commands before this time (e.g. 1h, 2d, 2024-01-15)
    #[arg(long)]
    before: Option<String>,
    /// Filter by working directory prefix
    #[arg(long)]
    cwd: Option<String>,
    /// Maximum number of results
    #[arg(long, short = 'n', default_value_t = 25)]
    limit: usize,
}

#[derive(Subcommand, Debug, PartialEq)]
enum McpAction {
    /// Start the MCP server over stdio
    Serve,
}

/// Simple grid dimensions for Term::resize().
struct TermDimensions {
    columns: usize,
    screen_lines: usize,
}

impl Dimensions for TermDimensions {
    fn total_lines(&self) -> usize {
        self.screen_lines
    }
    fn screen_lines(&self) -> usize {
        self.screen_lines
    }
    fn columns(&self) -> usize {
        self.columns
    }
}

/// Per-window state: OS window handle, GPU renderer, and session multiplexer.
///
/// All terminal state (PTY, grid, block manager, history, etc.) lives inside
/// `Session` via `SessionMux`. WindowContext is thin: window + renderer + mux.
struct WindowContext {
    window: Arc<Window>,
    renderer: GlassRenderer,
    /// GPU text rendering pipeline.
    frame_renderer: FrameRenderer,
    /// Session multiplexer managing terminal sessions (single-session in Phase 21).
    session_mux: SessionMux,
    /// Whether the first-frame cold start metric has been logged.
    first_frame_logged: bool,
}

impl WindowContext {
    /// Get an immutable reference to the focused session.
    fn session(&self) -> &Session {
        self.session_mux.focused_session().expect("no focused session")
    }

    /// Get a mutable reference to the focused session.
    fn session_mut(&mut self) -> &mut Session {
        self.session_mux.focused_session_mut().expect("no focused session")
    }
}

/// Top-level application state. Holds all open windows.
///
/// The proxy is created from `EventLoop<AppEvent>` before `run_app()` is called,
/// because `ActiveEventLoop` (passed in callbacks) does not have `create_proxy()`.
struct Processor {
    windows: HashMap<WindowId, WindowContext>,
    /// Pre-created proxy for sending AppEvent from PTY threads to the winit event loop.
    proxy: EventLoopProxy<AppEvent>,
    /// Current keyboard modifier state, updated by ModifiersChanged events.
    modifiers: ModifiersState,
    /// User configuration loaded from ~/.glass/config.toml at startup.
    config: GlassConfig,
    /// Instant captured at program start for cold start measurement.
    cold_start: std::time::Instant,
}

/// Convert a ShellEvent (from glass_core) back to OscEvent (from glass_terminal)
/// so BlockManager.handle_event() can process it.
fn shell_event_to_osc(event: &ShellEvent) -> OscEvent {
    match event {
        ShellEvent::PromptStart => OscEvent::PromptStart,
        ShellEvent::CommandStart => OscEvent::CommandStart,
        ShellEvent::CommandExecuted => OscEvent::CommandExecuted,
        ShellEvent::CommandFinished { exit_code } => OscEvent::CommandFinished {
            exit_code: *exit_code,
        },
        ShellEvent::CurrentDirectory(path) => OscEvent::CurrentDirectory(path.clone()),
        ShellEvent::PipelineStart { stage_count } => OscEvent::PipelineStart { stage_count: *stage_count },
        ShellEvent::PipelineStage { index, total_bytes, temp_path } => OscEvent::PipelineStage {
            index: *index,
            total_bytes: *total_bytes,
            temp_path: temp_path.clone(),
        },
    }
}

impl ApplicationHandler<AppEvent> for Processor {
    /// Called at startup on desktop (Windows) and on app resume on mobile/web.
    ///
    /// In winit 0.30.13, `resumed` is the required method called once at startup on Windows.
    /// This is where the window, GPU surface, and PTY must be created.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Only create a window if we don't have one yet (handles re-resume on mobile)
        if !self.windows.is_empty() {
            return;
        }

        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes().with_title("Glass"))
                .expect("Failed to create window"),
        );

        // Parallelize font discovery with GPU init — FontSystem::new() enumerates
        // all system fonts (~35ms) and doesn't need the GPU device.
        let font_handle = std::thread::spawn(FontSystem::new);

        // wgpu init is async; block via pollster from this sync callback
        let renderer = pollster::block_on(GlassRenderer::new(Arc::clone(&window)));

        // Join font thread — should already be done since GPU init takes longer
        let font_system = font_handle.join().expect("Font system thread panicked");

        // Create FrameRenderer with pre-loaded font system
        let scale_factor = window.scale_factor() as f32;
        let frame_renderer = FrameRenderer::with_font_system(
            font_system,
            renderer.device(),
            renderer.queue(),
            renderer.surface_format(),
            &self.config.font_family,
            self.config.font_size,
            scale_factor,
        );

        // Compute initial terminal size from font metrics.
        // Subtract 1 line for the status bar so the PTY resize reflects actual content area.
        let (cell_w, cell_h) = frame_renderer.cell_size();
        let size = window.inner_size();
        let num_cols = (size.width as f32 / cell_w).floor().max(1.0) as u16;
        let num_lines = ((size.height as f32 / cell_h).floor().max(2.0) as u16).saturating_sub(1);

        tracing::info!(
            "Font metrics: cell={}x{} grid={}x{} (status bar reserves 1 line) scale={}",
            cell_w, cell_h, num_cols, num_lines, scale_factor
        );

        // Create SessionId for the first session and wire it into EventProxy
        let session_id = SessionId::new(0);
        let event_proxy = EventProxy::new(self.proxy.clone(), window.id(), session_id);

        // Spawn shell via PTY with a dedicated reader thread + OscScanner
        let max_output_kb = self.config.history.as_ref()
            .map(|h| h.max_output_capture_kb)
            .unwrap_or(50);
        let pipes_enabled = self.config.pipes.as_ref()
            .map(|p| p.enabled)
            .unwrap_or(true);
        let (pty_sender, term) = glass_terminal::spawn_pty(
            event_proxy,
            self.proxy.clone(),
            window.id(),
            self.config.shell.as_deref(),
            None, // working_directory -- initial session uses current dir
            max_output_kb,
            pipes_enabled,
        );

        // Send initial resize with correct font-metrics-based cell dimensions
        let initial_size = WindowSize {
            num_lines,
            num_cols,
            cell_width: cell_w as u16,
            cell_height: cell_h as u16,
        };
        let _ = pty_sender.send(PtyMsg::Resize(initial_size));

        // Also resize the Term grid to match
        term.lock().resize(TermDimensions {
            columns: num_cols as usize,
            screen_lines: num_lines as usize,
        });

        tracing::info!("PTY spawned — shell is running");

        // Auto-inject shell integration (platform-aware)
        let effective_shell = self.config.shell.as_deref()
            .unwrap_or("")
            .to_owned();
        let effective_shell_for_integration = if effective_shell.is_empty() {
            glass_mux::platform::default_shell()
        } else {
            effective_shell.clone()
        };

        if let Some(path) = find_shell_integration(&effective_shell_for_integration) {
            let inject_cmd = if effective_shell_for_integration.contains("fish") {
                format!("source {}\r\n", path.display())
            } else if effective_shell_for_integration.contains("pwsh")
                || effective_shell_for_integration.to_lowercase().contains("powershell")
            {
                format!(". '{}'\r\n", path.display())
            } else {
                // bash, zsh, and other POSIX shells
                format!("source '{}'\r\n", path.display())
            };
            let _ = pty_sender.send(PtyMsg::Input(Cow::Owned(inject_cmd.into_bytes())));
            tracing::info!("Auto-injecting shell integration: {}", path.display());
        } else {
            tracing::warn!("Shell integration script not found for shell: {}",
                if effective_shell_for_integration.is_empty() { "default" } else { &effective_shell_for_integration });
        }

        let default_colors = DefaultColors::default();

        // Open history database from initial cwd (non-fatal on failure)
        let history_db = match HistoryDb::open(&resolve_db_path(&std::env::current_dir().unwrap_or_default())) {
            Ok(db) => {
                tracing::info!("History database opened");
                Some(db)
            }
            Err(e) => {
                tracing::warn!("Failed to open history database: {} — history disabled", e);
                None
            }
        };

        // Open snapshot store from initial cwd (non-fatal on failure)
        let snapshot_store = {
            let glass_dir = glass_snapshot::resolve_glass_dir(
                &std::env::current_dir().unwrap_or_default(),
            );
            match glass_snapshot::SnapshotStore::open(&glass_dir) {
                Ok(store) => {
                    tracing::info!("Snapshot store opened");
                    Some(store)
                }
                Err(e) => {
                    tracing::warn!("Failed to open snapshot store: {} — snapshots disabled", e);
                    None
                }
            }
        };

        // Startup pruning: spawn background thread to clean old snapshots (STOR-01)
        if snapshot_store.is_some() {
            let glass_dir = glass_snapshot::resolve_glass_dir(
                &std::env::current_dir().unwrap_or_default(),
            );
            let snap_config = self.config.snapshot.clone();
            std::thread::Builder::new()
                .name("Glass pruning".into())
                .spawn(move || {
                    let store = match glass_snapshot::SnapshotStore::open(&glass_dir) {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::warn!("Pruning: failed to open store: {}", e);
                            return;
                        }
                    };
                    let (retention_days, max_count, max_size_mb) = match snap_config {
                        Some(ref cfg) => (cfg.retention_days, cfg.max_count, cfg.max_size_mb),
                        None => (30, 1000, 500), // defaults matching SnapshotSection
                    };
                    let pruner = glass_snapshot::Pruner::new(&store, retention_days, max_count, max_size_mb);
                    match pruner.prune() {
                        Ok(result) => tracing::info!(
                            "Pruning complete: {} snapshots, {} blobs removed",
                            result.snapshots_deleted, result.blobs_deleted,
                        ),
                        Err(e) => tracing::warn!("Pruning failed: {}", e),
                    }
                })
                .ok();
        }

        // Build Session with all terminal state, then wrap in SessionMux
        let session = Session {
            id: session_id,
            pty_sender,
            term,
            default_colors,
            block_manager: BlockManager::new(),
            status: StatusState::default(),
            history_db,
            last_command_id: None,
            command_started_wall: None,
            search_overlay: None,
            snapshot_store,
            pending_command_text: None,
            active_watcher: None,
            pending_snapshot_id: None,
            pending_parse_confidence: None,
            cursor_position: None,
            title: String::from("Glass"),
        };
        let session_mux = SessionMux::new(session);

        let id = window.id();
        self.windows.insert(
            id,
            WindowContext {
                window,
                renderer,
                frame_renderer,
                session_mux,
                first_frame_logged: false,
            },
        );
    }

    /// Handle per-window OS events.
    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(ctx) = self.windows.get_mut(&window_id) else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => {
                self.windows.remove(&window_id);
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                // Execute debounced search query
                {
                    let session = ctx.session_mux.focused_session_mut().unwrap();
                    if let Some(ref mut overlay) = session.search_overlay {
                        if overlay.should_search(std::time::Duration::from_millis(150)) {
                            overlay.mark_searched();
                            if !overlay.query.is_empty() {
                                let filter = glass_history::query::QueryFilter {
                                    text: Some(overlay.query.clone()),
                                    limit: 20,
                                    ..Default::default()
                                };
                                if let Some(ref db) = session.history_db {
                                    let results = db.filtered_query(&filter).unwrap_or_default();
                                    overlay.set_results(results);
                                }
                            } else {
                                overlay.set_results(Vec::new());
                            }
                        }
                    }
                }
                // Keep requesting redraws while search is pending (debounce timer not elapsed)
                if let Some(ref overlay) = ctx.session().search_overlay {
                    if overlay.search_pending {
                        ctx.window.request_redraw();
                    }
                }

                // Lock Term briefly for snapshot only, then release
                let snapshot = {
                    let session = ctx.session();
                    let term = session.term.lock();
                    snapshot_term(&term, &session.default_colors)
                };

                // Extract all session data needed for rendering into owned values.
                // This avoids borrow conflicts between session_mux and renderer/frame_renderer.
                let (visible_blocks, search_overlay_data, status_clone) = {
                    let session = ctx.session_mux.focused_session().unwrap();
                    let viewport_abs_start = snapshot.history_size.saturating_sub(snapshot.display_offset);
                    let vb: Vec<_> = session.block_manager.visible_blocks(
                        viewport_abs_start,
                        snapshot.screen_lines,
                    ).into_iter().cloned().collect();
                    let sod = session.search_overlay.as_ref().map(|overlay| {
                        let data = overlay.extract_display_data();
                        glass_renderer::frame::SearchOverlayRenderData {
                            query: data.query,
                            results: data.results.iter().map(|r| {
                                (r.command.clone(), r.exit_code, r.timestamp.clone(), r.output_preview.clone())
                            }).collect(),
                            selected: data.selected,
                        }
                    });
                    let sc = session.status.clone();
                    (vb, sod, sc)
                };

                // Get surface texture
                let Some(frame) = ctx.renderer.get_current_texture() else {
                    return;
                };
                let view = frame.texture.create_view(&Default::default());
                let sc = ctx.renderer.surface_config();

                // Convert owned blocks to references for draw_frame
                let visible_block_refs: Vec<&_> = visible_blocks.iter().collect();

                // Draw frame using FrameRenderer with block decorations and status bar
                ctx.frame_renderer.draw_frame(
                    ctx.renderer.device(),
                    ctx.renderer.queue(),
                    &view,
                    sc.width,
                    sc.height,
                    &snapshot,
                    &visible_block_refs,
                    Some(&status_clone),
                    search_overlay_data.as_ref(),
                );

                frame.present();

                if !ctx.first_frame_logged {
                    ctx.first_frame_logged = true;
                    tracing::info!("PERF cold_start={:?}", self.cold_start.elapsed());
                    if let Some(usage) = memory_stats::memory_stats() {
                        tracing::info!(
                            "PERF memory_physical={:.1}MB",
                            usage.physical_mem as f64 / 1_048_576.0
                        );
                    }
                }

                ctx.frame_renderer.trim();
            }
            WindowEvent::Resized(size) => {
                if size.width == 0 || size.height == 0 {
                    return;
                }
                ctx.renderer.resize(size.width, size.height);

                // Compute terminal grid size from font metrics.
                // Subtract 1 line for the status bar.
                let (cell_w, cell_h) = ctx.frame_renderer.cell_size();
                let num_cols = (size.width as f32 / cell_w).floor().max(1.0) as u16;
                let num_lines =
                    ((size.height as f32 / cell_h).floor().max(2.0) as u16).saturating_sub(1);

                // Notify PTY of the new terminal size with real cell dimensions
                let new_window_size = WindowSize {
                    num_lines,
                    num_cols,
                    cell_width: cell_w as u16,
                    cell_height: cell_h as u16,
                };
                {
                    let session = ctx.session_mux.focused_session_mut().unwrap();
                    let _ = session.pty_sender.send(PtyMsg::Resize(new_window_size));

                    // Also resize the Term grid so content reflows (CORE-07)
                    session.term.lock().resize(TermDimensions {
                        columns: num_cols as usize,
                        screen_lines: num_lines as usize,
                    });
                }

                // Request a redraw after resize so the surface is repainted immediately
                ctx.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                tracing::info!("Scale factor changed to {}", scale_factor);
                // FrameRenderer does not yet support dynamic scale factor updates.
                // Full font metric recalculation on scale factor change requires
                // rebuilding the glyph atlas, which is a future enhancement for
                // multi-monitor HiDPI support. For now, log the event.
                tracing::warn!(
                    "Dynamic scale factor update not yet supported; \
                     restart Glass to apply new DPI settings"
                );
                ctx.window.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed {
                    let modifiers = self.modifiers;

                    // Search overlay input interception -- must be FIRST to prevent PTY forwarding
                    // Uses an enum to capture what action to take, avoiding borrow conflicts
                    // between session_mux (for overlay) and window (for request_redraw).
                    enum OverlayAction { None, Handled, Close }
                    let overlay_action = {
                        let session = ctx.session_mux.focused_session_mut().unwrap();
                        if let Some(ref mut overlay) = session.search_overlay {
                            match &event.logical_key {
                                Key::Named(NamedKey::Escape) => {
                                    session.search_overlay = None;
                                    OverlayAction::Close
                                }
                                Key::Named(NamedKey::ArrowUp) => {
                                    overlay.move_up();
                                    OverlayAction::Handled
                                }
                                Key::Named(NamedKey::ArrowDown) => {
                                    overlay.move_down();
                                    OverlayAction::Handled
                                }
                                Key::Named(NamedKey::Enter) => {
                                    // Scroll-to-block: find the block whose started_epoch matches
                                    // the selected search result's started_at timestamp.
                                    if overlay.selected < overlay.results.len() {
                                        let result_epoch = overlay.results[overlay.selected].started_at;
                                        let all_blocks = session.block_manager.blocks();
                                        let matched_block = all_blocks.iter().find(|b| {
                                            b.started_epoch == Some(result_epoch)
                                        });
                                        if let Some(block) = matched_block {
                                            let target_line = block.prompt_start_line;
                                            let mut term = session.term.lock();
                                            let history_size = term.grid().history_size();
                                            let current_offset = term.grid().display_offset();
                                            let target_offset = history_size.saturating_sub(target_line);
                                            let delta = target_offset as i32 - current_offset as i32;
                                            if delta != 0 {
                                                term.scroll_display(Scroll::Delta(delta));
                                            }
                                        }
                                    }
                                    session.search_overlay = None;
                                    OverlayAction::Close
                                }
                                Key::Named(NamedKey::Backspace) => {
                                    overlay.pop_char();
                                    OverlayAction::Handled
                                }
                                Key::Character(c) => {
                                    // Allow Ctrl+Shift+F to toggle overlay closed even when open
                                    if modifiers.control_key()
                                        && modifiers.shift_key()
                                        && c.as_str().eq_ignore_ascii_case("f")
                                    {
                                        session.search_overlay = None;
                                        OverlayAction::Close
                                    } else {
                                        overlay.push_char(c.as_str());
                                        OverlayAction::Handled
                                    }
                                }
                                _ => OverlayAction::Handled // Swallow all other keys while overlay is open
                            }
                        } else {
                            OverlayAction::None
                        }
                    };
                    match overlay_action {
                        OverlayAction::None => {} // No overlay open, continue to normal key handling
                        OverlayAction::Handled | OverlayAction::Close => {
                            ctx.window.request_redraw();
                            return;
                        }
                    }

                    let mode = *ctx.session().term.lock().mode();

                    // Check for Glass-handled keys first
                    if modifiers.control_key() && modifiers.shift_key() {
                        match &event.logical_key {
                            Key::Character(c)
                                if c.as_str().eq_ignore_ascii_case("c") =>
                            {
                                clipboard_copy(&ctx.session().term);
                                return;
                            }
                            Key::Character(c)
                                if c.as_str().eq_ignore_ascii_case("v") =>
                            {
                                clipboard_paste(&ctx.session().pty_sender, mode);
                                return;
                            }
                            Key::Character(c)
                                if c.as_str().eq_ignore_ascii_case("f") =>
                            {
                                ctx.session_mut().search_overlay = Some(SearchOverlay::new());
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Character(c)
                                if c.as_str().eq_ignore_ascii_case("z") =>
                            {
                                {
                                    let session = ctx.session_mux.focused_session_mut().unwrap();
                                    if let Some(ref store) = session.snapshot_store {
                                        let engine = glass_snapshot::UndoEngine::new(store);
                                        match engine.undo_latest() {
                                            Ok(Some(result)) => {
                                                // Count outcomes for summary line
                                                let (mut restored, mut deleted, mut skipped, mut conflicts, mut errors) = (0u32, 0u32, 0u32, 0u32, 0u32);
                                                for (path, outcome) in &result.files {
                                                    match outcome {
                                                        glass_snapshot::FileOutcome::Restored => {
                                                            restored += 1;
                                                            tracing::info!("Undo: restored {}", path.display());
                                                        }
                                                        glass_snapshot::FileOutcome::Deleted => {
                                                            deleted += 1;
                                                            tracing::info!("Undo: deleted {}", path.display());
                                                        }
                                                        glass_snapshot::FileOutcome::Conflict { .. } => {
                                                            conflicts += 1;
                                                            tracing::warn!("Undo: CONFLICT {}", path.display());
                                                        }
                                                        glass_snapshot::FileOutcome::Error(e) => {
                                                            errors += 1;
                                                            tracing::error!("Undo: error {}: {}", path.display(), e);
                                                        }
                                                        glass_snapshot::FileOutcome::Skipped => {
                                                            skipped += 1;
                                                            tracing::info!("Undo: skipped {}", path.display());
                                                        }
                                                    }
                                                }
                                                tracing::info!(
                                                    "Undo complete: {} restored, {} deleted, {} skipped, {} conflicts, {} errors",
                                                    restored, deleted, skipped, conflicts, errors,
                                                );
                                                // Remove [undo] label from the undone block (visual feedback).
                                                let epoch_to_clear = session.block_manager.blocks().iter().rev()
                                                    .find(|b| b.has_snapshot)
                                                    .and_then(|b| b.started_epoch);
                                                if let Some(ep) = epoch_to_clear {
                                                    if let Some(b) = session.block_manager.find_block_by_epoch_mut(ep) {
                                                        b.has_snapshot = false;
                                                    }
                                                }
                                            }
                                            Ok(None) => {
                                                tracing::info!("Nothing to undo -- no file-modifying commands found");
                                            }
                                            Err(e) => {
                                                tracing::error!("Undo failed: {}", e);
                                            }
                                        }
                                    } else {
                                        tracing::debug!("Undo unavailable -- no snapshot store");
                                    }
                                }
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Character(c)
                                if c.as_str().eq_ignore_ascii_case("p") =>
                            {
                                // Ctrl+Shift+P: Toggle pipeline expansion on most recent pipeline block
                                {
                                    let session = ctx.session_mux.focused_session_mut().unwrap();
                                    if let Some(block) = session.block_manager.blocks_mut().iter_mut().rev()
                                        .find(|b| b.pipeline_stage_count.unwrap_or(0) > 0 || b.pipeline_stage_commands.len() > 1)
                                    {
                                        block.toggle_pipeline_expanded();
                                    }
                                }
                                ctx.window.request_redraw();
                                return;
                            }
                            _ => {}
                        }
                    }

                    // Shift+PageUp/Down: scrollback
                    if modifiers.shift_key()
                        && !modifiers.control_key()
                        && !modifiers.alt_key()
                    {
                        match &event.logical_key {
                            Key::Named(NamedKey::PageUp) => {
                                ctx.session().term.lock().scroll_display(Scroll::PageUp);
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::PageDown) => {
                                ctx.session().term.lock().scroll_display(Scroll::PageDown);
                                ctx.window.request_redraw();
                                return;
                            }
                            _ => {}
                        }
                    }

                    // Forward to PTY via encoder
                    let key_start = std::time::Instant::now();
                    if let Some(bytes) =
                        encode_key(&event.logical_key, modifiers, mode)
                    {
                        let _ = ctx
                            .session()
                            .pty_sender
                            .send(PtyMsg::Input(Cow::Owned(bytes)));
                        tracing::trace!("PERF key_latency={:?}", key_start.elapsed());
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                ctx.session_mut().cursor_position = Some((position.x, position.y));
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                let needs_redraw = if let Some((_, y)) = ctx.session().cursor_position {
                    let (cell_w, cell_h) = ctx.frame_renderer.cell_size();
                    let size = ctx.window.inner_size();
                    let viewport_h = size.height as f32;
                    let status_bar_h = cell_h; // status bar is always 1 cell tall

                    // Hit test pipeline stage panel (bottom of viewport)
                    let session = ctx.session_mux.focused_session_mut().unwrap();
                    if let Some((block_idx, hit)) = session.block_manager.pipeline_hit_test(
                        0.0, y as f32, cell_w, cell_h, viewport_h, status_bar_h,
                    ) {
                        match hit {
                            PipelineHit::StageRow(stage_idx) => {
                                if let Some(block) = session.block_manager.block_mut(block_idx) {
                                    if block.expanded_stage_index == Some(stage_idx) {
                                        block.set_expanded_stage(None);
                                    } else {
                                        block.set_expanded_stage(Some(stage_idx));
                                    }
                                }
                            }
                            PipelineHit::Header => {
                                if let Some(block) = session.block_manager.block_mut(block_idx) {
                                    block.toggle_pipeline_expanded();
                                }
                            }
                        }
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };
                if needs_redraw {
                    ctx.window.request_redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as i32,
                    MouseScrollDelta::PixelDelta(pos) => {
                        let (_, cell_h) = ctx.frame_renderer.cell_size();
                        (pos.y / cell_h as f64) as i32
                    }
                };
                if lines != 0 {
                    // Positive delta = scroll up (into history), negative = scroll down
                    ctx.session().term.lock().scroll_display(Scroll::Delta(lines));
                    ctx.window.request_redraw();
                }
            }
            _ => {}
        }
    }

    /// Handle custom AppEvents sent from the PTY reader thread.
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::TerminalDirty { window_id } => {
                if let Some(ctx) = self.windows.get(&window_id) {
                    tracing::trace!("Terminal output received — requesting redraw");
                    ctx.window.request_redraw();
                }
            }
            AppEvent::SetTitle { window_id, session_id: _, title } => {
                if let Some(ctx) = self.windows.get(&window_id) {
                    ctx.window.set_title(&title);
                }
            }
            AppEvent::TerminalExit { window_id, session_id: _ } => {
                tracing::info!("Shell exited — closing window");
                self.windows.remove(&window_id);
                // Exit the event loop when the shell exits
                event_loop.exit();
            }
            AppEvent::Shell {
                window_id,
                session_id,
                event: shell_event,
                line,
            } => {
                if let Some(ctx) = self.windows.get_mut(&window_id) {
                    // Route to session by session_id
                    if ctx.session_mux.session(session_id).is_none() {
                        return;
                    }

                    // Skip pipeline events entirely when pipes are disabled
                    let pipes_enabled = self.config.pipes.as_ref()
                        .map(|p| p.enabled)
                        .unwrap_or(true);
                    if !pipes_enabled {
                        if matches!(shell_event, ShellEvent::PipelineStart { .. } | ShellEvent::PipelineStage { .. }) {
                            return;
                        }
                    }

                    {
                        let session = ctx.session_mux.session_mut(session_id).unwrap();

                        // Convert ShellEvent to OscEvent for BlockManager
                        let osc_event = shell_event_to_osc(&shell_event);
                        session.block_manager.handle_event(&osc_event, line);

                        // Override auto-expand if config disables it (after handle_event sets pipeline_expanded)
                        if matches!(shell_event, ShellEvent::CommandFinished { .. }) {
                            let auto_expand = self.config.pipes.as_ref()
                                .map(|p| p.auto_expand)
                                .unwrap_or(true);
                            if !auto_expand {
                                if let Some(block) = session.block_manager.current_block_mut() {
                                    block.pipeline_expanded = false;
                                }
                            }
                        }

                        // Read temp files for pipeline stages and process through StageBuffer
                        if pipes_enabled {
                            if let ShellEvent::PipelineStage { index, total_bytes: _, ref temp_path } = shell_event {
                                match std::fs::read(temp_path) {
                                    Ok(raw_bytes) => {
                                        let max_bytes = self.config.pipes.as_ref()
                                            .map(|p| (p.max_capture_mb as usize) * 1024 * 1024)
                                            .unwrap_or(10 * 1024 * 1024);
                                        let policy = glass_pipes::BufferPolicy::new(max_bytes, 512 * 1024);
                                        let mut stage_buf = glass_pipes::StageBuffer::new(policy);
                                        stage_buf.append(&raw_bytes);
                                        let finalized = stage_buf.finalize();

                                        if let Some(block) = session.block_manager.current_block_mut() {
                                            if let Some(stage) = block.pipeline_stages.iter_mut().find(|s| s.index == index) {
                                                stage.data = finalized;
                                                stage.temp_path = None;
                                            }
                                        }

                                        let _ = std::fs::remove_file(temp_path);
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to read pipeline stage {} from {}: {}", index, temp_path, e);
                                    }
                                }
                            }
                        }

                        // Track wall-clock start time on CommandExecuted and extract command text
                        // from the terminal grid NOW (before output overwrites the grid).
                        // block_manager.handle_event() above has already set output_start_line.
                        if matches!(shell_event, ShellEvent::CommandExecuted) {
                            session.command_started_wall = Some(std::time::SystemTime::now());

                            // Extract command text from the terminal grid using block line info.
                            // command_start_line..output_start_line covers the input area.
                            let command_text = {
                                let blocks = session.block_manager.blocks();
                                if let Some(block) = blocks.last() {
                                    let start = block.command_start_line;
                                    let end = block.output_start_line
                                        .map(|o| o.max(start + 1))
                                        .unwrap_or(start + 1);
                                    let term_guard = session.term.lock();
                                    let hist = term_guard.grid().history_size();
                                    let cols = term_guard.columns();
                                    let topmost = term_guard.grid().topmost_line();
                                    let bottommost = term_guard.grid().bottommost_line();
                                    let mut text = String::new();
                                    for abs_line in start..end {
                                        let grid_line = Line(abs_line as i32 - hist as i32);
                                        if grid_line < topmost || grid_line > bottommost {
                                            continue;
                                        }
                                        let row = &term_guard.grid()[grid_line];
                                        for col in 0..cols {
                                            let c = row[Column(col)].c;
                                            if c != '\0' {
                                                text.push(c);
                                            }
                                        }
                                    }
                                    text.trim().to_string()
                                } else {
                                    String::new()
                                }
                            };
                            // Pre-exec snapshot: parse command, snapshot identified targets
                            let snapshot_enabled = self.config.snapshot.as_ref()
                                .map(|s| s.enabled)
                                .unwrap_or(true);
                            if snapshot_enabled {
                                if let Some(ref store) = session.snapshot_store {
                                    let cwd_path_snap = std::path::Path::new(session.status.cwd());
                                    let parse_result = glass_snapshot::command_parser::parse_command(
                                        &command_text, cwd_path_snap,
                                    );

                                    if parse_result.confidence != glass_snapshot::Confidence::ReadOnly
                                        && !parse_result.targets.is_empty()
                                    {
                                        match store.create_snapshot(0, session.status.cwd()) {
                                            Ok(sid) => {
                                                for target in &parse_result.targets {
                                                    if let Err(e) = store.store_file(sid, target, "parser") {
                                                        tracing::warn!("Pre-exec snapshot failed for {}: {}", target.display(), e);
                                                    }
                                                }
                                                tracing::info!(
                                                    "Pre-exec snapshot {} with {} targets (confidence: {:?})",
                                                    sid, parse_result.targets.len(), parse_result.confidence,
                                                );
                                                session.pending_snapshot_id = Some(sid);
                                                session.pending_parse_confidence = Some(parse_result.confidence);
                                                // Mark current block as having a snapshot for [undo] label
                                                if let Some(block) = session.block_manager.current_block_mut() {
                                                    block.has_snapshot = true;
                                                }
                                            }
                                            Err(e) => tracing::warn!("Pre-exec snapshot creation failed: {}", e),
                                        }
                                    }
                                }
                            } else {
                                tracing::debug!("Pre-exec snapshot skipped: snapshots disabled in config");
                            }

                            // Parse pipeline stages to extract per-stage command text.
                            // Strip prompt prefix (e.g. "PS C:\path> ") since command_text
                            // is extracted from the terminal grid which includes it.
                            let pipe_text = if let Some(pos) = command_text.find("> ") {
                                &command_text[pos + 2..]
                            } else {
                                &command_text
                            };
                            if let Some(idx) = session.block_manager.current_block_index() {
                                let pipeline = glass_pipes::parse_pipeline(pipe_text);
                                if pipeline.stages.len() > 1 {
                                    let stage_commands: Vec<String> = pipeline.stages.iter()
                                        .map(|s| s.command.clone())
                                        .collect();
                                    if let Some(block) = session.block_manager.block_mut(idx) {
                                        block.pipeline_stage_commands = stage_commands;
                                    }
                                }
                            }

                            session.pending_command_text = Some(command_text);

                            // Start filesystem watcher for this command's CWD
                            let cwd_path = std::path::Path::new(session.status.cwd());
                            let ignore = glass_snapshot::IgnoreRules::load(cwd_path);
                            session.active_watcher = match glass_snapshot::FsWatcher::new(cwd_path, ignore) {
                                Ok(w) => {
                                    tracing::debug!("FS watcher started for {}", cwd_path.display());
                                    Some(w)
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to start FS watcher: {}", e);
                                    None
                                }
                            };
                        }

                        // Insert CommandRecord on CommandFinished
                        if let ShellEvent::CommandFinished { exit_code } = &shell_event {
                            if let Some(ref db) = session.history_db {
                                let now = std::time::SystemTime::now();
                                let finished_epoch = now
                                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                                    .map(|d| d.as_secs() as i64)
                                    .unwrap_or(0);
                                let started_epoch = session.command_started_wall
                                    .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
                                    .map(|d| d.as_secs() as i64)
                                    .unwrap_or(finished_epoch);
                                let duration_ms = session.command_started_wall
                                    .and_then(|t| now.duration_since(t).ok())
                                    .map(|d| d.as_millis() as i64)
                                    .unwrap_or(0);

                                // Use command text extracted earlier at CommandExecuted time.
                                let command_text = session.pending_command_text.take().unwrap_or_default();

                                let record = CommandRecord {
                                    id: None,
                                    command: command_text,
                                    cwd: session.status.cwd().to_string(),
                                    exit_code: *exit_code,
                                    started_at: started_epoch,
                                    finished_at: finished_epoch,
                                    duration_ms,
                                    output: None,
                                };

                                match db.insert_command(&record) {
                                    Ok(id) => {
                                        session.last_command_id = Some(id);
                                        tracing::debug!("Inserted command record id={}", id);

                                        // Persist pipeline stage data if present
                                        if let Some(block) = session.block_manager.blocks().last() {
                                            if !block.pipeline_stages.is_empty() {
                                                let stages: Vec<glass_history::PipeStageRow> = block.pipeline_stages.iter().enumerate().map(|(i, stage)| {
                                                    let cmd_text = block.pipeline_stage_commands
                                                        .get(i)
                                                        .map(|s| s.as_str())
                                                        .unwrap_or("");
                                                    let (output, total_bytes, is_binary, is_sampled) = match &stage.data {
                                                        glass_pipes::FinalizedBuffer::Complete(data) => {
                                                            let text = String::from_utf8_lossy(data).into_owned();
                                                            (if text.is_empty() { None } else { Some(text) }, data.len() as i64, false, false)
                                                        }
                                                        glass_pipes::FinalizedBuffer::Sampled { head, tail, total_bytes } => {
                                                            let head_text = String::from_utf8_lossy(head);
                                                            let tail_text = String::from_utf8_lossy(tail);
                                                            let omitted = total_bytes - head.len() - tail.len();
                                                            let combined = format!("{}\n[...{} bytes omitted...]\n{}", head_text, omitted, tail_text);
                                                            (Some(combined), *total_bytes as i64, false, true)
                                                        }
                                                        glass_pipes::FinalizedBuffer::Binary { size } => {
                                                            (None, *size as i64, true, false)
                                                        }
                                                    };
                                                    glass_history::PipeStageRow {
                                                        stage_index: stage.index as i64,
                                                        command: cmd_text.to_string(),
                                                        output,
                                                        total_bytes,
                                                        is_binary,
                                                        is_sampled,
                                                    }
                                                }).collect();

                                                if let Err(e) = db.insert_pipe_stages(id, &stages) {
                                                    tracing::warn!("Failed to insert pipe stages: {}", e);
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to insert command record: {}", e);
                                        session.last_command_id = None;
                                    }
                                }
                            }
                            // Clear wall-clock tracker
                            session.command_started_wall = None;

                            // Update pre-exec snapshot with real command_id
                            if let (Some(sid), Some(ref store)) = (session.pending_snapshot_id.take(), &session.snapshot_store) {
                                let command_id = session.last_command_id.unwrap_or(0);
                                if let Err(e) = store.update_command_id(sid, command_id) {
                                    tracing::warn!("Failed to update snapshot {} command_id: {}", sid, e);
                                }
                            }
                            session.pending_parse_confidence = None;

                            // Drain filesystem watcher events and store modified files
                            if let Some(watcher) = session.active_watcher.take() {
                                let events = watcher.drain_events();
                                if !events.is_empty() {
                                    tracing::debug!("FS watcher captured {} events", events.len());
                                    if let Some(ref store) = session.snapshot_store {
                                        let command_id = session.last_command_id.unwrap_or(0);
                                        let cwd = session.status.cwd().to_string();
                                        match store.create_snapshot(command_id, &cwd) {
                                            Ok(snapshot_id) => {
                                                for event in &events {
                                                    if let Err(e) = store.store_file(snapshot_id, &event.path, "watcher") {
                                                        tracing::warn!("Failed to store watcher file {}: {}", event.path.display(), e);
                                                    }
                                                    // For Rename events, also store the destination path
                                                    if let glass_snapshot::WatcherEventKind::Rename { ref to } = event.kind {
                                                        if let Err(e) = store.store_file(snapshot_id, to, "watcher") {
                                                            tracing::warn!("Failed to store watcher rename target {}: {}", to.display(), e);
                                                        }
                                                    }
                                                }
                                                tracing::debug!("Stored {} watcher files in snapshot {}", events.len(), snapshot_id);
                                            }
                                            Err(e) => {
                                                tracing::warn!("Failed to create watcher snapshot: {}", e);
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // On CurrentDirectory events, update status and query git info
                        // Track whether we need to spawn a git query (can't spawn inside session borrow)
                        let spawn_git_query = if let ShellEvent::CurrentDirectory(ref cwd) = shell_event {
                            session.status.set_cwd(cwd.clone());
                            if !session.status.git_query_pending {
                                session.status.git_query_pending = true;
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                        let _ = spawn_git_query; // used below after session borrow ends
                    } // drop session borrow

                    // Spawn git query outside session borrow (needs self.proxy and window_id)
                    if let ShellEvent::CurrentDirectory(ref cwd) = shell_event {
                        // Re-check: only spawn if we set git_query_pending above
                        let session = ctx.session_mux.session(session_id).unwrap();
                        if session.status.git_query_pending {
                            let cwd_owned = cwd.clone();
                            let proxy = self.proxy.clone();
                            let wid = window_id;
                            let sid = session_id;
                            std::thread::Builder::new()
                                .name("Glass git query".into())
                                .spawn(move || {
                                    let git_info = query_git_status(&cwd_owned);
                                    let info = git_info.map(|gi| GitStatus {
                                        branch: gi.branch,
                                        dirty_count: gi.dirty_count,
                                    });
                                    let _ = proxy.send_event(AppEvent::GitInfo {
                                        window_id: wid,
                                        session_id: sid,
                                        info,
                                    });
                                })
                                .ok();
                        }
                    }

                    // Request redraw to reflect block state changes
                    ctx.window.request_redraw();
                }
            }
            AppEvent::CommandOutput { window_id, session_id, raw_output } => {
                if let Some(ctx) = self.windows.get_mut(&window_id) {
                    // Process raw bytes: binary detection, ANSI stripping, truncation
                    let max_kb = self.config.history.as_ref()
                        .map(|h| h.max_output_capture_kb)
                        .unwrap_or(50);
                    let processed = glass_history::output::process_output(Some(raw_output), max_kb);
                    if let Some(output) = processed {
                        if let Some(session) = ctx.session_mux.session_mut(session_id) {
                            // Update the last command record with captured output
                            if let (Some(ref db), Some(cmd_id)) = (&session.history_db, session.last_command_id) {
                                match db.update_output(cmd_id, &output) {
                                    Ok(()) => {
                                        tracing::debug!(
                                            "Updated command {} with {} bytes of output",
                                            cmd_id,
                                            output.len(),
                                        );
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to update command output: {}", e);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            AppEvent::GitInfo { window_id, session_id, info } => {
                if let Some(ctx) = self.windows.get_mut(&window_id) {
                    {
                        if let Some(session) = ctx.session_mux.session_mut(session_id) {
                            session.status.git_query_pending = false;
                            let git_info = info.map(|gi| glass_terminal::GitInfo {
                                branch: gi.branch,
                                dirty_count: gi.dirty_count,
                            });
                            session.status.set_git_info(git_info);
                        }
                    }
                    ctx.window.request_redraw();
                }
            }
        }
    }

    /// Called when the event queue is drained. No-op for Phase 1.
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {}
}

/// Copy the current terminal selection to the system clipboard.
/// Locate the shell integration script relative to the executable.
///
/// Platform-aware: selects glass.ps1/glass.zsh/glass.bash/glass.fish based on shell name.
///
/// Checks two layouts:
/// - Installed: `<exe_dir>/shell-integration/<script>`
/// - Development: `<exe_dir>/../../shell-integration/<script>` (exe in target/{debug,release}/)
fn find_shell_integration(shell_name: &str) -> Option<std::path::PathBuf> {
    let script_name = if shell_name.contains("pwsh") || shell_name.to_lowercase().contains("powershell") {
        "glass.ps1"
    } else if shell_name.contains("zsh") {
        "glass.zsh"
    } else if shell_name.contains("fish") {
        "glass.fish"
    } else {
        "glass.bash"
    };

    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;

    // Installed layout
    let candidate = exe_dir.join("shell-integration").join(script_name);
    if candidate.exists() {
        return Some(candidate);
    }

    // Development layout: exe in target/{debug,release}/
    if let Some(repo_root) = exe_dir.parent().and_then(|p| p.parent()) {
        let candidate = repo_root.join("shell-integration").join(script_name);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

fn clipboard_copy(term: &Arc<FairMutex<Term<EventProxy>>>) {
    let term = term.lock();
    if let Some(selection) = term.selection_to_string() {
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(selection);
        }
    }
}

/// Paste text from the system clipboard into the PTY.
/// Wraps with bracketed paste sequences when BRACKETED_PASTE mode is active.
fn clipboard_paste(sender: &PtySender, mode: TermMode) {
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if let Ok(text) = clipboard.get_text() {
            let bytes = if mode.contains(TermMode::BRACKETED_PASTE) {
                let mut buf = Vec::new();
                buf.extend_from_slice(b"\x1b[200~");
                buf.extend_from_slice(text.as_bytes());
                buf.extend_from_slice(b"\x1b[201~");
                buf
            } else {
                text.into_bytes()
            };
            let _ = sender.send(PtyMsg::Input(Cow::Owned(bytes)));
        }
    }
}

fn main() {
    let cold_start = std::time::Instant::now();

    // FIRST: set UTF-8 console code page on Windows before any PTY creation (Pitfall 5)
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::System::Console::{SetConsoleCP, SetConsoleOutputCP};
        SetConsoleCP(65001);
        SetConsoleOutputCP(65001);
    }

    // Parse CLI BEFORE creating the event loop — subcommands must not open a window.
    // Tracing is initialized per-branch: MCP mode writes to stderr (stdout is JSON-RPC),
    // while terminal mode uses the default stdout writer.
    let cli = Cli::parse();

    match cli.command {
        None => {
            // Initialize structured logging for terminal mode (stdout)
            tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
                .init();

            // No subcommand: launch the terminal GUI (default behavior)
            tracing::info!("Glass starting");

            let config = GlassConfig::load();
            tracing::info!("Config: font_family={}, font_size={}, shell={:?}",
                config.font_family, config.font_size, config.shell);

            let event_loop = EventLoop::<AppEvent>::with_user_event()
                .build()
                .expect("Failed to create event loop");

            // Create proxy BEFORE run_app() — EventLoopProxy<AppEvent> is Clone + Send,
            // so the PTY EventProxy stores a clone of this.
            let proxy = event_loop.create_proxy();

            let mut processor = Processor {
                windows: HashMap::new(),
                proxy,
                modifiers: ModifiersState::empty(),
                config,
                cold_start,
            };

            event_loop
                .run_app(&mut processor)
                .expect("Event loop exited with error");
        }
        Some(Commands::History { action }) => {
            // Initialize structured logging for CLI mode (stdout)
            tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
                .init();
            history::run_history(action);
        }
        Some(Commands::Undo { command_id }) => {
            // Initialize structured logging for CLI mode (stdout)
            tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
                .init();

            let cwd = std::env::current_dir().unwrap_or_default();
            let glass_dir = glass_snapshot::resolve_glass_dir(&cwd);
            match glass_snapshot::SnapshotStore::open(&glass_dir) {
                Ok(store) => {
                    let engine = glass_snapshot::UndoEngine::new(&store);
                    match engine.undo_command(command_id) {
                        Ok(Some(result)) => {
                            println!("Undo complete for command {} ({:?} confidence):", command_id, result.confidence);
                            for (path, outcome) in &result.files {
                                let status = match outcome {
                                    glass_snapshot::FileOutcome::Restored => "restored",
                                    glass_snapshot::FileOutcome::Deleted => "deleted",
                                    glass_snapshot::FileOutcome::Skipped => "skipped",
                                    glass_snapshot::FileOutcome::Conflict { .. } => "CONFLICT",
                                    glass_snapshot::FileOutcome::Error(e) => {
                                        eprintln!("  error {}: {}", path.display(), e);
                                        continue;
                                    }
                                };
                                println!("  {} {}", status, path.display());
                            }
                        }
                        Ok(None) => {
                            eprintln!("No snapshot found for command {}", command_id);
                            std::process::exit(1);
                        }
                        Err(e) => {
                            eprintln!("Undo failed: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to open snapshot store: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Some(Commands::Mcp { action: McpAction::Serve }) => {
            // MCP server mode: logging goes to stderr, stdout is reserved for JSON-RPC
            tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
                .with_writer(std::io::stderr)
                .with_ansi(false)
                .init();

            let rt = tokio::runtime::Runtime::new()
                .expect("Failed to create tokio runtime");
            if let Err(e) = rt.block_on(glass_mcp::run_mcp_server()) {
                eprintln!("MCP server error: {}", e);
                std::process::exit(1);
            }
        }
    }
}

#[cfg(test)]
mod tests;
