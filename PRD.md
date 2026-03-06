# Glass — Product Requirements Document

## 1. Overview

**Glass** is a cross-platform terminal emulator that understands what your commands *do*, not just what they *print*. It introduces three capabilities that no existing terminal offers: command-level undo with automatic filesystem snapshots, visual pipe debugging with inspectable intermediate stages, and structured scrollback that turns your terminal history into a queryable database.

Glass looks and feels like a normal terminal. You type commands exactly as you always have. The intelligence is passive — it watches, indexes, and snapshots in the background, and surfaces its power only when you need it.

---

## 2. Problem Statement

### 2.1 The Terminal Is Stuck in 1978

The terminal is the most fundamental developer tool, yet it treats all output as a flat, unstructured text stream. Modern terminals (Warp, Ghostty, Wave) have improved rendering, performance, and aesthetics — but the underlying model remains: you type text, you get text back, and that text scrolls away forever.

### 2.2 Specific Pain Points

| Pain Point | Impact | Who Feels It |
|---|---|---|
| **Destructive commands are irreversible.** A bad `sed -i`, `rm`, or script can damage files with no easy undo. Recovery requires git archaeology, backups, or re-doing work. | Lost work, wasted time, anxiety about running commands | All developers |
| **Pipeline debugging is blind.** Complex pipes (`cat | grep | sort | awk | uniq`) are debugged by inserting `tee` or `head` at each stage. There's no visibility into intermediate data. | Slow iteration, trial-and-error debugging | Anyone using Unix pipes |
| **Terminal history is a text buffer.** Scrollback is dumb text. You can't search by "commands that failed" or "commands that modified this file." Context is lost across sessions. | Re-running commands, re-investigating issues, lost context after breaks | All developers, especially those using AI coding assistants |
| **AI assistants lose context.** When an AI coding assistant's context window resets, it has no way to recover what happened in prior sessions. It starts blind every time. | Repeated work, slower AI-assisted debugging, manual context restoration | AI-assisted development workflows |

### 2.3 Why Now

- AI coding assistants (Claude Code, Codex CLI, Cursor, etc.) are becoming primary development interfaces, and they all run inside terminals. A smarter terminal directly amplifies their effectiveness.
- Developers now expect modern UX from their tools. The gap between a VS Code experience and a raw terminal experience has become unacceptable.
- Rust and GPU-accelerated rendering have matured enough to build a terminal that's both feature-rich and fast.

---

## 3. Target Users

### 3.1 Primary: Professional Developers

- Use the terminal daily for development, debugging, DevOps, and system administration.
- Comfortable with Unix commands and pipelines.
- Already use modern terminals (Warp, iTerm2, Ghostty, Windows Terminal) and would switch for meaningfully better features.

### 3.2 Secondary: AI-Assisted Developers

- Use AI coding assistants (Claude Code, Codex CLI, GitHub Copilot CLI) as part of their daily workflow.
- Frequently hit context window limits or lose session state.
- Would benefit from a terminal that serves as persistent memory for their AI tools.

### 3.3 Tertiary: DevOps / SRE Engineers

- Run complex diagnostic pipelines during incidents.
- Need to trace what commands were run, by whom, and what they changed during post-mortems.
- Would benefit from structured, searchable command history with filesystem change tracking.

---

## 4. Core Features

### 4.1 Command-Level Undo

**What:** Every command that modifies files on disk automatically triggers a lightweight snapshot of the affected files *before* the modification occurs. Users can revert any command's filesystem changes with a single action.

**How It Works:**

1. Glass intercepts command execution and monitors filesystem events (file writes, deletes, renames, permission changes) within the working directory scope.
2. Before a file is modified, its current state is copied to a local snapshot store (`.glass/snapshots/`).
3. Each snapshot is tagged with: command text, timestamp, command ID, list of affected files, and exit code.
4. The user can undo via:
   - `Ctrl+Z` — undoes the most recent file-modifying command.
   - Clicking the `[undo]` button on any command block in the UI.
   - Running `glass undo <command-id>` from the command line.
5. Undo restores the snapshotted file states. It does not reverse process-level side effects (network calls, database writes, etc.).

**Snapshot Storage & Limits:**

- Snapshots are stored locally in `.glass/snapshots/` within the project directory.
- Default retention: last 100 file-modifying commands or 500MB, whichever is hit first. Oldest snapshots are pruned automatically.
- Users can configure retention limits via `glass config`.
- Snapshots are excluded from git via an auto-managed `.gitignore` entry.

**Scope & Boundaries:**

