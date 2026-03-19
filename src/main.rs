// Suppress the console window on Windows when launching the GUI.
// CLI subcommands (history, undo, mcp) still work when launched from an existing terminal.
#![windows_subsystem = "windows"]

#[allow(dead_code)]
mod checkpoint_synth;
#[allow(dead_code)]
mod ephemeral_agent;
mod history;
#[allow(dead_code)]
mod orchestrator;
mod orchestrator_events;
#[allow(dead_code)]
mod script_bridge;
mod usage_tracker;

use std::borrow::Cow;
use std::collections::HashMap;
use std::io::Write as _;
use std::sync::Arc;

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Term, TermMode};
use clap::{Parser, Subcommand};
use glass_core::config::GlassConfig;
use glass_core::event::{AppEvent, GitStatus, SessionId, ShellEvent, VerifyEventResult};
use glass_history::{
    db::{CommandRecord, HistoryDb},
    resolve_db_path,
};
use glass_mux::{
    FocusDirection, SearchOverlay, Session, SessionMux, SplitDirection, ViewportLayout,
};
use glass_renderer::tab_bar::{TabDisplayInfo, TabHitResult};
use glass_renderer::{
    DividerRect, FontSystem, FrameRenderer, GlassRenderer, PaneViewport, ScrollbarHit,
    SCROLLBAR_WIDTH,
};
use glass_terminal::{
    encode_key, query_git_status, snapshot_term, Block, BlockManager, DefaultColors, EventProxy,
    GridSnapshot, OscEvent, PipelineHit, PtyMsg, PtySender, StatusState,
};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowId};

// ---------------------------------------------------------------------------
// Fatal error helper
// ---------------------------------------------------------------------------

/// Show a fatal error message and exit. On Windows (where stderr is hidden
/// due to windows_subsystem="windows"), uses a native message box.
fn show_fatal_error(msg: &str) -> ! {
    eprintln!("Glass fatal error: {msg}");
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        let wide_msg: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
        let wide_title: Vec<u16> = "Glass Error"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                wide_msg.as_ptr(),
                wide_title.as_ptr(),
                MB_ICONERROR | MB_OK,
            );
        }
    }
    std::process::exit(1);
}

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
    /// Script profile management (export/import bundles)
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },
}

#[derive(Subcommand, Debug, PartialEq)]
enum ProfileAction {
    /// Export confirmed scripts as a shareable profile bundle
    Export {
        /// Profile name
        #[arg(long)]
        name: String,
        /// Path to scripts directory (default: ~/.glass/scripts)
        #[arg(long)]
        scripts_dir: Option<String>,
        /// Output directory for the profile bundle
        #[arg(long)]
        output: String,
        /// Glass version to embed in profile metadata
        #[arg(long, default_value = env!("CARGO_PKG_VERSION"))]
        glass_version: String,
        /// Tech stack tags (comma-separated)
        #[arg(long, value_delimiter = ',')]
        tech_stack: Vec<String>,
    },
    /// Import scripts from a profile bundle
    Import {
        /// Path to the profile bundle directory
        #[arg(long)]
        path: String,
        /// Target scripts directory (default: ~/.glass/scripts)
        #[arg(long)]
        target: Option<String>,
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

/// Scrollbar drag tracking state.
struct ScrollbarDragInfo {
    /// Which pane's scrollbar is being dragged.
    pane_id: SessionId,
    /// Y offset within the thumb where drag started (for smooth dragging).
    thumb_grab_offset: f32,
    /// The scrollbar track top Y position.
    track_y: f32,
    /// The scrollbar track height.
    track_height: f32,
    /// The current thumb height (for drag math).
    thumb_height: f32,
}

/// Tab drag reorder tracking state.
struct TabDragState {
    /// Index of the tab being dragged.
    source_index: usize,
    /// X coordinate where the drag started (for threshold check).
    start_x: f32,
    /// Whether the drag threshold has been exceeded (drag is "active").
    active: bool,
    /// Current drop target slot (insertion point index, 0..=tab_count).
    drop_index: Option<usize>,
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
    /// Whether the left mouse button is currently held (for drag selection).
    mouse_left_pressed: bool,
    /// Scrollbar drag tracking state (active when thumb is being dragged).
    scrollbar_dragging: Option<ScrollbarDragInfo>,
    /// Which pane's scrollbar the mouse is currently hovering over.
    scrollbar_hovered_pane: Option<SessionId>,
    /// Which tab the mouse is currently hovering over (for close button visibility).
    tab_bar_hovered_tab: Option<usize>,
    /// Tab drag reorder tracking state (active when a tab is being dragged).
    tab_drag_state: Option<TabDragState>,
}

impl WindowContext {
    /// Get an immutable reference to the focused session (if any).
    fn session(&self) -> Option<&Session> {
        self.session_mux.focused_session()
    }

    /// Get a mutable reference to the focused session (if any).
    fn session_mut(&mut self) -> Option<&mut Session> {
        self.session_mux.focused_session_mut()
    }
}

/// Transient state for the proposal toast notification.
///
/// Created when a new AgentProposal arrives; cleared after 30 seconds or
/// when agent mode goes inactive. The toast renders as a bottom-right banner.
struct ProposalToast {
    /// Description text shown in the toast.
    description: String,
    /// Index into agent_proposal_worktrees for the proposal that triggered this toast.
    #[allow(dead_code)]
    proposal_idx: usize,
    /// When the toast was created -- used to compute remaining seconds.
    created_at: std::time::Instant,
}

/// Encapsulates the agent subprocess lifecycle.
///
/// Lives as `Option<AgentRuntime>` on Processor -- None when agent.mode == Off
/// or when no claude binary is found on PATH.
struct AgentRuntime {
    /// The claude child process (taken after stdin/stdout extracted for threads).
    child: Option<std::process::Child>,
    /// Rate-limit gate: checked on Processor side for restart timing.
    /// The writer thread manages its own inline cooldown to avoid shared-state complexity.
    #[allow(dead_code)]
    cooldown: glass_core::agent_runtime::CooldownTracker,
    /// Accumulated cost gate: stops events when budget is exceeded.
    budget: glass_core::agent_runtime::BudgetTracker,
    /// Runtime configuration (mode, budget, cooldown, tools).
    config: glass_core::agent_runtime::AgentRuntimeConfig,
    /// Number of crash-restart attempts this session (max 3).
    restart_count: u32,
    /// Timestamp of last crash, used for exponential backoff.
    last_crash: Option<std::time::Instant>,
    /// Coordination agent ID (UUID), if registration succeeded (AGTC-05).
    agent_id: Option<String>,
    /// Session nonce for coordination operations (S-4 auth).
    coord_nonce: Option<String>,
    /// Project root path used for coordination lock, if registration succeeded.
    #[allow(dead_code)]
    project_root: Option<String>,
    /// Shared writer for sending orchestrator messages to the agent's stdin.
    orchestrator_writer:
        Option<std::sync::Arc<parking_lot::Mutex<std::io::BufWriter<std::process::ChildStdin>>>>,
    /// Generation counter — incremented on each respawn. Used to ignore stale AgentCrashed
    /// events from orphaned reader threads of previously killed agents.
    generation: u64,
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
    /// Current config parse error, if any. Displayed as an overlay banner.
    config_error: Option<glass_core::config::ConfigError>,
    /// Whether the config file watcher has been spawned (only once).
    watcher_spawned: bool,
    /// Available update info, if a newer version was found.
    update_info: Option<glass_core::updater::UpdateInfo>,
    /// Current coordination state from background poller.
    coordination_state: glass_core::coordination_poller::CoordinationState,
    /// The last ticker event ID that was displayed, used to detect new events.
    last_ticker_event_id: Option<i64>,
    /// Counter for ticker display cycles. When > 0, show ticker text.
    ticker_display_cycles: u32,
    /// Sender half of the agent activity stream channel.
    /// Use try_send() only -- never blocking send() on winit main thread.
    activity_stream_tx:
        Option<std::sync::mpsc::SyncSender<glass_core::activity_stream::ActivityEvent>>,
    /// Receiver half: consumed by agent runtime spawn logic (taken once).
    activity_stream_rx:
        Option<std::sync::mpsc::Receiver<glass_core::activity_stream::ActivityEvent>>,
    /// Activity filter: dedup, rate limit, budget window.
    activity_filter: glass_core::activity_stream::ActivityFilter,
    /// Agent subprocess lifecycle. None when mode is Off or binary not found.
    agent_runtime: Option<AgentRuntime>,
    /// Generation counter for agent runtime — incremented on each respawn.
    agent_generation: u64,
    /// Orchestrator state for autonomous Claude Code collaboration.
    orchestrator: orchestrator::OrchestratorState,
    /// Shared usage tracker state (polled from background thread).
    usage_state: Option<std::sync::Arc<std::sync::Mutex<usage_tracker::UsageState>>>,
    /// Cumulative agent query cost this session in USD.
    agent_cost_usd: f64,
    /// True when budget has been exceeded -- gates further event forwarding.
    agent_proposals_paused: bool,
    /// Manages agent worktree lifecycle (create, apply, dismiss, prune).
    worktree_manager: Option<glass_agent::WorktreeManager>,
    /// Pending agent proposals paired with their worktree handles for Phase 58 approval UI.
    agent_proposal_worktrees: Vec<(
        glass_core::agent_runtime::AgentProposalData,
        Option<glass_agent::WorktreeHandle>,
    )>,
    /// Active toast notification for the most recent proposal. Auto-dismisses after 30s.
    active_toast: Option<ProposalToast>,
    /// Whether the activity stream overlay is visible.
    activity_overlay_visible: bool,
    /// Current filter tab in the activity overlay.
    activity_view_filter: glass_renderer::ActivityViewFilter,
    /// Scroll offset in the activity overlay timeline.
    activity_scroll_offset: usize,
    /// Whether verbose mode is on (shows dismissed events).
    activity_verbose: bool,
    /// Orchestrator event ring buffer for the overlay transcript.
    orchestrator_event_buffer: orchestrator_events::OrchestratorEventBuffer,
    /// Separate scroll offset for orchestrator transcript (independent of activity overlay).
    orchestrator_scroll_offset: usize,
    /// When orchestrator was activated (for relative timestamps in transcript).
    orchestrator_activated_at: Option<std::time::Instant>,
    /// File-based verification baseline for general mode.
    file_verify_baseline: orchestrator::FileVerifyBaseline,
    /// Whether the settings overlay is visible.
    settings_overlay_visible: bool,
    /// Active tab in the settings overlay.
    settings_overlay_tab: glass_renderer::SettingsTab,
    /// Selected sidebar section index in the Settings tab.
    settings_section_index: usize,
    /// Selected field index within the current section.
    settings_field_index: usize,
    /// Whether a text field is in inline edit mode.
    settings_editing: bool,
    /// Buffer for inline text editing.
    settings_edit_buffer: String,
    /// Scroll offset for the Shortcuts tab.
    settings_shortcuts_scroll: usize,
    /// Whether the proposal review overlay is open (Ctrl+Shift+A to toggle).
    agent_review_open: bool,
    /// Selected proposal index in the review overlay. Clamped to list bounds.
    proposal_review_selected: usize,
    /// Cached diff for the currently selected proposal: (index, diff_text).
    /// Cleared when selection changes to trigger regeneration on next redraw.
    proposal_diff_cache: Option<(usize, String)>,
    /// Windows Job Object handle for orphan prevention (Windows only).
    /// Must remain alive for the app lifetime -- dropping closes the handle
    /// (via `JobObjectHandle`'s `Drop` impl), which triggers kill-on-close
    /// for all processes in the job.
    #[cfg(target_os = "windows")]
    #[allow(dead_code)]
    job_object_handle: Option<JobObjectHandle>,
    /// Thread handle for the artifact completion watcher (if active).
    artifact_watcher_thread: Option<std::thread::JoinHandle<()>>,
    /// Feedback loop state for the current orchestrator run.
    feedback_state: Option<glass_feedback::FeedbackState>,
    /// Guard to prevent config reload from overwriting feedback-written values.
    feedback_write_pending: bool,
    /// Suppress config reload agent restarts until this instant.
    /// Set when the orchestrator enable handler writes to config.toml.
    config_write_suppress_until: Option<std::time::Instant>,
    /// Captured at feedback LLM spawn time so the response handler uses the
    /// correct project root even if the user switches projects before it completes.
    feedback_llm_project_root: Option<String>,
    /// Max prompt hints captured at spawn time for the same reason.
    feedback_llm_max_hints: usize,
    /// Captured at Tier 4 script generation spawn time so the response handler
    /// writes scripts to the correct project even if the user switches.
    script_gen_project_root: Option<String>,
    /// Bridge to the Rhai scripting engine for hook-based automation.
    script_bridge: script_bridge::ScriptBridge,
    /// Consecutive Tier 4 script generation parse failures. When >= 3,
    /// new Tier 4 ephemeral agents are suppressed to avoid wasting resources.
    /// Reset to 0 on any successful parse.
    script_gen_parse_failures: u32,
}

impl Drop for AgentRuntime {
    fn drop(&mut self) {
        // AGTC-05: Deregister from coordination (soft errors -- never prevent shutdown).
        if let (Some(ref agent_id), Some(ref nonce)) = (&self.agent_id, &self.coord_nonce) {
            if let Ok(mut db) = glass_coordination::CoordinationDb::open_default() {
                if let Err(e) = db.unlock_all(agent_id, nonce) {
                    tracing::warn!("AgentRuntime: failed to release coordination locks: {}", e);
                }
                if let Err(e) = db.deregister(agent_id, nonce) {
                    tracing::warn!(
                        "AgentRuntime: failed to deregister from coordination: {}",
                        e
                    );
                }
            }
        }

        if let Some(ref mut child) = self.child {
            // Dropping stdin (done in writer thread) causes EOF to claude.
            // Give it a moment to exit cleanly, then kill.
            match child.try_wait() {
                Ok(Some(_)) => {} // already exited
                _ => {
                    if let Err(e) = child.kill() {
                        tracing::debug!("Agent child kill: {e}");
                    }
                    if let Err(e) = child.wait() {
                        tracing::warn!("Agent child wait failed: {e}");
                    }
                }
            }
        }
    }
}

/// Fire a scripting hook on the given bridge. This is a free function so it can
/// be called while other fields of `Processor` are mutably borrowed (e.g.
/// `self.windows`). Short-circuits if no scripts match the hook.
fn fire_hook_on_bridge(
    bridge: &mut script_bridge::ScriptBridge,
    orchestrator_project_root: &str,
    hook: glass_scripting::HookPoint,
    event: &glass_scripting::HookEventData,
) {
    if !bridge.has_scripts_for(hook.clone()) {
        return;
    }
    let cwd = if orchestrator_project_root.is_empty() {
        std::env::current_dir()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    } else {
        orchestrator_project_root.to_string()
    };
    let ctx = glass_scripting::HookContext {
        cwd,
        ..Default::default()
    };
    let actions = bridge.run_hook(hook, &ctx, event);
    if !actions.is_empty() {
        if let Some(root) = bridge.project_root() {
            let root = root.to_string();
            bridge.execute_actions(&actions, &root);
        }
    }
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
        ShellEvent::PipelineStart { stage_count } => OscEvent::PipelineStart {
            stage_count: *stage_count,
        },
        ShellEvent::PipelineStage {
            index,
            total_bytes,
            temp_path,
        } => OscEvent::PipelineStage {
            index: *index,
            total_bytes: *total_bytes,
            temp_path: temp_path.clone(),
        },
    }
}

/// Compute divider rectangles from the gaps between pane viewports.
///
/// Dividers fill the 2px gaps left by ViewportLayout::split() between adjacent panes.
fn compute_dividers(pane_layouts: &[(SessionId, ViewportLayout)]) -> Vec<DividerRect> {
    use glass_mux::layout::DIVIDER_GAP;

    let mut dividers = Vec::new();
    for i in 0..pane_layouts.len() {
        for j in (i + 1)..pane_layouts.len() {
            let (_, a) = &pane_layouts[i];
            let (_, b) = &pane_layouts[j];

            // Check for horizontal gap (a is left of b, same vertical range overlap)
            if a.x + a.width + DIVIDER_GAP == b.x {
                let top = a.y.max(b.y);
                let bottom = (a.y + a.height).min(b.y + b.height);
                if bottom > top {
                    dividers.push(DividerRect {
                        x: a.x + a.width,
                        y: top,
                        width: DIVIDER_GAP,
                        height: bottom - top,
                    });
                }
            }

            // Check for vertical gap (a is above b, same horizontal range overlap)
            if a.y + a.height + DIVIDER_GAP == b.y {
                let left = a.x.max(b.x);
                let right = (a.x + a.width).min(b.x + b.width);
                if right > left {
                    dividers.push(DividerRect {
                        x: left,
                        y: a.y + a.height,
                        width: right - left,
                        height: DIVIDER_GAP,
                    });
                }
            }

            // Also check reverse (b left/above a)
            if b.x + b.width + DIVIDER_GAP == a.x {
                let top = a.y.max(b.y);
                let bottom = (a.y + a.height).min(b.y + b.height);
                if bottom > top {
                    dividers.push(DividerRect {
                        x: b.x + b.width,
                        y: top,
                        width: DIVIDER_GAP,
                        height: bottom - top,
                    });
                }
            }
            if b.y + b.height + DIVIDER_GAP == a.y {
                let left = a.x.max(b.x);
                let right = (a.x + a.width).min(b.x + b.width);
                if right > left {
                    dividers.push(DividerRect {
                        x: left,
                        y: b.y + b.height,
                        width: right - left,
                        height: DIVIDER_GAP,
                    });
                }
            }
        }
    }
    dividers
}

/// Resize all panes' PTYs in the active tab with per-pane cell dimensions.
///
/// Computes container viewport (accounting for tab bar + status bar),
/// then for each pane: compute per-pane num_cols and num_lines from the
/// pane viewport dimensions divided by cell size, and send PTY resize.
fn resize_all_panes(
    session_mux: &mut SessionMux,
    frame_renderer: &FrameRenderer,
    window_width: u32,
    window_height: u32,
) {
    let (cell_w, cell_h) = frame_renderer.cell_size();

    // Container viewport: subtract tab bar (top) and status bar (bottom)
    let container = ViewportLayout {
        x: 0,
        y: cell_h as u32,
        width: window_width,
        height: window_height.saturating_sub((cell_h as u32) * 2),
    };

    // Compute pane layouts from the active tab's split tree
    let pane_layouts: Vec<(SessionId, ViewportLayout)> = session_mux
        .active_tab_root()
        .map(|root| root.compute_layout(&container))
        .unwrap_or_default();

    // Resize each pane's PTY with per-pane dimensions
    for (sid, vp) in &pane_layouts {
        let pane_cols = ((vp.width as f32 - SCROLLBAR_WIDTH) / cell_w)
            .floor()
            .max(1.0) as u16;
        let pane_lines = (vp.height as f32 / cell_h).floor().max(1.0) as u16;

        let pane_size = WindowSize {
            num_lines: pane_lines,
            num_cols: pane_cols,
            cell_width: cell_w as u16,
            cell_height: cell_h as u16,
        };

        if let Some(session) = session_mux.session_mut(*sid) {
            pty_send(&session.pty_sender, PtyMsg::Resize(pane_size));
            session.term.lock().resize(TermDimensions {
                columns: pane_cols as usize,
                screen_lines: pane_lines as usize,
            });
            session.block_manager.notify_resize(pane_cols as usize);
        }
    }
}

/// Create a new terminal session with PTY, shell integration, history DB, and snapshot store.
///
/// Encapsulates all the setup needed when creating a new tab.
#[allow(clippy::too_many_arguments)]
fn create_session(
    proxy: &EventLoopProxy<AppEvent>,
    window_id: WindowId,
    session_id: SessionId,
    config: &GlassConfig,
    working_directory: Option<&std::path::Path>,
    cell_w: f32,
    cell_h: f32,
    window_width: u32,
    window_height: u32,
    tab_bar_lines: u16,
) -> anyhow::Result<Session> {
    let event_proxy = EventProxy::new(proxy.clone(), window_id, session_id);

    let max_output_kb = config
        .history
        .as_ref()
        .map(|h| h.max_output_capture_kb)
        .unwrap_or(50);
    let pipes_enabled = config.pipes.as_ref().map(|p| p.enabled).unwrap_or(true);
    // Always create the SmartTrigger when a silence timeout is configured.
    // The OrchestratorSilence handler gates on self.orchestrator.active,
    // so events are harmlessly ignored when orchestration isn't running.
    let orchestrator_silence_secs = config
        .agent
        .as_ref()
        .and_then(|a| a.orchestrator.as_ref())
        .map(|o| o.silence_timeout_secs)
        .unwrap_or(0);
    let fast_trigger = config
        .agent
        .as_ref()
        .and_then(|a| a.orchestrator.as_ref())
        .map(|o| o.fast_trigger_secs)
        .unwrap_or(5);
    let prompt_pattern = config
        .agent
        .as_ref()
        .and_then(|a| a.orchestrator.as_ref())
        .and_then(|o| o.agent_prompt_pattern.clone());
    let (pty_sender, term) = glass_terminal::spawn_pty(
        event_proxy,
        proxy.clone(),
        window_id,
        config.shell.as_deref(),
        working_directory,
        max_output_kb,
        pipes_enabled,
        orchestrator_silence_secs,
        fast_trigger,
        prompt_pattern,
    )?;

    // Compute terminal size: subtract 1 line for status bar + tab_bar_lines
    let num_cols = ((window_width as f32 - SCROLLBAR_WIDTH) / cell_w)
        .floor()
        .max(1.0) as u16;
    let num_lines =
        ((window_height as f32 / cell_h).floor().max(2.0) as u16).saturating_sub(1 + tab_bar_lines);

    let initial_size = WindowSize {
        num_lines,
        num_cols,
        cell_width: cell_w as u16,
        cell_height: cell_h as u16,
    };
    pty_send(&pty_sender, PtyMsg::Resize(initial_size));
    term.lock().resize(TermDimensions {
        columns: num_cols as usize,
        screen_lines: num_lines as usize,
    });

    // Auto-inject shell integration
    let effective_shell = config.shell.as_deref().unwrap_or("").to_owned();
    let effective_shell_for_integration = if effective_shell.is_empty() {
        glass_mux::platform::default_shell()
    } else {
        effective_shell.clone()
    };

    if let Some(path) = find_shell_integration(&effective_shell_for_integration) {
        let inject_cmd = if effective_shell_for_integration.contains("fish") {
            format!("source {}\r\n", path.display())
        } else if effective_shell_for_integration.contains("pwsh")
            || effective_shell_for_integration
                .to_lowercase()
                .contains("powershell")
        {
            format!(". '{}'\r\n", path.display())
        } else {
            format!("source '{}'\r\n", path.display())
        };
        pty_send(
            &pty_sender,
            PtyMsg::Input(Cow::Owned(inject_cmd.into_bytes())),
        );
        tracing::info!("Auto-injecting shell integration: {}", path.display());
    }

    let default_colors = DefaultColors::default();

    // Determine CWD for history/snapshot DB paths
    let cwd = working_directory
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    let history_db = match HistoryDb::open(&resolve_db_path(&cwd)) {
        Ok(db) => Some(db),
        Err(e) => {
            tracing::warn!("Failed to open history database: {} -- history disabled", e);
            None
        }
    };

    let snapshot_store = {
        let glass_dir = glass_snapshot::resolve_glass_dir(&cwd);
        match glass_snapshot::SnapshotStore::open(&glass_dir) {
            Ok(store) => Some(store),
            Err(e) => {
                tracing::warn!("Failed to open snapshot store: {} -- snapshots disabled", e);
                None
            }
        }
    };

    // Derive tab title from working directory
    let title = working_directory
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| String::from("Glass"));

    Ok(Session {
        id: session_id,
        pty_sender,
        term,
        default_colors,
        block_manager: BlockManager::new(),
        status: StatusState::default(),
        history_db,
        last_command_id: None,
        last_soi_summary: None,
        command_started_wall: None,
        search_overlay: None,
        snapshot_store,
        pending_command_text: None,
        active_watcher: None,
        pending_snapshot_id: None,
        pending_parse_confidence: None,
        cursor_position: None,
        title,
    })
}

/// Send a message to the PTY, logging if the channel is dead.
///
/// Returns `true` if the send succeeded, `false` if the shell has already exited.
fn pty_send(sender: &PtySender, msg: PtyMsg) -> bool {
    match sender.send(msg) {
        Ok(()) => true,
        Err(e) => {
            tracing::debug!("PTY channel closed — shell has exited: {e}");
            false
        }
    }
}

/// Clean up a session by shutting down its PTY.
fn cleanup_session(session: Session) {
    pty_send(&session.pty_sender, PtyMsg::Shutdown);
    // Session is dropped here, releasing all resources
}

/// RAII wrapper for a Windows Job Object HANDLE.
///
/// Closes the underlying `HANDLE` when dropped. Because the Job Object was
/// created with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, closing the handle
/// causes the kernel to terminate all processes in the job — including any
/// claude subprocesses — when Glass exits (whether cleanly or via a crash).
#[cfg(target_os = "windows")]
struct JobObjectHandle(isize);

#[cfg(target_os = "windows")]
impl Drop for JobObjectHandle {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(
                self.0 as windows_sys::Win32::Foundation::HANDLE,
            );
        }
    }
}

/// Create a Windows Job Object with JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE and assign
/// the current process to it.  When Glass exits (handle dropped), the kernel kills
/// any processes in the job (including the claude subprocess).
///
/// Returns None on failure (logged as a warning). The returned `JobObjectHandle`
/// must be kept alive for the app lifetime (it is stored in `App`).
#[cfg(target_os = "windows")]
fn setup_windows_job_object() -> Option<JobObjectHandle> {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            tracing::warn!("AgentRuntime: CreateJobObjectW failed, orphan prevention unavailable");
            return None;
        }

        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION =
            std::mem::zeroed::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

        let ok = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &raw const info as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if ok == 0 {
            tracing::warn!(
                "AgentRuntime: SetInformationJobObject failed, orphan prevention unavailable"
            );
            return None;
        }

        // Assign current process to the job
        let current_process = windows_sys::Win32::System::Threading::GetCurrentProcess();
        let assigned = AssignProcessToJobObject(job, current_process as HANDLE);
        if assigned == 0 {
            tracing::warn!(
                "AgentRuntime: AssignProcessToJobObject failed, orphan prevention may be limited"
            );
            // Still return the handle -- future child processes may still get assigned
        }

        tracing::info!("AgentRuntime: Windows Job Object created (kill-on-close enabled)");
        Some(JobObjectHandle(job as isize))
    }
}

/// Attempt to spawn the claude agent subprocess and wire up reader/writer threads.
///
/// Returns Some(AgentRuntime) if spawn succeeded, None if claude was not found or
/// spawn failed (graceful degradation per AGTR-04).
fn try_spawn_agent(
    config: glass_core::agent_runtime::AgentRuntimeConfig,
    activity_rx: std::sync::mpsc::Receiver<glass_core::activity_stream::ActivityEvent>,
    proxy: winit::event_loop::EventLoopProxy<glass_core::event::AppEvent>,
    restart_count: u32,
    last_crash: Option<std::time::Instant>,
    project_root: String,
    initial_message: Option<String>,
) -> Option<AgentRuntime> {
    use std::io::{BufRead, BufReader, BufWriter, Write};
    use std::process::{Command, Stdio};

    // Debug: write spawn diagnostics to file
    let diag_path = dirs::home_dir()
        .map(|h| h.join(".glass").join("agent-diag.txt"))
        .unwrap_or_else(|| std::path::PathBuf::from("agent-diag.txt"));
    let mut diag = format!(
        "timestamp: {:?}\nPATH: {}\n",
        std::time::SystemTime::now(),
        std::env::var("PATH").unwrap_or_else(|_| "NOT SET".to_string()),
    );

    // Write system prompt
    let glass_dir = dirs::home_dir()
        .map(|h| h.join(".glass"))
        .unwrap_or_else(|| std::path::PathBuf::from(".glass"));
    let _ = std::fs::create_dir_all(&glass_dir);

    let prompt_path = glass_dir.join("agent-system-prompt.txt");

    let orchestrator_config = config.orchestrator.as_ref();
    let orchestrator_enabled = orchestrator_config.map(|o| o.enabled).unwrap_or(false);

    let system_prompt = if orchestrator_enabled {
        // Resolve paths relative to the terminal's project root, not Glass's CWD
        let project_dir = std::path::Path::new(&project_root);

        let prd_rel = orchestrator_config
            .map(|o| o.prd_path.clone())
            .unwrap_or_else(|| "PRD.md".to_string());
        let prd_rel = if glass_core::config::validate_config_path(&prd_rel) {
            prd_rel
        } else {
            tracing::warn!(path = %prd_rel, "prd_path contains '..', ignoring and using default");
            "PRD.md".to_string()
        };
        let prd_path = project_dir.join(&prd_rel);
        let prd_content = std::fs::read_to_string(&prd_path)
            .unwrap_or_else(|_| format!("(PRD not found at {})", prd_path.display()));
        // Truncate to ~4000 words
        let word_count = prd_content.split_whitespace().count();
        let prd_truncated: String = prd_content
            .split_whitespace()
            .take(4000)
            .collect::<Vec<_>>()
            .join(" ");
        let prd_truncated = if word_count > 4000 {
            format!(
                "{}\n\n[PRD TRUNCATED — {} words omitted. Read the full file at {} for complete requirements.]",
                prd_truncated,
                word_count - 4000,
                prd_rel,
            )
        } else {
            prd_truncated
        };

        let checkpoint_rel = orchestrator_config
            .map(|o| o.checkpoint_path.clone())
            .unwrap_or_else(|| ".glass/checkpoint.md".to_string());
        let checkpoint_rel = if glass_core::config::validate_config_path(&checkpoint_rel) {
            checkpoint_rel
        } else {
            tracing::warn!(path = %checkpoint_rel, "checkpoint_path contains '..', ignoring and using default");
            ".glass/checkpoint.md".to_string()
        };
        let checkpoint_path = project_dir.join(&checkpoint_rel);
        let checkpoint_content = std::fs::read_to_string(&checkpoint_path)
            .unwrap_or_else(|_| "Fresh start — no previous checkpoint.".to_string());

        let iterations_content = orchestrator::read_iterations_log_truncated(&project_root, 50);
        let iterations_content = if iterations_content.is_empty() {
            "No iterations yet.".to_string()
        } else {
            iterations_content
        };

        let artifact_path = orchestrator_config
            .map(|o| o.completion_artifact.as_str())
            .unwrap_or(".glass/done");

        let orch_mode = orchestrator_config
            .map(|o| o.orchestrator_mode.as_str())
            .unwrap_or("build");

        let mode_instructions = if orch_mode == "audit" {
            r#"ORCHESTRATOR MODE: AUDIT
You have access to ALL Glass MCP tools. Use them to test features interactively:
- glass_tab_create / glass_tab_send / glass_tab_output — spawn tabs, run commands, read results
- glass_history — verify commands are recorded correctly
- glass_undo / glass_file_diff — test undo and snapshot features
- glass_pipe_inspect — verify pipeline capture
- glass_extract_errors — test error extraction
- glass_compressed_context — test token-budgeted summaries
- glass_query / glass_query_trend / glass_query_drill — test SOI queries
- glass_agent_* tools — test multi-agent coordination

AUDIT PROTOCOL:
For each audit area in the plan:
1. TEST: Use MCP tools to test the feature as a user would (create tabs, run commands, check results)
2. VERIFY: Compare actual MCP tool output against expected behavior
3. REPORT: If you find a bug, tell Claude Code exactly what's wrong with file:line references
4. FIX: Tell Claude Code to fix the bug and write a regression test
5. RETEST: Use MCP tools again to verify the fix works
6. COMMIT: Tell Claude Code to commit the fix

You can test features YOURSELF using MCP tools. You CANNOT write code — tell Claude Code to do that.
Do NOT use Bash or Read tools — you don't have them. Use Glass MCP tools instead."#
        } else if orch_mode == "general" {
            r#"ORCHESTRATOR MODE: GENERAL
You are orchestrating a general task (research, planning, design, or mixed work).

ITERATION PROTOCOL:
1. READ the PRD deliverables and requirements
2. INSTRUCT Claude Code on the next deliverable to produce
3. MONITOR progress — is Claude Code making tangible output?
4. REDIRECT if Claude Code goes off-track or stalls
5. CHECK deliverable files exist and have content
6. When all deliverables are complete, respond with GLASS_DONE

Use whatever tools are needed: web search, file creation, shell commands, code.
Track progress by deliverable completion, not test counts.
You CANNOT create files yourself — instruct Claude Code to do it."#
        } else {
            r#"ORCHESTRATOR MODE: BUILD
ITERATION PROTOCOL:
For each feature, guide Claude Code through this cycle:
1. PLAN: Tell Claude Code what to build next and define acceptance criteria
2. IMPLEMENT: Let Claude Code work. Answer its questions with clear decisions.
3. COMMIT: Tell Claude Code to commit before verification
4. VERIFY: Tell Claude Code to write tests and run them
5. DECIDE: Tests pass → move to next feature. Tests fail → tell Claude Code to fix.
   Stuck after 3 attempts → tell Claude Code to revert and try different approach.

You CANNOT implement code yourself — you must instruct Claude Code to do it."#
        };

        let prd_missing = prd_content.starts_with("(PRD not found");
        let kickoff_instructions = if prd_missing {
            "\n\nKICKOFF MODE:\n\
             No PRD file exists yet. The user has been chatting with Claude Code to \
             describe what they want to build. Glass suppressed the orchestration loop \
             while the user was typing. Now the user has gone idle — it's your turn.\n\n\
             Read the terminal context carefully. Pick up from where the conversation \
             left off and push it forward:\n\
             - If the user's goals are clear enough, instruct Claude Code to write the PRD.\n\
             - If critical details are missing, ask ONE clarifying question.\n\
             - Do NOT repeat greetings or re-ask questions the user already answered.\n\n\
             Once the project goals are established, instruct Claude Code to:\n\
             \"Generate a detailed PRD file. Name it descriptively based on the project \
             goal (e.g., PRD-japan-vacation.md). Include:\n\
             - ## Deliverables (list each output file with path)\n\
             - ## Requirements (specific constraints)\n\
             - ## Research Areas (if applicable)\n\
             Write it to disk, then start executing it.\"\n\n\
             After Claude Code writes the PRD, continue with normal orchestration."
        } else {
            "\n\nKICKOFF MODE:\n\
             An existing PRD was found. Glass prompted the user: \"Continue with \
             existing PRD? (y/n)\". Your FIRST instruction to Claude Code: ask the \
             user what they would like to do.\n\
             - If the user wants to continue: proceed with the existing PRD plan.\n\
             - If the user wants to start fresh: ask what they want to build instead. \
             Gather their goals, requirements, and preferences through conversation. \
             Then generate a new PRD and begin execution.\n\n\
             Do NOT start autonomous work until the user has responded."
        };

        format!(
            r#"You are the Glass Agent, collaborating with Claude Code to build a project.
Claude Code is the implementer — it writes code, runs commands, builds features.
You are the reviewer and guide — you make product decisions, ensure quality,
and keep the project moving against the plan.

PROJECT DIRECTORY: {project_root}

PROJECT PLAN:
{prd_truncated}

CURRENT STATUS:
{checkpoint_content}

ITERATION HISTORY:
{iterations_content}

{mode_instructions}{kickoff_instructions}

TASK COMPLETION SIGNAL:
When the implementer is done with a task, have it create the file `{artifact_path}` to signal completion.

ADDITIONAL VERIFICATION DISCOVERY:
If you discover additional verification commands for this project (custom test scripts, integration tests, etc.), report them:
GLASS_VERIFY: {{"commands": [{{"name": "description", "cmd": "command to run"}}]}}

AUTOMATIC METRIC GUARD:
After each iteration, Glass will run verification commands automatically. If changes cause test regressions or build failures, they will be automatically reverted and you will be notified.

CONTEXT REFRESH:
When you've completed 2-3 features and context is getting heavy, emit:
GLASS_CHECKPOINT: {{"completed": "<summary>", "next": "<next PRD item>"}}

PROJECT COMPLETE:
When ALL items in the project plan are implemented, tested, and committed, emit:
GLASS_DONE: <brief summary of what was built>
This stops orchestration and tells Claude Code to do a final commit.

RESPONSE FORMAT:
Respond with ONLY one of:
1. Text to type into the terminal (sent as-is to Claude Code)
2. GLASS_WAIT (Claude Code is still working, check again later)
3. GLASS_CHECKPOINT: {{"completed": "...", "next": "..."}}
4. GLASS_DONE: <summary> (all PRD items complete)
5. GLASS_VERIFY: {{"commands": [{{"name": "...", "cmd": "..."}}]}}

No explanations, no meta-commentary. Just the response."#
        )
    } else {
        r#"You are Glass Agent, an AI assistant integrated into the Glass terminal emulator.

Your role is to monitor terminal activity and propose helpful fixes when commands fail or produce errors.

When you identify an issue worth addressing, emit a structured proposal using this exact format:
GLASS_PROPOSAL: {"description": "Brief description", "action": "shell command or fix", "severity": "error|warning|info"}

Guidelines:
- Only propose when you have high confidence the fix is correct
- Keep descriptions concise (under 80 chars)
- Prefer non-destructive actions
- For file modifications, prefer showing the diff rather than executing directly
- Available tools: glass_query (search command history), glass_context (get terminal context)
- Budget-aware: you are operating under a cost budget, so be concise in responses

Session Continuity:
- After completing each major task milestone, emit a checkpoint:
  GLASS_HANDOFF: {"work_completed":"what you did","work_remaining":"what is left","key_decisions":"important decisions made"}
- When you receive [CONTEXT_LIMIT_WARNING], emit GLASS_HANDOFF immediately before stopping
- The next agent session will receive your handoff as context
"#
        .to_string()
    };

    if let Err(e) = std::fs::write(&prompt_path, system_prompt) {
        tracing::warn!("AgentRuntime: failed to write system prompt: {}", e);
        return None;
    }

    // Generate MCP config JSON so the agent subprocess can discover Glass MCP tools.
    // Always pass --mcp-config to override the user's global ~/.mcp.json.
    let mcp_config_path = (|| -> Option<String> {
        let exe_path = std::env::current_exe().ok()?;
        let mcp_json_path = glass_dir.join("agent-mcp.json");
        let mcp_json = serde_json::json!({
            "mcpServers": {
                "glass": {
                    "command": exe_path.to_string_lossy(),
                    "args": ["mcp", "serve"]
                }
            }
        });
        match std::fs::write(&mcp_json_path, mcp_json.to_string()) {
            Ok(()) => Some(mcp_json_path.to_string_lossy().to_string()),
            Err(e) => {
                tracing::warn!("AgentRuntime: failed to write MCP config JSON: {}", e);
                None
            }
        }
    })()
    .unwrap_or_default();

    let args = glass_core::agent_runtime::build_agent_command_args(
        &config,
        &prompt_path.to_string_lossy(),
        &mcp_config_path,
    );

    // Build the command
    let mut cmd = Command::new("claude");
    cmd.args(&args);
    // Pipe stdout for stream-json. Stderr MUST be null — piping stderr causes
    // a deadlock: the Claude CLI writes to stderr during initialization, fills
    // the pipe buffer before Glass starts reading, and blocks forever.
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    // Windows: suppress the console window for the claude subprocess.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    // Linux: set PR_SET_PDEATHSIG so child is killed when parent dies.
    // (prctl is Linux-specific; macOS does not have it)
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                Ok(())
            });
        }
    }

    // macOS: spawn a watchdog thread in the child process that polls getppid().
    // When the parent (Glass) exits, the child is reparented to launchd (PID 1).
    // The watchdog detects this and calls process::exit so the child does not
    // become a long-running orphan.
    //
    // Implementation note: pre_exec runs *after* fork() but *before* exec(), so
    // code here executes in the child process's context (single-threaded at that
    // point). We use pre_exec to register the watchdog thread via the child's
    // tokio-free environment; the thread is safe because exec() will replace the
    // image immediately after, and if exec() fails the process exits anyway.
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                std::thread::Builder::new()
                    .name("glass-orphan-watchdog".into())
                    .spawn(|| loop {
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        // getppid() returns 1 when reparented to launchd after parent death
                        if unsafe { libc::getppid() } == 1 {
                            std::process::exit(1);
                        }
                    })
                    .ok();
                Ok(())
            });
        }
    }

    let args_str = args.join(" ");
    diag.push_str(&format!("spawn args: claude {}\n", args_str));
    let mut child = match cmd.spawn() {
        Ok(c) => {
            diag.push_str(&format!("spawn SUCCESS pid={}\n", c.id()));
            let _ = std::fs::write(&diag_path, &diag);
            c
        }
        Err(e) => {
            diag.push_str(&format!("spawn FAILED: {}\n", e));
            let _ = std::fs::write(&diag_path, &diag);
            tracing::warn!("AgentRuntime: failed to spawn claude process: {}", e);
            return None;
        }
    };

    // Extract stdin/stdout before storing child (stderr is null).
    let stdout = child.stdout.take().expect("stdout was piped");
    let mut stdin = child.stdin.take().expect("stdin was piped");

    // Claude CLI 2.1.77+ requires a message on stdin before it completes
    // initialization (MCP servers, plugins, init event). Write the initial
    // message immediately so the CLI can proceed.
    {
        use std::io::Write;
        // Always send an initial message — Claude CLI 2.1.77+ won't complete
        // initialization without one. Use the handoff if provided, else GLASS_WAIT.
        let content = initial_message.as_deref().unwrap_or("GLASS_WAIT");
        let json_msg = serde_json::json!({
            "type": "user",
            "message": { "role": "user", "content": content }
        })
        .to_string();
        match writeln!(stdin, "{json_msg}") {
            Ok(()) => {
                let _ = stdin.flush();
            }
            Err(e) => {
                tracing::warn!("AgentRuntime: failed to write initial message: {}", e);
            }
        }
    }

    // Now safe to do slower I/O.
    // Load prior handoff for this project and inject as first user message.
    let prior_handoff_msg = {
        match glass_agent::AgentSessionDb::open_default() {
            Ok(db) => {
                let canonical = std::fs::canonicalize(&project_root)
                    .unwrap_or_else(|_| std::path::PathBuf::from(&project_root));
                let canonical_str = canonical.to_string_lossy().to_string();
                match db.load_prior_handoff(&canonical_str) {
                    Ok(Some(record)) => {
                        let handoff_data = glass_core::agent_runtime::AgentHandoffData {
                            work_completed: record.handoff.work_completed,
                            work_remaining: record.handoff.work_remaining,
                            key_decisions: record.handoff.key_decisions,
                            previous_session_id: record.previous_session_id.clone(),
                        };
                        let msg = glass_core::agent_runtime::format_handoff_as_user_message(
                            &record.session_id,
                            &handoff_data,
                        );
                        tracing::info!(
                            "AgentRuntime: injecting prior session handoff (session_id={})",
                            record.session_id
                        );
                        Some(msg)
                    }
                    Ok(None) => {
                        tracing::debug!("AgentRuntime: no prior handoff found for project");
                        None
                    }
                    Err(e) => {
                        tracing::warn!("AgentRuntime: failed to load prior handoff: {}", e);
                        None
                    }
                }
            }
            Err(e) => {
                tracing::warn!("AgentRuntime: failed to open session db: {}", e);
                None
            }
        }
    };

    let proxy_reader = proxy.clone();
    let mode = config.mode;
    let project_root_reader = project_root.clone();

    // Reader thread: parses claude stdout JSON lines and routes AppEvents
    std::thread::Builder::new()
        .name("glass-agent-reader".into())
        .spawn(move || {
            let mut current_session_id = String::new();
            // Buffer the last assistant text so we only emit OrchestratorResponse
            // on "result" (end of conversation turn), not on intermediate tool-loop text.
            let mut buffered_response: Option<String> = None;
            let mut tool_id_to_name: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                if line.trim().is_empty() {
                    continue;
                }
                let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) else {
                    continue;
                };
                match val.get("type").and_then(|t| t.as_str()) {
                    Some("system") => {
                        if val.get("subtype").and_then(|s| s.as_str()) == Some("init") {
                            if let Some(id) = val.get("session_id").and_then(|v| v.as_str()) {
                                current_session_id = id.to_string();
                                tracing::info!("AgentRuntime: captured session_id={}", id);
                            }
                        }
                    }
                    Some("result") => {
                        let cost_usd =
                            glass_core::agent_runtime::parse_cost_from_result(&line).unwrap_or(0.0);
                        let _ = proxy_reader
                            .send_event(glass_core::event::AppEvent::AgentQueryResult { cost_usd });

                        // Emit the buffered response now that the turn is complete.
                        // This is the FINAL assistant text after all tool calls.
                        if let Some(response) = buffered_response.take() {
                            let _ = proxy_reader.send_event(
                                glass_core::event::AppEvent::OrchestratorResponse {
                                    response,
                                },
                            );
                        }
                    }
                    Some("assistant") => {
                        // Concatenate all text content blocks
                        let mut full_text = String::new();
                        if let Some(content) = val.get("message").and_then(|m| m.get("content")) {
                            if let Some(arr) = content.as_array() {
                                for block in arr {
                                    match block.get("type").and_then(|t| t.as_str()) {
                                        Some("text") => {
                                            if let Some(text) =
                                                block.get("text").and_then(|t| t.as_str())
                                            {
                                                full_text.push_str(text);
                                            }
                                        }
                                        Some("thinking") => {
                                            if let Some(text) =
                                                block.get("thinking").and_then(|t| t.as_str())
                                            {
                                                let _ = proxy_reader.send_event(
                                                    glass_core::event::AppEvent::OrchestratorThinking {
                                                        text: text.to_string(),
                                                    },
                                                );
                                            }
                                        }
                                        Some("tool_use") => {
                                            let name = block
                                                .get("name")
                                                .and_then(|n| n.as_str())
                                                .unwrap_or("?");
                                            let id = block
                                                .get("id")
                                                .and_then(|i| i.as_str())
                                                .unwrap_or("");
                                            let input = block
                                                .get("input")
                                                .map(|i| i.to_string())
                                                .unwrap_or_default();
                                            let summary =
                                                orchestrator_events::truncate_display(&input, 200);
                                            if !id.is_empty() {
                                                tool_id_to_name
                                                    .insert(id.to_string(), name.to_string());
                                            }
                                            let _ = proxy_reader.send_event(
                                                glass_core::event::AppEvent::OrchestratorToolCall {
                                                    name: name.to_string(),
                                                    params_summary: summary,
                                                },
                                            );
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        if let Some(proposal) =
                            glass_core::agent_runtime::extract_proposal(&full_text)
                        {
                            let _ = proxy_reader
                                .send_event(glass_core::event::AppEvent::AgentProposal(proposal));
                        }
                        if let Some((handoff, raw_json)) =
                            glass_core::agent_runtime::extract_handoff(&full_text)
                        {
                            let _ = proxy_reader.send_event(
                                glass_core::event::AppEvent::AgentHandoff {
                                    session_id: current_session_id.clone(),
                                    handoff,
                                    project_root: project_root_reader.clone(),
                                    raw_json,
                                },
                            );
                        }
                        // Buffer text for orchestrator — only emitted on "result"
                        if !full_text.is_empty() {
                            buffered_response = Some(full_text);
                        }
                    }
                    Some("user") => {
                        // Tool results for orchestrator transcript
                        if let Some(content) = val.get("message").and_then(|m| m.get("content")) {
                            if let Some(arr) = content.as_array() {
                                for block in arr {
                                    if block.get("type").and_then(|t| t.as_str())
                                        == Some("tool_result")
                                    {
                                        let tool_use_id = block
                                            .get("tool_use_id")
                                            .and_then(|t| t.as_str())
                                            .unwrap_or("?");
                                        let tool_name = tool_id_to_name
                                            .remove(tool_use_id)
                                            .unwrap_or_else(|| tool_use_id.to_string());
                                        let content_text = match block.get("content") {
                                            Some(c) if c.is_string() => {
                                                c.as_str().unwrap_or("").to_string()
                                            }
                                            Some(c) if c.is_array() => c
                                                .as_array()
                                                .unwrap()
                                                .iter()
                                                .filter_map(|b| {
                                                    b.get("text").and_then(|t| t.as_str())
                                                })
                                                .collect::<Vec<_>>()
                                                .join("\n"),
                                            _ => String::new(),
                                        };
                                        let summary = orchestrator_events::truncate_display(
                                            &content_text,
                                            200,
                                        );
                                        let _ = proxy_reader.send_event(
                                            glass_core::event::AppEvent::OrchestratorToolResult {
                                                name: tool_name,
                                                output_summary: summary,
                                            },
                                        );
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            // stdout closed -- signal crash
            let _ = proxy_reader.send_event(glass_core::event::AppEvent::AgentCrashed);
        })
        .ok();

    // Shared stdin writer for both activity thread and orchestrator
    let shared_writer = std::sync::Arc::new(parking_lot::Mutex::new(BufWriter::new(stdin)));
    let writer_clone = std::sync::Arc::clone(&shared_writer);

    // Writer thread: drains activity_stream_rx and writes JSON lines to claude stdin
    let cooldown_secs = config.cooldown_secs;
    std::thread::Builder::new()
        .name("glass-agent-writer".into())
        .spawn(move || {
            let mut last_sent: Option<std::time::Instant> = None;
            let cooldown = std::time::Duration::from_secs(cooldown_secs);

            // Inject prior session handoff as first message before processing new events
            if let Some(ref msg) = prior_handoff_msg {
                {
                    let mut w = writer_clone.lock();
                    let _ = writeln!(w, "{msg}");
                    let _ = w.flush();
                }
            }

            for event in activity_rx.iter() {
                // Mode gate: skip events that don't pass the severity filter
                if !glass_core::agent_runtime::should_send_in_mode(mode, &event.severity) {
                    continue;
                }

                // Cooldown gate: skip if within cooldown window
                if let Some(last) = last_sent {
                    if last.elapsed() < cooldown {
                        continue;
                    }
                }

                let msg = glass_core::agent_runtime::format_activity_as_user_message(&event);
                {
                    let mut w = writer_clone.lock();
                    if writeln!(w, "{msg}").is_err() || w.flush().is_err() {
                        // BrokenPipe: child process died
                        break;
                    }
                }
                last_sent = Some(std::time::Instant::now());
            }
        })
        .ok();

    tracing::info!(
        "AgentRuntime: claude subprocess spawned (mode={:?}, restart_count={})",
        config.mode,
        restart_count
    );

    // AGTC-05: Register with coordination DB (soft errors -- agent starts regardless).
    let (coord_agent_id, coord_nonce, coord_project_root) = {
        // Use dunce::canonicalize (via glass_coordination) to avoid \\?\ UNC prefix on Windows.
        // This must match the path format used by the coordination poller.
        let canonical_str =
            glass_coordination::canonicalize_path(std::path::Path::new(&project_root))
                .unwrap_or_else(|_| project_root.clone());
        match glass_coordination::CoordinationDb::open_default() {
            Ok(mut db) => {
                // Prune stale agents (dead PIDs or expired heartbeats) before registering.
                // Timeout of 120s: agents that haven't heartbeated in 2 minutes are stale.
                match db.prune_stale(120) {
                    Ok(pruned) if !pruned.is_empty() => {
                        tracing::info!(
                            "AgentRuntime: pruned {} stale agent(s): {:?}",
                            pruned.len(),
                            pruned
                        );
                    }
                    Err(e) => {
                        tracing::warn!("AgentRuntime: prune_stale failed (soft): {}", e);
                    }
                    _ => {}
                }
                let cwd = std::env::current_dir()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| canonical_str.clone());
                match db.register("glass-agent", "claude-code", &canonical_str, &cwd, None) {
                    Ok((agent_id, nonce)) => {
                        // Advisory lock on the project root directory
                        let lock_path = std::path::PathBuf::from(&canonical_str);
                        match db.lock_files(&agent_id, &[lock_path], Some("agent session"), &nonce)
                        {
                            Ok(_) => tracing::info!(
                                "AgentRuntime: registered with coordination (id={})",
                                agent_id
                            ),
                            Err(e) => tracing::warn!(
                                "AgentRuntime: coordination lock failed (soft): {}",
                                e
                            ),
                        }
                        (Some(agent_id), Some(nonce), Some(canonical_str))
                    }
                    Err(e) => {
                        tracing::warn!(
                            "AgentRuntime: coordination registration failed (soft): {}",
                            e
                        );
                        (None, None, None)
                    }
                }
            }
            Err(e) => {
                tracing::warn!("AgentRuntime: failed to open coordination DB (soft): {}", e);
                (None, None, None)
            }
        }
    };

    Some(AgentRuntime {
        child: Some(child),
        cooldown: glass_core::agent_runtime::CooldownTracker::new(config.cooldown_secs),
        budget: glass_core::agent_runtime::BudgetTracker::new(config.max_budget_usd),
        config,
        restart_count,
        last_crash,
        agent_id: coord_agent_id,
        coord_nonce,
        project_root: coord_project_root,
        orchestrator_writer: Some(shared_writer),
        generation: 0, // set by caller after spawn
    })
}

/// Resolve a tab by either tab_index or session_id from IPC params.
/// Returns the tab index or an error string.
fn resolve_tab_index(mux: &SessionMux, params: &serde_json::Value) -> Result<usize, String> {
    let tab_index = params.get("tab_index").and_then(|v| v.as_u64());
    let session_id = params.get("session_id").and_then(|v| v.as_u64());
    match (tab_index, session_id) {
        (Some(idx), None) => {
            let idx = idx as usize;
            if idx < mux.tab_count() {
                Ok(idx)
            } else {
                Err(format!(
                    "Tab index {} out of range (0..{})",
                    idx,
                    mux.tab_count()
                ))
            }
        }
        (None, Some(sid)) => {
            let target = SessionId::new(sid);
            mux.tabs()
                .iter()
                .enumerate()
                .find(|(_, tab)| tab.session_ids().contains(&target))
                .map(|(i, _)| i)
                .ok_or_else(|| format!("No tab contains session {}", sid))
        }
        (Some(_), Some(_)) => Err("Provide either tab_index or session_id, not both".into()),
        (None, None) => Err("Provide tab_index or session_id".into()),
    }
}

/// Extract the last `n` text lines from a terminal grid.
/// Create a `git` command with `CREATE_NO_WINDOW` on Windows to prevent console flashing.
fn git_cmd() -> std::process::Command {
    let mut cmd = std::process::Command::new("git");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd
}

fn extract_term_lines(term: &Arc<FairMutex<Term<EventProxy>>>, n: usize) -> Vec<String> {
    let term = term.lock();
    let grid = term.grid();
    let total = grid.screen_lines();
    let mut lines = Vec::with_capacity(total);
    for i in 0..total {
        let row = &grid[Line(i as i32)];
        let text: String = (0..grid.columns())
            .map(|col| row[Column(col)].c)
            .collect::<String>();
        lines.push(text.trim_end().to_string());
    }
    // Trim trailing empty lines
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let start = lines.len().saturating_sub(n);
    lines[start..].to_vec()
}

/// Fetch SOI context for the most recent command in a session.
/// Returns (exit_code, soi_summary, soi_error_strings).
fn fetch_latest_soi_context(
    session: &glass_mux::session::Session,
) -> (Option<i32>, Option<String>, Vec<String>) {
    // Get exit code from most recent completed block
    let exit_code = session
        .block_manager
        .blocks()
        .iter()
        .rev()
        .find(|b| b.state == glass_terminal::BlockState::Complete)
        .and_then(|b| b.exit_code);

    let command_id = match session.last_command_id {
        Some(id) => id,
        None => return (exit_code, None, Vec::new()),
    };

    let db = match session.history_db.as_ref() {
        Some(db) => db,
        None => return (exit_code, None, Vec::new()),
    };

    let conn = db.conn();

    let soi_summary = glass_history::soi::get_output_summary(conn, command_id)
        .ok()
        .flatten()
        .map(|s| s.one_line);

    let soi_errors =
        glass_history::soi::get_output_records(conn, command_id, Some("Error"), None, None, 100)
            .ok()
            .unwrap_or_default()
            .into_iter()
            .map(|r| {
                let file = r.file_path.as_deref().unwrap_or("");
                let data_preview = r.data.chars().take(200).collect::<String>();
                if file.is_empty() {
                    data_preview
                } else {
                    format!("{file} {data_preview}")
                }
            })
            .collect();

    (exit_code, soi_summary, soi_errors)
}

/// Extract test pass/fail counts from command output using common patterns.
fn parse_test_counts_from_output(output: &str) -> (Option<u32>, Option<u32>) {
    use std::sync::OnceLock;

    static RE_RUST: OnceLock<regex::Regex> = OnceLock::new();
    static RE_JEST: OnceLock<regex::Regex> = OnceLock::new();
    static RE_PASSED: OnceLock<regex::Regex> = OnceLock::new();
    static RE_FAILED: OnceLock<regex::Regex> = OnceLock::new();

    let re_rust = RE_RUST.get_or_init(|| regex::Regex::new(r"(\d+) passed; (\d+) failed").unwrap());
    let re_jest = RE_JEST
        .get_or_init(|| regex::Regex::new(r"Tests:\s*(?:(\d+) failed,\s*)?(\d+) passed").unwrap());
    let re_passed = RE_PASSED.get_or_init(|| regex::Regex::new(r"(\d+) passed").unwrap());
    let re_failed = RE_FAILED.get_or_init(|| regex::Regex::new(r"(\d+) failed").unwrap());

    // Rust: "test result: ok. 45 passed; 2 failed; 0 ignored"
    if let Some(caps) = re_rust.captures(output) {
        let passed = caps.get(1).and_then(|m| m.as_str().parse().ok());
        let failed = caps.get(2).and_then(|m| m.as_str().parse().ok());
        return (passed, failed);
    }
    // Jest/Node: "Tests: 2 failed, 45 passed, 47 total"
    if let Some(caps) = re_jest.captures(output) {
        let failed = caps.get(1).and_then(|m| m.as_str().parse().ok());
        let passed = caps.get(2).and_then(|m| m.as_str().parse().ok());
        return (passed, failed.or(Some(0)));
    }
    // Pytest: "5 passed, 2 failed" or "5 passed"
    if let Some(caps) = re_passed.captures(output) {
        let passed = caps.get(1).and_then(|m| m.as_str().parse().ok());
        let failed = re_failed
            .captures(output)
            .and_then(|c| c.get(1))
            .and_then(|m| m.as_str().parse().ok())
            .or(Some(0));
        return (passed, failed);
    }
    // Go: "ok" or "FAIL" — no counts, exit code only
    (None, None)
}

/// Parse numbered instructions from agent text (e.g., "1. Do X\n2. Do Y").
/// Returns individual instructions if 2+ are found, otherwise the original text.
fn parse_numbered_instructions(text: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        // Match "N.", "N)", "NN.", "NN)" etc. — handle multi-digit numbered items
        let is_numbered = {
            let mut chars = trimmed.chars();
            let mut has_digit = false;
            let mut found = false;
            for ch in chars.by_ref() {
                if ch.is_ascii_digit() {
                    has_digit = true;
                } else {
                    found = has_digit && (ch == '.' || ch == ')');
                    break;
                }
            }
            found
        };
        if is_numbered {
            if !current.trim().is_empty() {
                items.push(current.trim().to_string());
            }
            current = trimmed.to_string();
            continue;
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
    }
    if !current.trim().is_empty() {
        items.push(current.trim().to_string());
    }
    if items.len() >= 2 {
        items
    } else {
        vec![text.to_string()]
    }
}

/// Parse file paths from `git diff --stat` output.
/// Each line looks like: " src/main.rs | 42 +++---"
fn parse_diff_stat_files(diff_stat: &str) -> Vec<String> {
    diff_stat
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.contains('|') {
                Some(trimmed.split('|').next()?.trim().to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Spawn a notify file watcher that sends `OrchestratorSilence` when the
/// artifact file at `artifact_path` (relative to `cwd`) is created or modified.
///
/// The thread parks itself after setting up the watcher and stays alive until
/// explicitly unparked (when the orchestrator is disabled).
fn start_artifact_watcher(
    artifact_path: &str,
    cwd: &str,
    proxy: EventLoopProxy<AppEvent>,
    window_id: WindowId,
    session_id: SessionId,
) -> Option<std::thread::JoinHandle<()>> {
    if artifact_path.is_empty() {
        return None;
    }
    let full_path = std::path::PathBuf::from(cwd).join(artifact_path);
    let target_filename = full_path.file_name()?.to_owned();
    let watch_dir = full_path.parent()?.to_path_buf();

    // Ensure parent directory exists so the watcher can be created.
    let _ = std::fs::create_dir_all(&watch_dir);

    let handle = std::thread::Builder::new()
        .name("Glass artifact watcher".into())
        .spawn(move || {
            use notify::{recommended_watcher, RecursiveMode, Watcher};
            let proxy_clone = proxy;
            let target = target_filename;
            let mut watcher = match recommended_watcher(move |res: Result<notify::Event, _>| {
                if let Ok(ev) = res {
                    if matches!(
                        ev.kind,
                        notify::EventKind::Create(_) | notify::EventKind::Modify(_)
                    ) {
                        let is_target_file = ev.paths.iter().any(|p| {
                            p.file_name()
                                .map(|n| n == target.as_os_str())
                                .unwrap_or(false)
                        });
                        if is_target_file {
                            let _ = proxy_clone.send_event(AppEvent::OrchestratorSilence {
                                window_id,
                                session_id,
                            });
                        }
                    }
                }
            }) {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!("Failed to create artifact watcher: {e}");
                    return;
                }
            };
            if let Err(e) = watcher.watch(&watch_dir, RecursiveMode::NonRecursive) {
                tracing::warn!("Failed to watch artifact dir: {e}");
                return;
            }
            std::thread::park(); // Keep watcher alive; unpark to shut down
        })
        .ok()?;

    Some(handle)
}

/// Parse .glass/iterations.tsv into structured entries for the overlay.
fn parse_iteration_log(project_root: &str) -> Vec<glass_renderer::IterationLogEntry> {
    let path = std::path::Path::new(project_root)
        .join(".glass")
        .join("iterations.tsv");
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    content
        .lines()
        .skip(1) // skip header
        .filter_map(|line| {
            let cols: Vec<&str> = line.split('\t').collect();
            if cols.len() < 6 {
                return None;
            }
            Some(glass_renderer::IterationLogEntry {
                iteration: cols[0].trim().parse().unwrap_or(0),
                commit: cols[1].trim().to_string(),
                feature: cols[2].trim().to_string(),
                status: cols[4].trim().to_string(),
                description: cols[5].trim().to_string(),
            })
        })
        .collect()
}

impl Processor {
    /// Get the CWD of the focused session, falling back to the process CWD.
    fn get_focused_cwd(&self) -> String {
        self.windows
            .values()
            .next()
            .and_then(|ctx| ctx.session_mux.focused_session())
            .map(|s| s.status.cwd().to_string())
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            })
    }

    /// Run the feedback loop on_run_end, applying any config changes and logging results.
    fn run_feedback_on_end(&mut self) {
        if let Some(feedback_state) = self.feedback_state.take() {
            let run_data = self.build_feedback_run_data();
            let result = glass_feedback::on_run_end(feedback_state, run_data);
            if !result.config_changes.is_empty() {
                self.feedback_write_pending = true;
                if let Some(config_path) =
                    dirs::home_dir().map(|h| h.join(".glass").join("config.toml"))
                {
                    for (field, _old, new_val) in &result.config_changes {
                        let _ = glass_core::config::update_config_field(
                            &config_path,
                            Some("agent.orchestrator"),
                            field,
                            new_val,
                        );
                    }
                }
            }
            tracing::info!(
                "Feedback: {} findings, {} promoted, {} rejected, {} config changes",
                result.findings.len(),
                result.rules_promoted.len(),
                result.rules_rejected.len(),
                result.config_changes.len(),
            );

            // Apply script lifecycle transitions based on regression result.
            let regressed = matches!(
                result.regression,
                Some(glass_feedback::regression::RegressionResult::Regressed { .. })
            );
            self.script_bridge.on_feedback_run_end(regressed);

            // Spawn LLM analysis if prompt was generated
            if let Some(prompt) = result.llm_prompt {
                // Capture project root and max_prompt_hints NOW so the response
                // handler uses the correct values even if the user switches
                // projects before the ephemeral agent completes.
                self.feedback_llm_project_root = Some(self.orchestrator.project_root.clone());
                self.feedback_llm_max_hints = self
                    .config
                    .agent
                    .as_ref()
                    .and_then(|a| a.orchestrator.as_ref())
                    .map(|o| o.max_prompt_hints)
                    .unwrap_or(10);

                let request = ephemeral_agent::EphemeralAgentRequest {
                    system_prompt: "You are analyzing an orchestrator run for qualitative issues. Respond ONLY in the structured format requested.".to_string(),
                    user_message: prompt,
                    timeout: std::time::Duration::from_secs(60),
                    purpose: glass_core::event::EphemeralPurpose::FeedbackAnalysis,
                };
                if let Err(e) = ephemeral_agent::spawn_ephemeral_agent(request, self.proxy.clone())
                {
                    tracing::warn!("Feedback LLM: ephemeral spawn failed: {e:?}");
                }
            }

            // Tier 4: spawn ephemeral agent for script generation
            if let Some(script_prompt) = result.script_prompt {
                // Suppress Tier 4 after too many consecutive parse failures
                // to avoid wasting ephemeral agent resources.
                if self.script_gen_parse_failures >= 3 {
                    tracing::warn!(
                        "Tier 4: suppressing script generation — {} consecutive parse failures",
                        self.script_gen_parse_failures
                    );
                } else {
                    tracing::info!(
                        "Tier 4: spawning ephemeral agent for script generation ({} chars)",
                        script_prompt.len()
                    );
                    self.script_gen_project_root = Some(self.orchestrator.project_root.clone());
                    let request = ephemeral_agent::EphemeralAgentRequest {
                        system_prompt: "You are generating a Rhai script for the Glass terminal emulator's self-improvement system. Respond ONLY in the structured format requested.".to_string(),
                        user_message: script_prompt,
                        timeout: std::time::Duration::from_secs(60),
                        purpose: glass_core::event::EphemeralPurpose::ScriptGeneration,
                    };
                    if let Err(e) =
                        ephemeral_agent::spawn_ephemeral_agent(request, self.proxy.clone())
                    {
                        tracing::warn!("Tier 4 script generation: ephemeral spawn failed: {e:?}");
                    }
                }
            }
        }
    }

    /// Build a RunData snapshot from the current orchestrator state for the feedback loop.
    fn build_feedback_run_data(&self) -> glass_feedback::RunData {
        let root = &self.orchestrator.project_root;

        // Compute avg idle time from iteration timestamps
        let avg_idle = if self.orchestrator.feedback_iteration_timestamps.len() >= 2 {
            let ts = &self.orchestrator.feedback_iteration_timestamps;
            let total: f64 = ts
                .windows(2)
                .map(|w| w[1].duration_since(w[0]).as_secs_f64())
                .sum();
            total / (ts.len() - 1) as f64
        } else {
            0.0
        };

        // Collect fingerprint hashes for sequence analysis
        let fingerprint_seq: Vec<u64> = self
            .orchestrator
            .recent_fingerprints
            .iter()
            .map(|fp| fp.terminal_hash)
            .collect();

        // Read PRD content for scope creep detection
        let prd_content = self
            .config
            .agent
            .as_ref()
            .and_then(|a| a.orchestrator.as_ref())
            .and_then(|o| {
                let prd_path = std::path::Path::new(root).join(&o.prd_path);
                std::fs::read_to_string(prd_path).ok()
            });

        // Get git diff stat for scope creep detection
        let git_diff_stat = git_cmd()
            .args(["diff", "--stat"])
            .current_dir(root)
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    String::from_utf8(o.stdout).ok()
                } else {
                    None
                }
            });

        // Get git log for post-mortem
        let git_log = git_cmd()
            .args(["log", "--oneline", "-20"])
            .current_dir(root)
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    String::from_utf8(o.stdout).ok()
                } else {
                    None
                }
            });

        glass_feedback::RunData {
            project_root: root.clone(),
            iterations: self.orchestrator.iteration,
            duration_secs: self
                .orchestrator_activated_at
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(0),
            kickoff_duration_secs: 0,
            iterations_tsv: std::fs::read_to_string(
                std::path::Path::new(root)
                    .join(".glass")
                    .join("iterations.tsv"),
            )
            .unwrap_or_default(),
            revert_count: self
                .orchestrator
                .metric_baseline
                .as_ref()
                .map(|m| m.revert_count)
                .unwrap_or(0),
            keep_count: self
                .orchestrator
                .metric_baseline
                .as_ref()
                .map(|m| m.keep_count)
                .unwrap_or(0),
            stuck_count: self.orchestrator.feedback_stuck_count,
            checkpoint_count: self.orchestrator.feedback_checkpoint_count,
            waste_count: self.orchestrator.feedback_waste_iterations,
            commit_count: self.orchestrator.feedback_commit_count,
            completion_reason: self.orchestrator.feedback_completion_reason.clone(),
            prd_content,
            git_log,
            git_diff_stat,
            reverted_files: self.orchestrator.feedback_reverted_files.clone(),
            verify_pass_fail_sequence: self.orchestrator.feedback_verify_sequence.clone(),
            agent_responses: self.orchestrator.feedback_agent_responses.clone(),
            silence_interruptions: 0,
            fast_trigger_during_output: self.orchestrator.feedback_fast_trigger_during_output,
            avg_idle_between_iterations_secs: avg_idle,
            fingerprint_sequence: fingerprint_seq,
            config_silence_timeout: self
                .config
                .agent
                .as_ref()
                .and_then(|a| a.orchestrator.as_ref())
                .map(|o| o.silence_timeout_secs)
                .unwrap_or(30),
            config_max_retries: self
                .config
                .agent
                .as_ref()
                .and_then(|a| a.orchestrator.as_ref())
                .map(|o| o.max_retries_before_stuck)
                .unwrap_or(3),
        }
    }

    /// Kill the current agent and respawn with a fresh system prompt.
    /// `handoff_content` is the initial message sent to the new agent.
    fn respawn_orchestrator_agent(&mut self, cwd: &str, handoff_content: String) {
        self.orchestrator_event_buffer.push(
            orchestrator_events::OrchestratorEvent::AgentRespawn {
                reason: "checkpoint".to_string(),
            },
            self.orchestrator.iteration,
        );

        // Kill old agent and increment generation to ignore stale AgentCrashed events
        self.agent_runtime = None;
        self.agent_generation += 1;

        // Clear instruction buffer on respawn (fresh context)
        self.orchestrator.instruction_buffer.clear();

        // Create new activity channel
        let activity_config = glass_core::activity_stream::ActivityStreamConfig::default();
        let (new_tx, new_rx) = glass_core::activity_stream::create_channel(&activity_config);
        self.activity_stream_tx = Some(new_tx);

        // Build agent config — mark orchestrator enabled since this function
        // is only called when the orchestrator is active at runtime
        let agent_config = self
            .config
            .agent
            .clone()
            .map(|mut a| {
                // Ensure the orchestrator section is marked enabled so try_spawn_agent
                // generates the orchestrator system prompt (not the basic assistant prompt).
                // The TOML config may have enabled=false since it's toggled at runtime.
                if let Some(ref mut orch) = a.orchestrator {
                    orch.enabled = true;
                }
                glass_core::agent_runtime::AgentRuntimeConfig {
                    mode: a.mode,
                    max_budget_usd: a.max_budget_usd,
                    cooldown_secs: a.cooldown_secs,
                    allowed_tools: a.allowed_tools,
                    orchestrator: a.orchestrator,
                }
            })
            .unwrap_or_default();

        // Spawn new agent with handoff as the initial stdin message.
        // Claude CLI 2.1.77+ needs a message on stdin before it completes init.
        self.agent_runtime = try_spawn_agent(
            agent_config,
            new_rx,
            self.proxy.clone(),
            0,
            None,
            cwd.to_string(),
            Some(handoff_content),
        );

        // If spawn failed, deactivate orchestrator — can't orchestrate without an agent
        if self.agent_runtime.is_none() {
            tracing::error!("Orchestrator: agent respawn failed — deactivating orchestrator");
            {
                let mut event = glass_scripting::HookEventData::new();
                event.set("iterations", self.orchestrator.iteration as i64);
                fire_hook_on_bridge(
                    &mut self.script_bridge,
                    &self.orchestrator.project_root,
                    glass_scripting::HookPoint::OrchestratorRunEnd,
                    &event,
                );
            }
            self.run_feedback_on_end();
            self.orchestrator.active = false;
            self.orchestrator.response_pending = false;
            return;
        }

        // Handoff was sent as initial_message in try_spawn_agent.
        // Suppress silence trigger until the agent responds.
        self.orchestrator.response_pending = true;

        // Tag with current generation so stale AgentCrashed events are ignored
        if let Some(ref mut rt) = self.agent_runtime {
            rt.generation = self.agent_generation;
        }
        tracing::info!(
            "Orchestrator: respawned agent gen={} for {}",
            self.agent_generation,
            cwd
        );
    }

    /// Gather data and start checkpoint synthesis (or fallback).
    fn trigger_checkpoint_synthesis(&mut self, completed: &str, next: &str) {
        self.orchestrator.feedback_checkpoint_count += 1;
        let cwd = self.orchestrator.project_root.clone();
        let (git_log, git_diff_stat, git_diff_names) = checkpoint_synth::gather_git_state(&cwd);
        let iterations_tsv = orchestrator::read_iterations_log_truncated(&cwd, 50);
        let metric_summary =
            checkpoint_synth::build_metric_summary(self.orchestrator.metric_baseline.as_ref());

        let data = checkpoint_synth::CheckpointData {
            soi_errors: Vec::new(),
            iterations_tsv,
            git_log,
            git_diff_stat,
            git_diff_names,
            metric_summary,
            prd_content: String::new(),
            coverage_gaps: self.orchestrator.coverage_gaps_context.clone(),
            completed: completed.to_string(),
            next: next.to_string(),
        };

        let fallback = checkpoint_synth::build_fallback_checkpoint(&data);
        self.orchestrator.begin_synthesis(completed, next, fallback);

        // Check usage before spawning ephemeral agent
        let usage_ok = self
            .usage_state
            .as_ref()
            .and_then(|s| s.lock().ok())
            .map(|s| {
                s.data
                    .as_ref()
                    .map(|d| d.five_hour_utilization < 0.8)
                    .unwrap_or(true)
            })
            .unwrap_or(true);

        if usage_ok {
            let request = ephemeral_agent::EphemeralAgentRequest {
                system_prompt: checkpoint_synth::synthesis_system_prompt(),
                user_message: checkpoint_synth::synthesis_user_message(&data),
                timeout: std::time::Duration::from_secs(orchestrator::SYNTHESIS_TIMEOUT_SECS),
                purpose: glass_core::event::EphemeralPurpose::CheckpointSynthesis,
            };
            if let Err(e) = ephemeral_agent::spawn_ephemeral_agent(request, self.proxy.clone()) {
                tracing::warn!("Ephemeral spawn failed: {e:?}, using fallback");
                self.write_checkpoint_and_respawn(&cwd);
            }
        } else {
            tracing::info!("Usage above threshold, using fallback checkpoint");
            self.write_checkpoint_and_respawn(&cwd);
        }

        // Quality verification for general mode projects
        let orchestrator_mode = self
            .config
            .agent
            .as_ref()
            .and_then(|a| a.orchestrator.as_ref())
            .map(|o| o.orchestrator_mode.as_str())
            .unwrap_or("build");

        if orchestrator_mode == "general" && !self.orchestrator.prd_deliverable_files.is_empty() {
            // Read deliverable file contents
            let mut deliverable_content = String::new();
            for file in &self.orchestrator.prd_deliverable_files {
                let path = std::path::Path::new(&cwd).join(file);
                if let Ok(content) = std::fs::read_to_string(&path) {
                    deliverable_content.push_str(&format!("### {file}\n\n{content}\n\n"));
                }
            }

            if !deliverable_content.is_empty() {
                // Read PRD requirements
                let prd_content = self
                    .config
                    .agent
                    .as_ref()
                    .and_then(|a| a.orchestrator.as_ref())
                    .and_then(|o| {
                        let prd_path = std::path::Path::new(&cwd).join(&o.prd_path);
                        std::fs::read_to_string(prd_path).ok()
                    })
                    .unwrap_or_default();

                if !prd_content.is_empty() {
                    let quality_request = ephemeral_agent::EphemeralAgentRequest {
                        system_prompt: glass_feedback::quality::quality_system_prompt(),
                        user_message: glass_feedback::quality::quality_user_message(
                            &deliverable_content,
                            &prd_content,
                            self.orchestrator.last_quality_score,
                        ),
                        timeout: std::time::Duration::from_secs(90),
                        purpose: glass_core::event::EphemeralPurpose::QualityVerification,
                    };

                    if let Err(e) =
                        ephemeral_agent::spawn_ephemeral_agent(quality_request, self.proxy.clone())
                    {
                        tracing::warn!("Quality verification spawn failed: {e:?}");
                    }
                }
            }
        }
    }

    /// Write the cached fallback checkpoint and respawn.
    fn write_checkpoint_and_respawn(&mut self, cwd: &str) {
        let content = self
            .orchestrator
            .cached_checkpoint_fallback
            .take()
            .unwrap_or_else(|| "(no checkpoint data available)".to_string());
        self.write_checkpoint_content_and_respawn(cwd, &content);
    }

    /// Write explicit checkpoint content and respawn the agent.
    fn write_checkpoint_content_and_respawn(&mut self, cwd: &str, content: &str) {
        let cp_path = std::path::Path::new(cwd)
            .join(".glass")
            .join("checkpoint.md");
        if let Some(parent) = cp_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&cp_path, content) {
            tracing::warn!("Failed to write checkpoint.md: {e}");
        }
        self.orchestrator.checkpoint_phase = orchestrator::CheckpointPhase::Idle;
        self.orchestrator.cached_checkpoint_fallback = None;
        self.orchestrator.reset_stuck();
        // Respawn the agent
        let handoff =
            "Resume from checkpoint. Read .glass/checkpoint.md and continue.\n".to_string();
        self.respawn_orchestrator_agent(cwd, handoff);
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

        let mut attrs = Window::default_attributes().with_title("Glass");

        // Load app icon from embedded PNG for the window title bar and taskbar
        if let Some(icon) = load_window_icon() {
            attrs = attrs.with_window_icon(Some(icon));
        }

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .unwrap_or_else(|e| show_fatal_error(&format!("Failed to create window: {e}"))),
        );

        // Parallelize font discovery with GPU init — FontSystem::new() enumerates
        // all system fonts (~35ms) and doesn't need the GPU device.
        let font_handle = std::thread::spawn(FontSystem::new);

        // wgpu init is async; block via pollster from this sync callback
        let renderer = pollster::block_on(GlassRenderer::new(Arc::clone(&window)));

        // Join font thread — should already be done since GPU init takes longer
        let font_system = font_handle.join().unwrap_or_else(|_| {
            tracing::warn!("Font system thread panicked, using default");
            FontSystem::new()
        });

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
        // Subtract 2 lines for the status bar + tab bar so the PTY resize reflects actual content area.
        let (cell_w, cell_h) = frame_renderer.cell_size();
        let size = window.inner_size();
        let num_cols = ((size.width as f32 - SCROLLBAR_WIDTH) / cell_w)
            .floor()
            .max(1.0) as u16;
        let num_lines = ((size.height as f32 / cell_h).floor().max(2.0) as u16).saturating_sub(2);

        tracing::info!(
            "Font metrics: cell={}x{} grid={}x{} (status bar + tab bar reserve 2 lines) scale={}",
            cell_w,
            cell_h,
            num_cols,
            num_lines,
            scale_factor
        );

        // Create the initial session using the helper
        let session_id = SessionId::new(0);
        let session = match create_session(
            &self.proxy,
            window.id(),
            session_id,
            &self.config,
            None, // working_directory -- initial session uses current dir
            cell_w,
            cell_h,
            size.width,
            size.height,
            1, // 1 tab bar line
        ) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("PTY spawn failed: {e}");
                event_loop.exit();
                return;
            }
        };

        tracing::info!("PTY spawned -- shell is running");

        // Startup pruning: spawn background thread to clean old snapshots (STOR-01)
        {
            let glass_dir =
                glass_snapshot::resolve_glass_dir(&std::env::current_dir().unwrap_or_default());
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
                    let pruner =
                        glass_snapshot::Pruner::new(&store, retention_days, max_count, max_size_mb);
                    match pruner.prune() {
                        Ok(result) => tracing::info!(
                            "Pruning complete: {} snapshots, {} blobs removed",
                            result.snapshots_deleted,
                            result.blobs_deleted,
                        ),
                        Err(e) => tracing::warn!("Pruning failed: {}", e),
                    }
                })
                .ok();
        }

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
                mouse_left_pressed: false,
                scrollbar_dragging: None,
                scrollbar_hovered_pane: None,
                tab_bar_hovered_tab: None,
                tab_drag_state: None,
            },
        );

        // Spawn config file watcher and update checker (once)
        if !self.watcher_spawned {
            self.watcher_spawned = true;
            if let Some(config_path) = GlassConfig::config_path() {
                glass_core::config_watcher::spawn_config_watcher(config_path, self.proxy.clone());
            }
            glass_core::updater::spawn_update_checker(
                env!("CARGO_PKG_VERSION"),
                self.proxy.clone(),
            );

            // Spawn coordination poller for agent/lock status
            let focused_cwd = self.get_focused_cwd();
            let project_root =
                glass_coordination::canonicalize_path(std::path::Path::new(&focused_cwd))
                    .unwrap_or(focused_cwd);
            glass_core::coordination_poller::spawn_coordination_poller(
                project_root,
                self.proxy.clone(),
            );

            // Start IPC listener for MCP command channel
            glass_core::ipc::start_ipc_listener(self.proxy.clone());

            // Create bounded activity stream channel (AGTA-01)
            let activity_config = glass_core::activity_stream::ActivityStreamConfig::default();
            let (tx, rx) = glass_core::activity_stream::create_channel(&activity_config);
            self.activity_stream_tx = Some(tx);

            // Spawn agent runtime if mode is not Off (AGTR-01, AGTR-04)
            let agent_config = self
                .config
                .agent
                .clone()
                .map(|a| glass_core::agent_runtime::AgentRuntimeConfig {
                    mode: a.mode,
                    max_budget_usd: a.max_budget_usd,
                    cooldown_secs: a.cooldown_secs,
                    allowed_tools: a.allowed_tools,
                    orchestrator: a.orchestrator,
                })
                .unwrap_or_default();

            if agent_config.mode != glass_core::agent_runtime::AgentMode::Off {
                self.agent_runtime = try_spawn_agent(
                    agent_config,
                    rx,
                    self.proxy.clone(),
                    0,
                    None,
                    self.get_focused_cwd(),
                    None,
                );
                // Start usage polling if orchestrator is configured
                if self
                    .config
                    .agent
                    .as_ref()
                    .and_then(|a| a.orchestrator.as_ref())
                    .is_some()
                {
                    self.usage_state = Some(usage_tracker::start_polling(self.proxy.clone()));
                }

                // AGTC-04: Show config hint when claude binary is missing (mode != Off but spawn failed).
                if self.agent_runtime.is_none() {
                    self.config_error = Some(glass_core::config::ConfigError {
                        message: "'claude' CLI not found on PATH. Install from https://claude.ai/download, or set agent.mode = \"off\" in ~/.glass/config.toml".to_string(),
                        line: None,
                        column: None,
                        snippet: None,
                    });
                }
            } else {
                // Store rx so it isn't dropped -- activity events are silently discarded
                // when agent is Off (channel fills up and try_send returns Err, which is ignored)
                self.activity_stream_rx = Some(rx);
            }
        }
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
                fire_hook_on_bridge(
                    &mut self.script_bridge,
                    &self.orchestrator.project_root,
                    glass_scripting::HookPoint::SessionEnd,
                    &glass_scripting::HookEventData::new(),
                );
                self.windows.remove(&window_id);
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                // Clear toast if agent is no longer active (agent_runtime was dropped).
                if self.agent_runtime.is_none() {
                    self.active_toast = None;
                }
                // Auto-dismiss toast after 30 seconds; keep redrawing so countdown updates.
                if let Some(ref toast) = self.active_toast {
                    if toast.created_at.elapsed() >= std::time::Duration::from_secs(30) {
                        self.active_toast = None;
                    } else {
                        // Keep render loop spinning so toast countdown eventually expires.
                        ctx.window.request_redraw();
                    }
                }

                // Execute debounced search query
                if let Some(session) = ctx.session_mux.focused_session_mut() {
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
                if let Some(session) = ctx.session() {
                    if let Some(ref overlay) = session.search_overlay {
                        if overlay.search_pending {
                            ctx.window.request_redraw();
                        }
                    }
                }

                // Determine if we have multiple panes in the active tab
                let pane_count = ctx
                    .session_mux
                    .active_tab_root()
                    .map(|r| r.leaf_count())
                    .unwrap_or(1);

                // Get surface texture
                let Some(frame) = ctx.renderer.get_current_texture() else {
                    return;
                };
                let view = frame.texture.create_view(&Default::default());
                let sc = ctx.renderer.surface_config();

                // Build tab display info for tab bar rendering
                let tab_display: Vec<TabDisplayInfo> = ctx
                    .session_mux
                    .tabs()
                    .iter()
                    .enumerate()
                    .map(|(i, tab)| {
                        let is_active = i == ctx.session_mux.active_tab_index();
                        TabDisplayInfo {
                            title: tab.title.clone(),
                            is_active,
                            has_locks: is_active && self.coordination_state.lock_count > 0,
                            agent_created: false,
                        }
                    })
                    .collect();

                if pane_count <= 1 {
                    // Single-pane path: use existing draw_frame for backward compatibility
                    let snapshot = {
                        let Some(session) = ctx.session() else {
                            return;
                        };
                        let term = session.term.lock();
                        snapshot_term(&term, &session.default_colors)
                    };

                    let (visible_blocks, search_overlay_data, status_clone) = {
                        let Some(session) = ctx.session_mux.focused_session() else {
                            return;
                        };
                        let viewport_abs_start = snapshot
                            .history_size
                            .saturating_sub(snapshot.display_offset);
                        let vb: Vec<_> = session
                            .block_manager
                            .visible_blocks(viewport_abs_start, snapshot.screen_lines)
                            .into_iter()
                            .cloned()
                            .collect();
                        let sod = session.search_overlay.as_ref().map(|overlay| {
                            let data = overlay.extract_display_data();
                            glass_renderer::frame::SearchOverlayRenderData {
                                query: data.query,
                                results: data
                                    .results
                                    .iter()
                                    .map(|r| {
                                        (
                                            r.command.clone(),
                                            r.exit_code,
                                            r.timestamp.clone(),
                                            r.output_preview.clone(),
                                        )
                                    })
                                    .collect(),
                                selected: data.selected,
                            }
                        });
                        let sc = session.status.clone();
                        (vb, sod, sc)
                    };

                    let visible_block_refs: Vec<&_> = visible_blocks.iter().collect();

                    let update_text = self
                        .update_info
                        .as_ref()
                        .map(|info| format!("Update v{} available (Ctrl+Shift+U)", info.latest));

                    let has_agents = !self.coordination_state.agents.is_empty();
                    let coordination_text = if self.coordination_state.agent_count > 0
                        && !has_agents
                    {
                        // Fallback: old format when agents vec not populated
                        Some(format!(
                            "agents: {} locks: {}",
                            self.coordination_state.agent_count, self.coordination_state.lock_count
                        ))
                    } else {
                        None
                    };

                    // Build agent activity line for two-line status bar
                    let agent_activity_line = if has_agents {
                        if self.ticker_display_cycles > 0 {
                            // Show ticker event text briefly
                            self.coordination_state
                                .ticker_event
                                .as_ref()
                                .map(|e| e.summary.clone())
                        } else {
                            let agents: Vec<_> = self
                                .coordination_state
                                .agents
                                .iter()
                                .map(|a| (a.name.clone(), a.status.clone(), a.task.clone()))
                                .collect();
                            Some(glass_renderer::status_bar::build_agent_activity_line(
                                &agents,
                                self.coordination_state.lock_count,
                                100,
                            ))
                        }
                    } else {
                        None
                    };

                    let agent_cost_display =
                        if self.agent_runtime.is_some() && self.agent_cost_usd > 0.0 {
                            if self.agent_proposals_paused {
                                Some(
                                    self.agent_runtime
                                        .as_ref()
                                        .map(|r| r.budget.paused_text())
                                        .unwrap_or_else(|| "PAUSED".to_string()),
                                )
                            } else {
                                Some(
                                    self.agent_runtime
                                        .as_ref()
                                        .map(|r| r.budget.cost_text())
                                        .unwrap_or_default(),
                                )
                            }
                        } else {
                            None
                        };

                    let drop_index = ctx.tab_drag_state.as_ref().and_then(|d| {
                        if d.active {
                            d.drop_index
                        } else {
                            None
                        }
                    });

                    // Agent mode and proposal count for status bar display.
                    let agent_mode_text = self.agent_runtime.as_ref().map(|_r| {
                        let usage_prefix = self
                            .usage_state
                            .as_ref()
                            .and_then(|s| s.lock().ok())
                            .map(|st| usage_tracker::format_status_bar(&st))
                            .unwrap_or_default();
                        if self.orchestrator.active {
                            let orch_status = if self.orchestrator.iteration == 0
                                && !self.orchestrator_event_buffer.events.is_empty()
                            {
                                "[orchestrating | agent working (first turn)]".to_string()
                            } else if self.orchestrator.response_pending {
                                format!(
                                    "[orchestrating | iter #{} | waiting for agent]",
                                    self.orchestrator.iteration
                                )
                            } else {
                                format!("[orchestrating | iter #{}]", self.orchestrator.iteration)
                            };
                            if usage_prefix.is_empty() {
                                orch_status
                            } else {
                                format!("{} | {}", usage_prefix, orch_status)
                            }
                        } else {
                            let mode = self
                                .config
                                .agent
                                .as_ref()
                                .map(|a| format!("{:?}", a.mode).to_lowercase())
                                .unwrap_or_else(|| "off".to_string());
                            if usage_prefix.is_empty() {
                                format!("[agent: {mode}]")
                            } else {
                                format!("{usage_prefix} | [agent: {mode}]")
                            }
                        }
                    });
                    let proposal_count_text = if !self.agent_proposal_worktrees.is_empty() {
                        let n = self.agent_proposal_worktrees.len();
                        Some(if n == 1 {
                            "1 proposal".to_string()
                        } else {
                            format!("{n} proposals")
                        })
                    } else {
                        None
                    };

                    // Toast render data (only while toast is active).
                    let proposal_toast_data = self.active_toast.as_ref().map(|t| {
                        glass_renderer::ProposalToastRenderData {
                            description: t.description.clone(),
                            remaining_secs: 30u64.saturating_sub(t.created_at.elapsed().as_secs()),
                        }
                    });

                    // Overlay render data with cached diff.
                    let proposal_overlay_data =
                        if self.agent_review_open && !self.agent_proposal_worktrees.is_empty() {
                            let selected = self
                                .proposal_review_selected
                                .min(self.agent_proposal_worktrees.len() - 1);
                            // Regenerate diff when selection changes.
                            if self
                                .proposal_diff_cache
                                .as_ref()
                                .is_none_or(|(idx, _)| *idx != selected)
                            {
                                let diff = self
                                    .agent_proposal_worktrees
                                    .get(selected)
                                    .and_then(|(_, handle_opt)| handle_opt.as_ref())
                                    .and_then(|handle| {
                                        self.worktree_manager
                                            .as_ref()
                                            .map(|wm| wm.generate_diff(handle))
                                    })
                                    .and_then(|r| r.ok())
                                    .unwrap_or_default();
                                self.proposal_diff_cache = Some((selected, diff));
                            }
                            let diff_preview = self
                                .proposal_diff_cache
                                .as_ref()
                                .map(|(_, d)| d.clone())
                                .unwrap_or_default();
                            Some(glass_renderer::ProposalOverlayRenderData {
                                proposals: self
                                    .agent_proposal_worktrees
                                    .iter()
                                    .map(|(p, _)| (p.description.clone(), p.action.clone()))
                                    .collect(),
                                selected,
                                diff_preview,
                            })
                        } else {
                            None
                        };

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
                        Some(&tab_display),
                        ctx.tab_bar_hovered_tab,
                        drop_index,
                        update_text.as_deref(),
                        coordination_text.as_deref(),
                        agent_cost_display.as_deref(),
                        self.agent_proposals_paused,
                        ctx.scrollbar_hovered_pane.is_some(),
                        ctx.scrollbar_dragging.is_some(),
                        agent_mode_text.as_deref(),
                        proposal_count_text.as_deref(),
                        proposal_toast_data.as_ref(),
                        proposal_overlay_data.as_ref(),
                        agent_activity_line.as_deref(),
                        self.orchestrator.active,
                    );
                } else {
                    // Multi-pane path: compute layout, snapshot all panes, render with offsets
                    let (_cell_w, cell_h) = ctx.frame_renderer.cell_size();

                    // Container viewport: subtract tab bar (top) and status bar (bottom)
                    let status_lines: u32 = if !self.coordination_state.agents.is_empty() {
                        2
                    } else {
                        1
                    };
                    let container = ViewportLayout {
                        x: 0,
                        y: cell_h as u32,
                        width: sc.width,
                        height: sc
                            .height
                            .saturating_sub((cell_h as u32) * (1 + status_lines)),
                    };

                    // Compute pane layouts from the active tab's split tree
                    let focused_id = ctx.session_mux.focused_session_id();
                    let pane_layouts: Vec<(SessionId, ViewportLayout)> = ctx
                        .session_mux
                        .active_tab_root()
                        .map(|root| root.compute_layout(&container))
                        .unwrap_or_default();

                    // Pre-extract PaneRenderData: snapshot + blocks for each pane (owned)
                    struct PaneData {
                        viewport: ViewportLayout,
                        snapshot: GridSnapshot,
                        blocks: Vec<Block>,
                        is_focused: bool,
                    }

                    let pane_data: Vec<PaneData> = pane_layouts
                        .iter()
                        .filter_map(|(sid, vp)| {
                            let session = ctx.session_mux.session(*sid)?;
                            let term = session.term.lock();
                            let snapshot = snapshot_term(&term, &session.default_colors);
                            drop(term);
                            let viewport_abs_start = snapshot
                                .history_size
                                .saturating_sub(snapshot.display_offset);
                            let blocks: Vec<_> = session
                                .block_manager
                                .visible_blocks(viewport_abs_start, snapshot.screen_lines)
                                .into_iter()
                                .cloned()
                                .collect();
                            Some(PaneData {
                                viewport: vp.clone(),
                                snapshot,
                                blocks,
                                is_focused: focused_id == Some(*sid),
                            })
                        })
                        .collect();

                    // Extract status from focused session
                    let status_clone = ctx
                        .session_mux
                        .focused_session()
                        .map(|s| s.status.clone())
                        .unwrap_or_default();

                    // Build pane render tuples with references
                    let block_refs: Vec<Vec<&Block>> = pane_data
                        .iter()
                        .map(|pd| pd.blocks.iter().collect())
                        .collect();

                    let panes: Vec<(PaneViewport, &GridSnapshot, &[&Block], bool)> = pane_data
                        .iter()
                        .enumerate()
                        .map(|(i, pd)| {
                            (
                                PaneViewport {
                                    x: pd.viewport.x,
                                    y: pd.viewport.y,
                                    width: pd.viewport.width,
                                    height: pd.viewport.height,
                                },
                                &pd.snapshot,
                                block_refs[i].as_slice(),
                                pd.is_focused,
                            )
                        })
                        .collect();

                    // Compute divider rects from gaps between pane viewports
                    let dividers = compute_dividers(&pane_layouts);

                    let update_text = self
                        .update_info
                        .as_ref()
                        .map(|info| format!("Update v{} available (Ctrl+Shift+U)", info.latest));

                    let has_agents_mp = !self.coordination_state.agents.is_empty();
                    let coordination_text = if self.coordination_state.agent_count > 0
                        && !has_agents_mp
                    {
                        Some(format!(
                            "agents: {} locks: {}",
                            self.coordination_state.agent_count, self.coordination_state.lock_count
                        ))
                    } else {
                        None
                    };

                    let agent_activity_line_mp = if has_agents_mp {
                        if self.ticker_display_cycles > 0 {
                            self.coordination_state
                                .ticker_event
                                .as_ref()
                                .map(|e| e.summary.clone())
                        } else {
                            let agents: Vec<_> = self
                                .coordination_state
                                .agents
                                .iter()
                                .map(|a| (a.name.clone(), a.status.clone(), a.task.clone()))
                                .collect();
                            Some(glass_renderer::status_bar::build_agent_activity_line(
                                &agents,
                                self.coordination_state.lock_count,
                                100,
                            ))
                        }
                    } else {
                        None
                    };

                    let agent_cost_display_mp =
                        if self.agent_runtime.is_some() && self.agent_cost_usd > 0.0 {
                            if self.agent_proposals_paused {
                                Some(
                                    self.agent_runtime
                                        .as_ref()
                                        .map(|r| r.budget.paused_text())
                                        .unwrap_or_else(|| "PAUSED".to_string()),
                                )
                            } else {
                                Some(
                                    self.agent_runtime
                                        .as_ref()
                                        .map(|r| r.budget.cost_text())
                                        .unwrap_or_default(),
                                )
                            }
                        } else {
                            None
                        };

                    // Build per-pane scrollbar hover/drag state
                    let scrollbar_state: Vec<(bool, bool)> = pane_layouts
                        .iter()
                        .map(|(sid, _)| {
                            let hovered = ctx.scrollbar_hovered_pane == Some(*sid);
                            let dragging = ctx
                                .scrollbar_dragging
                                .as_ref()
                                .map(|d| d.pane_id == *sid)
                                .unwrap_or(false);
                            (hovered, dragging)
                        })
                        .collect();

                    let drop_index_mp = ctx.tab_drag_state.as_ref().and_then(|d| {
                        if d.active {
                            d.drop_index
                        } else {
                            None
                        }
                    });

                    // Agent mode and proposal count for multi-pane status bar.
                    let agent_mode_text_mp = self.agent_runtime.as_ref().map(|_r| {
                        let usage_prefix = self
                            .usage_state
                            .as_ref()
                            .and_then(|s| s.lock().ok())
                            .map(|st| usage_tracker::format_status_bar(&st))
                            .unwrap_or_default();
                        if self.orchestrator.active {
                            let orch_status = if self.orchestrator.iteration == 0
                                && !self.orchestrator_event_buffer.events.is_empty()
                            {
                                "[orchestrating | agent working (first turn)]".to_string()
                            } else if self.orchestrator.response_pending {
                                format!(
                                    "[orchestrating | iter #{} | waiting for agent]",
                                    self.orchestrator.iteration
                                )
                            } else {
                                format!("[orchestrating | iter #{}]", self.orchestrator.iteration)
                            };
                            if usage_prefix.is_empty() {
                                orch_status
                            } else {
                                format!("{} | {}", usage_prefix, orch_status)
                            }
                        } else {
                            let mode = self
                                .config
                                .agent
                                .as_ref()
                                .map(|a| format!("{:?}", a.mode).to_lowercase())
                                .unwrap_or_else(|| "off".to_string());
                            if usage_prefix.is_empty() {
                                format!("[agent: {mode}]")
                            } else {
                                format!("{usage_prefix} | [agent: {mode}]")
                            }
                        }
                    });
                    let proposal_count_text_mp = if !self.agent_proposal_worktrees.is_empty() {
                        let n = self.agent_proposal_worktrees.len();
                        Some(if n == 1 {
                            "1 proposal".to_string()
                        } else {
                            format!("{n} proposals")
                        })
                    } else {
                        None
                    };

                    // Toast render data for multi-pane path.
                    let proposal_toast_data_mp = self.active_toast.as_ref().map(|t| {
                        glass_renderer::ProposalToastRenderData {
                            description: t.description.clone(),
                            remaining_secs: 30u64.saturating_sub(t.created_at.elapsed().as_secs()),
                        }
                    });

                    // Overlay render data for multi-pane path (reuse cached diff from single-pane
                    // if available, otherwise generate fresh -- overlay is window-global).
                    let proposal_overlay_data_mp =
                        if self.agent_review_open && !self.agent_proposal_worktrees.is_empty() {
                            let selected = self
                                .proposal_review_selected
                                .min(self.agent_proposal_worktrees.len() - 1);
                            if self
                                .proposal_diff_cache
                                .as_ref()
                                .is_none_or(|(idx, _)| *idx != selected)
                            {
                                let diff = self
                                    .agent_proposal_worktrees
                                    .get(selected)
                                    .and_then(|(_, handle_opt)| handle_opt.as_ref())
                                    .and_then(|handle| {
                                        self.worktree_manager
                                            .as_ref()
                                            .map(|wm| wm.generate_diff(handle))
                                    })
                                    .and_then(|r| r.ok())
                                    .unwrap_or_default();
                                self.proposal_diff_cache = Some((selected, diff));
                            }
                            let diff_preview = self
                                .proposal_diff_cache
                                .as_ref()
                                .map(|(_, d)| d.clone())
                                .unwrap_or_default();
                            Some(glass_renderer::ProposalOverlayRenderData {
                                proposals: self
                                    .agent_proposal_worktrees
                                    .iter()
                                    .map(|(p, _)| (p.description.clone(), p.action.clone()))
                                    .collect(),
                                selected,
                                diff_preview,
                            })
                        } else {
                            None
                        };

                    ctx.frame_renderer.draw_multi_pane_frame(
                        ctx.renderer.device(),
                        ctx.renderer.queue(),
                        &view,
                        sc.width,
                        sc.height,
                        &panes,
                        &dividers,
                        Some(&status_clone),
                        Some(&tab_display),
                        ctx.tab_bar_hovered_tab,
                        drop_index_mp,
                        update_text.as_deref(),
                        coordination_text.as_deref(),
                        agent_cost_display_mp.as_deref(),
                        self.agent_proposals_paused,
                        &scrollbar_state,
                        agent_mode_text_mp.as_deref(),
                        proposal_count_text_mp.as_deref(),
                        proposal_toast_data_mp.as_ref(),
                        proposal_overlay_data_mp.as_ref(),
                        agent_activity_line_mp.as_deref(),
                        self.orchestrator.active,
                    );
                }

                // Config error overlay: render a red banner on top of everything
                if let Some(ref config_err) = self.config_error {
                    ctx.frame_renderer.draw_config_error_overlay(
                        ctx.renderer.device(),
                        ctx.renderer.queue(),
                        &view,
                        sc.width,
                        sc.height,
                        config_err,
                    );
                }

                // Conflict overlay: render amber warning when 2+ agents active with locks
                if self.coordination_state.agent_count >= 2
                    && self.coordination_state.lock_count > 0
                {
                    ctx.frame_renderer.draw_conflict_overlay(
                        ctx.renderer.device(),
                        ctx.renderer.queue(),
                        &view,
                        sc.width,
                        sc.height,
                        self.coordination_state.agent_count,
                        self.coordination_state.lock_count,
                    );
                }

                // Activity stream overlay (fullscreen, on top of everything)
                let iteration_log_cwd = ctx
                    .session_mux
                    .focused_session()
                    .map(|s| s.status.cwd().to_string())
                    .unwrap_or_default();
                if self.activity_overlay_visible {
                    let agents: Vec<glass_renderer::activity_overlay::ActivityAgentCard> = self
                        .coordination_state
                        .agents
                        .iter()
                        .map(|a| glass_renderer::activity_overlay::ActivityAgentCard {
                            name: a.name.clone(),
                            agent_type: a.agent_type.clone(),
                            status: a.status.clone(),
                            task: a.task.clone(),
                            locked_files: a.locked_files.clone(),
                            is_idle: a.status == "idle",
                        })
                        .collect();

                    let events: Vec<glass_renderer::activity_overlay::ActivityTimelineEvent> = self
                        .coordination_state
                        .recent_events
                        .iter()
                        .map(
                            |e| glass_renderer::activity_overlay::ActivityTimelineEvent {
                                timestamp: e.timestamp,
                                agent_name: e.agent_name.clone(),
                                category: e.category.clone(),
                                event_type: e.event_type.clone(),
                                summary: e.summary.clone(),
                                pinned: e.pinned,
                            },
                        )
                        .collect();

                    let pinned: Vec<glass_renderer::activity_overlay::ActivityPinnedAlert> = self
                        .coordination_state
                        .recent_events
                        .iter()
                        .filter(|e| e.pinned)
                        .map(|e| glass_renderer::activity_overlay::ActivityPinnedAlert {
                            id: e.id,
                            summary: e.summary.clone(),
                            timestamp: e.timestamp,
                        })
                        .collect();

                    // Build orchestrator dashboard data
                    let orch_dashboard = if self.orchestrator_activated_at.is_some() {
                        let mode = self
                            .config
                            .agent
                            .as_ref()
                            .and_then(|a| a.orchestrator.as_ref())
                            .map(|o| o.orchestrator_mode.clone())
                            .unwrap_or_else(|| "build".to_string());
                        let verify_mode = self
                            .config
                            .agent
                            .as_ref()
                            .and_then(|a| a.orchestrator.as_ref())
                            .map(|o| o.verify_mode.clone())
                            .unwrap_or_else(|| "floor".to_string());
                        let (tests_passed, keep_count, revert_count) =
                            if let Some(ref baseline) = self.orchestrator.metric_baseline {
                                (
                                    baseline.last_results.first().and_then(|r| r.tests_passed),
                                    baseline.keep_count,
                                    baseline.revert_count,
                                )
                            } else {
                                (None, 0, 0)
                            };
                        let checkpoint_phase = match &self.orchestrator.checkpoint_phase {
                            orchestrator::CheckpointPhase::Idle => "idle".to_string(),
                            orchestrator::CheckpointPhase::Synthesizing { .. } => {
                                "synthesizing...".to_string()
                            }
                        };
                        let paused_reason = if self
                            .usage_state
                            .as_ref()
                            .and_then(|s| s.lock().ok())
                            .map(|s| s.paused)
                            .unwrap_or(false)
                        {
                            Some("Usage limit".to_string())
                        } else {
                            None
                        };
                        Some(glass_renderer::OrchestratorDashboard {
                            iteration: self.orchestrator.iteration,
                            iterations_since_checkpoint: self
                                .orchestrator
                                .iterations_since_checkpoint,
                            max_iterations: self.orchestrator.max_iterations,
                            mode,
                            verify_mode,
                            tests_passed,
                            keep_count,
                            revert_count,
                            last_completed: self.orchestrator.last_checkpoint_completed.clone(),
                            next_item: self.orchestrator.last_checkpoint_next.clone(),
                            active: self.orchestrator.active,
                            response_pending: self.orchestrator.response_pending,
                            checkpoint_phase,
                            paused_reason,
                        })
                    } else {
                        None
                    };

                    // Build orchestrator event displays — only when on the Orchestrator tab
                    let activated_at = self.orchestrator_activated_at;
                    let orch_events: Vec<glass_renderer::OrchestratorEventDisplay> = if self
                        .activity_view_filter
                        != glass_renderer::ActivityViewFilter::Orchestrator
                    {
                        Vec::new()
                    } else {
                        self.orchestrator_event_buffer
                            .events
                            .iter()
                            .map(|entry| {
                                let relative_time = activated_at
                                    .map(|at| {
                                        let elapsed = entry.timestamp.duration_since(at);
                                        let total_secs = elapsed.as_secs();
                                        format!("{:02}:{:02}", total_secs / 60, total_secs % 60)
                                    })
                                    .unwrap_or_else(|| "--:--".to_string());

                                let (kind, text, expandable) = match &entry.event {
                                    orchestrator_events::OrchestratorEvent::Thinking {
                                        token_estimate,
                                        ..
                                    } => (
                                        glass_renderer::OrchestratorEventKind::Thinking,
                                        format!("Thinking...  ({token_estimate} tokens)"),
                                        true,
                                    ),
                                    orchestrator_events::OrchestratorEvent::ToolCall {
                                        name,
                                        params_summary,
                                    } => (
                                        glass_renderer::OrchestratorEventKind::ToolCall,
                                        format!("-> {name}({params_summary})"),
                                        false,
                                    ),
                                    orchestrator_events::OrchestratorEvent::ToolResult {
                                        name,
                                        output_summary,
                                    } => (
                                        glass_renderer::OrchestratorEventKind::ToolResult,
                                        format!("-> {name} -> {output_summary}"),
                                        false,
                                    ),
                                    orchestrator_events::OrchestratorEvent::AgentText { text } => (
                                        glass_renderer::OrchestratorEventKind::AgentText,
                                        format!(
                                            "Agent: \"{}\"",
                                            orchestrator_events::truncate_display(text, 120)
                                        ),
                                        false,
                                    ),
                                    orchestrator_events::OrchestratorEvent::ContextSent {
                                        line_count,
                                        has_soi,
                                        has_nudge,
                                    } => {
                                        let mut details = format!("{line_count} lines");
                                        if *has_soi {
                                            details.push_str(", SOI");
                                        }
                                        if *has_nudge {
                                            details.push_str(", nudge");
                                        }
                                        (
                                            glass_renderer::OrchestratorEventKind::ContextSent,
                                            format!("Context sent ({details})"),
                                            false,
                                        )
                                    }
                                    orchestrator_events::OrchestratorEvent::AgentRespawn {
                                        reason,
                                    } => (
                                        glass_renderer::OrchestratorEventKind::Respawn,
                                        format!("--- Agent respawned ({reason}) ---"),
                                        false,
                                    ),
                                    orchestrator_events::OrchestratorEvent::VerifyResult {
                                        passed,
                                        failed,
                                        regressed,
                                    } => {
                                        let icon = if *regressed { "X" } else { "ok" };
                                        let p = passed
                                            .map(|v| v.to_string())
                                            .unwrap_or_else(|| "?".into());
                                        let f = failed
                                            .map(|v| v.to_string())
                                            .unwrap_or_else(|| "?".into());
                                        (
                                            glass_renderer::OrchestratorEventKind::Verify,
                                            format!("{icon} Verify: {p} passed, {f} failed"),
                                            false,
                                        )
                                    }
                                };

                                let expanded = if expandable {
                                    self.orchestrator_event_buffer.is_expanded(entry.id)
                                } else {
                                    false
                                };

                                // If expanded, replace summary text with full thinking text
                                let display_text = if expanded {
                                    if let orchestrator_events::OrchestratorEvent::Thinking {
                                        text,
                                        ..
                                    } = &entry.event
                                    {
                                        text.clone()
                                    } else {
                                        text
                                    }
                                } else {
                                    text
                                };

                                glass_renderer::OrchestratorEventDisplay {
                                    id: entry.id,
                                    iteration: entry.iteration,
                                    relative_time,
                                    kind,
                                    text: display_text,
                                    expanded,
                                    expandable,
                                }
                            })
                            .collect()
                    };

                    let render_data = glass_renderer::ActivityOverlayRenderData {
                        agents,
                        events,
                        pinned,
                        filter: self.activity_view_filter,
                        scroll_offset: self.activity_scroll_offset,
                        verbose: self.activity_verbose,
                        orchestrator_active: self.orchestrator.active,
                        orchestrator_iteration: self.orchestrator.iteration,
                        orchestrator_paused_reason: if self
                            .usage_state
                            .as_ref()
                            .and_then(|s| s.lock().ok())
                            .map(|s| s.paused)
                            .unwrap_or(false)
                        {
                            Some("Usage limit".to_string())
                        } else {
                            None
                        },
                        usage_text: self
                            .usage_state
                            .as_ref()
                            .and_then(|s| s.lock().ok())
                            .map(|st| crate::usage_tracker::format_status_bar(&st)),
                        orchestrator_dashboard: orch_dashboard,
                        orchestrator_events: orch_events,
                        orchestrator_scroll_offset: self.orchestrator_scroll_offset,
                        iteration_log: if self.activity_view_filter
                            != glass_renderer::ActivityViewFilter::Orchestrator
                        {
                            Vec::new()
                        } else {
                            parse_iteration_log(&iteration_log_cwd)
                        },
                    };

                    ctx.frame_renderer.draw_activity_overlay(
                        ctx.renderer.device(),
                        ctx.renderer.queue(),
                        &view,
                        sc.width,
                        sc.height,
                        &render_data,
                    );
                }

                // Settings overlay (fullscreen, on top of everything)
                if self.settings_overlay_visible {
                    let config_snapshot =
                        glass_renderer::settings_overlay::SettingsConfigSnapshot {
                            font_family: self.config.font_family.clone(),
                            font_size: self.config.font_size,
                            agent_enabled: self
                                .config
                                .agent
                                .as_ref()
                                .map(|a| a.mode != glass_core::agent_runtime::AgentMode::Off)
                                .unwrap_or(false),
                            agent_mode: self
                                .config
                                .agent
                                .as_ref()
                                .map(|a| format!("{:?}", a.mode))
                                .unwrap_or_else(|| "Off".to_string()),
                            agent_budget: self
                                .config
                                .agent
                                .as_ref()
                                .map(|a| a.max_budget_usd)
                                .unwrap_or(1.0),
                            agent_cooldown: self
                                .config
                                .agent
                                .as_ref()
                                .map(|a| a.cooldown_secs)
                                .unwrap_or(30),
                            soi_enabled: self
                                .config
                                .soi
                                .as_ref()
                                .map(|s| s.enabled)
                                .unwrap_or(true),
                            soi_shell_summary: self
                                .config
                                .soi
                                .as_ref()
                                .map(|s| s.shell_summary)
                                .unwrap_or(false),
                            soi_min_lines: self
                                .config
                                .soi
                                .as_ref()
                                .map(|s| s.min_lines)
                                .unwrap_or(0),
                            snapshot_enabled: self
                                .config
                                .snapshot
                                .as_ref()
                                .map(|s| s.enabled)
                                .unwrap_or(true),
                            snapshot_max_mb: self
                                .config
                                .snapshot
                                .as_ref()
                                .map(|s| s.max_size_mb)
                                .unwrap_or(500),
                            snapshot_retention_days: self
                                .config
                                .snapshot
                                .as_ref()
                                .map(|s| s.retention_days)
                                .unwrap_or(30),
                            pipes_enabled: self
                                .config
                                .pipes
                                .as_ref()
                                .map(|p| p.enabled)
                                .unwrap_or(true),
                            pipes_auto_expand: self
                                .config
                                .pipes
                                .as_ref()
                                .map(|p| p.auto_expand)
                                .unwrap_or(true),
                            pipes_max_capture_mb: self
                                .config
                                .pipes
                                .as_ref()
                                .map(|p| p.max_capture_mb)
                                .unwrap_or(10),
                            history_max_output_kb: self
                                .config
                                .history
                                .as_ref()
                                .map(|h| h.max_output_capture_kb)
                                .unwrap_or(50),
                            orchestrator_enabled: self.orchestrator.active,
                            orchestrator_max_iterations: self
                                .config
                                .agent
                                .as_ref()
                                .and_then(|a| a.orchestrator.as_ref())
                                .and_then(|o| o.max_iterations)
                                .unwrap_or(0),
                            orchestrator_silence_secs: self
                                .config
                                .agent
                                .as_ref()
                                .and_then(|a| a.orchestrator.as_ref())
                                .map(|o| o.silence_timeout_secs)
                                .unwrap_or(60),
                            orchestrator_prd_path: self
                                .config
                                .agent
                                .as_ref()
                                .and_then(|a| a.orchestrator.as_ref())
                                .map(|o| o.prd_path.clone())
                                .unwrap_or_else(|| "PRD.md".to_string()),
                            orchestrator_mode: self
                                .config
                                .agent
                                .as_ref()
                                .and_then(|a| a.orchestrator.as_ref())
                                .map(|o| o.orchestrator_mode.clone())
                                .unwrap_or_else(|| "build".to_string()),
                            orchestrator_verify_mode: self
                                .config
                                .agent
                                .as_ref()
                                .and_then(|a| a.orchestrator.as_ref())
                                .map(|o| o.verify_mode.clone())
                                .unwrap_or_else(|| "floor".to_string()),
                            orchestrator_feedback_llm: self
                                .config
                                .agent
                                .as_ref()
                                .and_then(|a| a.orchestrator.as_ref())
                                .map(|o| o.feedback_llm)
                                .unwrap_or(false),
                            orchestrator_max_prompt_hints: self
                                .config
                                .agent
                                .as_ref()
                                .and_then(|a| a.orchestrator.as_ref())
                                .map(|o| o.max_prompt_hints)
                                .unwrap_or(10),
                            orchestrator_ablation_enabled: self
                                .config
                                .agent
                                .as_ref()
                                .and_then(|a| a.orchestrator.as_ref())
                                .map(|o| o.ablation_enabled)
                                .unwrap_or(true),
                            orchestrator_ablation_sweep_interval: self
                                .config
                                .agent
                                .as_ref()
                                .and_then(|a| a.orchestrator.as_ref())
                                .map(|o| o.ablation_sweep_interval)
                                .unwrap_or(20),
                        };

                    let render_data = glass_renderer::SettingsOverlayRenderData {
                        tab: self.settings_overlay_tab,
                        section_index: self.settings_section_index,
                        field_index: self.settings_field_index,
                        editing: self.settings_editing,
                        edit_buffer: self.settings_edit_buffer.clone(),
                        config: config_snapshot,
                        shortcuts_scroll: self.settings_shortcuts_scroll,
                    };

                    ctx.frame_renderer.draw_settings_overlay(
                        ctx.renderer.device(),
                        ctx.renderer.queue(),
                        &view,
                        sc.width,
                        sc.height,
                        &render_data,
                    );
                }

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

                let (cell_w, cell_h) = ctx.frame_renderer.cell_size();

                // Active tab: use per-pane resize for correct split dimensions (SPLIT-09)
                if ctx.session_mux.active_tab_pane_count() > 1 {
                    resize_all_panes(
                        &mut ctx.session_mux,
                        &ctx.frame_renderer,
                        size.width,
                        size.height,
                    );
                } else {
                    // Single-pane active tab: full window dimensions minus chrome
                    let num_cols = ((size.width as f32 - SCROLLBAR_WIDTH) / cell_w)
                        .floor()
                        .max(1.0) as u16;
                    let num_lines =
                        ((size.height as f32 / cell_h).floor().max(2.0) as u16).saturating_sub(2);
                    let full_size = WindowSize {
                        num_lines,
                        num_cols,
                        cell_width: cell_w as u16,
                        cell_height: cell_h as u16,
                    };
                    if let Some(session) = ctx.session_mux.focused_session_mut() {
                        pty_send(&session.pty_sender, PtyMsg::Resize(full_size));
                        session.term.lock().resize(TermDimensions {
                            columns: num_cols as usize,
                            screen_lines: num_lines as usize,
                        });
                        session.block_manager.notify_resize(num_cols as usize);
                    }
                }

                // Background tabs: resize with full window dimensions
                // (they will recompute when activated)
                let num_cols = ((size.width as f32 - SCROLLBAR_WIDTH) / cell_w)
                    .floor()
                    .max(1.0) as u16;
                let num_lines =
                    ((size.height as f32 / cell_h).floor().max(2.0) as u16).saturating_sub(2);
                let full_size = WindowSize {
                    num_lines,
                    num_cols,
                    cell_width: cell_w as u16,
                    cell_height: cell_h as u16,
                };
                let active_idx = ctx.session_mux.active_tab_index();
                let bg_session_ids: Vec<_> = ctx
                    .session_mux
                    .tabs()
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != active_idx)
                    .flat_map(|(_, t)| t.session_ids())
                    .collect();
                for sid in bg_session_ids {
                    if let Some(session) = ctx.session_mux.session_mut(sid) {
                        pty_send(&session.pty_sender, PtyMsg::Resize(full_size));
                        session.term.lock().resize(TermDimensions {
                            columns: num_cols as usize,
                            screen_lines: num_lines as usize,
                        });
                        session.block_manager.notify_resize(num_cols as usize);
                    }
                }

                // Request a redraw after resize so the surface is repainted immediately
                ctx.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let scale = scale_factor as f32;
                tracing::info!("DPI scale factor changed to {scale}");

                // Rebuild font metrics with new scale factor
                ctx.frame_renderer.update_font(
                    &self.config.font_family,
                    self.config.font_size,
                    scale,
                );

                let size = ctx.window.inner_size();
                if size.width > 0 && size.height > 0 {
                    ctx.renderer.resize(size.width, size.height);

                    let (cell_w, cell_h) = ctx.frame_renderer.cell_size();

                    // Active tab: use per-pane resize for correct split dimensions
                    if ctx.session_mux.active_tab_pane_count() > 1 {
                        resize_all_panes(
                            &mut ctx.session_mux,
                            &ctx.frame_renderer,
                            size.width,
                            size.height,
                        );
                    } else {
                        let num_cols = ((size.width as f32 - SCROLLBAR_WIDTH) / cell_w)
                            .floor()
                            .max(1.0) as u16;
                        let num_lines = ((size.height as f32 / cell_h).floor().max(2.0) as u16)
                            .saturating_sub(2);
                        let full_size = WindowSize {
                            num_lines,
                            num_cols,
                            cell_width: cell_w as u16,
                            cell_height: cell_h as u16,
                        };
                        if let Some(session) = ctx.session_mux.focused_session_mut() {
                            pty_send(&session.pty_sender, PtyMsg::Resize(full_size));
                            session.term.lock().resize(TermDimensions {
                                columns: num_cols as usize,
                                screen_lines: num_lines as usize,
                            });
                            session.block_manager.notify_resize(num_cols as usize);
                        }
                    }

                    // Background tabs: resize with full window dimensions
                    let num_cols = ((size.width as f32 - SCROLLBAR_WIDTH) / cell_w)
                        .floor()
                        .max(1.0) as u16;
                    let num_lines =
                        ((size.height as f32 / cell_h).floor().max(2.0) as u16).saturating_sub(2);
                    let full_size = WindowSize {
                        num_lines,
                        num_cols,
                        cell_width: cell_w as u16,
                        cell_height: cell_h as u16,
                    };
                    let active_idx = ctx.session_mux.active_tab_index();
                    let bg_session_ids: Vec<_> = ctx
                        .session_mux
                        .tabs()
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| *i != active_idx)
                        .flat_map(|(_, t)| t.session_ids())
                        .collect();
                    for sid in bg_session_ids {
                        if let Some(session) = ctx.session_mux.session_mut(sid) {
                            pty_send(&session.pty_sender, PtyMsg::Resize(full_size));
                            session.term.lock().resize(TermDimensions {
                                columns: num_cols as usize,
                                screen_lines: num_lines as usize,
                            });
                            session.block_manager.notify_resize(num_cols as usize);
                        }
                    }
                }

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
                    enum OverlayAction {
                        None,
                        Handled,
                        Close,
                    }
                    let overlay_action = {
                        let Some(session) = ctx.session_mux.focused_session_mut() else {
                            return;
                        };
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
                                        let result_epoch =
                                            overlay.results[overlay.selected].started_at;
                                        let all_blocks = session.block_manager.blocks();
                                        let matched_block = all_blocks
                                            .iter()
                                            .find(|b| b.started_epoch == Some(result_epoch));
                                        if let Some(block) = matched_block {
                                            let target_line = block.prompt_start_line;
                                            let mut term = session.term.lock();
                                            let history_size = term.grid().history_size();
                                            let current_offset = term.grid().display_offset();
                                            let target_offset =
                                                history_size.saturating_sub(target_line);
                                            let delta =
                                                target_offset as i32 - current_offset as i32;
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
                                _ => OverlayAction::Handled, // Swallow all other keys while overlay is open
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

                    let Some(session) = ctx.session() else {
                        return;
                    };
                    let mode = *session.term.lock().mode();

                    // Tab/pane management shortcuts (Ctrl+Shift on Win/Linux, Cmd on macOS)
                    if glass_mux::is_glass_shortcut(modifiers) {
                        match &event.logical_key {
                            Key::Character(c) if c.as_str().eq_ignore_ascii_case("t") => {
                                // New tab: inherit CWD from current session
                                let cwd = ctx
                                    .session()
                                    .map(|s| s.status.cwd().to_string())
                                    .unwrap_or_default();
                                let session_id = ctx.session_mux.next_session_id();
                                let (cell_w, cell_h) = ctx.frame_renderer.cell_size();
                                let size = ctx.window.inner_size();
                                let session = match create_session(
                                    &self.proxy,
                                    window_id,
                                    session_id,
                                    &self.config,
                                    Some(std::path::Path::new(&cwd)),
                                    cell_w,
                                    cell_h,
                                    size.width,
                                    size.height,
                                    1,
                                ) {
                                    Ok(s) => s,
                                    Err(e) => {
                                        tracing::error!("PTY spawn failed for new tab: {e}");
                                        return;
                                    }
                                };
                                ctx.session_mux.add_tab(session, false);
                                {
                                    let tab_idx = ctx.session_mux.tab_count().saturating_sub(1);
                                    let mut event = glass_scripting::HookEventData::new();
                                    event.set("tab_index", tab_idx as i64);
                                    fire_hook_on_bridge(
                                        &mut self.script_bridge,
                                        &self.orchestrator.project_root,
                                        glass_scripting::HookPoint::TabCreate,
                                        &event,
                                    );
                                }
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Character(c) if c.as_str().eq_ignore_ascii_case("w") => {
                                // Close pane if multiple panes, otherwise close tab
                                if ctx.session_mux.active_tab_pane_count() > 1 {
                                    // Close focused pane
                                    if let Some(focused_id) = ctx.session_mux.focused_session_id() {
                                        let tab_count_before = ctx.session_mux.tab_count();
                                        if let Some(session) =
                                            ctx.session_mux.close_pane(focused_id)
                                        {
                                            cleanup_session(session);
                                        }
                                        // If close_pane closed the tab (shouldn't happen with >1 pane, but guard)
                                        if ctx.session_mux.tab_count() < tab_count_before
                                            && ctx.session_mux.tab_count() == 0
                                        {
                                            fire_hook_on_bridge(
                                                &mut self.script_bridge,
                                                &self.orchestrator.project_root,
                                                glass_scripting::HookPoint::SessionEnd,
                                                &glass_scripting::HookEventData::new(),
                                            );
                                            self.windows.remove(&window_id);
                                            event_loop.exit();
                                            return;
                                        }
                                        // Resize remaining panes' PTYs
                                        let size = ctx.window.inner_size();
                                        resize_all_panes(
                                            &mut ctx.session_mux,
                                            &ctx.frame_renderer,
                                            size.width,
                                            size.height,
                                        );
                                    }
                                } else {
                                    // Single pane: close the entire tab
                                    let idx = ctx.session_mux.active_tab_index();
                                    {
                                        let mut event = glass_scripting::HookEventData::new();
                                        event.set("tab_index", idx as i64);
                                        fire_hook_on_bridge(
                                            &mut self.script_bridge,
                                            &self.orchestrator.project_root,
                                            glass_scripting::HookPoint::TabClose,
                                            &event,
                                        );
                                    }
                                    if let Some(session) = ctx.session_mux.close_tab(idx) {
                                        cleanup_session(session);
                                    }
                                    ctx.tab_bar_hovered_tab = None;
                                    if ctx.session_mux.tab_count() == 0 {
                                        fire_hook_on_bridge(
                                            &mut self.script_bridge,
                                            &self.orchestrator.project_root,
                                            glass_scripting::HookPoint::SessionEnd,
                                            &glass_scripting::HookEventData::new(),
                                        );
                                        self.windows.remove(&window_id);
                                        event_loop.exit();
                                        return;
                                    }
                                }
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Character(c) if c.as_str().eq_ignore_ascii_case("d") => {
                                // Horizontal split (new pane to the right)
                                let cwd = ctx
                                    .session()
                                    .map(|s| s.status.cwd().to_string())
                                    .unwrap_or_default();
                                let session_id = ctx.session_mux.next_session_id();
                                let (cell_w, cell_h) = ctx.frame_renderer.cell_size();
                                let size = ctx.window.inner_size();
                                let session = match create_session(
                                    &self.proxy,
                                    window_id,
                                    session_id,
                                    &self.config,
                                    Some(std::path::Path::new(&cwd)),
                                    cell_w,
                                    cell_h,
                                    size.width,
                                    size.height,
                                    1,
                                ) {
                                    Ok(s) => s,
                                    Err(e) => {
                                        tracing::error!(
                                            "PTY spawn failed for horizontal split: {e}"
                                        );
                                        return;
                                    }
                                };
                                if ctx
                                    .session_mux
                                    .split_pane(SplitDirection::Horizontal, session)
                                    .is_none()
                                {
                                    return; // max depth reached
                                }

                                // Resize all panes' PTYs with per-pane dimensions
                                resize_all_panes(
                                    &mut ctx.session_mux,
                                    &ctx.frame_renderer,
                                    size.width,
                                    size.height,
                                );
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Character(c) if c.as_str().eq_ignore_ascii_case("e") => {
                                // Vertical split (new pane below)
                                let cwd = ctx
                                    .session()
                                    .map(|s| s.status.cwd().to_string())
                                    .unwrap_or_default();
                                let session_id = ctx.session_mux.next_session_id();
                                let (cell_w, cell_h) = ctx.frame_renderer.cell_size();
                                let size = ctx.window.inner_size();
                                let session = match create_session(
                                    &self.proxy,
                                    window_id,
                                    session_id,
                                    &self.config,
                                    Some(std::path::Path::new(&cwd)),
                                    cell_w,
                                    cell_h,
                                    size.width,
                                    size.height,
                                    1,
                                ) {
                                    Ok(s) => s,
                                    Err(e) => {
                                        tracing::error!("PTY spawn failed for vertical split: {e}");
                                        return;
                                    }
                                };
                                if ctx
                                    .session_mux
                                    .split_pane(SplitDirection::Vertical, session)
                                    .is_none()
                                {
                                    return; // max depth reached
                                }
                                // Resize all panes' PTYs with per-pane dimensions
                                resize_all_panes(
                                    &mut ctx.session_mux,
                                    &ctx.frame_renderer,
                                    size.width,
                                    size.height,
                                );
                                ctx.window.request_redraw();
                                return;
                            }
                            _ => {} // Fall through to existing Ctrl+Shift shortcuts
                        }
                    }

                    // Check for Glass-handled keys first
                    if modifiers.control_key() && modifiers.shift_key() {
                        match &event.logical_key {
                            // Ctrl+Shift+A: Toggle agent proposal review overlay.
                            Key::Character(c) if c.as_str().eq_ignore_ascii_case("a") => {
                                if self.agent_runtime.is_some() {
                                    self.agent_review_open = !self.agent_review_open;
                                    if self.agent_review_open {
                                        self.proposal_review_selected = 0;
                                        self.proposal_diff_cache = None;
                                    }
                                    ctx.window.request_redraw();
                                    return;
                                }
                            }
                            // Ctrl+Shift+G: Toggle activity stream overlay.
                            Key::Character(c) if c.as_str().eq_ignore_ascii_case("g") => {
                                self.activity_overlay_visible = !self.activity_overlay_visible;
                                if !self.activity_overlay_visible {
                                    self.activity_view_filter = Default::default();
                                    self.activity_scroll_offset = 0;
                                    self.activity_verbose = false;
                                }
                                ctx.window.request_redraw();
                                return;
                            }
                            // Ctrl+Shift+,: Toggle settings overlay.
                            Key::Character(c) if c.as_str() == "<" || c.as_str() == "," => {
                                self.settings_overlay_visible = !self.settings_overlay_visible;
                                if !self.settings_overlay_visible {
                                    self.settings_overlay_tab = Default::default();
                                    self.settings_section_index = 0;
                                    self.settings_field_index = 0;
                                    self.settings_editing = false;
                                    self.settings_edit_buffer.clear();
                                    self.settings_shortcuts_scroll = 0;
                                }
                                ctx.window.request_redraw();
                                return;
                            }
                            // Ctrl+Shift+Y: Accept selected proposal (only when overlay open).
                            Key::Character(c)
                                if c.as_str().eq_ignore_ascii_case("y")
                                    && self.agent_review_open =>
                            {
                                if !self.agent_proposal_worktrees.is_empty() {
                                    let idx = self
                                        .proposal_review_selected
                                        .min(self.agent_proposal_worktrees.len() - 1);
                                    let (_proposal, handle_opt) =
                                        self.agent_proposal_worktrees.remove(idx);
                                    if let (Some(wm), Some(handle)) =
                                        (self.worktree_manager.as_ref(), handle_opt)
                                    {
                                        if let Err(e) = wm.apply(handle) {
                                            tracing::error!("Failed to apply proposal: {e}");
                                        }
                                    }
                                    self.proposal_review_selected = self
                                        .proposal_review_selected
                                        .min(self.agent_proposal_worktrees.len().saturating_sub(1));
                                    self.proposal_diff_cache = None;
                                    if self.agent_proposal_worktrees.is_empty() {
                                        self.agent_review_open = false;
                                    }
                                }
                                ctx.window.request_redraw();
                                return;
                            }
                            // Ctrl+Shift+N: Reject selected proposal (only when overlay open).
                            Key::Character(c)
                                if c.as_str().eq_ignore_ascii_case("n")
                                    && self.agent_review_open =>
                            {
                                if !self.agent_proposal_worktrees.is_empty() {
                                    let idx = self
                                        .proposal_review_selected
                                        .min(self.agent_proposal_worktrees.len() - 1);
                                    let (_proposal, handle_opt) =
                                        self.agent_proposal_worktrees.remove(idx);
                                    if let (Some(wm), Some(handle)) =
                                        (self.worktree_manager.as_ref(), handle_opt)
                                    {
                                        if let Err(e) = wm.dismiss(handle) {
                                            tracing::error!("Failed to dismiss proposal: {e}");
                                        }
                                    }
                                    self.proposal_review_selected = self
                                        .proposal_review_selected
                                        .min(self.agent_proposal_worktrees.len().saturating_sub(1));
                                    self.proposal_diff_cache = None;
                                    if self.agent_proposal_worktrees.is_empty() {
                                        self.agent_review_open = false;
                                    }
                                }
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Character(c) if c.as_str().eq_ignore_ascii_case("c") => {
                                if let Some(session) = ctx.session() {
                                    clipboard_copy(&session.term);
                                }
                                return;
                            }
                            Key::Character(c) if c.as_str().eq_ignore_ascii_case("v") => {
                                if let Some(session) = ctx.session() {
                                    clipboard_paste(&session.pty_sender, mode);
                                }
                                return;
                            }
                            Key::Character(c) if c.as_str().eq_ignore_ascii_case("f") => {
                                if let Some(session) = ctx.session_mut() {
                                    session.search_overlay = Some(SearchOverlay::new());
                                }
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Character(c) if c.as_str().eq_ignore_ascii_case("z") => {
                                if let Some(session) = ctx.session_mux.focused_session_mut() {
                                    if let Some(ref store) = session.snapshot_store {
                                        let engine = glass_snapshot::UndoEngine::new(store);
                                        match engine.undo_latest() {
                                            Ok(Some(result)) => {
                                                // Count outcomes for summary line
                                                let (
                                                    mut restored,
                                                    mut deleted,
                                                    mut skipped,
                                                    mut conflicts,
                                                    mut errors,
                                                ) = (0u32, 0u32, 0u32, 0u32, 0u32);
                                                for (path, outcome) in &result.files {
                                                    match outcome {
                                                        glass_snapshot::FileOutcome::Restored => {
                                                            restored += 1;
                                                            tracing::info!(
                                                                "Undo: restored {}",
                                                                path.display()
                                                            );
                                                        }
                                                        glass_snapshot::FileOutcome::Deleted => {
                                                            deleted += 1;
                                                            tracing::info!(
                                                                "Undo: deleted {}",
                                                                path.display()
                                                            );
                                                        }
                                                        glass_snapshot::FileOutcome::Conflict {
                                                            ..
                                                        } => {
                                                            conflicts += 1;
                                                            tracing::warn!(
                                                                "Undo: CONFLICT {}",
                                                                path.display()
                                                            );
                                                        }
                                                        glass_snapshot::FileOutcome::Error(e) => {
                                                            errors += 1;
                                                            tracing::error!(
                                                                "Undo: error {}: {}",
                                                                path.display(),
                                                                e
                                                            );
                                                        }
                                                        glass_snapshot::FileOutcome::Skipped => {
                                                            skipped += 1;
                                                            tracing::info!(
                                                                "Undo: skipped {}",
                                                                path.display()
                                                            );
                                                        }
                                                    }
                                                }
                                                tracing::info!(
                                                    "Undo complete: {} restored, {} deleted, {} skipped, {} conflicts, {} errors",
                                                    restored, deleted, skipped, conflicts, errors,
                                                );
                                                // Remove [undo] label from the undone block (visual feedback).
                                                let epoch_to_clear = session
                                                    .block_manager
                                                    .blocks()
                                                    .iter()
                                                    .rev()
                                                    .find(|b| b.has_snapshot)
                                                    .and_then(|b| b.started_epoch);
                                                if let Some(ep) = epoch_to_clear {
                                                    if let Some(b) = session
                                                        .block_manager
                                                        .find_block_by_epoch_mut(ep)
                                                    {
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
                            Key::Character(c) if c.as_str().eq_ignore_ascii_case("u") => {
                                // Ctrl+Shift+U: Apply available update
                                if let Some(ref info) = self.update_info {
                                    if let Err(e) = glass_core::updater::apply_update(info) {
                                        tracing::warn!("Failed to apply update: {}", e);
                                    }
                                }
                                return;
                            }
                            Key::Character(c) if c.as_str().eq_ignore_ascii_case("p") => {
                                // Ctrl+Shift+P: Toggle pipeline expansion on most recent pipeline block
                                if let Some(session) = ctx.session_mux.focused_session_mut() {
                                    if let Some(block) =
                                        session.block_manager.blocks_mut().iter_mut().rev().find(
                                            |b| {
                                                b.pipeline_stage_count.unwrap_or(0) > 0
                                                    || b.pipeline_stage_commands.len() > 1
                                            },
                                        )
                                    {
                                        block.toggle_pipeline_expanded();
                                    }
                                }
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Character(c) if c.as_str().eq_ignore_ascii_case("o") => {
                                // Ctrl+Shift+O: Toggle orchestrator on/off
                                self.orchestrator.active = !self.orchestrator.active;
                                if self.orchestrator.active {
                                    tracing::info!("Orchestrator: enabled by user");
                                    self.orchestrator_activated_at =
                                        Some(std::time::Instant::now());
                                    self.orchestrator.reset_stuck();
                                    self.orchestrator.iterations_since_checkpoint = 0;
                                    self.orchestrator.bounded_stop_pending = false;
                                    self.orchestrator.kickoff_complete = false;
                                    self.orchestrator.last_user_keypress = None;
                                    self.orchestrator.max_iterations = self
                                        .config
                                        .agent
                                        .as_ref()
                                        .and_then(|a| a.orchestrator.as_ref())
                                        .and_then(|o| o.max_iterations);

                                    // Respawn agent with fresh system prompt using the
                                    // terminal's current CWD (not Glass's CWD)
                                    let current_cwd = ctx
                                        .session_mux
                                        .focused_session()
                                        .map(|s| s.status.cwd().to_string())
                                        .unwrap_or_else(|| {
                                            std::env::current_dir()
                                                .unwrap_or_default()
                                                .to_string_lossy()
                                                .to_string()
                                        });

                                    // Store project root for all subsequent orchestrator
                                    // operations. The shell's OSC 7 CWD stops updating once
                                    // Claude Code starts, so we capture it here.
                                    self.orchestrator.project_root = current_cwd.clone();

                                    // Load scripts for the project now that we know its root.
                                    self.script_bridge.load_for_project(&current_cwd);

                                    // Reset per-run script tracking for the new orchestrator run.
                                    self.script_bridge.reset_run_tracking();

                                    // Fire scripting OrchestratorRunStart hook
                                    fire_hook_on_bridge(
                                        &mut self.script_bridge,
                                        &self.orchestrator.project_root,
                                        glass_scripting::HookPoint::OrchestratorRunStart,
                                        &glass_scripting::HookEventData::new(),
                                    );

                                    // Kickoff flow: check PRD existence
                                    let prd_rel = self
                                        .config
                                        .agent
                                        .as_ref()
                                        .and_then(|a| a.orchestrator.as_ref())
                                        .map(|o| o.prd_path.clone())
                                        .unwrap_or_else(|| "PRD.md".to_string());
                                    let prd_path =
                                        std::path::Path::new(&current_cwd).join(&prd_rel);
                                    if prd_path.exists() {
                                        // PRD exists — prompt user to continue or start fresh
                                        if let Some(session) = ctx.session_mux.focused_session() {
                                            let msg = format!(
                                                "\r\n[GLASS] Found existing PRD at {}. Continue with it? (y=continue, n=start fresh)\r\n",
                                                prd_rel
                                            );
                                            let bytes = msg.into_bytes();
                                            pty_send(
                                                &session.pty_sender,
                                                PtyMsg::Input(std::borrow::Cow::Owned(bytes)),
                                            );
                                        }
                                        tracing::info!(
                                            "Orchestrator: PRD found at {}, prompting user",
                                            prd_path.display()
                                        );
                                    } else {
                                        // No PRD — tell user to describe what they want
                                        if let Some(session) = ctx.session_mux.focused_session() {
                                            let msg = "\r\n[GLASS] Orchestrator active. No PRD found — describe what you want to build, then wait for the loop to start.\r\n";
                                            pty_send(
                                                &session.pty_sender,
                                                PtyMsg::Input(std::borrow::Cow::Owned(
                                                    msg.as_bytes().to_vec(),
                                                )),
                                            );
                                        }
                                        tracing::info!(
                                            "Orchestrator: no PRD at {} — kickoff mode",
                                            prd_path.display()
                                        );
                                    }

                                    // Auto-detect mode from project + PRD
                                    let prd_content = std::fs::read_to_string(&prd_path).ok();
                                    let (detected_mode, detected_verify, detected_files) =
                                        orchestrator::auto_detect_orchestrator_config(
                                            &current_cwd,
                                            prd_content.as_deref(),
                                        );
                                    tracing::info!(
                                        "Orchestrator auto-detect: mode={}, verify={}, files={:?}",
                                        detected_mode,
                                        detected_verify,
                                        detected_files
                                    );

                                    // Suppress config reload agent restarts for 2 seconds.
                                    // We're about to write 3 values to config.toml, which
                                    // triggers ConfigReloaded events that would kill the
                                    // agent we just spawned.
                                    self.config_write_suppress_until = Some(
                                        std::time::Instant::now()
                                            + std::time::Duration::from_secs(2),
                                    );

                                    // Write auto-detected values to config.toml
                                    if let Some(config_path) = dirs::home_dir()
                                        .map(|h| h.join(".glass").join("config.toml"))
                                    {
                                        let _ = glass_core::config::update_config_field(
                                            &config_path,
                                            Some("agent.orchestrator"),
                                            "orchestrator_mode",
                                            &format!("\"{}\"", detected_mode),
                                        );
                                        let _ = glass_core::config::update_config_field(
                                            &config_path,
                                            Some("agent.orchestrator"),
                                            "verify_mode",
                                            &format!("\"{}\"", detected_verify),
                                        );
                                        if !detected_files.is_empty() {
                                            let files_toml = format!(
                                                "[{}]",
                                                detected_files
                                                    .iter()
                                                    .map(|f| format!("\"{f}\""))
                                                    .collect::<Vec<_>>()
                                                    .join(", ")
                                            );
                                            let _ = glass_core::config::update_config_field(
                                                &config_path,
                                                Some("agent.orchestrator"),
                                                "verify_files",
                                                &files_toml,
                                            );
                                        }
                                    }

                                    // Capture context for handoff (gather from ctx before calling helper)
                                    let terminal_context = ctx
                                        .session_mux
                                        .focused_session()
                                        .map(|session| {
                                            extract_term_lines(&session.term, 100).join("\n")
                                        })
                                        .unwrap_or_default();

                                    // Check for handoff note
                                    let handoff_path = std::path::Path::new(&current_cwd)
                                        .join(".glass")
                                        .join("handoff.md");
                                    let handoff_note = std::fs::read_to_string(&handoff_path).ok();

                                    let git_log = git_cmd()
                                        .args(["log", "--oneline", "-10"])
                                        .current_dir(&current_cwd)
                                        .output()
                                        .ok()
                                        .and_then(|o| {
                                            if o.status.success() {
                                                String::from_utf8(o.stdout).ok()
                                            } else {
                                                None
                                            }
                                        });

                                    // Regenerate checkpoint.md from current git state so the
                                    // agent sees accurate context — not a stale checkpoint from
                                    // a previous run that the user may have worked past manually.
                                    {
                                        let cp_dir =
                                            std::path::Path::new(&current_cwd).join(".glass");
                                        let _ = std::fs::create_dir_all(&cp_dir);
                                        let git_diff = git_cmd()
                                            .args(["diff", "--stat"])
                                            .current_dir(&current_cwd)
                                            .output()
                                            .ok()
                                            .and_then(|o| {
                                                if o.status.success() {
                                                    String::from_utf8(o.stdout).ok()
                                                } else {
                                                    None
                                                }
                                            });
                                        let prd_status = if prd_path.exists() {
                                            "PRD exists"
                                        } else {
                                            "No PRD yet"
                                        };
                                        let checkpoint = format!(
                                            "# Checkpoint (auto-generated on re-enable)\n\n\
                                             ## Status\n{prd_status}\n\n\
                                             ## Recent Commits\n{}\n\n\
                                             ## Uncommitted Changes\n{}\n\n\
                                             ## Next\nReview terminal context and PRD, then continue.\n",
                                            git_log.as_deref().unwrap_or("(no git history)"),
                                            git_diff.as_deref().unwrap_or("(clean working tree)"),
                                        );
                                        let _ = std::fs::write(
                                            cp_dir.join("checkpoint.md"),
                                            &checkpoint,
                                        );
                                    }

                                    let mut content = String::from("[ORCHESTRATOR_HANDOFF]\nThe user just enabled orchestration. Pick up where they left off.\n");
                                    if let Some(ref note) = handoff_note {
                                        content
                                            .push_str(&format!("\nUSER INSTRUCTIONS:\n{}\n", note));
                                    }
                                    if let Some(log) = git_log {
                                        content.push_str(&format!(
                                            "\nRECENT GIT HISTORY:\n{}\n",
                                            log.trim()
                                        ));
                                    }
                                    content.push_str(&format!(
                                        "\nTERMINAL CONTEXT (last 100 lines):\n{}\n",
                                        terminal_context
                                    ));

                                    // Drop ctx borrow, then respawn (needs &mut self)
                                    let _ = ctx;
                                    self.respawn_orchestrator_agent(&current_cwd, content);

                                    // Delete handoff.md only after agent starts successfully
                                    if handoff_note.is_some() && self.agent_runtime.is_some() {
                                        let _ = std::fs::remove_file(&handoff_path);
                                    }

                                    // Initialize metric guard
                                    let verify_mode = self
                                        .config
                                        .agent
                                        .as_ref()
                                        .and_then(|a| a.orchestrator.as_ref())
                                        .map(|o| o.verify_mode.as_str())
                                        .unwrap_or("floor");

                                    if verify_mode == "floor" {
                                        let user_cmd = self
                                            .config
                                            .agent
                                            .as_ref()
                                            .and_then(|a| a.orchestrator.as_ref())
                                            .and_then(|o| o.verify_command.clone());

                                        let commands = if let Some(cmd) = user_cmd {
                                            vec![orchestrator::VerifyCommand {
                                                name: cmd.clone(),
                                                cmd,
                                            }]
                                        } else {
                                            orchestrator::auto_detect_verify_commands(&current_cwd)
                                        };

                                        if !commands.is_empty() {
                                            // Preserve existing baseline across re-activations
                                            // (e.g., user toggles off/on, or checkpoint respawn).
                                            // Only create a new baseline if none exists.
                                            if self.orchestrator.metric_baseline.is_none() {
                                                let cmd_count = commands.len();
                                                let mut baseline =
                                                    orchestrator::MetricBaseline::new();
                                                baseline.commands = commands;
                                                self.orchestrator.metric_baseline = Some(baseline);
                                                tracing::info!(
                                                    "Metric guard initialized with {cmd_count} commands"
                                                );
                                            } else {
                                                tracing::info!(
                                                    "Metric guard: preserving existing baseline"
                                                );
                                            }
                                        }
                                    }

                                    // Baseline verification deferred to first iteration.
                                    // Running cargo test at enable time blocks the orchestrator
                                    // for minutes while compiling + running all tests. Instead,
                                    // the VerifyComplete handler establishes the baseline lazily
                                    // on the first verification run after the agent's first iteration.

                                    // Start artifact watcher
                                    let artifact_path = self
                                        .config
                                        .agent
                                        .as_ref()
                                        .and_then(|a| a.orchestrator.as_ref())
                                        .map(|o| o.completion_artifact.clone())
                                        .unwrap_or_else(|| ".glass/done".to_string());
                                    if let Some(session) = self
                                        .windows
                                        .get(&window_id)
                                        .and_then(|ctx| ctx.session_mux.focused_session())
                                    {
                                        let sid = session.id;
                                        self.artifact_watcher_thread = start_artifact_watcher(
                                            &artifact_path,
                                            &current_cwd,
                                            self.proxy.clone(),
                                            window_id,
                                            sid,
                                        );
                                    }

                                    // Initialize feedback loop
                                    self.orchestrator.feedback_waste_iterations = 0;
                                    self.orchestrator.feedback_stuck_count = 0;
                                    self.orchestrator.feedback_checkpoint_count = 0;
                                    self.orchestrator.feedback_verify_sequence.clear();
                                    self.orchestrator.feedback_agent_responses.clear();
                                    self.orchestrator.feedback_completion_reason.clear();
                                    self.orchestrator.feedback_commit_count = 0;
                                    self.orchestrator.feedback_reverted_files.clear();
                                    self.orchestrator.feedback_fast_trigger_during_output = 0;
                                    self.orchestrator.feedback_iteration_timestamps.clear();

                                    let feedback_config = glass_feedback::FeedbackConfig {
                                        project_root: current_cwd.clone(),
                                        feedback_llm: self
                                            .config
                                            .agent
                                            .as_ref()
                                            .and_then(|a| a.orchestrator.as_ref())
                                            .map(|o| o.feedback_llm)
                                            .unwrap_or(false),
                                        max_prompt_hints: self
                                            .config
                                            .agent
                                            .as_ref()
                                            .and_then(|a| a.orchestrator.as_ref())
                                            .map(|o| o.max_prompt_hints)
                                            .unwrap_or(10),
                                        silence_timeout_secs: self
                                            .config
                                            .agent
                                            .as_ref()
                                            .and_then(|a| a.orchestrator.as_ref())
                                            .map(|o| o.silence_timeout_secs),
                                        max_retries_before_stuck: self
                                            .config
                                            .agent
                                            .as_ref()
                                            .and_then(|a| a.orchestrator.as_ref())
                                            .map(|o| o.max_retries_before_stuck),
                                        ablation_enabled: self
                                            .config
                                            .agent
                                            .as_ref()
                                            .and_then(|a| a.orchestrator.as_ref())
                                            .map(|o| o.ablation_enabled)
                                            .unwrap_or(true),
                                        ablation_sweep_interval: self
                                            .config
                                            .agent
                                            .as_ref()
                                            .and_then(|a| a.orchestrator.as_ref())
                                            .map(|o| o.ablation_sweep_interval)
                                            .unwrap_or(20),
                                    };

                                    self.feedback_state = Some(glass_feedback::on_run_start(
                                        &current_cwd,
                                        &feedback_config,
                                    ));

                                    // Cache PRD deliverables for scope guard
                                    let prd_full = std::path::Path::new(&current_cwd).join(prd_rel);
                                    if let Ok(prd_text) = std::fs::read_to_string(&prd_full) {
                                        self.orchestrator.prd_deliverable_files =
                                            orchestrator::parse_prd_deliverables(&prd_text);
                                    }

                                    // Reset enforcement state
                                    self.orchestrator.instruction_buffer.clear();
                                    self.orchestrator.dependency_block = None;
                                    self.orchestrator.dependency_block_iterations = 0;
                                    self.orchestrator.iterations_since_last_commit = 0;
                                    self.orchestrator.last_known_head = None;
                                } else {
                                    tracing::info!("Orchestrator: disabled by user");
                                    // Fire scripting OrchestratorRunEnd hook
                                    {
                                        let mut event = glass_scripting::HookEventData::new();
                                        event.set("iterations", self.orchestrator.iteration as i64);
                                        fire_hook_on_bridge(
                                            &mut self.script_bridge,
                                            &self.orchestrator.project_root,
                                            glass_scripting::HookPoint::OrchestratorRunEnd,
                                            &event,
                                        );
                                    }
                                    self.orchestrator.feedback_completion_reason =
                                        "user_cancelled".to_string();
                                    // Handle active synthesis: write fallback checkpoint
                                    if matches!(
                                        self.orchestrator.checkpoint_phase,
                                        orchestrator::CheckpointPhase::Synthesizing { .. }
                                    ) {
                                        tracing::info!("Orchestrator disabled during synthesis — writing fallback checkpoint");
                                        let cwd = ctx
                                            .session_mux
                                            .focused_session()
                                            .map(|s| s.status.cwd().to_string())
                                            .unwrap_or_default();
                                        let cp_path = std::path::Path::new(&cwd)
                                            .join(".glass")
                                            .join("checkpoint.md");
                                        if let Some(parent) = cp_path.parent() {
                                            let _ = std::fs::create_dir_all(parent);
                                        }
                                        if let Some(fallback) =
                                            self.orchestrator.cached_checkpoint_fallback.take()
                                        {
                                            let _ = std::fs::write(&cp_path, &fallback);
                                        }
                                        self.orchestrator.checkpoint_phase =
                                            orchestrator::CheckpointPhase::Idle;
                                        self.orchestrator.cached_checkpoint_fallback = None;
                                    }
                                    let _ = ctx;
                                    self.run_feedback_on_end();
                                    // Stop artifact watcher
                                    if let Some(handle) = self.artifact_watcher_thread.take() {
                                        handle.thread().unpark();
                                    }
                                }
                                if let Some(ctx) = self.windows.get(&window_id) {
                                    ctx.window.request_redraw();
                                }
                                return;
                            }
                            _ => {}
                        }
                    }

                    // Shift+PageUp/Down: scrollback
                    if modifiers.shift_key() && !modifiers.control_key() && !modifiers.alt_key() {
                        match &event.logical_key {
                            Key::Named(NamedKey::PageUp) => {
                                if let Some(session) = ctx.session() {
                                    session.term.lock().scroll_display(Scroll::PageUp);
                                }
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::PageDown) => {
                                if let Some(session) = ctx.session() {
                                    session.term.lock().scroll_display(Scroll::PageDown);
                                }
                                ctx.window.request_redraw();
                                return;
                            }
                            _ => {}
                        }
                    }

                    // Ctrl+Tab / Ctrl+Shift+Tab: cycle tabs
                    if modifiers.control_key() {
                        if let Key::Named(NamedKey::Tab) = &event.logical_key {
                            if modifiers.shift_key() {
                                ctx.session_mux.prev_tab();
                            } else {
                                ctx.session_mux.next_tab();
                            }
                            ctx.window.request_redraw();
                            return;
                        }
                    }

                    // Ctrl+1-9 / Cmd+1-9: jump to tab by index
                    if glass_mux::is_action_modifier(modifiers) {
                        if let Key::Character(c) = &event.logical_key {
                            if let Some(digit) =
                                c.as_str().chars().next().and_then(|ch| ch.to_digit(10))
                            {
                                if (1..=9).contains(&digit) {
                                    ctx.session_mux.activate_tab((digit as usize) - 1);
                                    ctx.window.request_redraw();
                                    return;
                                }
                            }
                        }
                    }

                    // Alt+Arrow: move focus between panes
                    // Alt+Shift+Arrow: resize split ratio
                    if modifiers.alt_key() && !modifiers.control_key() {
                        let arrow_dir = match &event.logical_key {
                            Key::Named(NamedKey::ArrowLeft) => Some(FocusDirection::Left),
                            Key::Named(NamedKey::ArrowRight) => Some(FocusDirection::Right),
                            Key::Named(NamedKey::ArrowUp) => Some(FocusDirection::Up),
                            Key::Named(NamedKey::ArrowDown) => Some(FocusDirection::Down),
                            _ => None,
                        };

                        if let Some(dir) = arrow_dir {
                            if modifiers.shift_key() {
                                // Alt+Shift+Arrow: resize split ratio
                                let (split_dir, delta) = match dir {
                                    FocusDirection::Left => (SplitDirection::Horizontal, -0.05f32),
                                    FocusDirection::Right => (SplitDirection::Horizontal, 0.05f32),
                                    FocusDirection::Up => (SplitDirection::Vertical, -0.05f32),
                                    FocusDirection::Down => (SplitDirection::Vertical, 0.05f32),
                                };
                                ctx.session_mux.resize_focused_split(split_dir, delta);
                                // Resize all panes' PTYs with new dimensions
                                let size = ctx.window.inner_size();
                                resize_all_panes(
                                    &mut ctx.session_mux,
                                    &ctx.frame_renderer,
                                    size.width,
                                    size.height,
                                );
                                ctx.window.request_redraw();
                                return;
                            } else {
                                // Alt+Arrow: move focus
                                if ctx.session_mux.active_tab_pane_count() > 1 {
                                    let (_cell_w, cell_h) = ctx.frame_renderer.cell_size();
                                    let sc = ctx.renderer.surface_config();
                                    let container = ViewportLayout {
                                        x: 0,
                                        y: cell_h as u32,
                                        width: sc.width,
                                        height: sc.height.saturating_sub((cell_h as u32) * 2),
                                    };
                                    if let Some(focused) = ctx.session_mux.focused_session_id() {
                                        if let Some(root) = ctx.session_mux.active_tab_root() {
                                            if let Some(target) =
                                                root.find_neighbor(focused, dir, &container)
                                            {
                                                ctx.session_mux.set_focused_pane(target);
                                                ctx.window.request_redraw();
                                                return;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // When the settings overlay is open, intercept all navigation keys.
                    if self.settings_overlay_visible && event.state == ElementState::Pressed {
                        match &event.logical_key {
                            Key::Named(NamedKey::Escape) => {
                                if self.settings_editing {
                                    // Cancel inline edit
                                    self.settings_editing = false;
                                    self.settings_edit_buffer.clear();
                                } else {
                                    self.settings_overlay_visible = false;
                                    self.settings_overlay_tab = Default::default();
                                    self.settings_section_index = 0;
                                    self.settings_field_index = 0;
                                    self.settings_shortcuts_scroll = 0;
                                }
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::Tab) if modifiers.shift_key() => {
                                self.settings_overlay_tab = self.settings_overlay_tab.prev();
                                self.settings_field_index = 0;
                                self.settings_shortcuts_scroll = 0;
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::Tab) => {
                                self.settings_overlay_tab = self.settings_overlay_tab.next();
                                self.settings_field_index = 0;
                                self.settings_shortcuts_scroll = 0;
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::ArrowUp) => {
                                match self.settings_overlay_tab {
                                    glass_renderer::SettingsTab::Settings => {
                                        if self.settings_field_index > 0 {
                                            self.settings_field_index -= 1;
                                        } else if self.settings_section_index > 0 {
                                            self.settings_section_index -= 1;
                                            self.settings_field_index = 0;
                                        }
                                    }
                                    glass_renderer::SettingsTab::Shortcuts => {
                                        self.settings_shortcuts_scroll =
                                            self.settings_shortcuts_scroll.saturating_sub(1);
                                    }
                                    _ => {}
                                }
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::ArrowDown) => {
                                match self.settings_overlay_tab {
                                    glass_renderer::SettingsTab::Settings => {
                                        self.settings_field_index += 1;
                                        // Clamping happens in renderer (fields_for_section length)
                                    }
                                    glass_renderer::SettingsTab::Shortcuts => {
                                        self.settings_shortcuts_scroll += 1;
                                    }
                                    _ => {}
                                }
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::ArrowLeft) => {
                                if self.settings_section_index > 0 {
                                    self.settings_section_index -= 1;
                                    self.settings_field_index = 0;
                                }
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::ArrowRight) => {
                                if self.settings_section_index
                                    < glass_renderer::settings_overlay::SETTINGS_SECTIONS.len() - 1
                                {
                                    self.settings_section_index += 1;
                                    self.settings_field_index = 0;
                                }
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                                if matches!(
                                    self.settings_overlay_tab,
                                    glass_renderer::SettingsTab::Settings
                                ) {
                                    if let Some((section, key, value)) = handle_settings_activate(
                                        &self.config,
                                        self.settings_section_index,
                                        self.settings_field_index,
                                    ) {
                                        if let Some(config_path) =
                                            glass_core::config::GlassConfig::config_path()
                                        {
                                            if let Err(e) = glass_core::config::update_config_field(
                                                &config_path,
                                                section,
                                                key,
                                                &value,
                                            ) {
                                                tracing::warn!(
                                                    "Settings: failed to write config: {}",
                                                    e
                                                );
                                            }
                                        }
                                    }
                                    ctx.window.request_redraw();
                                }
                                return;
                            }
                            Key::Character(c) if c.as_str() == "+" || c.as_str() == "=" => {
                                if matches!(
                                    self.settings_overlay_tab,
                                    glass_renderer::SettingsTab::Settings
                                ) {
                                    if let Some((section, key, value)) = handle_settings_increment(
                                        &self.config,
                                        self.settings_section_index,
                                        self.settings_field_index,
                                        true,
                                    ) {
                                        if let Some(config_path) =
                                            glass_core::config::GlassConfig::config_path()
                                        {
                                            if let Err(e) = glass_core::config::update_config_field(
                                                &config_path,
                                                section,
                                                key,
                                                &value,
                                            ) {
                                                tracing::warn!(
                                                    "Settings: failed to write config: {}",
                                                    e
                                                );
                                            }
                                        }
                                    }
                                    ctx.window.request_redraw();
                                }
                                return;
                            }
                            Key::Character(c) if c.as_str() == "-" => {
                                if matches!(
                                    self.settings_overlay_tab,
                                    glass_renderer::SettingsTab::Settings
                                ) {
                                    if let Some((section, key, value)) = handle_settings_increment(
                                        &self.config,
                                        self.settings_section_index,
                                        self.settings_field_index,
                                        false,
                                    ) {
                                        if let Some(config_path) =
                                            glass_core::config::GlassConfig::config_path()
                                        {
                                            if let Err(e) = glass_core::config::update_config_field(
                                                &config_path,
                                                section,
                                                key,
                                                &value,
                                            ) {
                                                tracing::warn!(
                                                    "Settings: failed to write config: {}",
                                                    e
                                                );
                                            }
                                        }
                                    }
                                    ctx.window.request_redraw();
                                }
                                return;
                            }
                            _ => {
                                return; // Consume all other keys
                            }
                        }
                    }

                    // When the activity overlay is open, intercept navigation keys.
                    if self.activity_overlay_visible && event.state == ElementState::Pressed {
                        match &event.logical_key {
                            Key::Named(NamedKey::Escape) => {
                                self.activity_overlay_visible = false;
                                self.activity_view_filter = Default::default();
                                self.activity_scroll_offset = 0;
                                self.orchestrator_scroll_offset = 0;
                                self.activity_verbose = false;
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::Tab) if modifiers.shift_key() => {
                                self.activity_view_filter = self.activity_view_filter.prev();
                                self.activity_scroll_offset = 0;
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::Tab) => {
                                self.activity_view_filter = self.activity_view_filter.next();
                                self.activity_scroll_offset = 0;
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::ArrowUp) | Key::Named(NamedKey::PageUp)
                                if self.activity_view_filter
                                    == glass_renderer::ActivityViewFilter::Orchestrator =>
                            {
                                let step =
                                    if matches!(event.logical_key, Key::Named(NamedKey::PageUp)) {
                                        20
                                    } else {
                                        1
                                    };
                                self.orchestrator_scroll_offset =
                                    self.orchestrator_scroll_offset.saturating_add(step);
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::ArrowDown) | Key::Named(NamedKey::PageDown)
                                if self.activity_view_filter
                                    == glass_renderer::ActivityViewFilter::Orchestrator =>
                            {
                                let step = if matches!(
                                    event.logical_key,
                                    Key::Named(NamedKey::PageDown)
                                ) {
                                    20
                                } else {
                                    1
                                };
                                self.orchestrator_scroll_offset =
                                    self.orchestrator_scroll_offset.saturating_sub(step);
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::ArrowUp) => {
                                self.activity_scroll_offset =
                                    self.activity_scroll_offset.saturating_sub(1);
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::ArrowDown) => {
                                self.activity_scroll_offset += 1;
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Character(c) if c.as_str().eq_ignore_ascii_case("v") => {
                                self.activity_verbose = !self.activity_verbose;
                                ctx.window.request_redraw();
                                return;
                            }
                            _ => {} // Fall through to PTY
                        }
                    }

                    // When the proposal review overlay is open, intercept arrow keys and Escape
                    // for navigation. All other keys fall through to PTY (AGTU-05).
                    if self.agent_review_open && event.state == ElementState::Pressed {
                        match &event.logical_key {
                            Key::Named(NamedKey::ArrowUp) => {
                                self.proposal_review_selected =
                                    self.proposal_review_selected.saturating_sub(1);
                                self.proposal_diff_cache = None;
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::ArrowDown) => {
                                let max = self.agent_proposal_worktrees.len().saturating_sub(1);
                                self.proposal_review_selected =
                                    (self.proposal_review_selected + 1).min(max);
                                self.proposal_diff_cache = None;
                                ctx.window.request_redraw();
                                return;
                            }
                            Key::Named(NamedKey::Escape) => {
                                self.agent_review_open = false;
                                ctx.window.request_redraw();
                                return;
                            }
                            _ => {} // Fall through to PTY -- do NOT swallow (AGTU-05)
                        }
                    }

                    // Forward to PTY via encoder
                    let key_start = std::time::Instant::now();
                    if let Some(bytes) = encode_key(&event.logical_key, modifiers, mode) {
                        // Track user keypress for kickoff suppression.
                        // During the kickoff phase, the silence trigger is suppressed
                        // as long as the user is actively typing.
                        if self.orchestrator.active && !self.orchestrator.kickoff_complete {
                            self.orchestrator.mark_user_keypress();
                        }

                        // Orchestrator no longer auto-pauses on user input.
                        // Only Ctrl+Shift+O toggles orchestration on/off.
                        if let Some(session) = ctx.session() {
                            pty_send(&session.pty_sender, PtyMsg::Input(Cow::Owned(bytes)));
                        }
                        tracing::trace!("PERF key_latency={:?}", key_start.elapsed());
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(session) = ctx.session_mut() {
                    session.cursor_position = Some((position.x, position.y));
                }

                let mouse_x = position.x as f32;
                let mouse_y = position.y as f32;

                // Handle active scrollbar drag: update scroll position from mouse Y
                if let Some(ref drag) = ctx.scrollbar_dragging {
                    let effective_y = mouse_y - drag.thumb_grab_offset;
                    let scrollable_track = drag.track_height - drag.thumb_height;
                    if scrollable_track > 0.0 {
                        let ratio =
                            ((effective_y - drag.track_y) / scrollable_track).clamp(0.0, 1.0);
                        let pane_id = drag.pane_id;
                        if let Some(session) = ctx.session_mux.session(pane_id) {
                            let mut term = session.term.lock();
                            // Use current history_size, not the stale captured value.
                            // History grows during long sessions (orchestrator runs),
                            // and stale size causes the scroll to snap back or get stuck.
                            let current_history = term.grid().history_size();
                            // ratio 0.0 = top (oldest), 1.0 = bottom (newest)
                            let target_offset =
                                ((1.0 - ratio) * current_history as f32).round() as i32;
                            let current = term.grid().display_offset() as i32;
                            let delta = target_offset - current;
                            if delta != 0 {
                                term.scroll_display(Scroll::Delta(delta));
                            }
                            drop(term);
                        }
                        ctx.window.request_redraw();
                    }
                    return;
                }

                // Update scrollbar hover state
                {
                    let (_, cell_h) = ctx.frame_renderer.cell_size();
                    let sc = ctx.renderer.surface_config();
                    let grid_y_offset = if ctx.session_mux.tab_count() > 0 {
                        cell_h
                    } else {
                        0.0
                    };
                    let status_bar_h = cell_h;
                    let pane_height = sc.height as f32 - grid_y_offset - status_bar_h;

                    let new_hovered = if ctx.session_mux.active_tab_pane_count() > 1 {
                        // Multi-pane: check each pane's scrollbar
                        let container = ViewportLayout {
                            x: 0,
                            y: cell_h as u32,
                            width: sc.width,
                            height: sc.height.saturating_sub((cell_h as u32) * 2),
                        };
                        let pane_layouts: Vec<(SessionId, ViewportLayout)> = ctx
                            .session_mux
                            .active_tab_root()
                            .map(|root| root.compute_layout(&container))
                            .unwrap_or_default();

                        let mut found = None;
                        for (sid, vp) in &pane_layouts {
                            let scrollbar_x = (vp.x + vp.width) as f32 - SCROLLBAR_WIDTH;
                            let vp_y = vp.y as f32;
                            let vp_h = vp.height as f32;
                            if let Some(session) = ctx.session_mux.session(*sid) {
                                let term = session.term.lock();
                                let display_offset = term.grid().display_offset();
                                let history_size = term.grid().history_size();
                                let screen_lines = term.screen_lines();
                                drop(term);
                                if ctx
                                    .frame_renderer
                                    .scrollbar()
                                    .hit_test(
                                        mouse_x,
                                        mouse_y,
                                        scrollbar_x,
                                        vp_y,
                                        vp_h,
                                        display_offset,
                                        history_size,
                                        screen_lines,
                                    )
                                    .is_some()
                                {
                                    found = Some(*sid);
                                    break;
                                }
                            }
                        }
                        found
                    } else {
                        // Single-pane: check the one scrollbar
                        let scrollbar_x = sc.width as f32 - SCROLLBAR_WIDTH;
                        let focused_sid = ctx.session_mux.focused_session_id();
                        if let Some(sid) = focused_sid {
                            if let Some(session) = ctx.session_mux.session(sid) {
                                let term = session.term.lock();
                                let display_offset = term.grid().display_offset();
                                let history_size = term.grid().history_size();
                                let screen_lines = term.screen_lines();
                                drop(term);
                                if ctx
                                    .frame_renderer
                                    .scrollbar()
                                    .hit_test(
                                        mouse_x,
                                        mouse_y,
                                        scrollbar_x,
                                        grid_y_offset,
                                        pane_height,
                                        display_offset,
                                        history_size,
                                        screen_lines,
                                    )
                                    .is_some()
                                {
                                    Some(sid)
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    };

                    if new_hovered != ctx.scrollbar_hovered_pane {
                        ctx.scrollbar_hovered_pane = new_hovered;
                        ctx.window.request_redraw();
                    }
                }

                // Handle active tab drag: update drop index from mouse X
                if let Some(ref mut drag) = ctx.tab_drag_state {
                    if !drag.active && (mouse_x - drag.start_x).abs() > 5.0 {
                        drag.active = true;
                    }
                    if drag.active {
                        let viewport_w = ctx.window.inner_size().width as f32;
                        let drop_idx = ctx.frame_renderer.tab_bar().drag_drop_index(
                            mouse_x,
                            ctx.session_mux.tab_count(),
                            viewport_w,
                        );
                        drag.drop_index = Some(drop_idx);
                        ctx.window.request_redraw();
                    }
                    return; // Consume event during drag
                }

                // Tab bar hover tracking
                if ctx.session_mux.tab_count() > 0 {
                    let (_, cell_h) = ctx.frame_renderer.cell_size();
                    let new_tab_hovered = if mouse_y < cell_h {
                        let viewport_w = ctx.window.inner_size().width as f32;
                        ctx.frame_renderer.tab_bar().hit_test_tab_index(
                            mouse_x,
                            ctx.session_mux.tab_count(),
                            viewport_w,
                        )
                    } else {
                        None
                    };
                    if new_tab_hovered != ctx.tab_bar_hovered_tab {
                        ctx.tab_bar_hovered_tab = new_tab_hovered;
                        ctx.window.request_redraw();
                    }
                }

                // Update selection during mouse drag
                if ctx.mouse_left_pressed {
                    let (cell_w, cell_h) = ctx.frame_renderer.cell_size();
                    let grid_y_offset = if ctx.session_mux.tab_count() > 0 {
                        cell_h
                    } else {
                        0.0
                    };
                    let px = position.x as f32;
                    let py = position.y as f32 - grid_y_offset;
                    if py >= 0.0 {
                        let col = (px / cell_w) as usize;
                        let row = (py / cell_h) as usize;
                        let side = if (px % cell_w) < cell_w / 2.0 {
                            Side::Left
                        } else {
                            Side::Right
                        };
                        if let Some(session) = ctx.session() {
                            let mut term = session.term.lock();
                            let display_offset = term.grid().display_offset();
                            let columns = term.columns();
                            let screen_lines = term.screen_lines();
                            let col = col.min(columns.saturating_sub(1));
                            let row = row.min(screen_lines.saturating_sub(1));
                            let point = alacritty_terminal::index::Point::new(
                                Line(row as i32 - display_offset as i32),
                                Column(col),
                            );
                            if let Some(ref mut sel) = term.selection {
                                sel.update(point, side);
                            }
                            drop(term);
                            ctx.window.request_redraw();
                        }
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                ctx.mouse_left_pressed = true;

                // Tab bar click handling
                if let Some((x, y)) = ctx.session().and_then(|s| s.cursor_position) {
                    let (_, cell_h) = ctx.frame_renderer.cell_size();
                    if (y as f32) < cell_h {
                        // Click is in tab bar region
                        ctx.mouse_left_pressed = false;
                        let viewport_w = ctx.window.inner_size().width as f32;
                        match ctx.frame_renderer.tab_bar().hit_test(
                            x as f32,
                            ctx.session_mux.tab_count(),
                            viewport_w,
                        ) {
                            Some(TabHitResult::Tab(tab_idx)) => {
                                ctx.tab_drag_state = Some(TabDragState {
                                    source_index: tab_idx,
                                    start_x: x as f32,
                                    active: false,
                                    drop_index: None,
                                });
                            }
                            Some(TabHitResult::CloseButton(tab_idx)) => {
                                {
                                    let mut event = glass_scripting::HookEventData::new();
                                    event.set("tab_index", tab_idx as i64);
                                    fire_hook_on_bridge(
                                        &mut self.script_bridge,
                                        &self.orchestrator.project_root,
                                        glass_scripting::HookPoint::TabClose,
                                        &event,
                                    );
                                }
                                if let Some(session) = ctx.session_mux.close_tab(tab_idx) {
                                    cleanup_session(session);
                                }
                                ctx.tab_bar_hovered_tab = None;
                                if ctx.session_mux.tab_count() == 0 {
                                    fire_hook_on_bridge(
                                        &mut self.script_bridge,
                                        &self.orchestrator.project_root,
                                        glass_scripting::HookPoint::SessionEnd,
                                        &glass_scripting::HookEventData::new(),
                                    );
                                    self.windows.remove(&window_id);
                                    event_loop.exit();
                                    return;
                                }
                                ctx.window.request_redraw();
                            }
                            Some(TabHitResult::NewTabButton) => {
                                let cwd = ctx
                                    .session()
                                    .map(|s| s.status.cwd().to_string())
                                    .unwrap_or_default();
                                let session_id = ctx.session_mux.next_session_id();
                                let (cell_w, cell_h_inner) = ctx.frame_renderer.cell_size();
                                let size = ctx.window.inner_size();
                                let session = match create_session(
                                    &self.proxy,
                                    window_id,
                                    session_id,
                                    &self.config,
                                    Some(std::path::Path::new(&cwd)),
                                    cell_w,
                                    cell_h_inner,
                                    size.width,
                                    size.height,
                                    1,
                                ) {
                                    Ok(s) => s,
                                    Err(e) => {
                                        tracing::error!("PTY spawn failed for new tab button: {e}");
                                        return;
                                    }
                                };
                                ctx.session_mux.add_tab(session, false);
                                {
                                    let tab_idx = ctx.session_mux.tab_count().saturating_sub(1);
                                    let mut event = glass_scripting::HookEventData::new();
                                    event.set("tab_index", tab_idx as i64);
                                    fire_hook_on_bridge(
                                        &mut self.script_bridge,
                                        &self.orchestrator.project_root,
                                        glass_scripting::HookPoint::TabCreate,
                                        &event,
                                    );
                                }
                                ctx.window.request_redraw();
                            }
                            None => {}
                        }
                        return; // Don't fall through to pipeline hit test
                    }
                }

                // Scrollbar click handling (before text selection)
                if let Some((x, y)) = ctx.session().and_then(|s| s.cursor_position) {
                    let (_, cell_h) = ctx.frame_renderer.cell_size();
                    let sc = ctx.renderer.surface_config();
                    let grid_y_offset = if ctx.session_mux.tab_count() > 0 {
                        cell_h
                    } else {
                        0.0
                    };
                    let status_bar_h = cell_h;

                    let scrollbar_hit_result = if ctx.session_mux.active_tab_pane_count() > 1 {
                        // Multi-pane: check each pane's scrollbar
                        let container = ViewportLayout {
                            x: 0,
                            y: cell_h as u32,
                            width: sc.width,
                            height: sc.height.saturating_sub((cell_h as u32) * 2),
                        };
                        let pane_layouts: Vec<(SessionId, ViewportLayout)> = ctx
                            .session_mux
                            .active_tab_root()
                            .map(|root| root.compute_layout(&container))
                            .unwrap_or_default();

                        let mut found = None;
                        for (sid, vp) in &pane_layouts {
                            let scrollbar_x = (vp.x + vp.width) as f32 - SCROLLBAR_WIDTH;
                            let vp_y = vp.y as f32;
                            let vp_h = vp.height as f32;
                            if let Some(session) = ctx.session_mux.session(*sid) {
                                let term = session.term.lock();
                                let display_offset = term.grid().display_offset();
                                let history_size = term.grid().history_size();
                                let screen_lines = term.screen_lines();
                                drop(term);
                                if let Some(hit) = ctx.frame_renderer.scrollbar().hit_test(
                                    x as f32,
                                    y as f32,
                                    scrollbar_x,
                                    vp_y,
                                    vp_h,
                                    display_offset,
                                    history_size,
                                    screen_lines,
                                ) {
                                    found = Some((
                                        *sid,
                                        hit,
                                        vp_y,
                                        vp_h,
                                        display_offset,
                                        history_size,
                                        screen_lines,
                                    ));
                                    break;
                                }
                            }
                        }
                        found
                    } else {
                        // Single-pane
                        let scrollbar_x = sc.width as f32 - SCROLLBAR_WIDTH;
                        let pane_height = sc.height as f32 - grid_y_offset - status_bar_h;
                        if let Some(sid) = ctx.session_mux.focused_session_id() {
                            if let Some(session) = ctx.session_mux.session(sid) {
                                let term = session.term.lock();
                                let display_offset = term.grid().display_offset();
                                let history_size = term.grid().history_size();
                                let screen_lines = term.screen_lines();
                                drop(term);
                                ctx.frame_renderer
                                    .scrollbar()
                                    .hit_test(
                                        x as f32,
                                        y as f32,
                                        scrollbar_x,
                                        grid_y_offset,
                                        pane_height,
                                        display_offset,
                                        history_size,
                                        screen_lines,
                                    )
                                    .map(|hit| {
                                        (
                                            sid,
                                            hit,
                                            grid_y_offset,
                                            pane_height,
                                            display_offset,
                                            history_size,
                                            screen_lines,
                                        )
                                    })
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    };

                    if let Some((
                        sid,
                        hit,
                        track_y,
                        track_height,
                        display_offset,
                        history_size,
                        screen_lines,
                    )) = scrollbar_hit_result
                    {
                        ctx.mouse_left_pressed = false; // Prevent text selection

                        match hit {
                            ScrollbarHit::Thumb => {
                                // Start drag: compute thumb geometry for grab offset
                                let (thumb_y_offset, thumb_height) =
                                    ctx.frame_renderer.scrollbar().compute_thumb_geometry(
                                        track_height,
                                        history_size,
                                        screen_lines,
                                        display_offset,
                                    );
                                let thumb_top = track_y + thumb_y_offset;
                                ctx.scrollbar_dragging = Some(ScrollbarDragInfo {
                                    pane_id: sid,
                                    thumb_grab_offset: y as f32 - thumb_top,
                                    track_y,
                                    track_height,
                                    thumb_height,
                                });
                            }
                            ScrollbarHit::TrackAbove => {
                                if let Some(session) = ctx.session_mux.session(sid) {
                                    session.term.lock().scroll_display(Scroll::PageUp);
                                }
                            }
                            ScrollbarHit::TrackBelow => {
                                if let Some(session) = ctx.session_mux.session(sid) {
                                    session.term.lock().scroll_display(Scroll::PageDown);
                                }
                            }
                        }

                        // If clicked pane is not focused (multi-pane), focus it
                        if ctx.session_mux.focused_session_id() != Some(sid) {
                            ctx.session_mux.set_focused_pane(sid);
                        }

                        ctx.window.request_redraw();
                        return;
                    }
                }

                // Multi-pane click focus: if click is in a different pane, change focus
                if ctx.session_mux.active_tab_pane_count() > 1 {
                    if let Some((click_x, click_y)) = ctx.session().and_then(|s| s.cursor_position)
                    {
                        let (_, cell_h) = ctx.frame_renderer.cell_size();
                        let sc = ctx.renderer.surface_config();
                        let container = ViewportLayout {
                            x: 0,
                            y: cell_h as u32,
                            width: sc.width,
                            height: sc.height.saturating_sub((cell_h as u32) * 2),
                        };
                        if let Some(root) = ctx.session_mux.active_tab_root() {
                            let pane_layouts = root.compute_layout(&container);
                            let focused_id = ctx.session_mux.focused_session_id();
                            // Find which pane contains the click
                            let clicked_pane = pane_layouts.iter().find(|(_, vp)| {
                                let cx = click_x as u32;
                                let cy = click_y as u32;
                                cx >= vp.x
                                    && cx < vp.x + vp.width
                                    && cy >= vp.y
                                    && cy < vp.y + vp.height
                            });
                            if let Some((target_id, _)) = clicked_pane {
                                if focused_id != Some(*target_id) {
                                    ctx.session_mux.set_focused_pane(*target_id);
                                    ctx.window.request_redraw();
                                    // Don't return -- still allow pipeline hit test below
                                }
                            }
                        }
                    }
                }

                // Start text selection at the clicked grid position
                if let Some((x, y)) = ctx.session().and_then(|s| s.cursor_position) {
                    let (cell_w, cell_h) = ctx.frame_renderer.cell_size();
                    let grid_y_offset = if ctx.session_mux.tab_count() > 0 {
                        cell_h
                    } else {
                        0.0
                    };
                    let px = x as f32;
                    let py = y as f32 - grid_y_offset;
                    if py >= 0.0 {
                        let col = (px / cell_w) as usize;
                        let row = (py / cell_h) as usize;
                        let side = if (px % cell_w) < cell_w / 2.0 {
                            Side::Left
                        } else {
                            Side::Right
                        };
                        if let Some(session) = ctx.session() {
                            let mut term = session.term.lock();
                            let display_offset = term.grid().display_offset();
                            let columns = term.columns();
                            let screen_lines = term.screen_lines();
                            let col = col.min(columns.saturating_sub(1));
                            let row = row.min(screen_lines.saturating_sub(1));
                            let point = alacritty_terminal::index::Point::new(
                                Line(row as i32 - display_offset as i32),
                                Column(col),
                            );
                            term.selection =
                                Some(Selection::new(SelectionType::Simple, point, side));
                            drop(term);
                            ctx.window.request_redraw();
                        }
                    }
                }

                let needs_redraw = if let Some((_, y)) =
                    ctx.session().and_then(|s| s.cursor_position)
                {
                    let (cell_w, cell_h) = ctx.frame_renderer.cell_size();
                    let size = ctx.window.inner_size();
                    let viewport_h = size.height as f32;
                    let status_bar_h = cell_h; // status bar is always 1 cell tall

                    // Hit test pipeline stage panel (bottom of viewport)
                    if let Some(session) = ctx.session_mux.focused_session_mut() {
                        if let Some((block_idx, hit)) = session.block_manager.pipeline_hit_test(
                            0.0,
                            y as f32,
                            cell_w,
                            cell_h,
                            viewport_h,
                            status_bar_h,
                        ) {
                            match hit {
                                PipelineHit::StageRow(stage_idx) => {
                                    if let Some(block) = session.block_manager.block_mut(block_idx)
                                    {
                                        if block.expanded_stage_index == Some(stage_idx) {
                                            block.set_expanded_stage(None);
                                        } else {
                                            block.set_expanded_stage(Some(stage_idx));
                                        }
                                    }
                                }
                                PipelineHit::Header => {
                                    if let Some(block) = session.block_manager.block_mut(block_idx)
                                    {
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
                    }
                } else {
                    false
                };
                if needs_redraw {
                    ctx.window.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                // Handle tab drag release: complete reorder or activate tab
                if let Some(drag) = ctx.tab_drag_state.take() {
                    if drag.active {
                        if let Some(drop_idx) = drag.drop_index {
                            // Convert drop slot to final position index.
                            // drop_idx is an insertion slot (0..=tab_count).
                            // After remove(source), indices shift, so we need:
                            let to = if drop_idx > drag.source_index {
                                drop_idx - 1 // Account for removal shifting indices down
                            } else {
                                drop_idx
                            };
                            if to != drag.source_index {
                                ctx.session_mux.reorder_tab(drag.source_index, to);
                            }
                        }
                    } else {
                        // Was a click, not a drag -- activate the tab
                        ctx.session_mux.activate_tab(drag.source_index);
                    }
                    ctx.window.request_redraw();
                    return;
                }
                // If scrollbar was being dragged, just release it (no clipboard copy)
                if ctx.scrollbar_dragging.is_some() {
                    ctx.scrollbar_dragging = None;
                    ctx.window.request_redraw();
                    return;
                }
                ctx.mouse_left_pressed = false;
                // Copy selection to clipboard on mouse release
                if let Some(session) = ctx.session() {
                    clipboard_copy(&session.term);
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: winit::event::MouseButton::Middle,
                ..
            } => {
                if let Some((x, y)) = ctx.session().and_then(|s| s.cursor_position) {
                    let (_, cell_h) = ctx.frame_renderer.cell_size();
                    if (y as f32) < cell_h {
                        let viewport_w = ctx.window.inner_size().width as f32;
                        match ctx.frame_renderer.tab_bar().hit_test(
                            x as f32,
                            ctx.session_mux.tab_count(),
                            viewport_w,
                        ) {
                            Some(TabHitResult::Tab(tab_idx))
                            | Some(TabHitResult::CloseButton(tab_idx)) => {
                                {
                                    let mut event = glass_scripting::HookEventData::new();
                                    event.set("tab_index", tab_idx as i64);
                                    fire_hook_on_bridge(
                                        &mut self.script_bridge,
                                        &self.orchestrator.project_root,
                                        glass_scripting::HookPoint::TabClose,
                                        &event,
                                    );
                                }
                                if let Some(session) = ctx.session_mux.close_tab(tab_idx) {
                                    cleanup_session(session);
                                }
                                ctx.tab_bar_hovered_tab = None;
                                if ctx.session_mux.tab_count() == 0 {
                                    fire_hook_on_bridge(
                                        &mut self.script_bridge,
                                        &self.orchestrator.project_root,
                                        glass_scripting::HookPoint::SessionEnd,
                                        &glass_scripting::HookEventData::new(),
                                    );
                                    self.windows.remove(&window_id);
                                    event_loop.exit();
                                    return;
                                }
                                ctx.window.request_redraw();
                            }
                            Some(TabHitResult::NewTabButton) | None => {}
                        }
                    }
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
                    // Overlay scroll: redirect wheel to overlay scroll offset
                    if self.activity_overlay_visible {
                        if self.activity_view_filter
                            == glass_renderer::ActivityViewFilter::Orchestrator
                        {
                            if lines > 0 {
                                self.orchestrator_scroll_offset = self
                                    .orchestrator_scroll_offset
                                    .saturating_add(lines as usize);
                            } else {
                                self.orchestrator_scroll_offset = self
                                    .orchestrator_scroll_offset
                                    .saturating_sub((-lines) as usize);
                            }
                        } else {
                            // Other overlay tabs use the shared activity scroll offset
                            if lines > 0 {
                                self.activity_scroll_offset =
                                    self.activity_scroll_offset.saturating_sub(lines as usize);
                            } else {
                                self.activity_scroll_offset += (-lines) as usize;
                            }
                        }
                        ctx.window.request_redraw();
                    } else if self.settings_overlay_visible {
                        if lines > 0 {
                            self.settings_shortcuts_scroll = self
                                .settings_shortcuts_scroll
                                .saturating_sub(lines as usize);
                        } else {
                            self.settings_shortcuts_scroll += (-lines) as usize;
                        }
                        ctx.window.request_redraw();
                    } else {
                        // Normal terminal scroll
                        // Positive delta = scroll up (into history), negative = scroll down
                        if let Some(session) = ctx.session() {
                            session.term.lock().scroll_display(Scroll::Delta(lines));
                        }
                        ctx.window.request_redraw();
                    }
                }
            }
            WindowEvent::DroppedFile(path) => {
                // Send the file path as input to the active PTY.
                // Quote paths containing spaces so they work in shell commands.
                let path_str = path.to_string_lossy();
                let text = if path_str.contains(' ') {
                    format!("\"{}\"", path_str)
                } else {
                    path_str.into_owned()
                };
                if let Some(session) = ctx.session() {
                    pty_send(
                        &session.pty_sender,
                        PtyMsg::Input(Cow::Owned(text.into_bytes())),
                    );
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
            AppEvent::SetTitle {
                window_id,
                session_id,
                title,
            } => {
                if let Some(ctx) = self.windows.get_mut(&window_id) {
                    // Update window title only if this is the active tab
                    if ctx.session_mux.focused_session_id() == Some(session_id) {
                        ctx.window.set_title(&title);
                    }
                    // Update tab title in the mux
                    if let Some(tab) = ctx
                        .session_mux
                        .tabs_mut()
                        .iter_mut()
                        .find(|t| t.session_ids().contains(&session_id))
                    {
                        tab.title = title.clone();
                    }
                }
            }
            AppEvent::TerminalExit {
                window_id,
                session_id,
                exit_code,
            } => {
                // Show exit message for non-zero codes
                if let Some(code) = exit_code {
                    if code != 0 {
                        tracing::info!("Shell exited with code {code} (session {session_id})");
                    }
                }
                if let Some(ctx) = self.windows.get_mut(&window_id) {
                    // Find the tab containing this session
                    let tab_idx = ctx
                        .session_mux
                        .tabs()
                        .iter()
                        .position(|t| t.session_ids().contains(&session_id));
                    if let Some(idx) = tab_idx {
                        let pane_count = ctx.session_mux.tabs()[idx].pane_count();
                        if pane_count > 1 {
                            // Multi-pane tab: close only the exited pane
                            let tab_count_before = ctx.session_mux.tab_count();
                            if let Some(session) = ctx.session_mux.close_pane(session_id) {
                                cleanup_session(session);
                            }
                            // Guard: if close_pane collapsed the tab (shouldn't with >1 pane)
                            if ctx.session_mux.tab_count() < tab_count_before
                                && ctx.session_mux.tab_count() == 0
                            {
                                fire_hook_on_bridge(
                                    &mut self.script_bridge,
                                    &self.orchestrator.project_root,
                                    glass_scripting::HookPoint::SessionEnd,
                                    &glass_scripting::HookEventData::new(),
                                );
                                self.windows.remove(&window_id);
                                event_loop.exit();
                                return;
                            }
                            // Resize remaining panes' PTYs
                            let size = ctx.window.inner_size();
                            resize_all_panes(
                                &mut ctx.session_mux,
                                &ctx.frame_renderer,
                                size.width,
                                size.height,
                            );
                        } else {
                            // Single pane: close the entire tab
                            {
                                let mut event = glass_scripting::HookEventData::new();
                                event.set("tab_index", idx as i64);
                                fire_hook_on_bridge(
                                    &mut self.script_bridge,
                                    &self.orchestrator.project_root,
                                    glass_scripting::HookPoint::TabClose,
                                    &event,
                                );
                            }
                            if let Some(session) = ctx.session_mux.close_tab(idx) {
                                cleanup_session(session);
                            }
                            ctx.tab_bar_hovered_tab = None;
                        }
                    }
                    if ctx.session_mux.tab_count() == 0 {
                        tracing::info!("Last tab closed -- exiting");
                        fire_hook_on_bridge(
                            &mut self.script_bridge,
                            &self.orchestrator.project_root,
                            glass_scripting::HookPoint::SessionEnd,
                            &glass_scripting::HookEventData::new(),
                        );
                        self.windows.remove(&window_id);
                        event_loop.exit();
                    } else {
                        ctx.window.request_redraw();
                    }
                }
            }
            AppEvent::Shell {
                window_id,
                session_id,
                event: shell_event,
                line,
            } => {
                // Holds data for scripting hooks (fired outside windows borrow).
                let mut hook_command_start_text: Option<String> = None;
                let mut hook_command_complete_data: Option<(String, Option<i32>, i64)> = None;

                if let Some(ctx) = self.windows.get_mut(&window_id) {
                    // Route to session by session_id
                    if ctx.session_mux.session(session_id).is_none() {
                        return;
                    }

                    // Skip pipeline events entirely when pipes are disabled
                    let pipes_enabled = self
                        .config
                        .pipes
                        .as_ref()
                        .map(|p| p.enabled)
                        .unwrap_or(true);
                    if !pipes_enabled
                        && matches!(
                            shell_event,
                            ShellEvent::PipelineStart { .. } | ShellEvent::PipelineStage { .. }
                        )
                    {
                        return;
                    }

                    // Holds command event data for emit_command_event (extracted inside borrow).
                    let mut command_event_data: Option<(String, String)> = None;

                    if let Some(session) = ctx.session_mux.session_mut(session_id) {
                        // Convert ShellEvent to OscEvent for BlockManager
                        let osc_event = shell_event_to_osc(&shell_event);
                        session.block_manager.handle_event(&osc_event, line);

                        // Fix #3: Orchestrator crash recovery with grace period.
                        // Only trigger if orchestrating, had iterations, AND not within
                        // the grace period after the orchestrator itself typed something.
                        if matches!(shell_event, ShellEvent::PromptStart)
                            && self.orchestrator.active
                            && self.orchestrator.iteration > 0
                            && !self.orchestrator.in_grace_period()
                        {
                            tracing::info!(
                                "Orchestrator: shell prompt detected — Claude Code may have exited, restarting"
                            );
                            let cp_rel = self
                                .config
                                .agent
                                .as_ref()
                                .and_then(|a| a.orchestrator.as_ref())
                                .map(|o| o.checkpoint_path.as_str())
                                .unwrap_or(".glass/checkpoint.md");
                            let restart_msg = format!(
                                "claude --dangerously-skip-permissions -p \"Read {} and continue the project from where you left off. Follow the iteration protocol: plan, implement, commit, verify, decide.\"\n",
                                cp_rel,
                            );
                            let bytes = restart_msg.into_bytes();
                            pty_send(
                                &session.pty_sender,
                                PtyMsg::Input(std::borrow::Cow::Owned(bytes)),
                            );
                            self.orchestrator.mark_pty_write();
                        }

                        // Override auto-expand if config disables it (after handle_event sets pipeline_expanded)
                        if matches!(shell_event, ShellEvent::CommandFinished { .. }) {
                            let auto_expand = self
                                .config
                                .pipes
                                .as_ref()
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
                            if let ShellEvent::PipelineStage {
                                index,
                                total_bytes: _,
                                ref temp_path,
                            } = shell_event
                            {
                                match std::fs::read(temp_path) {
                                    Ok(raw_bytes) => {
                                        let max_bytes = self
                                            .config
                                            .pipes
                                            .as_ref()
                                            .map(|p| (p.max_capture_mb as usize) * 1024 * 1024)
                                            .unwrap_or(10 * 1024 * 1024);
                                        let policy =
                                            glass_pipes::BufferPolicy::new(max_bytes, 512 * 1024);
                                        let mut stage_buf = glass_pipes::StageBuffer::new(policy);
                                        stage_buf.append(&raw_bytes);
                                        let finalized = stage_buf.finalize();

                                        if let Some(block) =
                                            session.block_manager.current_block_mut()
                                        {
                                            if let Some(stage) = block
                                                .pipeline_stages
                                                .iter_mut()
                                                .find(|s| s.index == index)
                                            {
                                                stage.data = finalized;
                                                stage.temp_path = None;
                                            }
                                        }

                                        let _ = std::fs::remove_file(temp_path);
                                    }
                                    Err(e) => {
                                        tracing::warn!(
                                            "Failed to read pipeline stage {} from {}: {}",
                                            index,
                                            temp_path,
                                            e
                                        );
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
                                    let end = block
                                        .output_start_line
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
                            let snapshot_enabled = self
                                .config
                                .snapshot
                                .as_ref()
                                .map(|s| s.enabled)
                                .unwrap_or(true);
                            // Parse command to determine confidence before deciding whether
                            // to start the watcher. ReadOnly commands (cd, ls, etc.) don't
                            // need a watcher — it can produce spurious snapshot entries.
                            let cwd_path_snap = std::path::Path::new(session.status.cwd());
                            let parse_confidence = if snapshot_enabled {
                                let parse_result = glass_snapshot::command_parser::parse_command(
                                    &command_text,
                                    cwd_path_snap,
                                );
                                let confidence = parse_result.confidence;

                                if confidence != glass_snapshot::Confidence::ReadOnly
                                    && !parse_result.targets.is_empty()
                                {
                                    if let Some(ref store) = session.snapshot_store {
                                        match store.create_snapshot(0, session.status.cwd()) {
                                            Ok(sid) => {
                                                for target in &parse_result.targets {
                                                    if let Err(e) =
                                                        store.store_file(sid, target, "parser")
                                                    {
                                                        tracing::warn!(
                                                            "Pre-exec snapshot failed for {}: {}",
                                                            target.display(),
                                                            e
                                                        );
                                                    }
                                                }
                                                tracing::info!(
                                                    "Pre-exec snapshot {} with {} targets (confidence: {:?})",
                                                    sid, parse_result.targets.len(), confidence,
                                                );
                                                session.pending_snapshot_id = Some(sid);
                                                session.pending_parse_confidence = Some(confidence);
                                                // Mark current block as having a snapshot for [undo] label
                                                if let Some(block) =
                                                    session.block_manager.current_block_mut()
                                                {
                                                    block.has_snapshot = true;
                                                }
                                            }
                                            Err(e) => tracing::warn!(
                                                "Pre-exec snapshot creation failed: {}",
                                                e
                                            ),
                                        }
                                    }
                                }
                                confidence
                            } else {
                                tracing::debug!(
                                    "Pre-exec snapshot skipped: snapshots disabled in config"
                                );
                                glass_snapshot::Confidence::Low
                            };

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
                                    let stage_commands: Vec<String> =
                                        pipeline.stages.iter().map(|s| s.command.clone()).collect();
                                    if let Some(block) = session.block_manager.block_mut(idx) {
                                        block.pipeline_stage_commands = stage_commands;
                                    }
                                }
                            }

                            // Capture command text for command.started event
                            command_event_data = Some((
                                "started".to_string(),
                                format!("command started: {}", command_text),
                            ));

                            // Capture for scripting CommandStart hook (fired outside borrow).
                            hook_command_start_text = Some(command_text.clone());

                            session.pending_command_text = Some(command_text);

                            // Start filesystem watcher for this command's CWD.
                            // Skip for ReadOnly commands (cd, ls, etc.) — they never modify
                            // files and the watcher can produce spurious entries (e.g. the
                            // command name itself appearing as a file path).
                            if parse_confidence != glass_snapshot::Confidence::ReadOnly {
                                let cwd_path = std::path::Path::new(session.status.cwd());
                                let ignore = glass_snapshot::IgnoreRules::load(cwd_path);
                                session.active_watcher =
                                    match glass_snapshot::FsWatcher::new(cwd_path, ignore) {
                                        Ok(w) => {
                                            tracing::debug!(
                                                "FS watcher started for {}",
                                                cwd_path.display()
                                            );
                                            Some(w)
                                        }
                                        Err(e) => {
                                            tracing::warn!("Failed to start FS watcher: {}", e);
                                            None
                                        }
                                    };
                            }
                        }

                        // Insert CommandRecord on CommandFinished
                        if let ShellEvent::CommandFinished { exit_code } = &shell_event {
                            if let Some(ref db) = session.history_db {
                                let now = std::time::SystemTime::now();
                                let finished_epoch = now
                                    .duration_since(std::time::SystemTime::UNIX_EPOCH)
                                    .map(|d| d.as_secs() as i64)
                                    .unwrap_or(0);
                                let started_epoch = session
                                    .command_started_wall
                                    .and_then(|t| {
                                        t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok()
                                    })
                                    .map(|d| d.as_secs() as i64)
                                    .unwrap_or(finished_epoch);
                                let duration_ms = session
                                    .command_started_wall
                                    .and_then(|t| now.duration_since(t).ok())
                                    .map(|d| d.as_millis() as i64)
                                    .unwrap_or(0);

                                // Use command text extracted earlier at CommandExecuted time.
                                let command_text =
                                    session.pending_command_text.take().unwrap_or_default();

                                // Capture command.finished event data
                                let duration_secs = duration_ms as f64 / 1000.0;
                                let exit_str = exit_code.map_or("?".to_string(), |c| c.to_string());
                                command_event_data = Some((
                                    "finished".to_string(),
                                    format!(
                                        "command finished {} (exit: {}, {:.1}s)",
                                        &command_text, exit_str, duration_secs
                                    ),
                                ));

                                // Capture for scripting CommandComplete hook (fired outside borrow).
                                hook_command_complete_data =
                                    Some((command_text.clone(), *exit_code, duration_ms));

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
                                                    tracing::warn!(
                                                        "Failed to insert pipe stages: {}",
                                                        e
                                                    );
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
                            if let (Some(sid), Some(ref store)) =
                                (session.pending_snapshot_id.take(), &session.snapshot_store)
                            {
                                let command_id = session.last_command_id.unwrap_or(0);
                                if let Err(e) = store.update_command_id(sid, command_id) {
                                    tracing::warn!(
                                        "Failed to update snapshot {} command_id: {}",
                                        sid,
                                        e
                                    );
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
                                                    if let Err(e) = store.store_file(
                                                        snapshot_id,
                                                        &event.path,
                                                        "watcher",
                                                    ) {
                                                        tracing::warn!(
                                                            "Failed to store watcher file {}: {}",
                                                            event.path.display(),
                                                            e
                                                        );
                                                    }
                                                    // For Rename events, also store the destination path
                                                    if let glass_snapshot::WatcherEventKind::Rename { ref to } = event.kind {
                                                        if let Err(e) = store.store_file(snapshot_id, to, "watcher") {
                                                            tracing::warn!("Failed to store watcher rename target {}: {}", to.display(), e);
                                                        }
                                                    }
                                                }
                                                tracing::debug!(
                                                    "Stored {} watcher files in snapshot {}",
                                                    events.len(),
                                                    snapshot_id
                                                );
                                            }
                                            Err(e) => {
                                                tracing::warn!(
                                                    "Failed to create watcher snapshot: {}",
                                                    e
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        // NOTE: SOI parse is now triggered from CommandOutput handler
                        // (after output is stored in DB) to avoid a race condition where
                        // the SOI worker queries the DB before output is written.

                        // On CurrentDirectory events, update status and query git info
                        // Track whether we need to spawn a git query (can't spawn inside session borrow)
                        let spawn_git_query =
                            if let ShellEvent::CurrentDirectory(ref cwd) = shell_event {
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

                    // Emit command context event (outside session borrow)
                    if let Some((event_type, summary)) = command_event_data {
                        emit_command_event(&self.agent_runtime, &event_type, &summary);
                    }

                    // Update tab title from CWD change
                    if let ShellEvent::CurrentDirectory(ref path) = shell_event {
                        let title = std::path::Path::new(path)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| path.clone());
                        if let Some(tab) = ctx
                            .session_mux
                            .tabs_mut()
                            .iter_mut()
                            .find(|t| t.session_ids().contains(&session_id))
                        {
                            tab.title = title;
                        }
                    }

                    // Spawn git query outside session borrow (needs self.proxy and window_id)
                    if let ShellEvent::CurrentDirectory(ref cwd) = shell_event {
                        // Re-check: only spawn if we set git_query_pending above
                        let git_pending = ctx
                            .session_mux
                            .session(session_id)
                            .map(|s| s.status.git_query_pending)
                            .unwrap_or(false);
                        if git_pending {
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
                // Fire scripting hooks (outside windows borrow)
                if let Some(cmd_text) = hook_command_start_text {
                    let mut event = glass_scripting::HookEventData::new();
                    event.set("command", cmd_text);
                    fire_hook_on_bridge(
                        &mut self.script_bridge,
                        &self.orchestrator.project_root,
                        glass_scripting::HookPoint::CommandStart,
                        &event,
                    );
                }
                if let Some((cmd_text, exit_code, duration_ms)) = hook_command_complete_data {
                    let mut event = glass_scripting::HookEventData::new();
                    event.set("command", cmd_text);
                    event.set("exit_code", exit_code.unwrap_or(-1) as i64);
                    event.set("duration_ms", duration_ms);
                    fire_hook_on_bridge(
                        &mut self.script_bridge,
                        &self.orchestrator.project_root,
                        glass_scripting::HookPoint::CommandComplete,
                        &event,
                    );
                }
            }
            AppEvent::CommandOutput {
                window_id,
                session_id,
                raw_output,
            } => {
                let mut soi_spawn_data: Option<(std::path::PathBuf, i64)> = None;
                if let Some(ctx) = self.windows.get_mut(&window_id) {
                    // Process raw bytes: binary detection, ANSI stripping, truncation
                    let max_kb = self
                        .config
                        .history
                        .as_ref()
                        .map(|h| h.max_output_capture_kb)
                        .unwrap_or(50);
                    let processed = glass_history::output::process_output(Some(raw_output), max_kb);
                    if let Some(ref output) = processed {
                        if let Some(session) = ctx.session_mux.session_mut(session_id) {
                            // Update the last command record with captured output
                            if let (Some(ref db), Some(cmd_id)) =
                                (&session.history_db, session.last_command_id)
                            {
                                match db.update_output(cmd_id, output) {
                                    Ok(()) => {
                                        tracing::debug!(
                                            "Updated command {} with {} bytes of output",
                                            cmd_id,
                                            output.len(),
                                        );
                                        // Output is now in the DB — safe to spawn SOI parse
                                        soi_spawn_data = Some((db.path().to_path_buf(), cmd_id));
                                    }
                                    Err(e) => {
                                        tracing::warn!("Failed to update command output: {}", e);
                                    }
                                }
                            }
                        }
                    }
                }

                // Spawn SOI parse worker AFTER output is stored in DB (avoids race condition)
                if let Some((db_path, cmd_id)) = soi_spawn_data {
                    let proxy = self.proxy.clone();
                    let wid = window_id;
                    let sid = session_id;
                    std::thread::Builder::new()
                        .name("Glass SOI parse".into())
                        .spawn(move || {
                            let db = match glass_history::db::HistoryDb::open(&db_path) {
                                Ok(db) => db,
                                Err(e) => {
                                    tracing::warn!("SOI worker: failed to open DB: {}", e);
                                    return;
                                }
                            };

                            let output_text = match db.get_output_for_command(cmd_id) {
                                Ok(text) => text,
                                Err(e) => {
                                    tracing::warn!(
                                        "SOI worker: failed to fetch output for cmd {}: {}",
                                        cmd_id,
                                        e
                                    );
                                    None
                                }
                            };

                            let command_text = db
                                .get_command_text(cmd_id)
                                .ok()
                                .flatten()
                                .unwrap_or_default();

                            let (summary, severity, raw_line_count) = match output_text {
                                None => {
                                    ("no output captured".to_string(), "Info".to_string(), 0i64)
                                }
                                Some(ref text) if text.is_empty() => {
                                    ("no output captured".to_string(), "Info".to_string(), 0i64)
                                }
                                Some(text) => {
                                    let output_type =
                                        glass_soi::classify(&text, Some(&command_text));
                                    let parsed =
                                        glass_soi::parse(&text, output_type, Some(&command_text));
                                    if let Err(e) = db.insert_parsed_output(cmd_id, &parsed) {
                                        tracing::warn!(
                                            "SOI: insert_parsed_output failed cmd={}: {}",
                                            cmd_id,
                                            e
                                        );
                                    }
                                    let sev_str = match parsed.summary.severity {
                                        glass_soi::Severity::Error => "Error",
                                        glass_soi::Severity::Warning => "Warning",
                                        glass_soi::Severity::Info => "Info",
                                        glass_soi::Severity::Success => "Success",
                                    };
                                    let rlc = parsed.raw_line_count as i64;
                                    (parsed.summary.one_line, sev_str.to_string(), rlc)
                                }
                            };

                            let _ = proxy.send_event(AppEvent::SoiReady {
                                window_id: wid,
                                session_id: sid,
                                command_id: cmd_id,
                                summary,
                                severity,
                                raw_line_count,
                            });
                        })
                        .ok();
                }
            }
            AppEvent::ConfigReloaded { config, error } => {
                if let Some(err) = error {
                    tracing::warn!("Config reload error: {}", err);
                    self.config_error = Some(err);
                    // Request redraw on all windows to show error overlay
                    for ctx in self.windows.values() {
                        ctx.window.request_redraw();
                    }
                } else {
                    // Clear any previous error
                    self.config_error = None;

                    let new_config = *config;
                    let font_changed = self.config.font_changed(&new_config);

                    if font_changed {
                        for ctx in self.windows.values_mut() {
                            let scale = ctx.window.scale_factor() as f32;
                            ctx.frame_renderer.update_font(
                                &new_config.font_family,
                                new_config.font_size,
                                scale,
                            );
                            // Recalculate terminal grid size for all sessions
                            let size = ctx.window.inner_size();
                            resize_all_panes(
                                &mut ctx.session_mux,
                                &ctx.frame_renderer,
                                size.width,
                                size.height,
                            );
                            ctx.window.request_redraw();
                        }
                    }

                    // AGTC-01: Detect agent section changes before swapping config.
                    let old_agent = self.config.agent.clone();
                    // Swap config (applies non-visual changes like history thresholds)
                    self.config = new_config;
                    tracing::info!(
                        "Config reloaded successfully (font_changed={})",
                        font_changed
                    );

                    // Update scripting bridge enabled state from new config.
                    self.script_bridge.update_config(&self.config);

                    // AGTC-01: Restart agent runtime when [agent] section changes.
                    let agent_config_changed = old_agent != self.config.agent;
                    if agent_config_changed {
                        // Skip agent restart when we wrote to config ourselves.
                        // The orchestrator enable handler and feedback loop both write
                        // to config.toml, triggering ConfigReloaded events. These must
                        // NOT kill/respawn the agent we just set up.
                        // feedback_write_pending covers single writes; config_write_suppress
                        // covers bursts (3 writes during orchestrator enable).
                        if self.feedback_write_pending
                            || self
                                .config_write_suppress_until
                                .map(|t| std::time::Instant::now() < t)
                                .unwrap_or(false)
                        {
                            self.feedback_write_pending = false;
                            tracing::debug!(
                                "Skipping agent restart — config change was self-initiated"
                            );
                            for ctx in self.windows.values() {
                                ctx.window.request_redraw();
                            }
                            return;
                        }

                        // Flush any pending collapsed event before dropping the runtime.
                        if let Some(event) = self.activity_filter.flush_collapsed() {
                            if let Some(tx) = &self.activity_stream_tx {
                                let _ = tx.try_send(event);
                            }
                        }
                        // Clear response_pending to prevent hang if a verify thread is in-flight
                        self.orchestrator.response_pending = false;
                        // Drop old runtime (triggers Drop -> kill child + deregister).
                        self.agent_runtime = None;

                        // Build new runtime config from updated agent section.
                        let new_agent_config = self
                            .config
                            .agent
                            .clone()
                            .map(|a| glass_core::agent_runtime::AgentRuntimeConfig {
                                mode: a.mode,
                                max_budget_usd: a.max_budget_usd,
                                cooldown_secs: a.cooldown_secs,
                                allowed_tools: a.allowed_tools,
                                orchestrator: a.orchestrator,
                            })
                            .unwrap_or_default();

                        // Create fresh channel -- old rx was consumed by previous writer thread.
                        let activity_config =
                            glass_core::activity_stream::ActivityStreamConfig::default();
                        let (new_tx, new_rx) =
                            glass_core::activity_stream::create_channel(&activity_config);
                        self.activity_stream_tx = Some(new_tx);
                        self.activity_filter =
                            glass_core::activity_stream::ActivityFilter::new(activity_config);

                        if new_agent_config.mode != glass_core::agent_runtime::AgentMode::Off {
                            self.agent_runtime = try_spawn_agent(
                                new_agent_config.clone(),
                                new_rx,
                                self.proxy.clone(),
                                0,
                                None,
                                self.get_focused_cwd(),
                                None,
                            );
                            // AGTC-04: Show hint if mode != Off but spawn failed.
                            if self.agent_runtime.is_none() {
                                self.config_error = Some(glass_core::config::ConfigError {
                                    message: "'claude' CLI not found on PATH. Install from https://claude.ai/download, or set agent.mode = \"off\" in ~/.glass/config.toml".to_string(),
                                    line: None,
                                    column: None,
                                    snippet: None,
                                });
                            }
                        } else {
                            // Store rx so channel doesn't drop (events silently discarded).
                            self.activity_stream_rx = Some(new_rx);
                        }

                        tracing::info!("Agent config reloaded: mode={:?}", new_agent_config.mode);

                        // (feedback_write_pending guard moved to top of agent_config_changed block)

                        // Sync orchestrator.active with config.agent.orchestrator.enabled
                        // so the settings overlay toggle actually activates orchestration.
                        // Only activate/deactivate when the `enabled` field itself changed,
                        // not when other orchestrator fields (feedback_llm, etc.) changed.
                        let orch_enabled = self
                            .config
                            .agent
                            .as_ref()
                            .and_then(|a| a.orchestrator.as_ref())
                            .map(|o| o.enabled)
                            .unwrap_or(false);
                        let was_enabled = old_agent
                            .as_ref()
                            .and_then(|a| a.orchestrator.as_ref())
                            .map(|o| o.enabled)
                            .unwrap_or(false);

                        if orch_enabled && !was_enabled && self.orchestrator_activated_at.is_some()
                        {
                            // Activating via settings overlay toggle — only when the user
                            // has previously used the orchestrator (activated_at is set by
                            // Ctrl+Shift+O). Prevents auto-activation on first config load
                            // when enabled=true in config.toml — Ctrl+Shift+O is required
                            // to start the orchestration loop with a proper handoff.
                            self.orchestrator.active = true;
                            self.orchestrator_activated_at = Some(std::time::Instant::now());
                            self.orchestrator.reset_stuck();
                            self.orchestrator.iterations_since_checkpoint = 0;
                            self.orchestrator.bounded_stop_pending = false;
                            self.orchestrator.kickoff_complete = false;
                            self.orchestrator.last_user_keypress = None;
                            self.orchestrator.max_iterations = self
                                .config
                                .agent
                                .as_ref()
                                .and_then(|a| a.orchestrator.as_ref())
                                .and_then(|o| o.max_iterations);

                            // Initialize metric guard
                            let verify_mode = self
                                .config
                                .agent
                                .as_ref()
                                .and_then(|a| a.orchestrator.as_ref())
                                .map(|o| o.verify_mode.as_str())
                                .unwrap_or("floor");
                            if verify_mode == "floor" && self.orchestrator.metric_baseline.is_none()
                            {
                                let cwd = self.orchestrator.project_root.clone();
                                let commands = orchestrator::auto_detect_verify_commands(&cwd);
                                if !commands.is_empty() {
                                    let mut baseline = orchestrator::MetricBaseline::new();
                                    baseline.commands = commands;
                                    self.orchestrator.metric_baseline = Some(baseline);
                                }
                            }
                            // Initialize feedback loop
                            self.orchestrator.feedback_waste_iterations = 0;
                            self.orchestrator.feedback_stuck_count = 0;
                            self.orchestrator.feedback_checkpoint_count = 0;
                            self.orchestrator.feedback_verify_sequence.clear();
                            self.orchestrator.feedback_agent_responses.clear();
                            self.orchestrator.feedback_completion_reason.clear();
                            self.orchestrator.feedback_commit_count = 0;
                            self.orchestrator.feedback_reverted_files.clear();
                            self.orchestrator.feedback_fast_trigger_during_output = 0;
                            self.orchestrator.feedback_iteration_timestamps.clear();

                            let cwd_for_feedback = self.orchestrator.project_root.clone();
                            let feedback_config = glass_feedback::FeedbackConfig {
                                project_root: cwd_for_feedback.clone(),
                                feedback_llm: self
                                    .config
                                    .agent
                                    .as_ref()
                                    .and_then(|a| a.orchestrator.as_ref())
                                    .map(|o| o.feedback_llm)
                                    .unwrap_or(false),
                                max_prompt_hints: self
                                    .config
                                    .agent
                                    .as_ref()
                                    .and_then(|a| a.orchestrator.as_ref())
                                    .map(|o| o.max_prompt_hints)
                                    .unwrap_or(10),
                                silence_timeout_secs: self
                                    .config
                                    .agent
                                    .as_ref()
                                    .and_then(|a| a.orchestrator.as_ref())
                                    .map(|o| o.silence_timeout_secs),
                                max_retries_before_stuck: self
                                    .config
                                    .agent
                                    .as_ref()
                                    .and_then(|a| a.orchestrator.as_ref())
                                    .map(|o| o.max_retries_before_stuck),
                                ablation_enabled: self
                                    .config
                                    .agent
                                    .as_ref()
                                    .and_then(|a| a.orchestrator.as_ref())
                                    .map(|o| o.ablation_enabled)
                                    .unwrap_or(true),
                                ablation_sweep_interval: self
                                    .config
                                    .agent
                                    .as_ref()
                                    .and_then(|a| a.orchestrator.as_ref())
                                    .map(|o| o.ablation_sweep_interval)
                                    .unwrap_or(20),
                            };
                            self.feedback_state = Some(glass_feedback::on_run_start(
                                &cwd_for_feedback,
                                &feedback_config,
                            ));

                            // Cache PRD deliverables for scope guard
                            let prd_rel_reload = self
                                .config
                                .agent
                                .as_ref()
                                .and_then(|a| a.orchestrator.as_ref())
                                .map(|o| o.prd_path.as_str())
                                .unwrap_or("PRD.md");
                            let prd_full_reload =
                                std::path::Path::new(&cwd_for_feedback).join(prd_rel_reload);
                            if let Ok(prd_text) = std::fs::read_to_string(&prd_full_reload) {
                                self.orchestrator.prd_deliverable_files =
                                    orchestrator::parse_prd_deliverables(&prd_text);
                            }

                            // Reset enforcement state
                            self.orchestrator.instruction_buffer.clear();
                            self.orchestrator.dependency_block = None;
                            self.orchestrator.dependency_block_iterations = 0;
                            self.orchestrator.iterations_since_last_commit = 0;
                            self.orchestrator.last_known_head = None;

                            tracing::info!(
                                "Orchestrator: activated via config reload (settings overlay)"
                            );
                        } else if !orch_enabled && was_enabled {
                            {
                                let mut event = glass_scripting::HookEventData::new();
                                event.set("iterations", self.orchestrator.iteration as i64);
                                fire_hook_on_bridge(
                                    &mut self.script_bridge,
                                    &self.orchestrator.project_root,
                                    glass_scripting::HookPoint::OrchestratorRunEnd,
                                    &event,
                                );
                            }
                            self.run_feedback_on_end();
                            self.orchestrator.active = false;
                            tracing::info!(
                                "Orchestrator: deactivated via config reload (settings overlay)"
                            );
                        }
                    }

                    // Restart artifact watcher when orchestrator completion_artifact changes.
                    if self.orchestrator.active {
                        if let Some(handle) = self.artifact_watcher_thread.take() {
                            handle.thread().unpark();
                            // Don't join() here — notify watcher Drop on Windows can
                            // block on ReadDirectoryChangesW I/O completion, freezing
                            // the event loop. Let the thread exit on its own.
                        }
                        let artifact_path = self
                            .config
                            .agent
                            .as_ref()
                            .and_then(|a| a.orchestrator.as_ref())
                            .map(|o| o.completion_artifact.clone())
                            .unwrap_or_else(|| ".glass/done".to_string());
                        let cwd = self.orchestrator.project_root.clone();
                        if let Some((wid, sid)) =
                            self.windows.iter().next().and_then(|(wid, ctx)| {
                                ctx.session_mux.focused_session().map(|s| (*wid, s.id))
                            })
                        {
                            self.artifact_watcher_thread = start_artifact_watcher(
                                &artifact_path,
                                &cwd,
                                self.proxy.clone(),
                                wid,
                                sid,
                            );
                        }
                    }

                    // Request redraw to clear error overlay even if font didn't change
                    if !font_changed {
                        for ctx in self.windows.values() {
                            ctx.window.request_redraw();
                        }
                    }
                }
            }
            AppEvent::GitInfo {
                window_id,
                session_id,
                info,
            } => {
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
            AppEvent::UpdateAvailable(info) => {
                tracing::info!(
                    "Update available: v{} -> v{} ({})",
                    info.current,
                    info.latest,
                    info.release_url
                );
                self.update_info = Some(info);
                // Request redraw on all windows to show notification
                for ctx in self.windows.values() {
                    ctx.window.request_redraw();
                }
            }
            AppEvent::CoordinationUpdate(state) => {
                // Decrement ticker BEFORE checking for new events
                if self.ticker_display_cycles > 0 {
                    self.ticker_display_cycles -= 1;
                }

                // Detect new ticker event
                if let Some(ref evt) = state.ticker_event {
                    let is_new = self.last_ticker_event_id != Some(evt.id);
                    if is_new {
                        self.last_ticker_event_id = Some(evt.id);
                        self.ticker_display_cycles = 1; // Show for 1 poll cycle (5s)
                    }
                }

                self.coordination_state = state;
                // Request redraw on all windows to show updated coordination info
                for ctx in self.windows.values() {
                    ctx.window.request_redraw();
                }
            }
            AppEvent::SoiReady {
                window_id,
                session_id,
                command_id,
                summary,
                severity,
                raw_line_count,
            } => {
                if let Some(ctx) = self.windows.get_mut(&window_id) {
                    if let Some(session) = ctx.session_mux.session_mut(session_id) {
                        if session.last_command_id == Some(command_id) {
                            // Store session-level summary
                            session.last_soi_summary = Some(glass_mux::session::SoiSummary {
                                command_id,
                                one_line: summary.clone(),
                                severity: severity.clone(),
                            });

                            tracing::debug!("SOI ready for cmd {}: {}", command_id, summary);

                            // SOID-01: Populate block fields if enabled
                            let soi_enabled =
                                self.config.soi.as_ref().map(|s| s.enabled).unwrap_or(true);
                            if soi_enabled {
                                if let Some(block) = session
                                    .block_manager
                                    .blocks_mut()
                                    .iter_mut()
                                    .rev()
                                    .find(|b| b.state == glass_terminal::BlockState::Complete)
                                {
                                    block.soi_summary = Some(summary.clone());
                                    block.soi_severity = Some(severity.clone());
                                }
                            }

                            // SOID-02: Inject hint line if shell_summary enabled
                            let (shell_summary_on, min_lines) = match self.config.soi.as_ref() {
                                Some(s) => (s.enabled && s.shell_summary, s.min_lines),
                                None => (false, 0),
                            };
                            if let Some(hint) = glass_terminal::build_soi_hint_line(
                                &summary,
                                soi_enabled,
                                shell_summary_on,
                                min_lines,
                                raw_line_count,
                            ) {
                                pty_send(
                                    &session.pty_sender,
                                    glass_terminal::PtyMsg::Input(std::borrow::Cow::Owned(
                                        hint.into_bytes(),
                                    )),
                                );
                            }
                        }
                    }
                    ctx.window.request_redraw();
                }

                // Emit observation events for the activity stream overlay.
                if self.agent_runtime.is_some() {
                    emit_observe_event(
                        &self.agent_runtime,
                        "output_parsed",
                        &format!("agent-mode analyzed output — {}", severity),
                    );
                    if severity == "Error" || severity == "Warning" {
                        emit_observe_event(
                            &self.agent_runtime,
                            "error_noticed",
                            &format!("agent-mode noticed: {}", summary),
                        );
                    } else {
                        emit_observe_event(
                            &self.agent_runtime,
                            "dismissed",
                            &format!("agent-mode dismissed ({})", severity),
                        );
                    }
                }

                // AGTC-03: Check quiet rules before feeding activity stream.
                // Quiet rules suppress the agent activity stream only -- SOI display is unaffected.
                let quiet = self
                    .config
                    .agent
                    .as_ref()
                    .and_then(|a| a.quiet_rules.as_ref())
                    .map(|q| glass_core::agent_runtime::should_quiet(q, &summary, &severity))
                    .unwrap_or(false);

                // AGTA-01: Feed activity stream (after all UI updates, using owned values)
                if !quiet {
                    if let Some(event) = self
                        .activity_filter
                        .process(command_id, session_id, summary, severity)
                    {
                        if let Some(tx) = &self.activity_stream_tx {
                            if tx.try_send(event).is_err() {
                                tracing::debug!(
                                    "Activity stream channel full or disconnected, dropping event"
                                );
                            }
                        }
                    }
                }
            }
            AppEvent::AgentProposal(proposal) => {
                tracing::info!(
                    "Agent proposal: {} (action={}, files={})",
                    proposal.description,
                    proposal.action,
                    proposal.file_changes.len()
                );

                emit_observe_event(
                    &self.agent_runtime,
                    "proposing",
                    &format!("agent-mode proposing: {}", proposal.description),
                );

                // AGTC-02: Permission matrix -- classify proposal and check permission level.
                let kind = glass_core::agent_runtime::classify_proposal(&proposal);
                let permission_level = self
                    .config
                    .agent
                    .as_ref()
                    .and_then(|a| a.permissions.as_ref())
                    .map(|p| match kind {
                        glass_core::config::PermissionKind::EditFiles => p.edit_files,
                        glass_core::config::PermissionKind::RunCommands => p.run_commands,
                        glass_core::config::PermissionKind::GitOperations => p.git_operations,
                    })
                    .unwrap_or(glass_core::config::PermissionLevel::Approve);

                if permission_level == glass_core::config::PermissionLevel::Never {
                    tracing::info!(
                        "Agent proposal dropped by permission matrix (kind={:?})",
                        kind
                    );
                    // Never: drop without toast or worktree.
                } else {
                    let handle = if !proposal.file_changes.is_empty() {
                        if let Some(ref wm) = self.worktree_manager {
                            // Use the active session CWD as project root; fall back to process CWD.
                            let project_root = self
                                .windows
                                .values()
                                .next()
                                .and_then(|ctx| {
                                    ctx.session_mux
                                        .focused_session()
                                        .map(|s| std::path::PathBuf::from(s.status.cwd()))
                                })
                                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
                            let proposal_id =
                                format!("proposal-{}", self.agent_proposal_worktrees.len());
                            match wm.create_worktree(
                                &project_root,
                                &proposal_id,
                                &proposal.file_changes,
                            ) {
                                Ok(wt_handle) => {
                                    tracing::info!(
                                        "Created worktree {} for proposal",
                                        wt_handle.id
                                    );
                                    Some(wt_handle)
                                }
                                Err(e) => {
                                    tracing::error!("Failed to create worktree for proposal: {e}");
                                    None
                                }
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    if permission_level == glass_core::config::PermissionLevel::Auto {
                        // Auto: apply immediately without user interaction.
                        let auto_description = proposal.description.clone();
                        if let (Some(wm), Some(wt_handle)) =
                            (self.worktree_manager.as_ref(), handle)
                        {
                            if let Err(e) = wm.apply(wt_handle) {
                                tracing::error!(
                                    "Auto-apply failed for proposal \"{}\": {e}",
                                    auto_description
                                );
                            } else {
                                tracing::info!("Auto-applied proposal: {}", auto_description);
                            }
                        }
                        // Show brief auto-applied toast (no worktree in list).
                        self.active_toast = Some(ProposalToast {
                            description: format!("Auto-applied: {}", proposal.description),
                            proposal_idx: self.agent_proposal_worktrees.len(),
                            created_at: std::time::Instant::now(),
                        });
                    } else {
                        // Approve: existing behavior -- show overlay, let user decide.
                        // Clone description before push (push takes ownership of proposal).
                        let toast_description = proposal.description.clone();
                        self.agent_proposal_worktrees.push((proposal, handle));
                        let proposal_idx = self.agent_proposal_worktrees.len() - 1;
                        self.active_toast = Some(ProposalToast {
                            description: toast_description,
                            proposal_idx,
                            created_at: std::time::Instant::now(),
                        });
                    }

                    for ctx in self.windows.values() {
                        ctx.window.request_redraw();
                    }
                }
            }
            AppEvent::AgentQueryResult { cost_usd } => {
                if let Some(ref mut runtime) = self.agent_runtime {
                    runtime.budget.add_cost(cost_usd);
                    self.agent_cost_usd += cost_usd;
                    if runtime.budget.is_exceeded() && !self.agent_proposals_paused {
                        self.agent_proposals_paused = true;
                        tracing::warn!(
                            "AgentRuntime: budget exceeded (${:.4} / ${:.2}) -- pausing proposals",
                            self.agent_cost_usd,
                            runtime.config.max_budget_usd
                        );
                    }
                }
                for ctx in self.windows.values() {
                    ctx.window.request_redraw();
                }
            }
            AppEvent::AgentCrashed => {
                // Ignore stale crashes from orphaned reader threads of previously killed agents.
                // After respawn, self.agent_generation is incremented. If the current runtime's
                // generation doesn't match, this crash is from an old agent — ignore it.
                let current_gen = self
                    .agent_runtime
                    .as_ref()
                    .map(|r| r.generation)
                    .unwrap_or(0);
                if current_gen != self.agent_generation {
                    tracing::info!(
                        "AgentRuntime: ignoring stale AgentCrashed (gen {} vs current {})",
                        current_gen,
                        self.agent_generation
                    );
                    return;
                }
                tracing::error!("AgentRuntime: agent subprocess crashed or exited");
                let should_restart = if let Some(ref mut runtime) = self.agent_runtime {
                    let backoff_secs: u64 = match runtime.restart_count {
                        0 => 5,
                        1 => 15,
                        _ => 45,
                    };
                    let elapsed = runtime
                        .last_crash
                        .map(|t| t.elapsed().as_secs())
                        .unwrap_or(u64::MAX);
                    runtime.restart_count < 3 && elapsed >= backoff_secs
                } else {
                    false
                };

                if should_restart {
                    let (restart_count, config) = if let Some(ref mut runtime) = self.agent_runtime
                    {
                        runtime.last_crash = Some(std::time::Instant::now());
                        (runtime.restart_count + 1, runtime.config.clone())
                    } else {
                        return;
                    };

                    tracing::info!(
                        "AgentRuntime: attempting restart #{} with backoff",
                        restart_count
                    );

                    // Create a new activity channel for the restarted agent
                    let activity_config =
                        glass_core::activity_stream::ActivityStreamConfig::default();
                    let (new_tx, new_rx) =
                        glass_core::activity_stream::create_channel(&activity_config);
                    self.activity_stream_tx = Some(new_tx);

                    // Use stored project_root if orchestrator is active, else terminal CWD
                    let cwd =
                        if self.orchestrator.active && !self.orchestrator.project_root.is_empty() {
                            self.orchestrator.project_root.clone()
                        } else {
                            self.get_focused_cwd()
                        };

                    // On crash restart, provide checkpoint context so the agent
                    // can resume instead of starting blind with GLASS_WAIT.
                    let restart_msg = if self.orchestrator.active {
                        let cp_path = std::path::Path::new(&cwd)
                            .join(".glass")
                            .join("checkpoint.md");
                        let checkpoint = std::fs::read_to_string(&cp_path).unwrap_or_default();
                        if checkpoint.is_empty() {
                            None
                        } else {
                            Some(format!(
                                "[ORCHESTRATOR_RESTART]\nAgent crashed and restarted (attempt #{}).\nResume from checkpoint:\n{}\n",
                                restart_count, checkpoint
                            ))
                        }
                    } else {
                        None
                    };

                    self.agent_runtime = try_spawn_agent(
                        config,
                        new_rx,
                        self.proxy.clone(),
                        restart_count,
                        Some(std::time::Instant::now()),
                        cwd,
                        restart_msg,
                    );
                } else {
                    tracing::error!(
                        "AgentRuntime: restart limit reached or backoff not elapsed -- agent disabled"
                    );
                    // Flush any pending collapsed event before disabling the runtime.
                    if let Some(event) = self.activity_filter.flush_collapsed() {
                        if let Some(tx) = &self.activity_stream_tx {
                            let _ = tx.try_send(event);
                        }
                    }
                    self.agent_runtime = None;

                    // If orchestrating, deactivate — can't orchestrate without an agent
                    if self.orchestrator.active {
                        {
                            let mut event = glass_scripting::HookEventData::new();
                            event.set("iterations", self.orchestrator.iteration as i64);
                            fire_hook_on_bridge(
                                &mut self.script_bridge,
                                &self.orchestrator.project_root,
                                glass_scripting::HookPoint::OrchestratorRunEnd,
                                &event,
                            );
                        }
                        self.run_feedback_on_end();
                        self.orchestrator.active = false;
                        self.orchestrator.response_pending = false;
                        tracing::error!(
                            "Orchestrator: deactivated — agent crashed and could not be restarted"
                        );
                        if let Some(handle) = self.artifact_watcher_thread.take() {
                            handle.thread().unpark();
                            // Don't join — notify Drop on Windows blocks on I/O completion.
                        }
                        for ctx in self.windows.values() {
                            ctx.window.request_redraw();
                        }
                    }
                }
            }
            AppEvent::AgentHandoff {
                session_id,
                handoff,
                project_root,
                raw_json,
            } => {
                tracing::info!(
                    "AgentRuntime: received handoff from session_id={}",
                    session_id
                );
                match glass_agent::AgentSessionDb::open_default() {
                    Ok(mut db) => {
                        let canonical = std::fs::canonicalize(&project_root)
                            .unwrap_or_else(|_| std::path::PathBuf::from(&project_root));
                        let canonical_str = canonical.to_string_lossy().to_string();
                        let record = glass_agent::AgentSessionRecord {
                            id: uuid::Uuid::new_v4().to_string(),
                            project_root: canonical_str,
                            session_id: if session_id.is_empty() {
                                uuid::Uuid::new_v4().to_string()
                            } else {
                                session_id
                            },
                            previous_session_id: handoff.previous_session_id.clone(),
                            handoff: glass_agent::HandoffData {
                                work_completed: handoff.work_completed,
                                work_remaining: handoff.work_remaining,
                                key_decisions: handoff.key_decisions,
                                previous_session_id: handoff.previous_session_id,
                            },
                            raw_handoff: raw_json,
                            created_at: 0, // DB default (unixepoch()) handles this
                        };
                        if let Err(e) = db.insert_session(&record) {
                            tracing::warn!("AgentRuntime: failed to persist handoff: {}", e);
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "AgentRuntime: failed to open session db for handoff: {}",
                            e
                        );
                    }
                }
            }
            AppEvent::OrchestratorResponse { response } => {
                if !self.orchestrator.active {
                    return;
                }

                self.orchestrator.response_pending = false;

                // Push to orchestrator transcript
                self.orchestrator_event_buffer.push(
                    orchestrator_events::OrchestratorEvent::AgentText {
                        text: response.clone(),
                    },
                    self.orchestrator.iteration,
                );

                let parsed = orchestrator::parse_agent_response(&response);
                self.orchestrator.iteration += 1;
                self.orchestrator.iterations_since_checkpoint += 1;
                self.orchestrator
                    .feedback_iteration_timestamps
                    .push(std::time::Instant::now());

                // Check bounded iteration limit — but let Done/Checkpoint through
                // so the current response isn't silently dropped.
                if self.orchestrator.should_stop_bounded()
                    && !self.orchestrator.bounded_stop_pending
                    && !matches!(parsed, orchestrator::AgentResponse::Checkpoint { .. })
                    && !matches!(parsed, orchestrator::AgentResponse::Done { .. })
                {
                    self.orchestrator.bounded_stop_pending = true;
                    tracing::info!(
                        "Orchestrator: bounded limit reached at iteration {}",
                        self.orchestrator.iteration
                    );
                    self.orchestrator.feedback_completion_reason = "bounded_limit".to_string();
                    self.trigger_checkpoint_synthesis("bounded limit reached", "N/A");
                    for ctx in self.windows.values() {
                        ctx.window.request_redraw();
                    }
                    return;
                }

                // Fix #2: Auto-checkpoint after N iterations to prevent context exhaustion
                if self.orchestrator.should_auto_checkpoint()
                    && !matches!(parsed, orchestrator::AgentResponse::Checkpoint { .. })
                    && !matches!(parsed, orchestrator::AgentResponse::Done { .. })
                {
                    tracing::info!(
                        "Orchestrator: auto-checkpoint triggered after {} iterations",
                        self.orchestrator.iterations_since_checkpoint
                    );
                    self.trigger_checkpoint_synthesis("auto-refresh", "continue from PRD");
                    for ctx in self.windows.values() {
                        ctx.window.request_redraw();
                    }
                    return;
                }

                match parsed {
                    orchestrator::AgentResponse::Wait => {
                        tracing::debug!("Orchestrator: agent says WAIT");
                    }
                    orchestrator::AgentResponse::TypeText(text) => {
                        let text_stuck = self.orchestrator.record_response(&text);
                        let stuck = text_stuck || self.orchestrator.fingerprint_stuck;
                        if stuck {
                            self.orchestrator.fingerprint_stuck = false;
                            self.orchestrator.feedback_stuck_count += 1;

                            tracing::warn!(
                                "Orchestrator: stuck detected (text_stuck={}, fingerprint_stuck={}) after {} identical",
                                text_stuck,
                                !text_stuck, // fingerprint was the trigger if text wasn't
                                self.orchestrator.max_retries
                            );

                            // Log stuck iteration
                            orchestrator::append_iteration_log(
                                &self.orchestrator.project_root,
                                self.orchestrator.iteration,
                                "stuck",
                                "stuck",
                                &format!(
                                    "Stuck after {} identical responses: {}",
                                    self.orchestrator.max_retries,
                                    &text[..text.len().min(80)]
                                ),
                            );

                            // Tell Claude Code to revert
                            if let Some(ctx) = self.windows.values().next() {
                                if let Some(session) = ctx.session_mux.focused_session() {
                                    let msg = "You've tried this same approach multiple times without making progress. STOP and take a different approach:\n1. If you have uncommitted changes, stash them: git stash\n2. Think about WHY the current approach isn't working\n3. Try a fundamentally different strategy, not a minor variation\n";
                                    let bytes = msg.as_bytes().to_vec();
                                    pty_send(
                                        &session.pty_sender,
                                        PtyMsg::Input(std::borrow::Cow::Owned(bytes)),
                                    );
                                }
                            }

                            self.orchestrator.reset_stuck();
                            return;
                        }

                        // Cap at 50 most recent responses for instruction overload analysis
                        if self.orchestrator.feedback_agent_responses.len() < 50 {
                            self.orchestrator
                                .feedback_agent_responses
                                .push(text.clone());
                        }

                        // Instruction splitting enforcement: if the
                        // smaller_instructions rule is active, split numbered
                        // instructions and buffer all but the first.
                        let text_to_type = if self
                            .feedback_state
                            .as_ref()
                            .map(|fs| fs.engine.is_rule_active("smaller_instructions"))
                            .unwrap_or(false)
                        {
                            let items = parse_numbered_instructions(&text);
                            if items.len() >= 2 {
                                let first = items[0].clone();
                                self.orchestrator.instruction_buffer = items[1..].to_vec();
                                tracing::info!(
                                    "Orchestrator: split {} instructions, buffering {}",
                                    items.len(),
                                    items.len() - 1
                                );
                                first
                            } else {
                                text.clone()
                            }
                        } else {
                            text.clone()
                        };

                        // During kickoff, defer all TypeText — the user is still
                        // chatting with Claude Code and must not be interrupted.
                        if !self.orchestrator.kickoff_complete {
                            tracing::debug!(
                                "Orchestrator: deferring TypeText — kickoff not complete"
                            );
                            self.orchestrator
                                .deferred_type_text
                                .push(text_to_type.clone());
                        } else if let Some(ctx) = self.windows.values().next() {
                            // Type the text into the active PTY — but only if a command
                            // isn't actively executing (avoid interrupting Claude Code mid-turn)
                            if let Some(session) = ctx.session_mux.focused_session() {
                                let block_executing = session
                                    .block_manager
                                    .current_block_index()
                                    .and_then(|idx| session.block_manager.blocks().get(idx))
                                    .map(|b| {
                                        b.state
                                            == glass_terminal::block_manager::BlockState::Executing
                                    })
                                    .unwrap_or(false);

                                if block_executing {
                                    tracing::debug!(
                                        "Orchestrator: deferring TypeText — command is executing"
                                    );
                                    self.orchestrator
                                        .deferred_type_text
                                        .push(text_to_type.clone());
                                } else {
                                    let bytes = format!("{}\n", text_to_type).into_bytes();
                                    pty_send(
                                        &session.pty_sender,
                                        PtyMsg::Input(std::borrow::Cow::Owned(bytes)),
                                    );
                                    self.orchestrator.mark_pty_write();
                                }
                            }
                        }
                    }
                    orchestrator::AgentResponse::Checkpoint { completed, next } => {
                        tracing::info!(
                            "Orchestrator: checkpoint — completed={}, next={}",
                            completed,
                            next
                        );

                        // Log the checkpoint iteration
                        orchestrator::append_iteration_log(
                            &self.orchestrator.project_root,
                            self.orchestrator.iteration,
                            &completed,
                            "checkpoint",
                            &format!("Context refresh: completed {completed}, next {next}"),
                        );

                        // Start the refresh cycle with checkpoint synthesis
                        self.trigger_checkpoint_synthesis(&completed, &next);
                    }
                    orchestrator::AgentResponse::Done { summary } => {
                        tracing::info!("Orchestrator: project complete — {}", summary);
                        self.orchestrator.feedback_completion_reason = if summary.is_empty() {
                            "complete".to_string()
                        } else {
                            format!("complete: {}", summary)
                        };

                        orchestrator::append_iteration_log(
                            &self.orchestrator.project_root,
                            self.orchestrator.iteration,
                            "done",
                            "complete",
                            &if summary.is_empty() {
                                "Project complete".to_string()
                            } else {
                                summary.clone()
                            },
                        );

                        // Generate post-mortem report
                        let prd_path = self
                            .config
                            .agent
                            .as_ref()
                            .and_then(|a| a.orchestrator.as_ref())
                            .map(|o| o.prd_path.clone())
                            .unwrap_or_default();
                        orchestrator::generate_postmortem(
                            &self.orchestrator.project_root,
                            self.orchestrator.iteration,
                            self.orchestrator_activated_at.map(|t| t.elapsed()),
                            self.orchestrator.metric_baseline.as_ref(),
                            &format!(
                                "Done ({})",
                                if summary.is_empty() {
                                    "no summary"
                                } else {
                                    &summary
                                }
                            ),
                            &prd_path,
                        );

                        {
                            let mut event = glass_scripting::HookEventData::new();
                            event.set("iterations", self.orchestrator.iteration as i64);
                            fire_hook_on_bridge(
                                &mut self.script_bridge,
                                &self.orchestrator.project_root,
                                glass_scripting::HookPoint::OrchestratorRunEnd,
                                &event,
                            );
                        }
                        self.run_feedback_on_end();
                        self.orchestrator.active = false;
                        if let Some(handle) = self.artifact_watcher_thread.take() {
                            handle.thread().unpark();
                            // Don't join — notify Drop on Windows blocks on I/O completion.
                        }

                        // Tell Claude Code to do a final commit
                        if let Some(ctx) = self.windows.values().next() {
                            if let Some(session) = ctx.session_mux.focused_session() {
                                let msg = format!(
                                    "All PRD items are complete. Commit any remaining changes with a summary commit message.{}\n",
                                    if summary.is_empty() { String::new() } else { format!(" Summary: {}", summary) }
                                );
                                let bytes = msg.into_bytes();
                                pty_send(
                                    &session.pty_sender,
                                    PtyMsg::Input(std::borrow::Cow::Owned(bytes)),
                                );
                                self.orchestrator.mark_pty_write();
                            }
                        }
                    }
                    orchestrator::AgentResponse::Verify { commands } => {
                        tracing::info!(
                            "Orchestrator: agent registered {} verify command(s)",
                            commands.len()
                        );
                        let baseline = self
                            .orchestrator
                            .metric_baseline
                            .get_or_insert_with(orchestrator::MetricBaseline::new);
                        baseline.commands.extend(commands);
                    }
                }

                // Request redraw for status bar update
                for ctx in self.windows.values() {
                    ctx.window.request_redraw();
                }
            }
            AppEvent::OrchestratorSilence {
                window_id,
                session_id,
            } => {
                if !self.orchestrator.active {
                    return;
                }

                if self.agent_runtime.is_none() {
                    return;
                }

                // Backpressure: skip if waiting for agent response
                if self.orchestrator.response_pending {
                    tracing::debug!("Orchestrator: skipping context send (response pending)");
                    return;
                }

                // Kickoff guard: during the kickoff phase, suppress the silence
                // trigger while the user is still actively engaged. This is
                // model-agnostic — it only tracks user keypress recency, not any
                // specific agent's prompt format.
                //
                // Three cases:
                // 1. User hasn't typed yet → suppress (still waiting for interaction)
                // 2. User typed recently → suppress (still engaged)
                // 3. User typed but has gone idle → kickoff complete, start loop
                if !self.orchestrator.kickoff_complete {
                    let threshold = std::time::Duration::from_secs(
                        self.config
                            .agent
                            .as_ref()
                            .and_then(|a| a.orchestrator.as_ref())
                            .map(|o| o.silence_timeout_secs)
                            .unwrap_or(30),
                    );
                    if self.orchestrator.last_user_keypress.is_none() {
                        // User hasn't typed anything yet — still waiting
                        tracing::debug!(
                            "Orchestrator: skipping context send during kickoff (no user input yet)"
                        );
                        return;
                    }
                    if self.orchestrator.user_recently_active(threshold) {
                        tracing::debug!(
                            "Orchestrator: skipping context send during kickoff (user recently active)"
                        );
                        return;
                    }
                    // User typed and has gone idle — kickoff is complete
                    self.orchestrator.kickoff_complete = true;
                    tracing::info!("Orchestrator: kickoff complete (user and terminal both idle)");
                }

                // Flush deferred text if a previous TypeText was blocked during
                // kickoff or while a command was executing.
                if !self.orchestrator.deferred_type_text.is_empty() {
                    if let Some(ctx) = self.windows.values().next() {
                        if let Some(session) = ctx.session_mux.focused_session() {
                            let block_executing = session
                                .block_manager
                                .current_block_index()
                                .and_then(|idx| session.block_manager.blocks().get(idx))
                                .map(|b| {
                                    b.state == glass_terminal::block_manager::BlockState::Executing
                                })
                                .unwrap_or(false);
                            if !block_executing {
                                let deferred = self.orchestrator.deferred_type_text.remove(0);
                                let bytes = format!("{}\n", deferred).into_bytes();
                                pty_send(
                                    &session.pty_sender,
                                    PtyMsg::Input(std::borrow::Cow::Owned(bytes)),
                                );
                                self.orchestrator.mark_pty_write();
                                tracing::info!("Orchestrator: flushed deferred TypeText ({} chars, {} remaining)", deferred.len(), self.orchestrator.deferred_type_text.len());
                                return; // Let the typed text be processed before next silence
                            }
                        }
                    }
                }

                // Check if we're in a checkpoint synthesis cycle
                if let orchestrator::CheckpointPhase::Synthesizing { started_at, .. } =
                    &self.orchestrator.checkpoint_phase
                {
                    if started_at.elapsed().as_secs() >= orchestrator::SYNTHESIS_TIMEOUT_SECS {
                        tracing::warn!("Checkpoint synthesis timed out — using fallback");
                        let cwd = self.orchestrator.project_root.clone();
                        self.write_checkpoint_and_respawn(&cwd);
                    }
                    return; // Don't send context while synthesizing
                }

                // Capture terminal context
                if let Some(ctx) = self.windows.get(&window_id) {
                    if let Some(session) = ctx.session_mux.session(session_id) {
                        let lines = extract_term_lines(&session.term, 80);
                        let (exit_code, soi_summary, soi_errors) =
                            fetch_latest_soi_context(session);
                        let mut context = orchestrator::build_orchestrator_context(
                            &lines,
                            exit_code,
                            soi_summary.as_deref(),
                            &soi_errors,
                        );
                        if !self.orchestrator.coverage_gaps_context.is_empty() {
                            context.push_str(&self.orchestrator.coverage_gaps_context);
                        }

                        // Build environment fingerprint for stuck detection
                        let cwd = session.status.cwd().to_string();
                        let git_diff = git_cmd()
                            .args(["diff", "--stat"])
                            .current_dir(&cwd)
                            .output()
                            .ok()
                            .and_then(|o| {
                                if o.status.success() {
                                    String::from_utf8(o.stdout).ok()
                                } else {
                                    None
                                }
                            });

                        // Reuse the 80-line extraction for fingerprint (last 50 of 80)
                        let fp_start = lines.len().saturating_sub(50);
                        let soi_for_fp = if exit_code.is_some_and(|c| c != 0) {
                            Some(soi_errors.as_slice())
                        } else {
                            None
                        };
                        let fingerprint = orchestrator::StateFingerprint::compute(
                            &lines[fp_start..],
                            soi_for_fp,
                            git_diff.as_deref(),
                        );
                        self.orchestrator.fingerprint_stuck =
                            self.orchestrator.record_fingerprint(fingerprint);

                        // Commit detection: track HEAD changes
                        let current_head = git_cmd()
                            .args(["rev-parse", "HEAD"])
                            .current_dir(&cwd)
                            .output()
                            .ok()
                            .and_then(|o| {
                                if o.status.success() {
                                    String::from_utf8(o.stdout)
                                        .ok()
                                        .map(|s| s.trim().to_string())
                                } else {
                                    None
                                }
                            });
                        if let Some(ref head_sha) = current_head {
                            if self.orchestrator.last_known_head.as_ref() != Some(head_sha) {
                                self.orchestrator.iterations_since_last_commit = 0;
                                self.orchestrator.last_known_head = Some(head_sha.clone());
                            } else {
                                self.orchestrator.iterations_since_last_commit += 1;
                            }
                        }

                        // Fix #4/#5: Check for nudge.md (course correction while running)
                        let nudge_path = std::path::Path::new(&cwd).join(".glass").join("nudge.md");
                        let nudge = std::fs::read_to_string(&nudge_path).ok();
                        if nudge.is_some() {
                            let _ = std::fs::remove_file(&nudge_path);
                        }

                        // Clean up artifact file if it exists (one-shot signal)
                        let artifact_path_cfg = self
                            .config
                            .agent
                            .as_ref()
                            .and_then(|a| a.orchestrator.as_ref())
                            .map(|o| o.completion_artifact.clone())
                            .unwrap_or_default();
                        if !artifact_path_cfg.is_empty() {
                            let full = std::path::Path::new(&cwd).join(&artifact_path_cfg);
                            if full.exists() {
                                let _ = std::fs::remove_file(&full);
                            }
                        }

                        // Record current commit for metric guard revert (reuse HEAD from commit detection above)
                        if self.orchestrator.metric_baseline.is_some() {
                            self.orchestrator.last_good_commit = current_head.clone();
                        }

                        // Metric guard: run verification on background thread
                        let verify_mode = self
                            .config
                            .agent
                            .as_ref()
                            .and_then(|a| a.orchestrator.as_ref())
                            .map(|o| o.verify_mode.as_str())
                            .unwrap_or("floor");

                        let already_verified = self
                            .orchestrator
                            .last_verified_iteration
                            .map(|v| v == self.orchestrator.iteration)
                            .unwrap_or(false);

                        if verify_mode == "floor" && !already_verified {
                            if let Some(ref baseline) = self.orchestrator.metric_baseline {
                                if !baseline.commands.is_empty() {
                                    let commands = baseline.commands.clone();
                                    let verify_cwd = cwd.clone();
                                    let proxy = self.proxy.clone();
                                    let spawn_result = std::thread::Builder::new()
                                        .name("Glass verify".into())
                                        .spawn(move || {
                                            let deadline = std::time::Instant::now()
                                                + std::time::Duration::from_secs(300); // 5 min timeout
                                            let results: Vec<VerifyEventResult> = commands
                                                .iter()
                                                .map(|cmd| {
                                                    // Check deadline before starting each command
                                                    if std::time::Instant::now() > deadline {
                                                        return VerifyEventResult {
                                                            command_name: cmd.name.clone(),
                                                            exit_code: -1,
                                                            tests_passed: None,
                                                            tests_failed: None,
                                                            output: "Verification timeout (5 min)"
                                                                .to_string(),
                                                        };
                                                    }
                                                    let mut proc = if cfg!(target_os = "windows") {
                                                        let mut c = std::process::Command::new("cmd");
                                                        c.args(["/C", &cmd.cmd]);
                                                        #[cfg(target_os = "windows")]
                                                        {
                                                            use std::os::windows::process::CommandExt;
                                                            c.creation_flags(0x08000000); // CREATE_NO_WINDOW
                                                        }
                                                        c
                                                    } else {
                                                        let mut c = std::process::Command::new("sh");
                                                        c.args(["-c", &cmd.cmd]);
                                                        c
                                                    };
                                                    let output = proc.current_dir(&verify_cwd)
                                                            .output();
                                                    match output {
                                                        Ok(o) => {
                                                            let stdout =
                                                                String::from_utf8_lossy(&o.stdout)
                                                                    .to_string();
                                                            let stderr =
                                                                String::from_utf8_lossy(&o.stderr)
                                                                    .to_string();
                                                            let combined =
                                                                format!("{stdout}\n{stderr}");
                                                            let (passed, failed) =
                                                                parse_test_counts_from_output(
                                                                    &combined,
                                                                );
                                                            let exit_code =
                                                                o.status.code().unwrap_or(-1);
                                                            // If exit code is non-zero but parser
                                                            // found no test counts, it's a build
                                                            // failure — report 0/0 so the metric
                                                            // guard display isn't "? / ?".
                                                            let (passed, failed) =
                                                                if exit_code != 0
                                                                    && passed.is_none()
                                                                    && failed.is_none()
                                                                {
                                                                    (Some(0), Some(0))
                                                                } else {
                                                                    (passed, failed)
                                                                };
                                                            VerifyEventResult {
                                                                command_name: cmd.name.clone(),
                                                                exit_code,
                                                                tests_passed: passed,
                                                                tests_failed: failed,
                                                                output: combined,
                                                            }
                                                        }
                                                        Err(e) => VerifyEventResult {
                                                            command_name: cmd.name.clone(),
                                                            exit_code: -1,
                                                            tests_passed: None,
                                                            tests_failed: None,
                                                            output: format!("Failed to run: {e}"),
                                                        },
                                                    }
                                                })
                                                .collect();
                                            let _ = proxy.send_event(AppEvent::VerifyComplete {
                                                window_id,
                                                session_id,
                                                results,
                                            });
                                        });
                                    if spawn_result.is_ok() {
                                        // Block sending context until verification completes
                                        self.orchestrator.response_pending = true;
                                        self.orchestrator.last_verified_iteration =
                                            Some(self.orchestrator.iteration);
                                        return;
                                    } else {
                                        tracing::warn!(
                                            "Orchestrator: failed to spawn verify thread"
                                        );
                                    }
                                }
                            }
                        }

                        // File-based verification for general mode
                        if verify_mode == "files" && !already_verified {
                            let verify_files = self
                                .config
                                .agent
                                .as_ref()
                                .and_then(|a| a.orchestrator.as_ref())
                                .map(|o| o.verify_files.clone())
                                .unwrap_or_default();
                            if !verify_files.is_empty() {
                                let (regressed, summary) = orchestrator::check_file_verification(
                                    &cwd,
                                    &verify_files,
                                    &mut self.file_verify_baseline,
                                );
                                orchestrator::append_iteration_log(
                                    &cwd,
                                    self.orchestrator.iteration,
                                    "verify",
                                    if regressed { "revert" } else { "keep" },
                                    &summary,
                                );
                                if regressed {
                                    if let Some(ref commit) = self.orchestrator.last_good_commit {
                                        let _ = git_cmd()
                                            .args(["reset", "--hard", commit])
                                            .current_dir(&cwd)
                                            .output();
                                        tracing::info!("File verify: reverted to {commit}");
                                    }
                                }
                                self.orchestrator.last_verified_iteration =
                                    Some(self.orchestrator.iteration);
                            }
                        }

                        // Dependency block check: if blocked, repeat the block message
                        if let Some(block_msg) = self.orchestrator.dependency_block.clone() {
                            self.orchestrator.dependency_block_iterations += 1;

                            // Check if resolved: look at last block's exit code
                            let resolved = if let Some(ctx_ref) = self.windows.get(&window_id) {
                                ctx_ref
                                    .session_mux
                                    .session(session_id)
                                    .and_then(|s| {
                                        s.block_manager
                                            .current_block_index()
                                            .and_then(|idx| s.block_manager.blocks().get(idx))
                                            .and_then(|b| b.exit_code)
                                    })
                                    .map(|code| code == 0)
                                    .unwrap_or(false)
                            } else {
                                false
                            };

                            if resolved
                                || self.orchestrator.dependency_block_iterations
                                    >= orchestrator::DEPENDENCY_BLOCK_MAX_ITERATIONS
                            {
                                self.orchestrator.dependency_block = None;
                                self.orchestrator.dependency_block_iterations = 0;
                                tracing::info!("Orchestrator: dependency block cleared");
                            } else {
                                // Type block message directly into PTY
                                if let Some(ctx_ref) = self.windows.get(&window_id) {
                                    if let Some(session_ref) =
                                        ctx_ref.session_mux.session(session_id)
                                    {
                                        let msg = format!("STOP current task. {}\n", block_msg);
                                        pty_send(
                                            &session_ref.pty_sender,
                                            PtyMsg::Input(std::borrow::Cow::Owned(
                                                msg.into_bytes(),
                                            )),
                                        );
                                        self.orchestrator.mark_pty_write();
                                    }
                                }
                                self.orchestrator.response_pending = true;
                                for ctx_ref in self.windows.values() {
                                    ctx_ref.window.request_redraw();
                                }
                                return;
                            }
                        }

                        // Feedback loop: check rules and enforce actions
                        let mut feedback_notifications = Vec::new();
                        if let Some(ref mut feedback_state) = self.feedback_state {
                            let run_state = glass_feedback::RunState {
                                iteration: self.orchestrator.iteration,
                                iterations_since_last_commit: self
                                    .orchestrator
                                    .iterations_since_last_commit,
                                revert_rate: if self.orchestrator.iteration > 0 {
                                    self.orchestrator
                                        .metric_baseline
                                        .as_ref()
                                        .map(|m| {
                                            m.revert_count as f64
                                                / self.orchestrator.iteration as f64
                                        })
                                        .unwrap_or(0.0)
                                } else {
                                    0.0
                                },
                                stuck_rate: if self.orchestrator.iteration > 0 {
                                    self.orchestrator.feedback_stuck_count as f64
                                        / self.orchestrator.iteration as f64
                                } else {
                                    0.0
                                },
                                waste_rate: if self.orchestrator.iteration > 0 {
                                    self.orchestrator.feedback_waste_iterations as f64
                                        / self.orchestrator.iteration as f64
                                } else {
                                    0.0
                                },
                                recent_reverted_files: self
                                    .orchestrator
                                    .feedback_reverted_files
                                    .clone(),
                                verify_alternations: self
                                    .orchestrator
                                    .feedback_verify_sequence
                                    .windows(2)
                                    .filter(|w| w[0] != w[1])
                                    .count()
                                    as u32,
                            };
                            let actions = glass_feedback::check_rules(feedback_state, &run_state);
                            for action in &actions {
                                match action {
                                    glass_feedback::RuleAction::ForceCommit => {
                                        // Check last verify wasn't regression before committing
                                        let last_regressed = self
                                            .orchestrator
                                            .metric_baseline
                                            .as_ref()
                                            .map(|m| {
                                                orchestrator::MetricBaseline::check_regression(
                                                    &m.baseline_results,
                                                    &m.last_results,
                                                )
                                            })
                                            .unwrap_or(false);
                                        if !last_regressed {
                                            let result = git_cmd()
                                                .args([
                                                    "commit",
                                                    "-am",
                                                    "checkpoint: auto-commit by orchestrator",
                                                ])
                                                .current_dir(&cwd)
                                                .output();
                                            if let Ok(o) = result {
                                                if o.status.success() {
                                                    self.orchestrator.last_good_commit = git_cmd()
                                                        .args(["rev-parse", "HEAD"])
                                                        .current_dir(&cwd)
                                                        .output()
                                                        .ok()
                                                        .and_then(|o2| {
                                                            if o2.status.success() {
                                                                String::from_utf8(o2.stdout)
                                                                    .ok()
                                                                    .map(|s| s.trim().to_string())
                                                            } else {
                                                                None
                                                            }
                                                        });
                                                    self.orchestrator
                                                        .iterations_since_last_commit = 0;
                                                    self.orchestrator.feedback_commit_count += 1;
                                                    tracing::info!(
                                                        "Enforcement: force-committed changes"
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    glass_feedback::RuleAction::IsolateCommit { file } => {
                                        // Check if file appears in git diff
                                        let in_diff = git_diff
                                            .as_deref()
                                            .map(|d| {
                                                parse_diff_stat_files(d).iter().any(|f| f == file)
                                            })
                                            .unwrap_or(false);
                                        if in_diff {
                                            let _ = git_cmd()
                                                .args(["add", file])
                                                .current_dir(&cwd)
                                                .output();
                                            let msg =
                                                format!("checkpoint: isolate-commit {}", file);
                                            let result = git_cmd()
                                                .args(["commit", "-m", &msg])
                                                .current_dir(&cwd)
                                                .output();
                                            if let Ok(o) = result {
                                                if o.status.success() {
                                                    self.orchestrator.last_good_commit = git_cmd()
                                                        .args(["rev-parse", "HEAD"])
                                                        .current_dir(&cwd)
                                                        .output()
                                                        .ok()
                                                        .and_then(|o2| {
                                                            if o2.status.success() {
                                                                String::from_utf8(o2.stdout)
                                                                    .ok()
                                                                    .map(|s| s.trim().to_string())
                                                            } else {
                                                                None
                                                            }
                                                        });
                                                    self.orchestrator
                                                        .iterations_since_last_commit = 0;
                                                    tracing::info!(
                                                        "Enforcement: isolate-committed {}",
                                                        file
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    glass_feedback::RuleAction::RevertOutOfScope { .. } => {
                                        // Compute out-of-scope files from git diff vs prd_deliverable_files
                                        let diff_files = git_diff
                                            .as_deref()
                                            .map(parse_diff_stat_files)
                                            .unwrap_or_default();
                                        let deliverables = &self.orchestrator.prd_deliverable_files;
                                        if !deliverables.is_empty() {
                                            let out_of_scope: Vec<String> = diff_files
                                                .iter()
                                                .filter(|f| {
                                                    !deliverables.iter().any(|d| {
                                                        f.contains(d) || d.contains(f.as_str())
                                                    })
                                                })
                                                .cloned()
                                                .collect();
                                            if out_of_scope.len() >= 3 {
                                                for oos_file in &out_of_scope {
                                                    let _ = git_cmd()
                                                        .args(["checkout", "--", oos_file])
                                                        .current_dir(&cwd)
                                                        .output();
                                                }
                                                tracing::info!(
                                                    "Enforcement: reverted {} out-of-scope files",
                                                    out_of_scope.len()
                                                );
                                                feedback_notifications.push(format!(
                                                    "Reverted {} out-of-scope files. Stay focused on PRD deliverables.",
                                                    out_of_scope.len()
                                                ));
                                            }
                                        }
                                    }
                                    glass_feedback::RuleAction::BlockUntilResolved { message } => {
                                        self.orchestrator.dependency_block = Some(message.clone());
                                        self.orchestrator.dependency_block_iterations = 0;
                                        tracing::info!(
                                            "Enforcement: dependency block set — {}",
                                            message
                                        );
                                    }
                                    glass_feedback::RuleAction::SplitInstructions => {
                                        // Handled in OrchestratorResponse handler
                                    }
                                    glass_feedback::RuleAction::ExtendSilence { .. }
                                    | glass_feedback::RuleAction::RunVerifyTwice
                                    | glass_feedback::RuleAction::EarlyStuck { .. } => {
                                        // These are flag-based; handled elsewhere
                                    }
                                    glass_feedback::RuleAction::TextInjection(text) => {
                                        feedback_notifications.push(text.clone());
                                    }
                                }
                            }
                        }

                        // Fire scripting OrchestratorIteration hook
                        {
                            let mut event = glass_scripting::HookEventData::new();
                            event.set("iteration", self.orchestrator.iteration as i64);
                            fire_hook_on_bridge(
                                &mut self.script_bridge,
                                &self.orchestrator.project_root,
                                glass_scripting::HookPoint::OrchestratorIteration,
                                &event,
                            );
                        }

                        // If no verification needed, proceed with normal context send
                        let has_nudge = nudge.is_some();
                        let mut content = String::from("[TERMINAL_CONTEXT]\n");
                        if let Some(nudge_text) = nudge {
                            content.push_str(&format!(
                                "[USER_NUDGE] The user left this course correction:\n{}\n\n",
                                nudge_text.trim()
                            ));
                            tracing::info!("Orchestrator: including nudge.md in context");
                        }
                        content.push_str(&context);

                        // Append feedback rule notifications
                        if !feedback_notifications.is_empty() {
                            content.push_str("\n[FEEDBACK_RULES]\n");
                            for instr in &feedback_notifications {
                                content.push_str(&format!("- {}\n", instr));
                            }
                        }

                        // Instruction buffer: if buffered instructions exist, send next one
                        if !self.orchestrator.instruction_buffer.is_empty() {
                            let next = self.orchestrator.instruction_buffer.remove(0);
                            if let Some(ctx_ib) = self.windows.get(&window_id) {
                                if let Some(session_ib) = ctx_ib.session_mux.session(session_id) {
                                    let msg = format!("{}\n", next);
                                    pty_send(
                                        &session_ib.pty_sender,
                                        PtyMsg::Input(std::borrow::Cow::Owned(msg.into_bytes())),
                                    );
                                    self.orchestrator.mark_pty_write();
                                }
                            }
                            self.orchestrator.response_pending = true;
                            tracing::info!(
                                "Orchestrator: sent buffered instruction ({} remaining)",
                                self.orchestrator.instruction_buffer.len()
                            );
                            return;
                        }

                        let msg = serde_json::json!({
                            "type": "user",
                            "message": {
                                "role": "user",
                                "content": content
                            }
                        })
                        .to_string();

                        // Send to agent via shared stdin writer
                        if let Some(ref runtime) = self.agent_runtime {
                            if let Some(ref writer) = runtime.orchestrator_writer {
                                {
                                    let mut w = writer.lock();
                                    let _ = writeln!(w, "{msg}");
                                    let _ = w.flush();
                                    self.orchestrator.response_pending = true;

                                    self.orchestrator_event_buffer.push(
                                        orchestrator_events::OrchestratorEvent::ContextSent {
                                            line_count: lines.len(),
                                            has_soi: soi_summary.is_some(),
                                            has_nudge,
                                        },
                                        self.orchestrator.iteration,
                                    );
                                }
                            }
                        }

                        tracing::debug!(
                            "Orchestrator: sent {} lines of terminal context to agent",
                            lines.len()
                        );
                    }
                }
            }
            AppEvent::VerifyComplete {
                window_id,
                session_id,
                results,
            } => {
                if !self.orchestrator.active {
                    self.orchestrator.response_pending = false;
                    return;
                }

                // Capture combined output for coverage gap analysis before consuming results
                let combined_output: String = results
                    .iter()
                    .map(|r| r.output.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");

                // Convert VerifyEventResult to VerifyResult
                let verify_results: Vec<orchestrator::VerifyResult> = results
                    .into_iter()
                    .map(|r| orchestrator::VerifyResult {
                        command_name: r.command_name,
                        exit_code: r.exit_code,
                        tests_passed: r.tests_passed,
                        tests_failed: r.tests_failed,
                        errors: if r.exit_code != 0 {
                            vec![r.output.lines().take(10).collect::<Vec<_>>().join("\n")]
                        } else {
                            vec![]
                        },
                    })
                    .collect();

                let mut guard_msg: Option<String> = None;

                // Capture first result for orchestrator transcript (before potential move)
                let first_verify_passed = verify_results.first().and_then(|r| r.tests_passed);
                let first_verify_failed = verify_results.first().and_then(|r| r.tests_failed);

                // Get CWD and commit before mutable borrow of orchestrator
                let revert_cwd = self.orchestrator.project_root.clone();
                let revert_commit = self.orchestrator.last_good_commit.clone();

                // Record pass/fail for flaky verification detection
                let all_passed = verify_results.iter().all(|r| r.exit_code == 0);
                self.orchestrator.feedback_verify_sequence.push(all_passed);

                if let Some(ref mut baseline) = self.orchestrator.metric_baseline {
                    // If baseline_results is empty, this is the first run — establish baseline
                    if baseline.baseline_results.is_empty() {
                        baseline.baseline_results = verify_results.clone();
                        baseline.last_results = verify_results.clone();
                        let baseline_desc = verify_results
                            .iter()
                            .map(|r| {
                                let p = r
                                    .tests_passed
                                    .map(|v| v.to_string())
                                    .unwrap_or_else(|| "?".into());
                                let f = r
                                    .tests_failed
                                    .map(|v| v.to_string())
                                    .unwrap_or_else(|| "?".into());
                                format!("{}: {} passed, {} failed", r.command_name, p, f)
                            })
                            .collect::<Vec<_>>()
                            .join("; ");
                        orchestrator::append_iteration_log(
                            &revert_cwd,
                            self.orchestrator.iteration,
                            "verify",
                            "baseline",
                            &format!("Baseline established: {baseline_desc}"),
                        );
                        tracing::info!(
                            "Metric guard: baseline established with {} command(s)",
                            baseline.commands.len()
                        );
                    } else {
                        let regressed = orchestrator::MetricBaseline::check_regression(
                            &baseline.baseline_results,
                            &verify_results,
                        );

                        if regressed {
                            // Revert via git
                            if let Some(ref commit) = revert_commit {
                                let _ = git_cmd()
                                    .args(["reset", "--hard", commit])
                                    .current_dir(&revert_cwd)
                                    .output();
                                tracing::info!("Metric guard: reverted to {commit}");
                            }
                            baseline.revert_count += 1;
                            baseline.last_results = verify_results.clone();

                            // Log revert to iterations.tsv
                            let revert_desc = verify_results
                                .iter()
                                .map(|r| {
                                    let p = r
                                        .tests_passed
                                        .map(|v| v.to_string())
                                        .unwrap_or_else(|| "?".into());
                                    let f = r
                                        .tests_failed
                                        .map(|v| v.to_string())
                                        .unwrap_or_else(|| "?".into());
                                    format!("{}: {} passed, {} failed", r.command_name, p, f)
                                })
                                .collect::<Vec<_>>()
                                .join("; ");
                            orchestrator::append_iteration_log(
                                &revert_cwd,
                                self.orchestrator.iteration,
                                "verify",
                                "revert",
                                &format!(
                                    "Regression detected, reverted to {}: {revert_desc}",
                                    revert_commit.as_deref().unwrap_or("unknown")
                                ),
                            );

                            // Build METRIC_GUARD message for agent context
                            guard_msg = Some(format!(
                                "[METRIC_GUARD] Your changes caused regression:\n{}\nChanges have been reverted. Try a different approach.",
                                verify_results
                                    .iter()
                                    .map(|r| {
                                        let mut s = format!(
                                            "  {}: exit_code={}",
                                            r.command_name, r.exit_code
                                        );
                                        if let (Some(p), Some(f)) =
                                            (r.tests_passed, r.tests_failed)
                                        {
                                            s.push_str(&format!(
                                                ", {} passed, {} failed",
                                                p, f
                                            ));
                                        }
                                        for e in &r.errors {
                                            s.push_str(&format!(
                                                "\n    {}",
                                                e.lines()
                                                    .take(5)
                                                    .collect::<Vec<_>>()
                                                    .join("\n    ")
                                            ));
                                        }
                                        s
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            ));

                            tracing::info!(
                                "Metric guard: regression detected, context updated with METRIC_GUARD"
                            );
                        } else {
                            baseline.update_baseline_if_improved(&verify_results);
                            baseline.keep_count += 1;

                            // Log keep to iterations.tsv
                            let keep_desc = verify_results
                                .iter()
                                .map(|r| {
                                    let p = r
                                        .tests_passed
                                        .map(|v| v.to_string())
                                        .unwrap_or_else(|| "?".into());
                                    let f = r
                                        .tests_failed
                                        .map(|v| v.to_string())
                                        .unwrap_or_else(|| "?".into());
                                    format!("{}: {} passed, {} failed", r.command_name, p, f)
                                })
                                .collect::<Vec<_>>()
                                .join("; ");
                            orchestrator::append_iteration_log(
                                &revert_cwd,
                                self.orchestrator.iteration,
                                "verify",
                                "keep",
                                &keep_desc,
                            );

                            baseline.last_results = verify_results;
                        }
                    }
                }

                // Push VerifyResult to orchestrator transcript
                if first_verify_passed.is_some() || first_verify_failed.is_some() {
                    self.orchestrator_event_buffer.push(
                        orchestrator_events::OrchestratorEvent::VerifyResult {
                            passed: first_verify_passed,
                            failed: first_verify_failed,
                            regressed: guard_msg.is_some(),
                        },
                        self.orchestrator.iteration,
                    );
                }

                // Compute coverage gaps from test output and git diff
                {
                    let changed_files_output = git_cmd()
                        .args(["diff", "--name-only"])
                        .current_dir(&revert_cwd)
                        .output()
                        .ok()
                        .and_then(|o| String::from_utf8(o.stdout).ok())
                        .unwrap_or_default();
                    let changed_files: Vec<String> = changed_files_output
                        .lines()
                        .filter(|l| !l.is_empty())
                        .map(|l| l.to_string())
                        .collect();

                    let gaps = glass_feedback::coverage::find_coverage_gaps(
                        &combined_output,
                        &changed_files,
                    );
                    self.orchestrator.coverage_gaps_context =
                        glass_feedback::coverage::format_gaps_for_context(&gaps);
                }

                // Now send context to agent (with or without METRIC_GUARD prefix)
                if let Some(ctx) = self.windows.get(&window_id) {
                    if let Some(session) = ctx.session_mux.session(session_id) {
                        let lines = extract_term_lines(&session.term, 80);
                        let (exit_code, soi_summary, soi_errors) =
                            fetch_latest_soi_context(session);
                        let mut context = orchestrator::build_orchestrator_context(
                            &lines,
                            exit_code,
                            soi_summary.as_deref(),
                            &soi_errors,
                        );
                        if !self.orchestrator.coverage_gaps_context.is_empty() {
                            context.push_str(&self.orchestrator.coverage_gaps_context);
                        }

                        let mut content = String::from("[TERMINAL_CONTEXT]\n");
                        if let Some(guard) = guard_msg {
                            content.push_str(&guard);
                            content.push('\n');
                        }
                        content.push_str(&context);

                        let msg = serde_json::json!({
                            "type": "user",
                            "message": {
                                "role": "user",
                                "content": content
                            }
                        })
                        .to_string();

                        if let Some(ref runtime) = self.agent_runtime {
                            if let Some(ref writer) = runtime.orchestrator_writer {
                                {
                                    let mut w = writer.lock();
                                    let _ = writeln!(w, "{msg}");
                                    let _ = w.flush();
                                    // Context was sent — mark pending so we wait for the
                                    // agent's response before the next silence fires.
                                    self.orchestrator.response_pending = true;
                                }
                            }
                        }

                        tracing::debug!("Orchestrator: sent context to agent after verification");
                    }
                }

                // Only clear response_pending if no context was sent above
                // (i.e., window/session lookup failed). If context was sent,
                // response_pending was already set to true inside the block.
                if !self.orchestrator.response_pending {
                    self.orchestrator.response_pending = false;
                }

                // Request redraw for status bar update
                for ctx in self.windows.values() {
                    ctx.window.request_redraw();
                }
            }
            AppEvent::UsagePause => {
                tracing::info!("Orchestrator: usage pause triggered (>=80%)");
                {
                    let mut event = glass_scripting::HookEventData::new();
                    event.set("iterations", self.orchestrator.iteration as i64);
                    fire_hook_on_bridge(
                        &mut self.script_bridge,
                        &self.orchestrator.project_root,
                        glass_scripting::HookPoint::OrchestratorRunEnd,
                        &event,
                    );
                }
                self.run_feedback_on_end();
                self.orchestrator.active = false;
                if let Some(handle) = self.artifact_watcher_thread.take() {
                    handle.thread().unpark();
                }
                for ctx in self.windows.values() {
                    ctx.window.request_redraw();
                }
            }
            AppEvent::UsageHardStop => {
                tracing::warn!("Orchestrator: usage hard stop (>=95%)");
                {
                    let mut event = glass_scripting::HookEventData::new();
                    event.set("iterations", self.orchestrator.iteration as i64);
                    fire_hook_on_bridge(
                        &mut self.script_bridge,
                        &self.orchestrator.project_root,
                        glass_scripting::HookPoint::OrchestratorRunEnd,
                        &event,
                    );
                }
                self.run_feedback_on_end();
                self.orchestrator.active = false;
                if let Some(handle) = self.artifact_watcher_thread.take() {
                    handle.thread().unpark();
                }

                // Write emergency checkpoint from Rust (no AI)
                if let Some(ctx) = self.windows.values().next() {
                    if let Some(session) = ctx.session_mux.focused_session() {
                        let lines = extract_term_lines(&session.term, 50);
                        let cwd = session.status.cwd().to_string();
                        let checkpoint = format!(
                            "# Emergency Checkpoint (written by Glass, not AI)\n\
                             Paused at: {}\n\
                             Reason: OAuth usage at 95%+\n\
                             Last terminal lines:\n{}\n\
                             Working directory: {}\n\
                             Resume: run `claude`, then read .glass/checkpoint.md and continue\n",
                            chrono::Utc::now().to_rfc3339(),
                            lines.join("\n"),
                            cwd,
                        );
                        let checkpoint_dir = std::path::Path::new(&cwd).join(".glass");
                        let _ = std::fs::create_dir_all(&checkpoint_dir);
                        let _ = std::fs::write(checkpoint_dir.join("checkpoint.md"), &checkpoint);
                    }
                }

                for ctx in self.windows.values() {
                    ctx.window.request_redraw();
                }
            }
            AppEvent::UsageResume => {
                tracing::info!("Orchestrator: usage resume triggered (<20%)");
                // Don't auto-enable orchestrator — user must toggle with Ctrl+Shift+O
                for ctx in self.windows.values() {
                    ctx.window.request_redraw();
                }
            }
            AppEvent::OrchestratorThinking { text } => {
                let token_estimate = orchestrator_events::estimate_tokens(&text);
                // Truncate thinking text to 2000 chars for storage — full text
                // can be 50KB+ and gets cloned on every overlay redraw.
                let truncated = orchestrator_events::truncate_display(&text, 2000);
                self.orchestrator_event_buffer.push(
                    orchestrator_events::OrchestratorEvent::Thinking {
                        text: truncated,
                        token_estimate,
                    },
                    self.orchestrator.iteration,
                );
                // No request_redraw — cosmetic update, next natural redraw picks it up
            }
            AppEvent::OrchestratorToolCall {
                name,
                params_summary,
            } => {
                self.orchestrator_event_buffer.push(
                    orchestrator_events::OrchestratorEvent::ToolCall {
                        name,
                        params_summary,
                    },
                    self.orchestrator.iteration,
                );
            }
            AppEvent::OrchestratorToolResult {
                name,
                output_summary,
            } => {
                self.orchestrator_event_buffer.push(
                    orchestrator_events::OrchestratorEvent::ToolResult {
                        name,
                        output_summary,
                    },
                    self.orchestrator.iteration,
                );
            }
            // EphemeralAgentComplete handled below (after MCP handlers)
            AppEvent::McpRequest(mcp_req) => {
                let glass_core::ipc::McpEventRequest { request, reply } = mcp_req;
                let response = match request.method.as_str() {
                    "ping" => {
                        glass_core::ipc::McpResponse::ok(request.id, glass_core::ipc::ping_result())
                    }
                    "tab_list" => {
                        if let Some(ctx) = self.windows.values().next() {
                            let active_idx = ctx.session_mux.active_tab_index();
                            let tabs: Vec<serde_json::Value> = ctx
                                .session_mux
                                .tabs()
                                .iter()
                                .enumerate()
                                .map(|(i, tab)| {
                                    let primary_sid = tab.focused_pane;
                                    let (cwd, has_running_command) = if let Some(session) =
                                        ctx.session_mux.session(primary_sid)
                                    {
                                        let cwd = session.status.cwd().to_string();
                                        let running = session
                                            .block_manager
                                            .current_block_index()
                                            .and_then(|idx| session.block_manager.blocks().get(idx))
                                            .map(|b| {
                                                b.state == glass_terminal::BlockState::Executing
                                            })
                                            .unwrap_or(false);
                                        (cwd, running)
                                    } else {
                                        (String::new(), false)
                                    };
                                    serde_json::json!({
                                        "index": i,
                                        "title": tab.title,
                                        "session_id": primary_sid.val(),
                                        "cwd": cwd,
                                        "is_active": i == active_idx,
                                        "has_running_command": has_running_command,
                                        "pane_count": tab.pane_count(),
                                    })
                                })
                                .collect();
                            glass_core::ipc::McpResponse::ok(request.id, serde_json::json!(tabs))
                        } else {
                            glass_core::ipc::McpResponse::err(
                                request.id,
                                "No windows available".into(),
                            )
                        }
                    }
                    "tab_create" => {
                        if let Some(ctx) = self.windows.values_mut().next() {
                            let shell_override = request
                                .params
                                .get("shell")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            let cwd_param = request
                                .params
                                .get("cwd")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            let cwd_path = cwd_param
                                .as_deref()
                                .or_else(|| {
                                    ctx.session_mux.focused_session().map(|s| s.status.cwd())
                                })
                                .map(std::path::PathBuf::from);
                            let session_id = ctx.session_mux.next_session_id();
                            let (cell_w, cell_h) = ctx.frame_renderer.cell_size();
                            let size = ctx.window.inner_size();

                            // If shell override provided, temporarily swap config
                            let mut config_clone;
                            let config_ref = if let Some(ref shell) = shell_override {
                                config_clone = self.config.clone();
                                config_clone.shell = Some(shell.clone());
                                &config_clone
                            } else {
                                &self.config
                            };

                            let window_id = ctx.window.id();
                            match create_session(
                                &self.proxy,
                                window_id,
                                session_id,
                                config_ref,
                                cwd_path.as_deref(),
                                cell_w,
                                cell_h,
                                size.width,
                                size.height,
                                1,
                            ) {
                                Err(e) => {
                                    tracing::error!("PTY spawn failed for MCP tab_create: {e}");
                                    glass_core::ipc::McpResponse::err(
                                        request.id,
                                        format!("PTY spawn failed: {e}"),
                                    )
                                }
                                Ok(session) => {
                                    let tab_id =
                                        ctx.session_mux.add_tab(session, self.orchestrator.active);
                                    let new_tab_index = ctx.session_mux.tab_count() - 1;
                                    {
                                        let mut event = glass_scripting::HookEventData::new();
                                        event.set("tab_index", new_tab_index as i64);
                                        fire_hook_on_bridge(
                                            &mut self.script_bridge,
                                            &self.orchestrator.project_root,
                                            glass_scripting::HookPoint::TabCreate,
                                            &event,
                                        );
                                    }
                                    ctx.window.request_redraw();
                                    glass_core::ipc::McpResponse::ok(
                                        request.id,
                                        serde_json::json!({
                                            "tab_index": new_tab_index,
                                            "session_id": session_id.val(),
                                            "tab_id": tab_id.val(),
                                        }),
                                    )
                                }
                            }
                        } else {
                            glass_core::ipc::McpResponse::err(
                                request.id,
                                "No windows available".into(),
                            )
                        }
                    }
                    "tab_send" => {
                        if let Some(ctx) = self.windows.values().next() {
                            match resolve_tab_index(&ctx.session_mux, &request.params) {
                                Ok(tab_idx) => {
                                    let command = request
                                        .params
                                        .get("command")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    let focused_sid = ctx.session_mux.tabs()[tab_idx].focused_pane;
                                    if let Some(session) = ctx.session_mux.session(focused_sid) {
                                        let input = format!("{}\r", command).into_bytes();
                                        pty_send(
                                            &session.pty_sender,
                                            PtyMsg::Input(Cow::Owned(input)),
                                        );
                                        glass_core::ipc::McpResponse::ok(
                                            request.id,
                                            serde_json::json!({
                                                "sent": true,
                                                "session_id": focused_sid.val(),
                                            }),
                                        )
                                    } else {
                                        glass_core::ipc::McpResponse::err(
                                            request.id,
                                            format!("Session {} not found", focused_sid.val()),
                                        )
                                    }
                                }
                                Err(e) => glass_core::ipc::McpResponse::err(request.id, e),
                            }
                        } else {
                            glass_core::ipc::McpResponse::err(
                                request.id,
                                "No windows available".into(),
                            )
                        }
                    }
                    "tab_output" => {
                        if let Some(ctx) = self.windows.values().next() {
                            match resolve_tab_index(&ctx.session_mux, &request.params) {
                                Ok(tab_idx) => {
                                    let n = request
                                        .params
                                        .get("lines")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(50)
                                        .min(10000)
                                        as usize;
                                    let mode = request
                                        .params
                                        .get("mode")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("tail");
                                    let pattern = request
                                        .params
                                        .get("pattern")
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string());
                                    let focused_sid = ctx.session_mux.tabs()[tab_idx].focused_pane;
                                    if let Some(session) = ctx.session_mux.session(focused_sid) {
                                        // Cap extraction at 10000 to prevent unbounded allocation
                                        let max_extract = n.min(10000);
                                        let mut lines =
                                            extract_term_lines(&session.term, max_extract);
                                        // Apply head/tail slicing
                                        if mode == "head" {
                                            lines.truncate(n);
                                        } else {
                                            let start = lines.len().saturating_sub(n);
                                            lines = lines[start..].to_vec();
                                        }
                                        if let Some(ref pat) = pattern {
                                            match regex::Regex::new(pat) {
                                                Ok(re) => {
                                                    lines.retain(|l| re.is_match(l));
                                                }
                                                Err(e) => {
                                                    let _ = reply.send(
                                                        glass_core::ipc::McpResponse::err(
                                                            request.id,
                                                            format!("Invalid regex: {}", e),
                                                        ),
                                                    );
                                                    return;
                                                }
                                            }
                                        }
                                        let count = lines.len();
                                        glass_core::ipc::McpResponse::ok(
                                            request.id,
                                            serde_json::json!({
                                                "lines": lines,
                                                "line_count": count,
                                                "session_id": focused_sid.val(),
                                            }),
                                        )
                                    } else {
                                        glass_core::ipc::McpResponse::err(
                                            request.id,
                                            format!("Session {} not found", focused_sid.val()),
                                        )
                                    }
                                }
                                Err(e) => glass_core::ipc::McpResponse::err(request.id, e),
                            }
                        } else {
                            glass_core::ipc::McpResponse::err(
                                request.id,
                                "No windows available".into(),
                            )
                        }
                    }
                    "tab_close" => {
                        if let Some(ctx) = self.windows.values_mut().next() {
                            if ctx.session_mux.tab_count() <= 1 {
                                glass_core::ipc::McpResponse::err(
                                    request.id,
                                    "Cannot close the last tab".into(),
                                )
                            } else {
                                match resolve_tab_index(&ctx.session_mux, &request.params) {
                                    Ok(tab_idx) => {
                                        {
                                            let mut event = glass_scripting::HookEventData::new();
                                            event.set("tab_index", tab_idx as i64);
                                            fire_hook_on_bridge(
                                                &mut self.script_bridge,
                                                &self.orchestrator.project_root,
                                                glass_scripting::HookPoint::TabClose,
                                                &event,
                                            );
                                        }
                                        if let Some(session) = ctx.session_mux.close_tab(tab_idx) {
                                            cleanup_session(session);
                                        }
                                        let remaining = ctx.session_mux.tab_count();
                                        ctx.window.request_redraw();
                                        glass_core::ipc::McpResponse::ok(
                                            request.id,
                                            serde_json::json!({
                                                "closed": true,
                                                "remaining_tabs": remaining,
                                            }),
                                        )
                                    }
                                    Err(e) => glass_core::ipc::McpResponse::err(request.id, e),
                                }
                            }
                        } else {
                            glass_core::ipc::McpResponse::err(
                                request.id,
                                "No windows available".into(),
                            )
                        }
                    }
                    "has_running_command" => {
                        if let Some(ctx) = self.windows.values().next() {
                            match resolve_tab_index(&ctx.session_mux, &request.params) {
                                Ok(tab_idx) => {
                                    let focused_sid = ctx.session_mux.tabs()[tab_idx].focused_pane;
                                    if let Some(session) = ctx.session_mux.session(focused_sid) {
                                        let (is_running, elapsed_seconds) = session
                                            .block_manager
                                            .current_block_index()
                                            .and_then(|idx| session.block_manager.blocks().get(idx))
                                            .filter(|b| {
                                                b.state == glass_terminal::BlockState::Executing
                                            })
                                            .map(|b| {
                                                let elapsed = b
                                                    .started_at
                                                    .map(|s| s.elapsed().as_secs_f64())
                                                    .unwrap_or(0.0);
                                                (true, Some(elapsed))
                                            })
                                            .unwrap_or((false, None));
                                        glass_core::ipc::McpResponse::ok(
                                            request.id,
                                            serde_json::json!({
                                                "is_running": is_running,
                                                "elapsed_seconds": elapsed_seconds,
                                                "session_id": focused_sid.val(),
                                            }),
                                        )
                                    } else {
                                        glass_core::ipc::McpResponse::err(
                                            request.id,
                                            format!("Session {} not found", focused_sid.val()),
                                        )
                                    }
                                }
                                Err(e) => glass_core::ipc::McpResponse::err(request.id, e),
                            }
                        } else {
                            glass_core::ipc::McpResponse::err(
                                request.id,
                                "No windows available".into(),
                            )
                        }
                    }
                    "cancel_command" => {
                        if let Some(ctx) = self.windows.values().next() {
                            match resolve_tab_index(&ctx.session_mux, &request.params) {
                                Ok(tab_idx) => {
                                    let focused_sid = ctx.session_mux.tabs()[tab_idx].focused_pane;
                                    if let Some(session) = ctx.session_mux.session(focused_sid) {
                                        let was_running = session
                                            .block_manager
                                            .current_block_index()
                                            .and_then(|idx| session.block_manager.blocks().get(idx))
                                            .map(|b| {
                                                b.state == glass_terminal::BlockState::Executing
                                            })
                                            .unwrap_or(false);
                                        // Send ETX byte (Ctrl+C) to PTY
                                        let input = vec![0x03u8];
                                        pty_send(
                                            &session.pty_sender,
                                            PtyMsg::Input(Cow::Owned(input)),
                                        );
                                        glass_core::ipc::McpResponse::ok(
                                            request.id,
                                            serde_json::json!({
                                                "signal_sent": true,
                                                "was_running": was_running,
                                                "session_id": focused_sid.val(),
                                            }),
                                        )
                                    } else {
                                        glass_core::ipc::McpResponse::err(
                                            request.id,
                                            format!("Session {} not found", focused_sid.val()),
                                        )
                                    }
                                }
                                Err(e) => glass_core::ipc::McpResponse::err(request.id, e),
                            }
                        } else {
                            glass_core::ipc::McpResponse::err(
                                request.id,
                                "No windows available".into(),
                            )
                        }
                    }
                    "script_tool" => {
                        let tool_name = request
                            .params
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let tool_params = request
                            .params
                            .get("params")
                            .cloned()
                            .unwrap_or(serde_json::json!({}));
                        match self.script_bridge.run_script_tool(tool_name, tool_params) {
                            Ok(result) => glass_core::ipc::McpResponse::ok(request.id, result),
                            Err(e) => glass_core::ipc::McpResponse::err(request.id, e),
                        }
                    }
                    "list_script_tools" => {
                        let tools = self.script_bridge.list_script_tools();
                        glass_core::ipc::McpResponse::ok(
                            request.id,
                            serde_json::json!({"tools": tools}),
                        )
                    }
                    _ => glass_core::ipc::McpResponse::err(
                        request.id,
                        format!("Unknown method: {}", request.method),
                    ),
                };
                let _ = reply.send(response);
            }
            AppEvent::EphemeralAgentComplete { result, purpose } => {
                tracing::debug!(
                    "EphemeralAgentComplete: purpose={purpose:?} ok={}",
                    result.is_ok()
                );
                match purpose {
                    glass_core::event::EphemeralPurpose::CheckpointSynthesis => {
                        let cwd = self.orchestrator.project_root.clone();
                        match result {
                            Ok(resp) => {
                                if let Some(cost) = resp.cost_usd {
                                    tracing::info!("Checkpoint synthesis cost: ${:.4}", cost);
                                }
                                orchestrator::append_iteration_log(
                                    &cwd,
                                    self.orchestrator.iteration,
                                    "checkpoint",
                                    "ephemeral",
                                    "synthesis complete",
                                );
                                self.write_checkpoint_content_and_respawn(&cwd, &resp.text);
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Checkpoint synthesis failed: {e:?}, using fallback"
                                );
                                self.write_checkpoint_and_respawn(&cwd);
                            }
                        }
                    }
                    glass_core::event::EphemeralPurpose::QualityVerification => {
                        if let Ok(resp) = result {
                            if let Ok(verdict) =
                                glass_feedback::quality::parse_quality_verdict(&resp.text)
                            {
                                tracing::info!(
                                    "Quality verdict: score={}/10, completeness={:.0}%, gaps={}, regressed={}",
                                    verdict.score,
                                    verdict.completeness * 100.0,
                                    verdict.gaps.len(),
                                    verdict.regressed
                                );

                                // Store score for next comparison
                                self.orchestrator.last_quality_score = Some(verdict.score);

                                // Append quality context for the agent
                                let mut quality_ctx = format!(
                                    "[QUALITY_CHECK] score={}/10 completeness={:.0}%",
                                    verdict.score,
                                    verdict.completeness * 100.0
                                );
                                if !verdict.gaps.is_empty() {
                                    quality_ctx.push_str(" gaps: ");
                                    quality_ctx.push_str(&verdict.gaps.join("; "));
                                }
                                if verdict.regressed {
                                    quality_ctx.push_str(" [REGRESSED from previous checkpoint]");
                                }
                                quality_ctx.push('\n');
                                self.orchestrator
                                    .coverage_gaps_context
                                    .push_str(&quality_ctx);

                                // Log to iterations.tsv
                                let cwd = self.orchestrator.project_root.clone();
                                let status = if verdict.regressed {
                                    "quality_regressed"
                                } else {
                                    "quality_ok"
                                };
                                orchestrator::append_iteration_log(
                                    &cwd,
                                    self.orchestrator.iteration,
                                    "quality",
                                    status,
                                    &format!(
                                        "score={} completeness={:.0}% gaps={}",
                                        verdict.score,
                                        verdict.completeness * 100.0,
                                        verdict.gaps.len()
                                    ),
                                );
                            }
                        }
                    }
                    glass_core::event::EphemeralPurpose::FeedbackAnalysis => {
                        // Use the project root captured at spawn time, not the
                        // current one — the user may have switched projects.
                        let project_root = self
                            .feedback_llm_project_root
                            .take()
                            .unwrap_or_else(|| self.orchestrator.project_root.clone());
                        let max_hints = self.feedback_llm_max_hints;
                        match result {
                            Ok(resp) => {
                                if let Some(cost) = resp.cost_usd {
                                    tracing::info!("Feedback LLM cost: ${:.4}", cost);
                                }
                                glass_feedback::apply_llm_findings(
                                    &project_root,
                                    &resp.text,
                                    max_hints,
                                );
                            }
                            Err(e) => {
                                tracing::warn!("Feedback LLM failed: {e:?}");
                            }
                        }
                    }
                    glass_core::event::EphemeralPurpose::ScriptGeneration => {
                        let project_root = self
                            .script_gen_project_root
                            .take()
                            .unwrap_or_else(|| self.orchestrator.project_root.clone());
                        match result {
                            Ok(resp) => {
                                if let Some(cost) = resp.cost_usd {
                                    tracing::info!("Tier 4 script generation cost: ${:.4}", cost);
                                }
                                match parse_script_response(&resp.text) {
                                    Some((name, hooks, source)) => {
                                        // Successful parse — reset consecutive failure counter.
                                        self.script_gen_parse_failures = 0;
                                        let scripts_dir = std::path::Path::new(&project_root)
                                            .join(".glass")
                                            .join("scripts")
                                            .join("feedback");
                                        let _ = std::fs::create_dir_all(&scripts_dir);

                                        // Deduplicate: skip if a non-archived manifest already exists.
                                        // Only archived scripts may be overwritten by a new generation.
                                        let manifest_path =
                                            scripts_dir.join(format!("{name}.toml"));
                                        let should_write = if manifest_path.exists() {
                                            match glass_scripting::lifecycle::read_manifest(&manifest_path) {
                                                Ok(existing) if existing.status != glass_scripting::ScriptStatus::Archived => {
                                                    tracing::info!(
                                                        "Tier 4: script '{name}' already exists (status={:?}), skipping",
                                                        existing.status
                                                    );
                                                    false
                                                }
                                                _ => true, // Archived or unreadable — safe to overwrite
                                            }
                                        } else {
                                            true
                                        };

                                        if should_write {
                                            // Write TOML manifest
                                            let now_secs = std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_secs();
                                            let manifest = format!(
                                                "name = \"{name}\"\nhooks = [{hooks}]\nstatus = \"provisional\"\norigin = \"feedback\"\nversion = 1\napi_version = \"1\"\ncreated = \"{now_secs}\"\ntype = \"hook\"\n"
                                            );
                                            let _ = std::fs::write(
                                                scripts_dir.join(format!("{name}.toml")),
                                                &manifest,
                                            );
                                            let _ = std::fs::write(
                                                scripts_dir.join(format!("{name}.rhai")),
                                                &source,
                                            );
                                            tracing::info!(
                                                "Tier 4: wrote provisional script '{name}'"
                                            );
                                            self.script_bridge.reload();
                                        }
                                    }
                                    None => {
                                        self.script_gen_parse_failures += 1;
                                        tracing::warn!(
                                            "Tier 4: could not parse script from LLM response (consecutive failures: {})",
                                            self.script_gen_parse_failures
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Tier 4 script generation failed: {e:?}");
                            }
                        }
                    }
                }
            }
        }
    }

    /// Called when the event queue is drained. No-op for Phase 1.
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {}
}

/// Parse a Tier 4 script generation response from an ephemeral LLM agent.
///
/// Expected format:
/// ```text
/// SCRIPT_NAME: my-script-name
/// SCRIPT_HOOKS: command_complete, orchestrator_iteration
/// ```rhai
/// // ... Rhai source code ...
/// ```
/// ```
///
/// Returns `(name, hooks_csv_quoted, source)` on success.
fn parse_script_response(text: &str) -> Option<(String, String, String)> {
    let name = text
        .lines()
        .find(|l| l.starts_with("SCRIPT_NAME:"))
        .map(|l| l.trim_start_matches("SCRIPT_NAME:").trim().to_string())?;
    let hooks_raw = text
        .lines()
        .find(|l| l.starts_with("SCRIPT_HOOKS:"))
        .map(|l| l.trim_start_matches("SCRIPT_HOOKS:").trim().to_string())?;
    let hooks = hooks_raw
        .split(',')
        .map(|h| format!("\"{}\"", h.trim()))
        .collect::<Vec<_>>()
        .join(", ");
    let source_start = text.find("```rhai").map(|i| i + 7)?;
    let source_end = text[source_start..].find("```").map(|i| source_start + i)?;
    let source = text[source_start..source_end].trim().to_string();
    if name.is_empty() || source.is_empty() {
        return None;
    }
    Some((name, hooks, source))
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
    let script_name =
        if shell_name.contains("pwsh") || shell_name.to_lowercase().contains("powershell") {
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
            pty_send(sender, PtyMsg::Input(Cow::Owned(bytes)));
        }
    }
}

/// Load the app icon PNG from the assets directory and convert to a winit Icon.
///
/// Returns `None` if the icon file is missing or malformed — the app still
/// launches, just without a custom icon.
fn load_window_icon() -> Option<winit::window::Icon> {
    let icon_bytes = include_bytes!("../assets/icon.png");
    let img = image::load_from_memory_with_format(icon_bytes, image::ImageFormat::Png).ok()?;
    let rgba = img.into_rgba8();
    let (width, height) = rgba.dimensions();
    winit::window::Icon::from_rgba(rgba.into_raw(), width, height).ok()
}

/// Install a panic hook that writes crash reports to ~/.glass/crash.log and
/// opens a pre-filled GitHub issue in the browser.
fn install_crash_handler() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Capture backtrace immediately
        let backtrace = std::backtrace::Backtrace::force_capture();

        // Extract panic message
        let message = if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".to_string()
        };

        // Location
        let location = if let Some(loc) = info.location() {
            format!("{}:{}:{}", loc.file(), loc.line(), loc.column())
        } else {
            "unknown location".to_string()
        };

        let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

        let version = env!("CARGO_PKG_VERSION");
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;
        let backtrace_str = format!("{}", backtrace);

        // Build crash report
        let report = format!(
            "=== CRASH REPORT ===\n\
             Time: {}\n\
             Version: {}\n\
             OS: {} {}\n\
             Panic: {}\n\
             Location: {}\n\
             \n\
             Backtrace:\n\
             {}\n\
             ====================\n\n",
            timestamp, version, os, arch, message, location, backtrace_str
        );

        // Write to ~/.glass/crash.log (append mode)
        if let Some(home) = dirs::home_dir() {
            let glass_dir = home.join(".glass");
            let _ = std::fs::create_dir_all(&glass_dir);
            let crash_log = glass_dir.join("crash.log");
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&crash_log)
            {
                use std::io::Write;
                let _ = file.write_all(report.as_bytes());
            }
        }

        // Print user-friendly message to stderr
        eprintln!("Glass crashed. Log saved to ~/.glass/crash.log");

        // Build GitHub issue URL
        let title = format!("crash: {}", &message);
        let body = if report.len() > 2000 {
            &report[..2000]
        } else {
            &report
        };
        let encoded_title = url_encode(&title);
        let encoded_body = url_encode(body);
        let url = format!(
            "https://github.com/candyhunterz/Glass/issues/new?title={}&body={}&labels=bug,crash",
            encoded_title, encoded_body
        );

        // Open in browser
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("cmd")
                .args(["/C", "start", "", &url])
                .creation_flags(0x08000000) // CREATE_NO_WINDOW
                .spawn();
        }
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(&url).spawn();
        }
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
        }

        // Run the default panic hook (prints the panic info)
        default_hook(info);
    }));
}

/// Percent-encode a string for use in a URL query parameter.
fn url_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            b' ' => result.push_str("%20"),
            _ => {
                result.push('%');
                result.push(char::from(b"0123456789ABCDEF"[(byte >> 4) as usize]));
                result.push(char::from(b"0123456789ABCDEF"[(byte & 0x0F) as usize]));
            }
        }
    }
    result
}

/// Emit an observation event to the coordination event log.
/// No-op if agent runtime has no project root or if DB access fails.
fn emit_observe_event(agent_runtime: &Option<AgentRuntime>, event_type: &str, summary: &str) {
    let project = match agent_runtime.as_ref().and_then(|r| r.project_root.as_ref()) {
        Some(p) => p.clone(),
        None => return,
    };
    if let Ok(db) = glass_coordination::CoordinationDb::open_default() {
        let _ = glass_coordination::event_log::insert_event(
            db.conn(),
            &project,
            "observe",
            None,
            Some("agent-mode"),
            event_type,
            summary,
            None,
            false,
        );
    }
}

/// Emit a command context event to the coordination event log.
/// No-op if agent runtime has no project root or if DB access fails.
fn emit_command_event(agent_runtime: &Option<AgentRuntime>, event_type: &str, summary: &str) {
    let project = match agent_runtime.as_ref().and_then(|r| r.project_root.as_ref()) {
        Some(p) => p.clone(),
        None => return,
    };
    if let Ok(db) = glass_coordination::CoordinationDb::open_default() {
        let _ = glass_coordination::event_log::insert_event(
            db.conn(),
            &project,
            "command",
            None,
            None,
            event_type,
            summary,
            None,
            false,
        );
    }
}

fn main() {
    install_crash_handler();

    let cold_start = std::time::Instant::now();

    // FIRST: set UTF-8 console code page on Windows before any PTY creation (Pitfall 5)
    #[cfg(target_os = "windows")]
    unsafe {
        use windows_sys::Win32::System::Console::{SetConsoleCP, SetConsoleOutputCP};
        SetConsoleCP(65001);
        SetConsoleOutputCP(65001);
    }

    // Default to the user's home directory when launched without a terminal
    // (e.g. double-clicking glass.exe) so the shell starts in ~/ instead of
    // wherever the binary happens to live.
    if let Some(home) = dirs::home_dir() {
        let _ = std::env::set_current_dir(&home);
    }

    // Parse CLI BEFORE creating the event loop — subcommands must not open a window.
    // Tracing is initialized per-branch: MCP mode writes to stderr (stdout is JSON-RPC),
    // while terminal mode uses the default stdout writer.
    let cli = Cli::parse();

    match cli.command {
        None => {
            // Initialize structured logging for terminal mode.
            // When the `perf` feature is enabled, also write a Chrome trace file
            // (glass-trace.json) for visualization in Perfetto / chrome://tracing.
            #[cfg(feature = "perf")]
            let _trace_guard = {
                use tracing_chrome::ChromeLayerBuilder;
                use tracing_subscriber::prelude::*;
                let (chrome_layer, guard) = ChromeLayerBuilder::new()
                    .file("glass-trace.json".to_string())
                    .build();
                tracing_subscriber::registry()
                    .with(chrome_layer)
                    .with(
                        tracing_subscriber::fmt::layer()
                            .with_filter(tracing_subscriber::EnvFilter::from_default_env()),
                    )
                    .init();
                guard // must outlive program -- stored in _trace_guard
            };

            #[cfg(not(feature = "perf"))]
            {
                tracing_subscriber::fmt()
                    .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
                    .init();
            }

            // No subcommand: launch the terminal GUI (default behavior)
            tracing::info!("Glass starting");

            let config = GlassConfig::load();
            tracing::info!(
                "Config: font_family={}, font_size={}, shell={:?}",
                config.font_family,
                config.font_size,
                config.shell
            );

            let event_loop = EventLoop::<AppEvent>::with_user_event()
                .build()
                .unwrap_or_else(|e| show_fatal_error(&format!("Failed to create event loop: {e}")));

            // Create proxy BEFORE run_app() — EventLoopProxy<AppEvent> is Clone + Send,
            // so the PTY EventProxy stores a clone of this.
            let proxy = event_loop.create_proxy();

            // Windows: create Job Object early so child processes inherit it
            #[cfg(target_os = "windows")]
            let job_object_handle = setup_windows_job_object();

            let orch_max_retries = config
                .agent
                .as_ref()
                .and_then(|a| a.orchestrator.as_ref())
                .map(|o| o.max_retries_before_stuck)
                .unwrap_or(3);

            let script_bridge = script_bridge::ScriptBridge::new(&config);

            let mut processor = Processor {
                windows: HashMap::new(),
                proxy,
                modifiers: ModifiersState::empty(),
                config,
                cold_start,
                config_error: None,
                watcher_spawned: false,
                update_info: None,
                coordination_state: Default::default(),
                last_ticker_event_id: None,
                ticker_display_cycles: 0,
                activity_stream_tx: None,
                activity_stream_rx: None,
                activity_filter: glass_core::activity_stream::ActivityFilter::new(
                    glass_core::activity_stream::ActivityStreamConfig::default(),
                ),
                agent_runtime: None,
                agent_generation: 0,
                orchestrator: orchestrator::OrchestratorState::new(orch_max_retries),
                usage_state: None,
                agent_cost_usd: 0.0,
                agent_proposals_paused: false,
                worktree_manager: {
                    match glass_agent::WorktreeManager::new_default() {
                        Ok(wm) => {
                            if let Err(e) = wm.prune_orphans() {
                                tracing::warn!("Failed to prune orphan worktrees: {e}");
                            }
                            Some(wm)
                        }
                        Err(e) => {
                            tracing::warn!("Failed to initialize WorktreeManager: {e}");
                            None
                        }
                    }
                },
                agent_proposal_worktrees: Vec::new(),
                active_toast: None,
                activity_overlay_visible: false,
                activity_view_filter: Default::default(),
                activity_scroll_offset: 0,
                activity_verbose: false,
                orchestrator_event_buffer: orchestrator_events::OrchestratorEventBuffer::new(),
                orchestrator_scroll_offset: 0,
                orchestrator_activated_at: None,
                file_verify_baseline: orchestrator::FileVerifyBaseline::new(),
                settings_overlay_visible: false,
                settings_overlay_tab: Default::default(),
                settings_section_index: 0,
                settings_field_index: 0,
                settings_editing: false,
                settings_edit_buffer: String::new(),
                settings_shortcuts_scroll: 0,
                agent_review_open: false,
                proposal_review_selected: 0,
                proposal_diff_cache: None,
                #[cfg(target_os = "windows")]
                job_object_handle,
                artifact_watcher_thread: None,
                feedback_state: None,
                feedback_write_pending: false,
                config_write_suppress_until: None,
                feedback_llm_project_root: None,
                feedback_llm_max_hints: 10,
                script_gen_project_root: None,
                script_bridge,
                script_gen_parse_failures: 0,
            };

            if let Err(e) = event_loop.run_app(&mut processor) {
                show_fatal_error(&format!("Event loop error: {e}"));
            }
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
                            println!(
                                "Undo complete for command {} ({:?} confidence):",
                                command_id, result.confidence
                            );
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
        Some(Commands::Mcp {
            action: McpAction::Serve,
        }) => {
            // MCP server mode: logging goes to stderr, stdout is reserved for JSON-RPC
            tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
                .with_writer(std::io::stderr)
                .with_ansi(false)
                .init();

            let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
            if let Err(e) = rt.block_on(glass_mcp::run_mcp_server()) {
                eprintln!("MCP server error: {}", e);
                std::process::exit(1);
            }
        }
        Some(Commands::Profile { action }) => {
            tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
                .init();

            match action {
                ProfileAction::Export {
                    name,
                    scripts_dir,
                    output,
                    glass_version,
                    tech_stack,
                } => {
                    let scripts_path = match scripts_dir {
                        Some(p) => std::path::PathBuf::from(p),
                        None => dirs::home_dir()
                            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))
                            .unwrap_or_else(|e| {
                                eprintln!("Error: {e}");
                                std::process::exit(1);
                            })
                            .join(".glass")
                            .join("scripts"),
                    };
                    let output_path = std::path::PathBuf::from(&output);

                    match glass_scripting::profile::export_profile(
                        &name,
                        &scripts_path,
                        &output_path,
                        &glass_version,
                        tech_stack,
                    ) {
                        Ok(()) => {
                            println!("Profile '{}' exported to {}", name, output);
                        }
                        Err(e) => {
                            eprintln!("Export failed: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                ProfileAction::Import { path, target } => {
                    let profile_path = std::path::PathBuf::from(&path);
                    let target_path = match target {
                        Some(p) => std::path::PathBuf::from(p),
                        None => dirs::home_dir()
                            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))
                            .unwrap_or_else(|e| {
                                eprintln!("Error: {e}");
                                std::process::exit(1);
                            })
                            .join(".glass")
                            .join("scripts"),
                    };

                    match glass_scripting::profile::import_profile(&profile_path, &target_path) {
                        Ok(result) => {
                            println!(
                                "Import complete: {} imported, {} skipped",
                                result.scripts_imported, result.scripts_skipped
                            );
                        }
                        Err(e) => {
                            eprintln!("Import failed: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
            }
        }
    }
}

/// Handle Enter/Space on a settings field: toggles booleans, cycles enums.
/// Returns (section, key, new_value) if a config write is needed.
fn handle_settings_activate(
    config: &glass_core::config::GlassConfig,
    section_index: usize,
    field_index: usize,
) -> Option<(Option<&'static str>, &'static str, String)> {
    match (section_index, field_index) {
        // Agent Mode: enabled (toggle mode Off <-> Watch)
        (1, 0) => {
            let current = config.agent.as_ref().map(|a| a.mode).unwrap_or_default();
            let new_mode = if current == glass_core::agent_runtime::AgentMode::Off {
                "\"Watch\""
            } else {
                "\"Off\""
            };
            Some((Some("agent"), "mode", new_mode.to_string()))
        }
        // Agent Mode: mode (cycle Watch -> Assist -> Autonomous -> Off)
        (1, 1) => {
            let current = config.agent.as_ref().map(|a| a.mode).unwrap_or_default();
            let new_mode = match current {
                glass_core::agent_runtime::AgentMode::Off => "\"Watch\"",
                glass_core::agent_runtime::AgentMode::Watch => "\"Assist\"",
                glass_core::agent_runtime::AgentMode::Assist => "\"Autonomous\"",
                glass_core::agent_runtime::AgentMode::Autonomous => "\"Off\"",
            };
            Some((Some("agent"), "mode", new_mode.to_string()))
        }
        // SOI: enabled
        (2, 0) => {
            let current = config.soi.as_ref().map(|s| s.enabled).unwrap_or(true);
            Some((Some("soi"), "enabled", (!current).to_string()))
        }
        // SOI: shell_summary
        (2, 1) => {
            let current = config
                .soi
                .as_ref()
                .map(|s| s.shell_summary)
                .unwrap_or(false);
            Some((Some("soi"), "shell_summary", (!current).to_string()))
        }
        // Snapshots: enabled
        (3, 0) => {
            let current = config.snapshot.as_ref().map(|s| s.enabled).unwrap_or(true);
            Some((Some("snapshot"), "enabled", (!current).to_string()))
        }
        // Pipes: enabled
        (4, 0) => {
            let current = config.pipes.as_ref().map(|p| p.enabled).unwrap_or(true);
            Some((Some("pipes"), "enabled", (!current).to_string()))
        }
        // Pipes: auto_expand
        (4, 1) => {
            let current = config.pipes.as_ref().map(|p| p.auto_expand).unwrap_or(true);
            Some((Some("pipes"), "auto_expand", (!current).to_string()))
        }
        // Orchestrator: enabled
        (6, 0) => {
            let current = config
                .agent
                .as_ref()
                .and_then(|a| a.orchestrator.as_ref())
                .map(|o| o.enabled)
                .unwrap_or(false);
            Some((
                Some("agent.orchestrator"),
                "enabled",
                (!current).to_string(),
            ))
        }
        // Orchestrator: feedback_llm toggle (field index 6)
        (6, 6) => {
            let current = config
                .agent
                .as_ref()
                .and_then(|a| a.orchestrator.as_ref())
                .map(|o| o.feedback_llm)
                .unwrap_or(false);
            Some((
                Some("agent.orchestrator"),
                "feedback_llm",
                (!current).to_string(),
            ))
        }
        // Orchestrator: ablation_enabled toggle (field index 8)
        (6, 8) => {
            let current = config
                .agent
                .as_ref()
                .and_then(|a| a.orchestrator.as_ref())
                .map(|o| o.ablation_enabled)
                .unwrap_or(true);
            Some((
                Some("agent.orchestrator"),
                "ablation_enabled",
                (!current).to_string(),
            ))
        }
        // Orchestrator: verify_mode and orchestrator_mode removed (auto-detected)
        _ => None,
    }
}

/// Handle +/- on a settings number field.
/// Returns (section, key, new_value) if a config write is needed.
fn handle_settings_increment(
    config: &glass_core::config::GlassConfig,
    section_index: usize,
    field_index: usize,
    increment: bool,
) -> Option<(Option<&'static str>, &'static str, String)> {
    let delta: i64 = if increment { 1 } else { -1 };
    match (section_index, field_index) {
        // Font size: step 0.5
        (0, 1) => {
            let current = config.font_size;
            let new_val = (current + delta as f32 * 0.5).clamp(6.0, 72.0);
            Some((None, "font_size", format!("{:.1}", new_val)))
        }
        // Agent budget: step 0.50
        (1, 2) => {
            let current = config
                .agent
                .as_ref()
                .map(|a| a.max_budget_usd)
                .unwrap_or(1.0);
            let new_val = (current + delta as f64 * 0.5).max(0.0);
            Some((Some("agent"), "max_budget_usd", format!("{:.2}", new_val)))
        }
        // Agent cooldown: step 5
        (1, 3) => {
            let current = config.agent.as_ref().map(|a| a.cooldown_secs).unwrap_or(30) as i64;
            let new_val = (current + delta * 5).max(0);
            Some((Some("agent"), "cooldown_secs", new_val.to_string()))
        }
        // SOI min_lines: step 1
        (2, 2) => {
            let current = config.soi.as_ref().map(|s| s.min_lines).unwrap_or(0) as i64;
            let new_val = (current + delta).max(0);
            Some((Some("soi"), "min_lines", new_val.to_string()))
        }
        // Snapshot max_mb: step 100
        (3, 1) => {
            let current = config
                .snapshot
                .as_ref()
                .map(|s| s.max_size_mb)
                .unwrap_or(500) as i64;
            let new_val = (current + delta * 100).max(100);
            Some((Some("snapshot"), "max_size_mb", new_val.to_string()))
        }
        // Snapshot retention_days: step 1
        (3, 2) => {
            let current = config
                .snapshot
                .as_ref()
                .map(|s| s.retention_days)
                .unwrap_or(30) as i64;
            let new_val = (current + delta).max(1);
            Some((Some("snapshot"), "retention_days", new_val.to_string()))
        }
        // Pipes max_capture_mb: step 1
        (4, 2) => {
            let current = config
                .pipes
                .as_ref()
                .map(|p| p.max_capture_mb)
                .unwrap_or(10) as i64;
            let new_val = (current + delta).max(1);
            Some((Some("pipes"), "max_capture_mb", new_val.to_string()))
        }
        // History max_output_kb: step 10
        (5, 0) => {
            let current = config
                .history
                .as_ref()
                .map(|h| h.max_output_capture_kb)
                .unwrap_or(50) as i64;
            let new_val = (current + delta * 10).max(10);
            Some((
                Some("history"),
                "max_output_capture_kb",
                new_val.to_string(),
            ))
        }
        // Orchestrator max_iterations: step 10 (field index 1)
        (6, 1) => {
            let current = config
                .agent
                .as_ref()
                .and_then(|a| a.orchestrator.as_ref())
                .and_then(|o| o.max_iterations)
                .unwrap_or(0) as i64;
            let new_val = (current + delta * 10).max(0);
            Some((
                Some("agent.orchestrator"),
                "max_iterations",
                new_val.to_string(),
            ))
        }
        // Orchestrator silence_timeout_secs: step 10 (field index 2)
        (6, 2) => {
            let current = config
                .agent
                .as_ref()
                .and_then(|a| a.orchestrator.as_ref())
                .map(|o| o.silence_timeout_secs)
                .unwrap_or(60) as i64;
            let new_val = (current + delta * 10).clamp(10, 300);
            Some((
                Some("agent.orchestrator"),
                "silence_timeout_secs",
                new_val.to_string(),
            ))
        }
        // Orchestrator max_prompt_hints: step 1 (field index 7)
        (6, 7) => {
            let current = config
                .agent
                .as_ref()
                .and_then(|a| a.orchestrator.as_ref())
                .map(|o| o.max_prompt_hints)
                .unwrap_or(10) as i64;
            let new_val = (current + delta).clamp(0, 50);
            Some((
                Some("agent.orchestrator"),
                "max_prompt_hints",
                new_val.to_string(),
            ))
        }
        // Orchestrator: ablation_sweep_interval: step 5 (field index 9)
        (6, 9) => {
            let current = config
                .agent
                .as_ref()
                .and_then(|a| a.orchestrator.as_ref())
                .map(|o| o.ablation_sweep_interval)
                .unwrap_or(20) as i64;
            let new_val = (current + delta * 5).clamp(5, 100);
            Some((
                Some("agent.orchestrator"),
                "ablation_sweep_interval",
                new_val.to_string(),
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
