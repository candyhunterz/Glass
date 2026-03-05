# Architecture Research: v1.1 Structured Scrollback + MCP Server

**Domain:** SQLite history DB, MCP server, search overlay, CLI query -- integrating into existing GPU-accelerated terminal emulator
**Researched:** 2026-03-05
**Confidence:** HIGH (based on direct source code analysis of existing Glass v1.0 codebase + official rusqlite/MCP docs)

---

## System Overview: v1.1 Additions

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        Application Layer                                │
│  glass binary (main.rs)                                                 │
│  ┌──────────────┐  ┌──────────────┐  ┌────────────────────────────┐    │
│  │  Processor   │  │  WindowCtx   │  │  CLI Subcommands           │    │
│  │  (winit loop)│  │  (per-window)│  │  (glass history search ..) │    │
│  └──────┬───────┘  └──────┬───────┘  └────────────────────────────┘    │
│         │                 │            NEW: CLI binary entry point      │
├─────────┼─────────────────┼────────────────────────────────────────────┤
│         │    Service Layer                                              │
│  ┌──────▼──────────────┐  │  ┌──────────────┐  ┌──────────────────┐   │
│  │  glass_terminal     │  │  │glass_renderer│  │  glass_core      │   │
│  │  PTY + OscScanner   │  │  │ wgpu + GPU   │  │  events, config  │   │
│  │  BlockManager ──────┼──┤  │              │  │                  │   │
│  │  +OutputCapture NEW │  │  │ +SearchOverlay│  │  +HistoryEvent   │   │
│  └─────────┬───────────┘  │  │  NEW         │  │  NEW             │   │
│            │              │  └──────────────┘  └──────────────────┘   │
│            │              │                                            │
│  ┌─────────▼───────────┐  │  ┌──────────────────────────────────────┐  │
│  │  glass_history NEW  │  │  │  glass_mcp NEW                      │  │
│  │  SQLite + FTS5      │◄─┘  │  JSON-RPC stdio server              │  │
│  │  HistoryDb          │◄────│  reads from glass_history            │  │
│  │  RetentionPolicy    │     │  GlassHistory + GlassContext tools   │  │
│  └─────────────────────┘     └──────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## New Components

### 1. glass_history: SQLite History Database

**Purpose:** Persist command metadata and output text into a queryable SQLite database with FTS5 full-text indexing.

**Dependencies:** `rusqlite 0.38` (already in workspace with `bundled` feature), `glass_core`

**Key types:**

```rust
// glass_history/src/lib.rs
pub struct HistoryDb {
    conn: rusqlite::Connection,
}

pub struct CommandRecord {
    pub id: i64,
    pub command_text: String,       // captured from OSC 133;B..133;C range
    pub output_text: String,        // captured from OSC 133;C..133;D range
    pub cwd: String,                // from OSC 7/9;9 at time of execution
    pub exit_code: Option<i32>,     // from OSC 133;D
    pub duration_ms: Option<u64>,   // computed from started_at/finished_at
    pub started_at: i64,            // Unix timestamp (milliseconds)
    pub finished_at: Option<i64>,   // Unix timestamp (milliseconds)
    pub hostname: String,           // machine identifier
    pub shell: String,              // "pwsh", "bash", etc.
}

pub struct SearchResult {
    pub record: CommandRecord,
    pub rank: f64,                  // BM25 relevance score from FTS5
    pub snippet: String,            // FTS5 snippet() highlight
}

pub struct RetentionPolicy {
    pub max_age_days: u32,          // default: 90
    pub max_entries: u64,           // default: 50_000
    pub max_db_size_mb: u64,        // default: 500
}
```

**Schema design:**