- Tracks file changes within the current working directory and its subdirectories by default.
- Does NOT snapshot system files, files outside the project, or changes made by background daemons.
- Does NOT reverse side effects like HTTP requests, database mutations, or IPC.
- Users can configure include/exclude patterns for snapshot tracking.

**Platform Implementation:**

| Platform | Mechanism |
|---|---|
| Linux | `inotify` / `fanotify` for filesystem monitoring |
| macOS | `FSEvents` API |
| Windows | `ReadDirectoryChangesW` API |

### 4.2 Visual Pipe Debugging

**What:** When a user runs a piped command (`cmd1 | cmd2 | cmd3`), Glass captures and displays the intermediate output at each stage of the pipeline, rendered as inspectable rows.

**How It Works:**

1. Glass parses the command to detect pipe operators (`|`).
2. It inserts transparent `tee`-like capture points between each stage.
3. Each stage's input and output is captured and stored in memory.
4. The output is rendered as a multi-row view:

```
┌─ pipeline: cat → grep → sort → uniq ──────────────┐
│                                                     │
│  cat access.log      → 14,230 lines                │
│  grep "ERROR"        → 87 lines                    │
│  sort                → 87 lines (sorted)           │
│  uniq -c             → 23 unique errors             │
│                                                     │
│  [expand stage 2: grep "ERROR"]                     │
└─────────────────────────────────────────────────────┘
```

5. Each stage row is expandable — clicking it shows the full intermediate output for that stage.
6. For large outputs, only a summary is shown by default (line count, byte size) with the option to expand or export.

**Limitations & Edge Cases:**

- Binary data in pipes: detected and shown as `[binary: 4.2KB]` rather than rendered.
- Very long pipelines (10+ stages): rendered with scrolling within the pipeline block.
- Commands using process substitution (`<()`, `>()`) or subshells: not decomposed, shown as a single block.
- Async / background pipes (`|&`, `2>&1 |`): stderr is captured separately and shown alongside stdout per stage.
- Pipe inspection can be toggled off globally or per-command with a `--no-glass` flag for performance-sensitive work.

**Performance:**

- Capture buffers are capped at 10MB per stage by default. If a stage exceeds this, only head/tail samples are retained.
- Pipe inspection adds <5ms latency per stage for typical commands. For high-throughput data pipes, the user should disable inspection.

### 4.3 Structured Scrollback / Queryable History

**What:** Glass indexes every command and its output into a local structured database, making terminal history searchable by metadata rather than raw text.

**Queryable Fields:**

| Field | Example Query |
|---|---|
| Command text | `history where command contains "docker"` |
| Exit code | `history where status = failed` |
| Timestamp / range | `history where time = "last 2 hours"` |
| Working directory | `history where cwd = "~/projects/api"` |
| Files modified | `history where modified "src/auth.ts"` |
| Duration | `history where duration > 10s` |
| Output content | `history where output contains "segfault"` |
| Undo status | `history where undone = true` |

**How It Works:**

1. Every command execution is logged to a local SQLite database (`.glass/history.db`).
2. Stored per command: command text, arguments, working directory, environment hash, start/end timestamps, exit code, output (truncated to configurable max), list of files read/modified, snapshot reference (if applicable).
3. Users query via:
   - `Ctrl+Shift+F` — opens a search overlay in the terminal UI.
   - `glass history <query>` — CLI-based querying.
   - Tool API — AI assistants can query history programmatically (see section 4.4).
4. Results are returned as structured blocks, clickable and expandable.

**Storage & Retention:**

- SQLite database stored at `.glass/history.db` (per-project) and `~/.glass/global-history.db` (cross-project).
- Default retention: 30 days or 1GB, configurable.
- Output storage is truncated to 50KB per command by default (full output available if snapshot exists).
- FTS5 full-text search index on command text and output.

### 4.4 AI Tool Interface

**What:** Glass exposes its history, snapshots, and pipe data as a structured tool API that AI coding assistants can query programmatically — enabling them to recover context without consuming excessive tokens.

**Exposed Tools:**

```
GlassHistory(query, timeframe, limit)
  → Returns matching commands with metadata. Token-efficient summaries.

GlassUndo(command_id)
  → Reverts filesystem changes from a specific command.

GlassPipeInspect(command_id, stage)
  → Returns intermediate output from a specific pipeline stage.

GlassFileDiff(command_id)
  → Returns the diff of files changed by a specific command.

GlassContext(timeframe)
  → Returns a high-level summary of recent activity:
    "12 commands run, 3 failed, 5 files modified, 1 undone"
```

**Design Principles:**

- Every tool returns the minimum data needed. Summaries first, details on request.
- AI assistants should call `GlassContext` first to orient, then drill into specifics.
- Token budget per query: aim for <500 tokens for summaries, <2000 tokens for detailed results.
- Tool definitions are exposed as an MCP (Model Context Protocol) server, making Glass compatible with any AI assistant that supports MCP.

