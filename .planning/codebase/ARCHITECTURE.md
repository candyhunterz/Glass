# Architecture

**Analysis Date:** 2026-03-18

## Pattern Overview

**Overall:** Multi-layered event-driven GPU-accelerated terminal with shell integration awareness and autonomous feedback loop.

**Key Characteristics:**
- **Event-driven main loop** — winit event loop consuming AppEvents (PTY output, OSC events, config changes, agent messages) and updating UI state
- **Layered crate architecture** — Core (events/config), Terminal (PTY/shell integration), Renderer (GPU), Mux (sessions), History (SQLite), Snapshots (file versioning), Orchestrator (autonomous agent feedback loop), Scripting (Rhai-based automation)
- **Async/multithreaded** — PTY reader threads, config watchers, agent subprocesses, coordination pollers all communicate via channels to main event loop
- **Safety-first snapshots** — Content-addressed blob store with SQLite metadata enables command-level undo via file state restoration
- **Shell integration OSC sequences** — Glass.bash/zsh/fish/ps1 emit OSC 133 boundaries and OSC 133;P pipeline stage markers for structured command awareness
- **GPU rendering pipeline** — wgpu 28 + glyphon 0.10 for text, custom block-based UI (tabs, status bar, overlays) on terminal grid
- **Orchestrator + feedback loop** — Agent subprocess running Claude Code, silence detection triggers proposal generation, metric guards reject regressions, multi-tier feedback (config, rules, prompts, scripts) auto-improves over time

## Layers

**Presentation Layer:**
- Purpose: Render terminal grid, UI overlays, and interactive elements to GPU surface
- Location: `crates/glass_renderer/`, `src/main.rs` (frame composition loop)
- Contains: Frame renderer, block renderer, tab bar, status bar, search/proposal/activity overlays, scrollbar hit detection
- Depends on: GPU surface (wgpu), glyph cache (glyphon), Session grid snapshots, BlockManager state
- Used by: Processor event handler on each frame (after terminal updates or input)

**Session Multiplexer Layer:**
- Purpose: Manage tabs, split panes, and focus/selection across multiple terminal sessions
- Location: `crates/glass_mux/`
- Contains: SessionMux (tab management), SplitNode (binary pane tree), ViewportLayout (geometry), Tab/Session abstractions
- Depends on: Terminal layer (PTY/grid), platform (shell detection, config dirs)
- Used by: Processor to route PTY messages and render multiple panes

**Terminal / PTY Layer:**
- Purpose: Spawn shell via platform PTY (ConPTY/forkpty), manage shell integration, parse OSC sequences into command blocks
- Location: `crates/glass_terminal/` — submodules: `pty.rs`, `block_manager.rs`, `osc_scanner.rs`, `grid_snapshot.rs`
- Contains:
  - `pty.rs` — PTY spawning, shell integration script injection, ConPTY/forkpty abstraction
  - `block_manager.rs` — PromptActive → InputActive → Executing → Complete state machine, line range tracking, exit code capture
  - `osc_scanner.rs` — OSC 133 sequence parsing (prompt/command/output markers) and OSC 133;P pipeline event parsing
  - `grid_snapshot.rs` — Snapshot of alacritty_terminal grid state with color resolution
- Depends on: alacritty_terminal (embedded =0.25.1 exact), platform PTY APIs
- Used by: Processor event handler (processes PtyMsg from reader threads)

**History & Snapshots Layer:**
- Purpose: Store queryable command history (SQLite FTS5) and file snapshots for undo
- Location: `crates/glass_history/` and `crates/glass_snapshot/`
- Contains:
  - `glass_history` — HistoryDb (command execution records, pipe stages, output summaries, compression metadata)
  - `glass_snapshot` — SnapshotStore (blob store + metadata DB), BlobStore (blake3 content-addressed files), UndoEngine (file restoration)