```sql
-- Main table
CREATE TABLE commands (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    command_text TEXT NOT NULL,
    output_text TEXT NOT NULL DEFAULT '',
    cwd TEXT NOT NULL,
    exit_code INTEGER,
    duration_ms INTEGER,
    started_at INTEGER NOT NULL,      -- Unix millis
    finished_at INTEGER,
    hostname TEXT NOT NULL,
    shell TEXT NOT NULL
);

-- FTS5 virtual table for full-text search over command + output
CREATE VIRTUAL TABLE commands_fts USING fts5(
    command_text,
    output_text,
    content='commands',
    content_rowid='id',
    tokenize='unicode61'
);

-- Triggers to keep FTS index in sync
CREATE TRIGGER commands_ai AFTER INSERT ON commands BEGIN
    INSERT INTO commands_fts(rowid, command_text, output_text)
    VALUES (new.id, new.command_text, new.output_text);
END;

CREATE TRIGGER commands_ad AFTER DELETE ON commands BEGIN
    INSERT INTO commands_fts(commands_fts, rowid, command_text, output_text)
    VALUES ('delete', old.id, old.command_text, old.output_text);
END;

-- Indexes for common query patterns
CREATE INDEX idx_commands_started_at ON commands(started_at DESC);
CREATE INDEX idx_commands_cwd ON commands(cwd);
CREATE INDEX idx_commands_exit_code ON commands(exit_code);
```

**Thread safety model:** `rusqlite::Connection` is `Send` but not `Sync`. The `HistoryDb` should be owned by a single writer thread. Reads for search/MCP use a separate read-only connection opened with `SQLITE_OPEN_READ_ONLY`. SQLite WAL mode enables concurrent readers with one writer.

```rust
impl HistoryDb {
    /// Open or create the database at ~/.glass/history.db
    pub fn open(path: &Path) -> Result<Self>;

    /// Insert a completed command record. Called from main thread
    /// when BlockManager transitions a block to Complete.
    pub fn insert(&self, record: &CommandRecord) -> Result<i64>;

    /// Full-text search across command text and output.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>>;

    /// Query by filters (cwd, exit_code, date range).
    pub fn query(&self, filter: &QueryFilter) -> Result<Vec<CommandRecord>>;

    /// Apply retention policy: delete old/excess entries.
    pub fn enforce_retention(&self, policy: &RetentionPolicy) -> Result<usize>;

    /// Open a read-only connection for concurrent access (MCP/CLI).
    pub fn open_readonly(path: &Path) -> Result<Self>;
}
```

### 2. glass_mcp: MCP Server

**Purpose:** Expose terminal history to AI assistants via the Model Context Protocol (JSON-RPC 2.0 over stdio).

**Dependencies:** `glass_history`, `glass_core`, `serde`, `serde_json`, `tokio` (already in workspace)

**Architecture decision: Use raw JSON-RPC, not rmcp SDK.** The MCP server for Glass is simple (2 tools, stdio transport only). The `rmcp` SDK adds significant dependency weight and complexity. Hand-rolling JSON-RPC parsing for 2 tool handlers is ~200 lines of straightforward serde code. This avoids pulling in the entire SDK for a narrow use case.

**Key design:**

```rust
// glass_mcp/src/lib.rs
pub struct McpServer {
    history_db: HistoryDb,  // read-only connection
}

// Tool definitions exposed to MCP clients
pub const TOOLS: &[ToolDefinition] = &[
    ToolDefinition {
        name: "GlassHistory",
        description: "Search terminal command history with full-text search",
        input_schema: /* JSON Schema for query, limit, cwd_filter, exit_code_filter */,
    },
    ToolDefinition {
        name: "GlassContext",
        description: "Get recent terminal context (last N commands with output)",
        input_schema: /* JSON Schema for count, cwd */,
    },
];
```

**Execution model:** The MCP server runs as a separate process, NOT inside the terminal's winit event loop. It is invoked as `glass mcp serve` which:
1. Opens `~/.glass/history.db` in read-only mode
2. Reads JSON-RPC requests from stdin
3. Writes JSON-RPC responses to stdout
4. Logs to stderr (critical: stdout is the protocol channel)

This matches the MCP stdio transport specification. The AI assistant (Claude, etc.) spawns `glass mcp serve` as a subprocess.

### 3. Output Capture (modification to glass_terminal)

**Purpose:** Capture command output text from the terminal grid for storage in the history database.