---

## 5. User Interface

### 5.1 Design Philosophy

- **It's a terminal first.** No sidebars, panels, or chrome by default. Dark background, monospace font, blinking cursor. You should forget you're using Glass until you need its features.
- **Progressive disclosure.** Features reveal themselves contextually — the undo button appears on command blocks, pipe visualization renders automatically when pipes are detected, search is a keyboard shortcut away.
- **Zero configuration required.** Glass works out of the box with sane defaults. No themes to download, no plugins to install, no dotfiles to configure.

### 5.2 Layout

```
┌──────────────────────────────────────────────────────────┐
│  ~/project                                   main  ↑2   │  ← Status bar (cwd, git branch, dirty count)
│──────────────────────────────────────────────────────────│
│                                                          │
│  ┌─ $ npm test                          ✓ 4.1s  ─────┐  │  ← Command block (collapsed)
│  └────────────────────────────────────────────────────┘  │
│                                                          │
│  ┌─ $ cat data.json | jq '.users' | wc -l  ✓ 0.2s ──┐  │  ← Pipeline block
│  │  cat data.json    → 1,240 lines                    │  │
│  │  jq '.users'      → 48 entries                     │  │
│  │  wc -l            → 48                             │  │
│  └────────────────────────────────────────────────────┘  │
│                                                          │
│  ┌─ $ sed -i 's/foo/bar/g' config.ts   ✓ 0.1s ──────┐  │  ← File-modifying block
│  │  Modified: config.ts (+3 -3)              [undo]   │  │
│  └────────────────────────────────────────────────────┘  │
│                                                          │
│  $ _                                                     │  ← Active prompt
│                                                          │
│──────────────────────────────────────────────────────────│
│  ⏪ 24 commands │ 🔍 Ctrl+Shift+F │ ⚡ 7 snapshots      │  ← Footer bar
└──────────────────────────────────────────────────────────┘
```

### 5.3 Command Block States

| State | Appearance |
|---|---|
| Running | Subtle spinner, live output streaming |
| Succeeded | Green `✓`, collapsed by default, click to expand |
| Failed | Red `✗`, expanded by default to show error |
| Undone | Dimmed with strikethrough, `[undone]` label |
| Has snapshot | Subtle `[undo]` button visible on hover |
| Pipeline | Expanded to show stage breakdown |

### 5.4 Key Bindings

| Action | Shortcut |
|---|---|
| Undo last file-modifying command | `Ctrl+Z` |
| Search history | `Ctrl+Shift+F` |
| Toggle pipe visualization | `Ctrl+Shift+P` |
| Collapse/expand command block | Click or `Ctrl+Click` |
| Copy command output | `Ctrl+Shift+C` |
| Clear screen (preserves history) | `Ctrl+L` |
| Open settings | `Ctrl+,` |

---

## 6. Technical Architecture

### 6.1 Technology Stack

| Component | Technology | Rationale |
|---|---|---|
| Core / shell integration | Rust | Performance, memory safety, cross-platform compilation |
| Terminal emulation | Fork of `alacritty_terminal` crate | Correct, battle-tested VTE — we add features on top, not rewrite terminal emulation (see section 6.4) |
| GPU rendering | `wgpu` | Cross-platform GPU abstraction (Vulkan, Metal, DX12) |
| UI framework | Custom immediate-mode renderer | Minimal overhead, terminal-native feel |
| History database | SQLite (via `rusqlite`) | Embedded, zero-config, battle-tested, FTS5 support |
| Filesystem monitoring | Platform-native APIs (`inotify`, `FSEvents`, `ReadDirectoryChangesW`) | Low overhead, real-time change detection |
| Snapshot storage | Copy-on-write file copies + content-addressed dedup | Space-efficient, fast restore |
| Configuration | TOML (`~/.glass/config.toml`) | Human-readable, Rust ecosystem standard |
| AI tool interface | MCP server (JSON-RPC over stdio) | Compatible with Claude Code, Codex, and other MCP clients |

### 6.2 High-Level Architecture