- Depends on: rusqlite (bundled), filesystem (file watching via notify)
- Used by: Main event loop (records on CommandFinished), MCP server (queries), orchestrator (baseline verification)

**Orchestrator (Autonomous Agent) Layer:**
- Purpose: Silence-triggered autonomous feedback loop with Claude Code agent subprocess
- Location: `src/orchestrator.rs`, `src/checkpoint_synth.rs`, `src/ephemeral_agent.rs`, `crates/glass_feedback/`
- Contains:
  - `orchestrator.rs` — State machine (Idle → Waiting → CheckpointReady → Proposing → PauseAction → Paused), silence detection via SilenceTracker, response parsing, metric guard (baseline regression check), iteration logging
  - `checkpoint_synth.rs` — Ephemeral agent call to synthesize recent activity as checkpoint text
  - `ephemeral_agent.rs` — Spawn `claude` CLI subprocess with stdin/stdout/stderr, timeout handling, JSON response parsing
  - `glass_feedback/analyzer.rs` — Rule-based findings (Tier 1: heuristics), tier registry (provisional/confirmed rules), regression detection
  - `glass_feedback/lifecycle.rs` — Apply findings with guarded rollback (bump config, apply rules, generate prompts/scripts), track changes, detect regressions
- Depends on: History layer (query context), Snapshots (get baseline), Terminal layer (silence detection), Scripts layer (Tier 4 generation)
- Used by: Processor (runs on OrchestratorSilence events, manages agent subprocess)

**Scripting Layer (Tier 4 Automation):**
- Purpose: Rhai-based event-driven automation with safeguards
- Location: `crates/glass_scripting/`, `src/script_bridge.rs`
- Contains:
  - `glass_scripting/engine.rs` — Rhai VM, script compilation, sandbox isolation (max cpu time, max memory, max array size)
  - `glass_scripting/hooks.rs` — Hook registry (SnapshotBefore, McpRequest, etc.), script filtering by hook point
  - `glass_scripting/lifecycle.rs` — Script promotion (Provisional → Confirmed) and rejection tracking
  - `glass_scripting/profile.rs` — Export/import bundles with metadata and tech stack tags
  - `src/script_bridge.rs` — HookRegistry integration with Processor, hook firing during Orchestrator events
- Depends on: rhai (embedded scripting), glass_feedback (for lifecycle promotion)
- Used by: Processor hooks, Orchestrator feedback generation, MCP tool invocations

**MCP Server Layer (AI Tool Integration):**
- Purpose: Expose terminal context to Claude Code via Model Context Protocol
- Location: `crates/glass_mcp/`
- Contains: GlassServer with tools: GlassHistory (query), GlassContext (activity summary), GlassUndo (restore), GlassFileDiff (pre-command state), GlassAgentLock (advisory locks), GlassAgentRegister (agent coordination)
- Depends on: History, Snapshot, Coordination layers
- Used by: Claude Code agent subprocess (via stdio JSON-RPC 2.0)

**Coordination Layer (Multi-Agent):**
- Purpose: Coordinate file access and communication between multiple AI agents
- Location: `crates/glass_coordination/`
- Contains: CoordinationDb (global `~/.glass/agents.db` in WAL mode), agent registry, file locking, inter-agent messaging
- Depends on: rusqlite
- Used by: MCP server (registers/locks/deregisters agents), Processor (checks for conflicts)

**SOI (Structured Output Intelligence) Layer:**
- Purpose: Parse command output into structured records for token-efficient AI processing
- Location: `crates/glass_soi/`
- Contains: OutputClassifier (maps command → OutputType), per-type parsers (cargo_test, pytest, npm, docker, kubectl, git, typescript, go, json, generic)
- Depends on: None (standalone)
- Used by: History layer (store parsed output), BlockManager (set soi_summary/severity on Block), MCP tools (reference summary)