**Integration point:** This is the trickiest new component because it needs to extract plain text from the `alacritty_terminal::Term` grid between specific line ranges (the block's output_start_line to output_end_line).

```rust
// glass_terminal/src/output_capture.rs (NEW)
use alacritty_terminal::term::Term;
use alacritty_terminal::grid::Dimensions;

/// Extract plain text from the terminal grid between two line ranges.
/// Used when a block transitions to Complete to capture its output.
pub fn capture_output(
    term: &Term<impl alacritty_terminal::event::EventListener>,
    start_line: usize,
    end_line: usize,
) -> String {
    // Walk the grid rows from start_line to end_line,
    // extract characters, trim trailing whitespace per line
    // This runs under the existing Term lock during the
    // CommandFinished event processing
}
```

**Critical constraint:** The output capture must happen while the term lock is held, during the same event processing that handles `CommandFinished`. The output lines are in the scrollback buffer, which can be overwritten if capture is deferred. This means capture MUST be synchronous with the block completion event.

### 4. Search Overlay (modification to glass_renderer)

**Purpose:** A floating search UI triggered by Ctrl+Shift+F that queries the history database and displays results over the terminal content.

**New renderer component:**

```rust
// glass_renderer/src/search_overlay.rs (NEW)
pub struct SearchOverlay {
    visible: bool,
    query: String,
    results: Vec<SearchResultDisplay>,
    selected_index: usize,
    cursor_position: usize,
}

pub struct SearchResultDisplay {
    pub command: String,
    pub cwd: String,
    pub exit_code: Option<i32>,
    pub timestamp: String,
    pub snippet: String,     // highlighted match from FTS5
}
```

**Rendering approach:** The search overlay draws on top of the terminal content as a semi-transparent panel. It uses the existing `RectRenderer` for the background panel and `GlyphCache`/`TextRenderer` for the text. It renders in a separate render pass after the main terminal frame, or as the last items in the same pass with a higher z-order (since wgpu does not have z-ordering, render order determines occlusion).

**Input handling:** When the overlay is visible, keyboard input is intercepted by the Processor before forwarding to the PTY. Escape closes the overlay. Enter selects a result. Arrow keys navigate results. Character keys update the search query.

### 5. CLI Query Interface (modification to glass binary)

**Purpose:** Allow `glass history search <query>` and similar commands from any terminal.

**Design:** The `glass` binary gains a subcommand router. If invoked with arguments (e.g., `glass history search "cargo build"`), it runs the CLI path instead of launching the terminal UI.

```rust
// src/main.rs modification
fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 && args[1] == "history" {
        // CLI mode: query the database directly, print results, exit
        cli::handle_history_command(&args[2..]);
        return;
    }

    if args.len() > 1 && args[1] == "mcp" {
        // MCP server mode: run JSON-RPC stdio loop
        mcp::run_server();
        return;
    }

    // Default: launch terminal UI (existing behavior)
    launch_terminal();
}
```

This avoids a separate binary. The `glass` executable serves three roles:
1. Terminal emulator (default, no args)
2. History CLI (`glass history ...`)
3. MCP server (`glass mcp serve`)

---

## Modifications to Existing Components

### glass_core: New Event Variants

**File:** `crates/glass_core/src/event.rs`

Add a new event variant for history recording:

```rust
pub enum AppEvent {
    // ... existing variants unchanged ...

    /// A command block has completed and should be recorded to history.
    /// Sent from main thread after output capture, received by history writer.
    CommandCompleted {
        window_id: WindowId,
        command_text: String,
        output_text: String,
        cwd: String,
        exit_code: Option<i32>,
        duration_ms: Option<u64>,
        started_at: i64,
    },
}
```

Wait -- `AppEvent` flows through the winit `EventLoopProxy`, which requires `Send`. But the history DB write should NOT happen on the main thread (it blocks). Two options:

**Option A (recommended): Direct channel to history writer thread.** Add a `mpsc::Sender<CommandRecord>` to `WindowContext`. When a block completes, capture output under the term lock, construct the record, send it through the channel. A background thread owns the `HistoryDb` and drains the channel.

**Option B: Use AppEvent.** Add the variant above and handle it in `user_event()` by spawning a blocking task. This is simpler but puts DB writes on the main thread's task spawner.

**Recommendation: Option A.** It keeps the main thread free from any DB I/O and matches the existing pattern of dedicated threads for blocking work (PTY reader, git query).

### glass_core: Config Extension

**File:** `crates/glass_core/src/config.rs`

```rust
pub struct GlassConfig {
    // ... existing fields ...

    /// History database settings
    pub history: HistoryConfig,
}

pub struct HistoryConfig {
    pub enabled: bool,              // default: true
    pub db_path: Option<PathBuf>,   // default: ~/.glass/history.db
    pub retention_days: u32,        // default: 90
    pub max_entries: u64,           // default: 50_000
    pub max_output_bytes: usize,    // default: 64KB per command (truncate long outputs)
}
```

### glass_terminal: BlockManager Enhancement

**File:** `crates/glass_terminal/src/block_manager.rs`

The existing `Block` struct uses `Instant` for timing, which cannot be serialized. Add serializable timestamps:

```rust
pub struct Block {
    // ... existing fields ...

    /// Unix timestamp (millis) when command started executing.
    /// Added for history DB recording (Instant is not serializable).
    pub started_at_unix: Option<i64>,

    /// The CWD at the time this command executed.
    pub cwd: Option<String>,

    /// The command text extracted from the input region.
    /// Captured when CommandExecuted fires (between B and C markers).
    pub command_text: Option<String>,
}
```

The `handle_event` method needs modification for `CommandExecuted` to capture:
1. The current CWD from `StatusState`
2. A Unix timestamp via `std::time::SystemTime::now()`
3. The command text from the terminal grid (between `command_start_line` and the current line)

### glass_terminal: Command Text Capture

The command text (what the user typed) lives in the terminal grid between the `command_start_line` (OSC 133;B) and the current cursor line when `CommandExecuted` (OSC 133;C) fires. This text must be extracted from the `Term` grid.

**Challenge:** When `CommandExecuted` fires, we are in the PTY reader thread inside `pty_read_with_scan`. The `Term` lock is already held. We can read grid cells at this point.

**Solution:** Extend `pty_read_with_scan` to pass the locked terminal reference when emitting shell events, so the main thread handler (or a callback) can capture text. Alternatively, capture in the PTY thread itself before sending the event.

**Recommended approach:** Capture in the PTY reader thread while the lock is held. Add a `TerminalAccessor` callback or closure that runs during event dispatch while the term is locked. This avoids a second lock acquisition on the main thread.

```rust
// In pty_read_with_scan, when OscEvent::CommandExecuted is detected:
let command_text = capture_grid_text(
    terminal_ref,
    block_command_start_line,
    current_cursor_line,
);
// Include command_text in the Shell event
```

**Problem:** The PTY reader thread does not currently know the `block_command_start_line`. BlockManager state is owned by the main thread. This creates a circular dependency.

**Resolution:** Move the line tracking (command_start_line bookkeeping) into a lightweight struct in the PTY thread, separate from the full BlockManager. The PTY thread tracks: "the line where the last 133;B was seen" and "the line where the last 133;C was seen". It sends both the text capture and the line numbers to the main thread. The main thread's BlockManager then uses these for block tracking and UI rendering.

```rust
// In the PTY reader loop, alongside OscScanner:
struct PtyBlockTracker {
    last_b_line: Option<usize>,  // line where last 133;B was seen
    last_cwd: Option<String>,    // last OSC 7/9;9 value
}
```

### glass_renderer: Search Overlay Integration

**File:** `crates/glass_renderer/src/frame.rs`

The `FrameRenderer::draw_frame` method needs an additional parameter for overlay state:

```rust
pub fn draw_frame(
    &mut self,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &wgpu::TextureView,
    width: u32,
    height: u32,
    snapshot: &GridSnapshot,
    visible_blocks: &[&Block],
    status: Option<&StatusState>,
    search_overlay: Option<&SearchOverlay>,  // NEW
) {
    // ... existing rendering ...

    // After status bar, render search overlay if visible
    if let Some(overlay) = search_overlay {
        self.search_overlay_renderer.draw(
            device, queue, view, width, height, overlay,
            &mut self.glyph_cache, &mut self.overlay_buffers,
        );
    }
}
```

### main.rs: Wiring Changes

The `Processor` struct gains:
1. A history writer channel (`mpsc::Sender<CommandRecord>`)
2. Search overlay state (per window, inside `WindowContext`)

The `WindowContext` struct gains:
```rust
struct WindowContext {
    // ... existing fields ...

    /// Search overlay state
    search_overlay: SearchOverlay,

    /// Channel to send completed commands to the history writer thread
    history_tx: mpsc::Sender<CommandRecord>,
}
```

Keyboard handling in `window_event` gains:
- Ctrl+Shift+F: toggle search overlay visibility
- When overlay is visible: intercept all keyboard input for search

The `user_event` handler for `AppEvent::Shell` gains:
- On `CommandFinished`: capture output from term grid, construct `CommandRecord`, send to `history_tx`

---

## Data Flow: Command Lifecycle to History DB

```
User types "cargo build" + Enter
    │
    ├── Shell emits OSC 133;B (command input start)
    │   └── PTY reader: PtyBlockTracker records last_b_line = cursor line
    │
    ├── Shell emits OSC 133;C (command executing)
    │   └── PTY reader: captures command text from grid[last_b_line..cursor_line]
    │       sends AppEvent::Shell { event: CommandExecuted, command_text: "cargo build" }
    │
    ├── Command runs, output streams to terminal
    │   └── Normal PTY read loop, VTE parser updates grid
    │
    ├── Shell emits OSC 133;D;0 (command finished, exit code 0)
    │   └── PTY reader: captures output text from grid[last_c_line..cursor_line]
    │       sends AppEvent::Shell { event: CommandFinished { exit_code: 0 }, output_text: "..." }
    │
    ▼
Main thread: user_event(AppEvent::Shell { CommandFinished })
    │
    ├── BlockManager.handle_event() → block transitions to Complete
    │
    ├── Construct CommandRecord {
    │     command_text, output_text, cwd, exit_code,
    │     duration_ms, started_at, hostname, shell
    │   }
    │
    └── history_tx.send(record)
         │
         ▼
History Writer Thread (background, owns HistoryDb)
    │
    ├── recv() from channel
    ├── HistoryDb::insert(record)
    └── Periodically: HistoryDb::enforce_retention(policy)
```

## Data Flow: Search Overlay

```
User presses Ctrl+Shift+F
    │
    ▼
Processor::window_event(KeyboardInput)
    │
    ├── search_overlay.visible = true
    ├── window.request_redraw()
    │
    ▼ (user types search query)
    │
Character keys → search_overlay.query.push(c)
    │
    ├── HistoryDb::search(query, limit: 20)  // read-only connection
    │   └── FTS5 MATCH query with BM25 ranking
    │
    ├── search_overlay.results = results
    └── window.request_redraw()

    ▼ (user presses Enter on a result)
    │
    ├── Copy selected command to clipboard / scroll to block
    └── search_overlay.visible = false
```

**Threading concern for search:** The search query hits SQLite, which is blocking I/O. Running it on the main thread would block rendering. Options:

**Option A (simple, acceptable for v1.1):** Open a read-only SQLite connection on the main thread. FTS5 queries on a local database are typically <5ms for reasonable corpus sizes. This is within the frame budget.

**Option B (future-proof):** Debounce the query (200ms after last keystroke) and run it on a background thread, sending results back via `EventLoopProxy`. More complex but prevents any stutter on large databases.

**Recommendation: Start with Option A**, measure actual query latency, add debouncing if it exceeds 5ms.

## Data Flow: MCP Server

```
AI Assistant (Claude Desktop, Cursor, etc.)
    │
    ├── Spawns: glass mcp serve (subprocess)
    │   └── stdin/stdout = JSON-RPC 2.0 channel
    │
    ├── Sends: {"jsonrpc":"2.0","method":"tools/list","id":1}
    │   └── Response: tool definitions (GlassHistory, GlassContext)
    │
    ├── Sends: {"jsonrpc":"2.0","method":"tools/call","params":{
    │     "name":"GlassHistory","arguments":{"query":"cargo build error"}
    │   },"id":2}
    │   │
    │   ▼
    │   McpServer reads history.db (read-only connection)
    │   FTS5 search → results
    │   │
    │   └── Response: {"jsonrpc":"2.0","result":{...matching commands...},"id":2}
    │
    └── AI uses results to inform its response to the user
```

---

## Component Responsibilities

| Component | Responsibility | New/Modified | Communicates With |
|-----------|----------------|--------------|-------------------|
| `glass_history::HistoryDb` | SQLite connection, FTS5 schema, CRUD, retention | NEW | glass_core (config) |
| `glass_history::RetentionPolicy` | Age/count/size limits, cleanup logic | NEW | HistoryDb |
| `glass_mcp::McpServer` | JSON-RPC stdio loop, tool dispatch | NEW | glass_history (read-only) |
| `glass_terminal::PtyBlockTracker` | Track B/C line positions in PTY thread | NEW | OscScanner, Term grid |
| `glass_terminal::output_capture` | Extract text from Term grid line ranges | NEW | alacritty_terminal::Term |
| `glass_renderer::SearchOverlay` | Search UI state and rendering | NEW | HistoryDb (read-only), FrameRenderer |
| `glass_core::event::AppEvent` | Add command text/output to Shell events | MODIFIED | All crates |
| `glass_core::config::GlassConfig` | Add HistoryConfig section | MODIFIED | glass_history |
| `glass_terminal::BlockManager` | Add unix timestamps, cwd tracking | MODIFIED | glass_history (indirect) |
| `main.rs::Processor` | Wire history channel, search overlay | MODIFIED | All crates |
| `main.rs::WindowContext` | Add search_overlay, history_tx | MODIFIED | glass_renderer, glass_history |

---

## Architectural Patterns

### Pattern 1: Dedicated Writer Thread with Channel

**What:** A single background thread owns the `HistoryDb` write connection and receives `CommandRecord` values via an `mpsc::channel`. This is the same pattern as the existing PTY reader thread.

**When to use:** Whenever blocking I/O (SQLite writes) must not block the winit event loop.

**Trade-offs:**
- Pro: Zero main-thread blocking, simple ownership model, no `Sync` requirement on `rusqlite::Connection`
- Con: Slight latency between command completion and DB persistence (typically <1ms for channel send + SQLite insert)

```rust
fn spawn_history_writer(
    db_path: PathBuf,
    policy: RetentionPolicy,
) -> mpsc::Sender<CommandRecord> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("Glass history writer".into())
        .spawn(move || {
            let db = HistoryDb::open(&db_path).expect("Failed to open history DB");
            let mut insert_count = 0u64;
            while let Ok(record) = rx.recv() {
                if let Err(e) = db.insert(&record) {
                    tracing::error!("History insert failed: {e}");
                }
                insert_count += 1;
                // Enforce retention every 100 inserts
                if insert_count % 100 == 0 {
                    let _ = db.enforce_retention(&policy);
                }
            }
        })
        .expect("Failed to spawn history writer thread");
    tx
}
```

### Pattern 2: WAL Mode for Concurrent Read/Write

**What:** SQLite WAL (Write-Ahead Logging) mode enables one writer and multiple concurrent readers without blocking each other.

**When to use:** When the history writer thread is inserting records while the search overlay and MCP server are querying.

**Trade-offs:**
- Pro: No read blocking during writes, no write blocking during reads
- Pro: Readers see a consistent snapshot even during writes
- Con: WAL file grows until checkpointed (automatic, but can use disk space)

```rust
impl HistoryDb {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = rusqlite::Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;  // safe with WAL
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // ... create tables if not exists ...
        Ok(Self { conn })
    }
}
```

### Pattern 3: Overlay Input Interception

**What:** When the search overlay is visible, the Processor intercepts keyboard events before they reach the PTY encoder. This prevents search keystrokes from being sent to the shell.

**When to use:** Any modal overlay (search, future command palette, etc.).

```rust
// In Processor::window_event, KeyboardInput handler:
if ctx.search_overlay.visible {
    match &event.logical_key {
        Key::Named(NamedKey::Escape) => ctx.search_overlay.close(),
        Key::Named(NamedKey::Enter) => ctx.search_overlay.select(),
        Key::Named(NamedKey::ArrowUp) => ctx.search_overlay.move_selection(-1),
        Key::Named(NamedKey::ArrowDown) => ctx.search_overlay.move_selection(1),
        Key::Character(c) => ctx.search_overlay.type_char(c),
        Key::Named(NamedKey::Backspace) => ctx.search_overlay.backspace(),
        _ => {}
    }
    ctx.window.request_redraw();
    return;  // Do NOT forward to PTY
}
```

---

## Anti-Patterns

### Anti-Pattern 1: SQLite on the Main Thread

**What people do:** Open a SQLite connection in the Processor and do inserts/queries during `user_event()` or `window_event()`.

**Why it's wrong:** SQLite I/O blocks the winit event loop. Even fast queries (1-5ms) steal frame budget. FTS5 index updates during inserts can take 10-50ms on large databases, causing visible frame drops.

**Do this instead:** Dedicated writer thread for inserts. For search queries, either use a read-only connection with measured latency (acceptable if <5ms) or offload to a background thread with async result delivery.

### Anti-Pattern 2: Capturing Output After Block Completion

**What people do:** When a block transitions to Complete, schedule output capture for "later" -- e.g., on the next frame or in a background task.

**Why it's wrong:** The terminal scrollback buffer has a fixed size (10,000 lines in Glass). If a subsequent command produces large output, the completed block's output lines can scroll out of the buffer before capture happens. Lost data.

**Do this instead:** Capture output text synchronously when `CommandFinished` fires, while the term lock is held and the grid lines are still in the scrollback buffer.

### Anti-Pattern 3: Sharing HistoryDb Across Threads with Mutex

**What people do:** Wrap `HistoryDb` in `Arc<Mutex<HistoryDb>>` so multiple threads can read and write.

**Why it's wrong:** The Mutex blocks one thread while another is doing I/O. SQLite already handles concurrency internally with WAL mode. Multiple connections are better than one shared connection.

**Do this instead:** One write connection (owned by writer thread), separate read-only connections for search overlay and MCP server. SQLite WAL mode handles the concurrency.

### Anti-Pattern 4: MCP Server Inside the Terminal Process

**What people do:** Run the MCP JSON-RPC handler inside the terminal's winit event loop, multiplexing protocol messages with terminal events.

**Why it's wrong:** MCP stdio transport requires exclusive ownership of stdin/stdout. The terminal process already uses ConPTY for stdin/stdout. Mixing them is impossible without a separate channel mechanism.

**Do this instead:** MCP server runs as a separate invocation of the `glass` binary (`glass mcp serve`). It is a different process that only reads the history database, completely independent of the running terminal.

---

## Suggested Build Order

Dependencies flow bottom-to-top. Build in this order to ensure each piece is testable before the next begins:

```
Phase 1: glass_history (SQLite + FTS5)
    │  No dependency on terminal/renderer
    │  Fully testable with unit tests
    │  Schema creation, CRUD, FTS5 search, retention
    ▼
Phase 2: Output capture + BlockManager enhancement
    │  Depends on: glass_terminal internals
    │  PtyBlockTracker, output_capture module
    │  Wire into PTY read loop and main thread
    │  History writer thread + channel
    ▼
Phase 3: CLI query interface
    │  Depends on: glass_history
    │  Subcommand routing in main.rs
    │  glass history search / list / stats
    │  Testable from command line immediately
    ▼
Phase 4: Search overlay UI
    │  Depends on: glass_history, glass_renderer
    │  SearchOverlay renderer component
    │  Input interception in Processor
    │  Ctrl+Shift+F keybinding
    ▼
Phase 5: MCP server
    │  Depends on: glass_history
    │  JSON-RPC stdio protocol handler
    │  GlassHistory + GlassContext tool implementations
    │  glass mcp serve subcommand
    ▼
Phase 6: Retention policies + polish
    │  Depends on: all above
    │  Periodic retention enforcement
    │  Config TOML extensions
    │  Performance measurement
```

**Rationale for this order:**
- **glass_history first** because everything else depends on it, and it has zero dependencies on the rest of the system. Can be developed and tested in complete isolation.
- **Output capture second** because it is the hardest integration -- it modifies the PTY read loop, which is the most sensitive code path. Getting this wrong breaks terminal functionality. Better to tackle it early when the scope is small.
- **CLI third** because it provides immediate validation of glass_history without requiring any renderer changes. You can verify the database is being populated correctly.
- **Search overlay fourth** because it requires renderer modifications and input handling changes. By this point, the database is proven working.
- **MCP server fifth** because it is the most self-contained new crate. It only reads from the database and has no interaction with the terminal's event loop. It could actually be built in parallel with phases 3-4.
- **Retention last** because it is a correctness/polish concern, not a functionality concern. The system works without it; it just grows unbounded.

---

## Integration Points

### Internal Boundaries

| Boundary | Communication | Thread Safety | Notes |
|----------|---------------|---------------|-------|
| PTY thread -> History writer | `mpsc::Sender<CommandRecord>` | Channel (Send) | Non-blocking send from PTY thread |
| Main thread -> History writer | Same channel | Channel (Send) | Alternative: main thread sends after output capture |
| Search overlay -> HistoryDb | Direct method call (read-only conn) | Single-threaded (main thread) | Consider background thread if latency >5ms |
| MCP server -> HistoryDb | Direct method call (read-only conn) | Single-threaded (MCP process) | Separate OS process, not shared memory |
| CLI -> HistoryDb | Direct method call (read-only conn) | Single-threaded (CLI process) | Separate OS process |
| main.rs -> SearchOverlay | Direct struct access in WindowContext | Main thread only | No locking needed |

### File System

| Path | Purpose | Created By |
|------|---------|------------|
| `~/.glass/history.db` | SQLite database | glass_history on first insert |
| `~/.glass/history.db-wal` | WAL journal | SQLite automatically |
| `~/.glass/history.db-shm` | Shared memory for WAL | SQLite automatically |
| `~/.glass/config.toml` | Config with [history] section | User (existing) |

### Cargo Dependencies

```toml
# glass_history/Cargo.toml
[dependencies]
glass_core = { path = "../glass_core" }
rusqlite = { workspace = true }
serde = { workspace = true }
tracing = { workspace = true }

# glass_mcp/Cargo.toml
[dependencies]
glass_history = { path = "../glass_history" }
glass_core = { path = "../glass_core" }
serde = { workspace = true }
serde_json = "1"
tokio = { workspace = true }  # for async stdin/stdout if needed
tracing = { workspace = true }

# Workspace Cargo.toml additions needed:
serde_json = "1"

# Root binary Cargo.toml additions:
glass_history = { path = "crates/glass_history" }
glass_mcp = { path = "crates/glass_mcp" }
```

---

## Scaling Considerations

| Scale | Architecture Approach |
|-------|-----------------------|
| 100 commands/day (typical user) | Everything works fine. DB stays <10MB for years. |
| 1,000 commands/day (power user) | FTS5 queries still sub-millisecond. Retention policy keeps DB bounded. |
| 50,000+ records (accumulated history) | FTS5 BM25 ranking remains fast. `LIMIT` clauses on queries. Retention enforcement matters. |
| Long outputs (>1MB per command) | `max_output_bytes` config truncates at 64KB default. Prevents DB bloat from `cat bigfile`. |
| Multiple Glass instances | WAL mode handles concurrent writers gracefully. Each instance writes to the same DB. |
| MCP server under load | Read-only connections don't block the writer. SQLite handles this natively with WAL. |

### First Bottleneck: Output Capture Size

Long-running commands (e.g., large compilation) produce megabytes of output. Capturing all of it would bloat the database. The `max_output_bytes` config parameter truncates output, keeping the last N bytes (tail, since recent output is most useful).

### Second Bottleneck: FTS5 Index Size

With 50,000+ records containing output text, the FTS5 index can grow to hundreds of MB. The retention policy (max_entries, max_age_days, max_db_size_mb) prevents unbounded growth. The `enforce_retention` method deletes old records AND rebuilds the FTS index with `INSERT INTO commands_fts(commands_fts) VALUES('rebuild')`.

---

## Sources

- [SQLite FTS5 Extension](https://sqlite.org/fts5.html) -- official FTS5 documentation, query syntax, BM25 ranking
- [rusqlite crate](https://docs.rs/rusqlite/0.38/rusqlite/) -- Rust SQLite bindings, version 0.38 with bundled feature
- [MCP Official Rust SDK (rmcp)](https://github.com/modelcontextprotocol/rust-sdk) -- official SDK, evaluated but not recommended for this use case
- [MCP Specification: stdio transport](https://modelcontextprotocol.io/docs/develop/build-server) -- JSON-RPC 2.0 over stdin/stdout
- [How to Build a stdio MCP Server in Rust](https://www.shuttle.dev/blog/2025/07/18/how-to-build-a-stdio-mcp-server-in-rust) -- practical implementation guide
- Glass v1.0 source code -- direct analysis of all files in the workspace

---

*Architecture research for: Glass v1.1 Structured Scrollback + MCP Server*
*Researched: 2026-03-05*