```
┌─────────────────────────────────────────────────────────┐
│                     Glass Terminal                       │
│                                                         │
│  ┌─────────────┐  ┌──────────────┐  ┌───────────────┐  │
│  │   Renderer   │  │ Command      │  │  Snapshot      │  │
│  │   (wgpu)     │  │ Parser       │  │  Engine        │  │
│  │              │  │              │  │                │  │
│  │  Blocks      │  │  Pipe detect │  │  FS monitor    │  │
│  │  Pipe views  │  │  Arg parse   │  │  Copy-on-write │  │
│  │  Search UI   │  │  Exit codes  │  │  Pruning       │  │
│  └──────┬───────┘  └──────┬───────┘  └───────┬────────┘  │
│         │                 │                   │          │
│  ┌──────┴─────────────────┴───────────────────┴────────┐ │
│  │              Core Event Bus                          │ │
│  └──────┬─────────────────┬───────────────────┬────────┘ │
│         │                 │                   │          │
│  ┌──────┴───────┐  ┌──────┴──────────┐  ┌────┴────────┐ │
│  │   VTE        │  │  History DB     │  │  MCP Server  │ │
│  │   (PTY mgmt) │  │  (SQLite/FTS5)  │  │  (AI tools)  │ │
│  └──────┬───────┘  └─────────────────┘  └─────────────┘ │
│         │                                                │
│  ┌──────┴───────┐                                        │
│  │   Shell      │  ← bash, zsh, fish, PowerShell         │
│  │   (child)    │                                        │
│  └──────────────┘                                        │
└─────────────────────────────────────────────────────────┘
```

### 6.4 Architectural Decision: Fork Alacritty's VTE

**Decision:** Glass will fork and embed the `alacritty_terminal` crate as its VTE layer. We will NOT build terminal emulation from scratch.

**Why:**