**Pipes Layer:**
- Purpose: Parse pipeline stages and capture per-stage output
- Location: `crates/glass_pipes/`
- Contains: parse_pipeline (identify stages), CapturedStage (name, stdout bytes, stderr bytes)
- Depends on: None (standalone)
- Used by: BlockManager (store pipeline_stages), OscScanner (emit PipelineStage events)

**Core Configuration Layer:**
- Purpose: Configuration management and system integration
- Location: `crates/glass_core/`
- Contains:
  - `config.rs` — GlassConfig (font, shell, history limits, snapshot retention, pipes, agent/orchestrator settings), hot reload via watcher
  - `config_watcher.rs` — File watcher monitoring ~/.glass/config.toml
  - `event.rs` — AppEvent enum, SessionId, ShellEvent (OSC-derived), EphemeralAgentResult/Error
  - `agent_runtime.rs` — AgentRuntimeConfig, CooldownTracker, BudgetTracker, UsageGate
  - `coordination_poller.rs` — Background thread polling agents.db for messages and conflicts
  - `updater.rs` — Check for newer Glass releases
- Depends on: serde (config parsing), notify (file watching), ureq (update checking)
- Used by: All layers

## Data Flow

**Command Execution Flow:**

1. **Shell emission** — Shell integration script emits OSC 133;A/B/C/D (prompt/input/exec/finish) with exit code
2. **PTY read thread** — `glass_terminal/pty.rs` reader thread decodes VT sequences (alacritty_terminal), extracts OSC events via OscScanner
3. **OSC dispatch** — OscScanner produces OscEvent (PromptStart, CommandStart, CommandExecuted, CommandFinished, PipelineStage)
4. **EventProxy** — PTY thread serializes event to AppEvent (Shell { ShellEvent, line }), sends via EventLoopProxy to main loop
5. **BlockManager processing** — Processor receives Shell event, calls BlockManager::handle_event() to update block state machine
6. **Grid capture** — On CommandFinished, Processor captures terminal grid (grid_snapshot) to get command output
7. **Output parsing** — Processor parses output via glass_soi::classify/parse, stores parsed summary in Block.soi_summary/severity
8. **History record** — Processor calls HistoryDb::insert_command with metadata (text, cwd, exit code, output summary, compression)
9. **Snapshot store** — If command is destructive (via snapshot/command_parser), create snapshot record and pre-command file snapshots
10. **Render** — Frame renderer draws Block with separator, prompt, command, exit badge (color by exit code), output, SOI summary line, [undo] label

**Orchestrator Feedback Flow:**

1. **Silence trigger** — SilenceTracker in glass_terminal detects quiet period, sends OrchestratorSilence event
2. **Checkpoint synthesis** — Orchestrator spawns ephemeral agent (claude checkpoint_synth) to summarize recent history
3. **Proposal generation** — Main orchestrator agent (persistent subprocess) receives checkpoint + context, proposes changes (config tweaks, rule updates, prompts)
4. **Metric guard** — Proposals tested against verification baseline (auto-detected cargo test / npm test / pytest); regression blocks acceptance
5. **Feedback analysis** — On high waste/stuck rates, spawn ephemeral agent for Tier 3 LLM analysis of findings
6. **Script generation** — If Tier 3 produces no findings, spawn Tier 4 agent to generate Rhai automation script
7. **Script promotion** — User accepts script → lifecycle.rs moves Provisional → Confirmed, enables for future runs
8. **Rollback** — If next run regresses metrics, revert config/rules/scripts to last known good baseline

**Scripting Hook Flow:**

1. **Hook registration** — Scripts in `~/.glass/scripts/` and `<project>/.glass/scripts/` loaded at startup, compiled to Rhai AST
2. **Hook point trigger** — On lifecycle event (e.g., SnapshotBefore before capturing pre-command file state), Processor calls ScriptSystem::run_hook()
3. **Script execution** — Rhai engine runs each script registered for the hook in sandbox (CPU time, memory, array size limits)
4. **Action aggregation** — Scripts return actions (ConfigValue, Log, MCP call); aggregated into ScriptRunResult
5. **Hook semantics** — SnapshotBefore uses AND aggregation (any confirmed/user script error = veto), McpRequest uses first-responder-wins
6. **Action execution** — Processor executes actions (e.g., ConfigValue → update config.toml, Log → log line)

**State Management:**

- **Session state** — Lives in Session struct (PTY sender/receiver, alacritty_terminal grid, BlockManager, history DB handle)
- **Terminal grid** — alacritty_terminal::Term (managed by PTY read thread, locked during render)
- **UI overlays** — SearchOverlay, ActivityOverlay, ProposalOverlay, SettingsOverlay states live on Processor
- **Configuration** — Loaded at startup, hot-reloaded via config watcher; snapshot taken at feedback start time
- **Orchestrator state** — OrchestratorState machine lives on Processor, events logged to OrchestratorEventBuffer
- **Agent subprocess** — AgentRuntime owns child process and stdin writer, managed by Processor event loop
- **Scripting state** — ScriptSystem (registry + engine) loaded once at startup, modified on script promotion/rejection

## Key Abstractions

**Block:**
- Purpose: Represent a single prompt-command-output cycle with lifecycle state
- Examples: `crates/glass_terminal/src/block_manager.rs`
- Pattern: State machine (PromptActive → InputActive → Executing → Complete), updated by BlockManager::handle_event(), rendered with decorators (exit code badge, [undo] label, SOI summary, pipeline overlay)

**Session:**
- Purpose: Encapsulate a single terminal instance (PTY, grid, block manager, history DB)
- Examples: `crates/glass_mux/src/session.rs`
- Pattern: Owns Arc<FairMutex<Term>>, PtySender for resize/input, BlockManager, HistoryDb handle

**SessionMux:**
- Purpose: Manage multiple sessions organized in tabs with split panes
- Examples: `crates/glass_mux/src/session_mux.rs`
- Pattern: HashMap of tabs, each tab has SplitNode (binary pane tree), focused session tracked via current focus

**SplitNode:**
- Purpose: Represent binary tree of split panes
- Examples: `crates/glass_mux/src/split_tree.rs`
- Pattern: Recursive enum (Leaf(SessionId) | VSplit(Box<L>, Box<R>) | HSplit(Box<T>, Box<B>)), compute_layout() recursively divides viewport

**OrchestratorState:**
- Purpose: Track autonomous agent feedback loop state machine
- Examples: `src/orchestrator.rs`
- Pattern: Enum variant per state (Idle, Waiting, CheckpointReady, Proposing, PauseAction, Paused), transitions on silence/response/user input, logs all transitions to OrchestratorEventBuffer

**FeedbackResult:**
- Purpose: Aggregate tier-1/2/3 findings with guarded lifecycle
- Examples: `crates/glass_feedback/src/lib.rs`
- Pattern: Findings vec, config changes, promoted/rejected rules, optional LLM prompt, optional Tier 4 script prompt, regression option

**ScriptSystem:**
- Purpose: Load, compile, and execute Rhai scripts with hook-based dispatch
- Examples: `crates/glass_scripting/src/lib.rs`
- Pattern: Owns Rhai engine and HookRegistry, run_hook() iterates registered scripts for hook point, applies hook-specific aggregation semantics

## Entry Points

**Main Loop:**
- Location: `src/main.rs` (~2200 lines)
- Triggers: winit event loop (window events, user input, PTY events via EventLoopProxy)
- Responsibilities:
  - Parse CLI subcommands (history, undo, mcp, profile)
  - Launch GUI: create window, GPU surface, session, event loop
  - Consume AppEvents: update session state, redraw, dispatch to UI handlers
  - Manage subprocess lifecycle (agent, config watcher, coordination poller)