- Building a correct VTE is a multi-year effort. Alacritty's has been battle-tested since 2017. It handles escape sequences, Unicode, sixel graphics, and edge cases we'd spend years discovering.
- Alacritty's crate is explicitly designed to be embeddable — it separates terminal emulation from rendering. Ghostty's is not (it's tightly coupled to its own renderer).
- Alacritty is Apache 2.0 licensed. Clean for any licensing model.
- This lets us focus 100% of our effort on the three features that make Glass different, rather than re-solving terminal emulation.

**What we build on top:**

- Block-based rendering layer that wraps the VTE output stream.
- Command boundary detection (prompt detection via shell integration hooks).
- Pipe interception layer between the shell and the VTE.
- Snapshot engine sitting alongside the VTE, watching filesystem events.

**Trade-off acknowledged:** We inherit Alacritty's architectural opinions and any bugs. We also take on the maintenance cost of staying in sync with upstream. This is worth it — shipping in months instead of years matters more than full control over escape sequence handling.

### 6.5 Performance Budgets

| Metric | Target |
|---|---|
| Cold start | <200ms |
| Input latency (keypress to screen) | <5ms |
| Pipe interception overhead | <5ms per stage |
| Snapshot creation (avg file) | <10ms |
| History query (FTS) | <50ms |
| Memory baseline (idle) | <50MB |
| Memory per open tab | <20MB |

---

## 7. Platform Support

### 7.1 Supported Platforms

| Platform | Priority | Shell Support |
|---|---|---|
| macOS (Apple Silicon + Intel) | P0 — launch platform | bash, zsh, fish |
| Linux (x86_64, aarch64) | P0 — launch platform | bash, zsh, fish |
| Windows 10/11 | P1 — fast follow | PowerShell, bash (WSL), cmd |

### 7.2 Platform-Specific Considerations

**macOS:**
- Native `.app` bundle with proper code signing and notarization.
- Uses `FSEvents` for filesystem monitoring.
- Integrates with macOS keychain for any credential storage.
- Supports native macOS keyboard shortcuts as alternatives.

**Linux:**
- Distributed as `.deb`, `.rpm`, AppImage, and Flatpak.
- Uses `inotify`/`fanotify` for filesystem monitoring.
- Respects XDG directory conventions.
- Wayland-native with X11 fallback.

**Windows:**
- Native Win32 app (not Electron).
- Uses `ReadDirectoryChangesW` for filesystem monitoring.
- ConPTY for terminal emulation.
- Proper support for PowerShell, cmd.exe, and WSL shells.

---

## 8. Configuration

### 8.1 Config File: `~/.glass/config.toml`

```toml
[general]
shell = "auto"                    # auto-detect, or specify: "bash", "zsh", "fish", "pwsh"
theme = "auto"                    # "dark", "light", "auto" (follows system)
font_family = "auto"              # auto-detect monospace font, or specify
font_size = 14

[snapshots]
enabled = true
max_count = 100                   # max snapshots retained
max_size_mb = 500                 # max total snapshot storage
retention_days = 7                # auto-prune after N days
watch_scope = "cwd"               # "cwd" (project dir) or "home" or custom paths
exclude_patterns = [              # files/dirs to never snapshot
  "node_modules/**",
  ".git/**",
  "*.log",
  "target/**",
  "dist/**"
]

[pipes]
enabled = true
max_capture_mb = 10               # max buffer per pipeline stage
auto_expand = true                # auto-show pipe stages on completion

[history]
enabled = true
db_location = "project"           # "project" (.glass/history.db) or "global" (~/.glass/global-history.db)
retention_days = 30
max_output_capture_kb = 50        # max output stored per command
fts_enabled = true                # full-text search indexing

[ai]
mcp_server = true                 # expose MCP tool interface
mcp_port = "stdio"                # "stdio" or port number

[keybindings]
undo = "ctrl+z"
search = "ctrl+shift+f"
toggle_pipes = "ctrl+shift+p"
```

---

## 9. Competitive Landscape

| Terminal | Block UI | Undo | Pipe Viz | Structured Search | AI Tools |
|---|---|---|---|---|---|
| **Glass** | Yes | **Yes** | **Yes** | **Yes** | **Yes (MCP)** |
| Warp | Yes | No | No | Partial (AI-assisted) | Yes (built-in AI) |
| Wave | Yes | No | No | No | Yes (built-in AI) |
| Ghostty | No | No | No | No | No |
| Alacritty | No | No | No | No | No |
| iTerm2 | No | No | No | Partial (search) | No |
| Kitty | No | No | No | No | No |
| Windows Terminal | No | No | No | No | No |
| Extraterm | Yes | No | No | No | No |

Glass does not compete on AI chat, themes, or raw speed. It competes exclusively on the three features in bold.

### 9.1 Why Nobody Has Built These Features

This matters. If three obviously useful features don't exist, there's usually a reason. We need to understand those reasons to know what we're signing up for.

**Command-level undo — why it doesn't exist:**

The terminal has never tracked command boundaries. It sees a stream of bytes, not discrete commands. To know "what files did this command change," you need: (1) shell integration to detect command start/end, (2) filesystem monitoring synchronized to command lifecycle, and (3) a snapshot store. This is three systems that only create value when combined. No single one is useful alone, so nobody builds the first one. Glass builds all three together.

Additionally, terminal emulators have historically avoided touching the filesystem — they're display layers. Snapshotting files is a fundamentally different responsibility. Glass breaks that boundary deliberately.

**Pipe visualization — why it doesn't exist:**

Unix pipes are implemented at the kernel level. The terminal emulator never sees intermediate data — it only receives the final stdout of the last command in the chain. To capture intermediate output, you must rewrite the pipeline to insert tee-like processes between each stage, which means parsing the command, understanding shell syntax (quoting, escaping, subshells), and transparently modifying execution. This is fragile and shell-specific.

The deeper problem: some commands behave differently when stdout is a pipe vs. a TTY (e.g., `ls` disables color, `git` disables paging). Inserting capture points changes the pipe/TTY status of each stage, which can change program behavior. This is a real technical risk and is why we include opt-out flags and TTY-sensitive command detection.

**Structured scrollback — why it doesn't exist:**

Again, the terminal doesn't know where one command ends and the next begins. Without shell integration (prompt detection), there are no boundaries to structure. Warp solved this with their block model, but they invested that capability into AI features rather than queryable history. The building blocks exist now — FTS5 in SQLite, shell integration protocols — but nobody has assembled them for this specific purpose.

### 9.2 Our Actual Risk

The honest assessment: pipe visualization is the riskiest feature technically. Undo is the hardest engineering-wise. Structured scrollback is the most straightforward. This informs our phasing (see section 10).

---

## 10. Milestones & Phasing

Phasing is ordered by: (1) what's foundational, (2) what's most straightforward, (3) what's most impactful, (4) what's riskiest. We ship value early and tackle the hardest problem last, when we understand the codebase best.

### Phase 0: Project Scaffold

**Goal:** Prove the foundation compiles and runs. A bare window that spawns a shell and lets you type commands. No features — just proof of life.

**Project Structure:**

```
Glass/
├── PRD.md                          # This document
├── Cargo.toml                      # Workspace root
├── crates/
│   ├── glass_core/                 # Core event bus, config, types
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs
│   ├── glass_terminal/             # VTE wrapper around alacritty_terminal
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs
│   ├── glass_renderer/             # wgpu-based rendering
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs
│   ├── glass_history/              # SQLite history DB (Phase 2)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs
│   ├── glass_snapshot/             # Filesystem snapshot engine (Phase 3)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs
│   ├── glass_pipes/                # Pipe interception and visualization (Phase 4)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs
│   └── glass_mcp/                  # MCP server for AI integration (Phase 2)
│       ├── Cargo.toml
│       └── src/
│           └── lib.rs
├── src/
│   └── main.rs                     # Entry point — wires crates together
├── config/
│   └── default.toml                # Default configuration
├── .gitignore
└── README.md
```

**Key Dependencies (Cargo.toml):**

```toml
[workspace]
members = ["crates/*"]

[workspace.dependencies]
alacritty_terminal = "0.24"        # VTE layer — pin to specific version
wgpu = "24"                        # GPU rendering
winit = "0.30"                     # Window management (cross-platform)
rusqlite = { version = "0.32", features = ["bundled", "fts5"] }
serde = { version = "1", features = ["derive"] }
toml = "0.8"                       # Config parsing
notify = "7"                       # Cross-platform filesystem watcher (wraps inotify/FSEvents/ReadDirectoryChangesW)
tokio = { version = "1", features = ["full"] }
```

**Phase 0 Tasks:**

1. `cargo init` the workspace with the crate structure above.
2. Add `alacritty_terminal` and `wgpu` and `winit` as dependencies.
3. Create a `winit` window with a `wgpu` surface.
4. Spawn a PTY child process (the user's shell) using `alacritty_terminal`.
5. Render the VTE output to the `wgpu` surface — monospace grid, cursor, basic colors.
6. Handle keyboard input → PTY stdin.
7. Verify it works on the current platform (Windows).
8. `git init`, initial commit.

**Exit Criteria:** You can open Glass, type `ls`, see output, type `echo hello`, see "hello." It's a working terminal — ugly, minimal, no features — but the foundation compiles and the architecture is proven.

**Important Notes for Implementation:**
- Check the latest versions of all crates before starting. The versions above are estimates — verify against crates.io.
- `alacritty_terminal` may have API changes between versions. Read its docs/source before integrating.
- On Windows, PTY spawning uses ConPTY via the `conpty` or `windows-rs` crate. Verify `alacritty_terminal` handles this or if we need a platform shim.
- The crates for Phase 2-4 (`glass_history`, `glass_snapshot`, `glass_pipes`, `glass_mcp`) should be created as empty stubs in Phase 0 — just `lib.rs` with a comment. This establishes the workspace structure upfront so later phases don't require restructuring.

### Phase 1: Foundation

**Goal:** A working terminal with block-based UI and shell integration. No Glass-specific features yet — just a solid terminal you could actually use daily.

- Embed `alacritty_terminal` crate as the VTE layer.
- GPU-accelerated rendering with `wgpu`.
- Shell integration hooks for bash, zsh, and fish (prompt detection, command start/end signals, cwd tracking).
- Block-based command output (collapsible, exit code, duration).
- Basic configuration via TOML.
- macOS and Linux support.

**Exit Criteria:** Glass is a usable daily terminal with block-based output. No Glass-specific features, but the shell integration hooks are in place — these are the foundation everything else depends on.

**Why this is first:** Everything depends on shell integration. Without reliable command boundary detection, undo doesn't know what to snapshot, history doesn't know what to index, and pipes don't know what to decompose. Get this right first.

### Phase 2: Structured Scrollback + MCP Server

**Goal:** Queryable terminal history and AI tool interface, shipped together.

- SQLite history database with FTS5 indexing.
- Command metadata logging (cwd, exit code, duration, output capture).
- Search overlay UI (`Ctrl+Shift+F`).
- CLI query interface (`glass history`).
- MCP server implementation (JSON-RPC over stdio).
- Tool definitions: `GlassHistory`, `GlassContext`.
- Retention policies and storage management.

**Exit Criteria:** You can search "failed commands in the last hour" and get structured results. An AI assistant running inside Glass can query history via MCP.

**Why this is second:** Structured scrollback is the most straightforward feature — it's mostly "log to SQLite and build a search UI." Shipping it alongside the MCP server means Glass becomes useful to AI assistants immediately. This is also where Glass starts to matter *to me* — I can query what happened before my context was cleared.

**Why MCP is bundled here, not in a later phase:** The MCP server is not a separate feature. It's an interface to the same data. Building `GlassHistory` is trivial once the history DB exists — it's just a JSON-RPC wrapper around a SQLite query. Deferring it to a separate phase would be artificial separation. Shipping them together means AI-assisted workflows work from the first feature release.

### Phase 3: Command-Level Undo

**Goal:** Automatic filesystem snapshots per command with one-keystroke revert.

- Filesystem monitoring engine (platform-native APIs).
- Pre-command snapshot creation synchronized with shell integration hooks.
- Snapshot storage with content-addressed deduplication.
- `Ctrl+Z` undo and `[undo]` button on command blocks.
- CLI interface: `glass undo <command-id>`.
- MCP tools: `GlassUndo`, `GlassFileDiff`.
- Storage management and pruning.
- File modification tracking integrated into history DB (extends Phase 2).

**Exit Criteria:** You can undo any file-modifying command. AI assistants can trigger undo and inspect file diffs via MCP.

**Why this is third, not first:** Undo is the hardest engineering challenge. It requires synchronized filesystem monitoring, reliable command boundary detection (from Phase 1), and snapshot management. Building it after we've shipped and battle-tested shell integration reduces the risk of snapshotting at the wrong boundaries. It also depends on the history DB (Phase 2) for tracking which commands modified which files.

### Phase 4: Pipe Visualization

**Goal:** Visual pipe debugging with inspectable intermediate stages.

- Pipe detection and parsing in the command parser.
- Transparent capture point insertion between pipeline stages.
- TTY-sensitive command detection and pass-through.
- Multi-row pipeline rendering in the UI.
- Expandable stage output with scroll and export.
- Performance guardrails (buffer caps, `--no-glass` opt-out flag).
- MCP tool: `GlassPipeInspect`.

**Exit Criteria:** Piped commands automatically show intermediate output at each stage, with opt-out for TTY-sensitive or high-throughput pipelines.

**Why this is last among core features:** Pipe visualization is the riskiest feature (see section 9.1). Modifying pipeline execution is fragile, shell-specific, and can change program behavior. By the time we build this, we'll have deep experience with shell integration, command parsing, and the block rendering system. We'll also have real users who can help us identify which commands break under pipe interception.

### Phase 5: Cross-Platform & Tabs

**Goal:** macOS and Linux support plus tabbed/split-pane terminal sessions.

**Note:** Glass was built Windows-first (ConPTY, DX12, PowerShell). This phase ports to macOS and Linux and adds multi-session UI.

**Cross-Platform:**

- macOS support: Metal backend via wgpu, `FSEvents` filesystem monitoring (via `notify` crate), native `.app` bundle structure, macOS keyboard conventions (`Cmd+C`/`Cmd+V`), zsh/bash/fish shell integration.
- Linux support: Vulkan/OpenGL backend via wgpu, `inotify` filesystem monitoring (via `notify` crate), Wayland-native with X11 fallback, XDG directory conventions, bash/zsh/fish shell integration.
- Platform abstraction layer for PTY spawning (ConPTY on Windows, `forkpty` on Unix).
- CI cross-compilation and per-platform test matrix.

**Tabs & Split Panes:**

- Tab bar with keyboard shortcuts (`Ctrl+Shift+T` new tab, `Ctrl+Shift+W` close, `Ctrl+Tab` switch).
- Vertical and horizontal split panes (`Ctrl+Shift+D` split, `Ctrl+Shift+Arrow` navigate).
- Independent PTY, history, and snapshot context per tab/pane.
- Tab titles auto-set from CWD or running command.

**Exit Criteria:** Glass runs reliably on macOS, Linux, and Windows with all core features. Users can open multiple tabs and split panes with independent terminal sessions.

### Phase 6: Packaging & Polish

**Goal:** Production-ready distribution, performance tuning, and public documentation.

- Windows: `.msi` installer, winget package, auto-update mechanism.
- macOS: `.dmg` with code signing and notarization, Homebrew cask.
- Linux: `.deb`, `.rpm`, AppImage, Flatpak.
- Auto-update mechanism (check for new versions, download in background).
- Performance profiling and optimization pass (startup time, memory, rendering throughput).
- Public documentation site and README.
- Configuration validation and error reporting on malformed `config.toml`.
- Config hot-reload (watch `config.toml` for changes, apply without restart).

**Exit Criteria:** Glass is installable via platform-native package managers, auto-updates, and has public documentation. Performance meets or exceeds PRD budgets on all three platforms.

---

## 11. Success Metrics

Vanity metrics (stars, DAU) are meaningless for a terminal. The only question that matters: **do people switch to Glass and stay?**

### 11.1 The Real Test

**Can I (or any developer) use Glass as my only terminal for a full work week without switching back?**

If yes, the foundation works. If not, nothing else matters — no amount of features will save a terminal that isn't solid enough for daily use.

### 11.2 Feature Validation

Each core feature has a simple "did it work?" test:

| Feature | Validated When |
|---|---|
| **Undo** | A user accidentally damages a file, hits undo, and recovers it without leaving the terminal. The reaction should be relief, not "I hope this works." |
| **Pipe visualization** | A user debugs a pipeline by reading the stage breakdown instead of inserting `tee`. They find the problem faster than they would have otherwise. |
| **Structured search** | A user finds a past command/error they would have otherwise re-investigated from scratch. Saves real time, not theoretical time. |
| **MCP integration** | An AI assistant recovers context after a session reset by querying Glass history. The user doesn't have to manually re-explain what happened. |

### 11.3 Technical Health

| Metric | Target |
|---|---|
| P95 input latency | <8ms |
| Crash rate | <0.1% of sessions |
| Snapshot creation latency | <10ms per file |
| History query latency (FTS) | <50ms |
| Cold start time | <200ms |

These are non-negotiable. A slow or crashy terminal gets uninstalled immediately, regardless of features.

---

## 12. Risks & Mitigations

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| **Filesystem monitoring misses changes** — race conditions between command start and monitor initialization | Medium | High — undo could restore stale state | Pre-command hooks, synchronous snapshot before execution, integration test suite for edge cases |
| **Pipe interception breaks commands** — some commands detect when stdout isn't a TTY and change behavior | High | Medium — altered output for some tools | Detect TTY-sensitive commands (e.g., `ls`, `git`) and preserve TTY pass-through; allow per-command opt-out |
| **Performance overhead deters adoption** — users switch to Glass but find it slower than their current terminal | Medium | High — primary churn reason | Strict performance budgets, continuous benchmarking in CI, opt-out flags for all features |
| **Snapshot storage bloat** — large projects with many file modifications consume excessive disk space | Medium | Medium — user frustration | Content-addressed dedup, aggressive pruning defaults, clear storage indicators in UI |
| **Shell compatibility issues** — different shells (bash, zsh, fish, PowerShell) have different piping and execution semantics | High | Medium — feature gaps per shell | Shell abstraction layer, extensive per-shell test matrix, community-contributed shell adapters |
| **Alacritty crate divergence** — upstream `alacritty_terminal` may evolve in directions incompatible with our modifications | Medium | Medium — maintenance burden | Pin to a specific version, contribute upstream where possible, maintain a thin adaptation layer to isolate our extensions from core VTE logic |

---

## 13. Decisions Made

These were originally open questions. Stances taken:

1. **VTE layer: Fork `alacritty_terminal`.** Decided. See section 6.4 for full rationale.

2. **Snapshots: Per-project by default, with global fallback.** If Glass detects a project root (via `.git`, `package.json`, `Cargo.toml`, etc.), snapshots go in `.glass/snapshots/` within that project. If no project root is found, snapshots go in `~/.glass/snapshots/` organized by absolute path. This covers both cases without requiring user configuration.

3. **Interactive commands in pipes: Auto-excluded from visualization.** Commands like `less`, `vim`, `fzf` that require TTY interaction are detected and shown as opaque blocks within the pipeline view. Glass maintains a known-interactive-commands list and also detects TTY allocation requests at runtime.

4. **Tabs and splits: Post-launch (Phase 5).** Not a differentiator. Users will request it, and we'll add it, but it's not why anyone switches to Glass.

5. **Licensing: MIT.** Keep it simple. No dual licensing, no commercial tier at launch. If enterprise features emerge later (team history sharing, audit logs), evaluate then. Premature licensing complexity kills contributor momentum.

## 14. Remaining Open Questions

1. **History query syntax.** The search overlay needs a query language. Options: (a) natural language parsed into SQL ("failed commands last hour"), (b) a minimal DSL (`status:failed time:1h`), (c) raw SQL for power users. Leaning toward (b) with (a) as a stretch goal — but need to prototype to know what feels right.

2. **How to handle `Ctrl+Z` conflicts.** Many CLI tools (bash job control, nano, vim) already use `Ctrl+Z` for suspend/SIGTSTP. Glass needs a strategy: (a) only intercept `Ctrl+Z` when no foreground process is running, (b) use a different keybinding like `Ctrl+Shift+Z`, (c) detect whether the current foreground process handles SIGTSTP and defer to it. Need to prototype and see what feels natural.

3. **Snapshot granularity for long-running commands.** A build script that runs for 60 seconds and modifies 200 files — do we snapshot all 200 files at command start? That could be slow and wasteful. Alternative: snapshot lazily on first write to each file (intercept at the FS monitor level). Needs benchmarking.

---

## 15. Non-Goals

These are explicitly out of scope for Glass:

- **Built-in AI chat.** Glass is not an AI assistant. It exposes data *to* AI assistants via MCP. Warp and Wave already do built-in AI.
- **IDE features.** No file explorer, no editor, no language server integration. Glass is a terminal, not VS Code.
- **Plugin system.** Not at launch. Plugins add complexity and maintenance burden. The core three features must be solid first.
- **Cloud sync.** History and snapshots stay local. No accounts, no telemetry, no cloud dependencies.
- **Theme marketplace.** Ship with one dark and one light theme that look good. That's it.