**PTY Reader Thread:**
- Location: `crates/glass_terminal/src/pty.rs` - `spawn_pty()` function
- Triggers: Spawned at session creation, reads from PTY descriptor in dedicated thread
- Responsibilities:
  - Read PTY bytes, feed to alacritty_terminal decoder
  - Parse OSC sequences via OscScanner
  - Send events back to main loop via EventProxy
  - Forward shell integration output to JSON file (glass_pty_loop.json) for agent consumption

**Orchestrator State Machine:**
- Location: `src/orchestrator.rs` - OrchestratorState transition handlers
- Triggers: OrchestratorSilence event (periodic, from SilenceTracker in PTY thread)
- Responsibilities:
  - Idle → Waiting: Capture checkpoint, spawn ephemeral agent
  - Waiting → CheckpointReady: Checkpoint synthesis complete, checkpoint text ready
  - CheckpointReady → Proposing: Main agent subprocess receives checkpoint + context, generates proposal
  - Proposing → PauseAction: Agent sends response, run metric guard, filter by regression
  - PauseAction → Paused: User views proposal, decision pending (accept/reject/skip)
  - Paused → Idle: User decision applied (config changed, rules promoted, script generated), metrics updated

**MCP Server:**
- Location: `src/main.rs` (CLI subcommand) → `crates/glass_mcp/src/lib.rs` - `run_mcp_server()`
- Triggers: `glass mcp serve` command (invoked by Claude Code subprocess)
- Responsibilities:
  - Resolve history DB and snapshot store paths
  - Create GlassServer with four tools
  - Serve JSON-RPC 2.0 requests over stdio
  - Query/undo/inspect terminal context for agent

## Error Handling

**Strategy:** Layered recovery with fallback defaults and error display.

**Patterns:**
- **Config errors** — Parse failures logged and displayed as overlay banner, defaults applied, reload triggered on fix
- **Database errors** — Log warning, continue operation (history reads return empty, snapshots unavailable), operations degrade gracefully
- **PTY failures** — Log error, trigger TerminalExit event, allow session to be closed and new one created
- **Orchestrator failures** — Ephemeral agent timeouts/crashes logged and suppressed (silent retry), proposal rejected, loop continues
- **Script errors** — Compilation errors collected and displayed, script disabled for that hook, other scripts continue
- **Agent subprocess crash** — Logged with restart count, exponential backoff cooldown (max 3 restarts), agent mode disabled if restart limit reached
- **Coordination failures** — Soft errors (lock conflicts, deregister failures) logged at warn level, never block shutdown
- **File lock conflicts** — Returned to caller (MCP tools) with identifying information, no blocking

## Cross-Cutting Concerns

**Logging:** tracing crate with env-filter subscriber, all modules use `tracing::{debug, info, warn, error}`. Feature flag `perf` enables tracing-chrome for flamegraph generation.

**Validation:**
- Config validation on parse (numeric bounds checked, shell path validated)
- Command parsing via glass_pipes (split by pipe, escape handling via shlex)
- Snapshot command detection via command_parser (destructive command heuristics)
- Output classification via glass_soi (regex-based OutputType detection)

**Authentication:** MCP auth stateless (runs in same process as Glass), agent coordination uses advisory file locks (no passwords), Cloud API usage tracked via oauth token in ~/.glass/ (outside repo).

**Concurrency:**
- Main thread (winit event loop) communicates with PTY/config/agent threads via channels
- Shared state protected by Arc<Mutex<>> (terminal grid, coordination DB)
- Lock-free ring buffer for orchestrator event logging
- tokio async runtime for background tasks

**Resilience:**
- PTY reader thread handles decoding errors, continues on VT sequence malformation
- SilenceTracker debounces rapid silence detection with fast-trigger cooldown
- Metric guard prevents regressive config changes (baseline captured, tested before applying)
- Orchestrator timeout gates prevent hanging on agent subprocess
- Agent subprocess restart with exponential backoff and max retry limit

---

*Architecture analysis: 2026-03-18*
