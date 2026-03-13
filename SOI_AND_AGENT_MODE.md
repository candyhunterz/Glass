# Glass v3.0: Structured Output Intelligence & Agent Mode

## Vision

Glass becomes the first terminal emulator designed for two audiences simultaneously: **human eyes** get the rendered terminal grid, **AI agents** get structured, compressed, queryable intelligence through the same output pipeline. On top of this, Agent Mode makes Glass a proactive development partner — watching terminal activity and autonomously proposing fixes, optimizations, and next steps.

SOI is the foundation. Agent Mode is the product. You can't build Agent Mode without SOI, and SOI alone is valuable even without Agent Mode.

---

## Problem Statement

AI agents working in terminals today face three fundamental problems:

1. **Context waste**: Raw terminal output consumes thousands of tokens per command. A single `cargo test` run burns ~2000 tokens on noise. Across a 50-command session, agents lose 50K+ tokens to unstructured text — leaving less room for reasoning.

2. **No continuity**: When an agent session ends (context exhaustion, crash, user closes it), all understanding of what happened is lost. The next session starts from scratch.

3. **Reactive only**: Agents wait for the user to invoke them. If a build fails at minute 20 of an autonomous session and the agent has moved on, nobody catches it until the user returns.

Glass is uniquely positioned to solve all three because it already processes every byte of terminal output, tracks command lifecycles, and runs an MCP server for agent communication.

---

## Architecture Overview

```
Terminal Output (raw bytes from PTY)
    |
    ├──→ VT Parser → GPU Renderer (existing — for human eyes)
    |
    └──→ SOI Pipeline (new)
         |
         ├── Output Classifier
         |   Detects output type: compiler, test runner, package manager,
         |   git, docker, k8s, generic structured, freeform text
         |
         ├── Format-Specific Parsers
         |   Rust/cargo, Node/npm, Python/pytest, Go, Docker, kubectl,
         |   generic file:line:col, JSON lines, TAP, JUnit XML
         |
         ├── Structured Record Store (SQLite)
         |   Normalized records: errors, warnings, test results,
         |   build artifacts, timing data, diffs
         |
         ├── Compression Engine
         |   Produces token-budgeted summaries for agent consumption
         |   "3 failed, 247 passed, 2 warnings in auth.rs"
         |
         ├── Shell Summary Injection
         |   Appends one-line structured summary to command output
         |   so agents see it in their Bash tool results naturally
         |
         └──→ Agent Mode (new)
              |
              ├── Activity Stream
              |   Feeds compressed SOI summaries to persistent agent session
              |
              ├── Agent Runtime
              |   Background Claude CLI process watching activity stream
              |   Decides when to act based on its own judgment
              |
              ├── Worktree Isolation
              |   Agent works in git worktree, never touches working tree directly
              |
              └── Approval UI
                  Status bar notifications, diff preview overlay,
                  [Apply] [Edit] [Dismiss] controls
```

---

## Feature 1: Structured Output Intelligence (SOI)

### What It Does

SOI intercepts command output after capture and produces structured, queryable, compressed representations that AI agents can access through MCP tools. It also injects brief summaries directly into the terminal output stream so agents using the Bash tool see them naturally.

### Components

#### 1.1 Output Classifier

Determines what kind of output a command produced, using a combination of:
- **Command hint**: The command text itself (e.g., `cargo test` → Rust test runner)
- **Output pattern matching**: Regex-based detection of known formats
- **Exit code**: Success/failure context

```rust
pub enum OutputType {
    // Build/compile
    RustCompiler,       // cargo build, rustc
    TypeScript,         // tsc
    GoBuild,            // go build
    CppCompiler,        // gcc, g++, clang
    GenericCompiler,    // file:line:col pattern

    // Test runners
    RustTest,           // cargo test
    Jest,               // jest, npx jest
    Pytest,             // pytest, python -m pytest
    GoTest,             // go test
    GenericTAP,         // TAP protocol output

    // Package managers
    Npm,                // npm install, npm run
    Cargo,              // cargo add, cargo update
    Pip,                // pip install

    // DevOps / Infrastructure
    Docker,             // docker build, docker compose
    Kubectl,            // kubectl get, apply, describe
    Terraform,          // terraform plan, apply

    // Version control
    Git,                // git status, diff, log, merge

    // Structured data
    JsonLines,          // NDJSON output
    JsonObject,         // Single JSON blob
    Csv,                // CSV/TSV output

    // Fallback
    FreeformText,       // Unrecognized — store raw with basic stats
}
```

#### 1.2 Format-Specific Parsers

Each OutputType has a parser that extracts normalized records. Parsers are modular — new ones can be added without changing the pipeline.

```rust
pub struct ParsedOutput {
    pub output_type: OutputType,
    pub summary: OutputSummary,
    pub records: Vec<OutputRecord>,
    pub raw_line_count: usize,
    pub raw_byte_count: usize,
}

pub struct OutputSummary {
    pub one_line: String,           // "3 failed, 247 passed, 2 warnings"
    pub token_estimate: usize,     // Approximate tokens for the summary
    pub severity: Severity,         // Error, Warning, Info, Success
}

pub enum OutputRecord {
    CompilerError {
        file: String,
        line: u32,
        column: Option<u32>,
        severity: Severity,
        code: Option<String>,       // E0308, TS2345, etc.
        message: String,
        context_lines: Option<String>, // Surrounding source code shown by compiler
    },
    TestResult {
        name: String,               // test::module::test_name
        status: TestStatus,         // Passed, Failed, Skipped, Ignored
        duration_ms: Option<u64>,
        failure_message: Option<String>,
        failure_location: Option<String>, // file:line
    },
    TestSummary {
        passed: u32,
        failed: u32,
        skipped: u32,
        ignored: u32,
        total_duration_ms: Option<u64>,
    },
    PackageEvent {
        action: String,             // "added", "removed", "updated", "audited"
        package: String,
        version: Option<String>,
        detail: Option<String>,     // "3 vulnerabilities found"
    },
    GitEvent {
        action: String,             // "merge", "pull", "push", "conflict"
        detail: String,
        files_changed: Option<u32>,
        insertions: Option<u32>,
        deletions: Option<u32>,
    },
    DockerEvent {
        action: String,             // "build", "pull", "push", "up", "error"
        image: Option<String>,
        detail: String,
    },
    GenericDiagnostic {
        file: Option<String>,
        line: Option<u32>,
        severity: Severity,
        message: String,
    },
    FreeformChunk {
        text: String,               // For unrecognized output — compressed/sampled
        line_count: usize,
    },
}

pub enum TestStatus { Passed, Failed, Skipped, Ignored }
pub enum Severity { Error, Warning, Info, Success, Unknown }
```

#### 1.3 Structured Record Store

Parsed output is stored alongside command history in SQLite, extending the existing `commands` table.

```sql
-- New table linked to existing commands table
CREATE TABLE command_output_records (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    command_id      INTEGER NOT NULL REFERENCES commands(id) ON DELETE CASCADE,
    output_type     TEXT NOT NULL,           -- OutputType enum as string
    summary_line    TEXT NOT NULL,           -- One-line human/agent summary
    severity        TEXT NOT NULL,           -- error, warning, info, success
    record_count    INTEGER NOT NULL,        -- Total structured records extracted
    raw_lines       INTEGER NOT NULL,        -- Original output line count
    raw_bytes       INTEGER NOT NULL,        -- Original output byte count
    parsed_json     TEXT NOT NULL            -- JSON array of OutputRecord
);
CREATE INDEX idx_cor_command ON command_output_records(command_id);
CREATE INDEX idx_cor_severity ON command_output_records(severity);
CREATE INDEX idx_cor_type ON command_output_records(output_type);

-- Individual records for granular querying
CREATE TABLE output_records (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    command_id      INTEGER NOT NULL REFERENCES commands(id) ON DELETE CASCADE,
    record_type     TEXT NOT NULL,           -- "compiler_error", "test_result", etc.
    severity        TEXT,
    file            TEXT,                    -- Source file (if applicable)
    line            INTEGER,                 -- Line number (if applicable)
    message         TEXT NOT NULL,           -- Primary message/name
    detail_json     TEXT                     -- Full record as JSON for drill-down
);
CREATE INDEX idx_or_command ON output_records(command_id);
CREATE INDEX idx_or_file ON output_records(file);
CREATE INDEX idx_or_severity ON output_records(severity);
CREATE INDEX idx_or_type ON output_records(record_type);
```

#### 1.4 Compression Engine

Produces token-budgeted summaries at multiple granularity levels:

```rust
pub enum CompressionLevel {
    OneLine,        // "3 errors in 2 files" (~10 tokens)
    Summary,        // Error list with file:line, no context (~100 tokens)
    Detailed,       // Errors with messages and context (~500 tokens)
    Full,           // Complete parsed output (~1000+ tokens)
}

impl CompressionEngine {
    /// Produce a summary within the given token budget
    pub fn compress(
        &self,
        parsed: &ParsedOutput,
        budget_tokens: usize,
    ) -> CompressedOutput;

    /// Drill into a specific record with full detail
    pub fn expand_record(
        &self,
        record_id: i64,
    ) -> OutputRecord;
}
```

#### 1.5 Shell Summary Injection

After a command completes, Glass injects a brief summary line into the terminal output stream. This is the critical integration point that makes SOI discoverable to AI agents without requiring them to change their workflow.

**Mechanism**: After `CommandFinished` is processed and SOI parsing completes, Glass writes a summary line to the PTY's input side (so it appears as terminal output). The line is visually subtle for humans but informative for agents:

```
$ cargo test
   Compiling myapp v0.1.0
   ... (normal output) ...
test result: FAILED. 3 failed; 247 passed; 0 ignored

⎡ Glass: 3 errors, 247 passed │ glass_query("last") for details ⎤
```

The summary uses Unicode box-drawing characters to be visually distinct. It includes:
- Compressed result (errors/warnings/pass count)
- A hint that structured data is available via MCP

**For agents**: When Claude Code runs a command through its Bash tool, the summary appears in the output. Claude Code naturally reads it and knows to call `glass_query` for structured drill-down instead of re-parsing 500 lines.

**Configuration**:
```toml
[soi]
enabled = true
shell_summary = true              # Inject summary line after commands
shell_summary_min_lines = 10      # Only for commands with significant output
shell_summary_format = "compact"  # compact | detailed | off
```

### New MCP Tools for SOI

```
glass_query(
    command_id: Option<i64>,       # Specific command, or "last"
    scope: Option<String>,         # "errors", "tests", "warnings", "all"
    file: Option<String>,          # Filter by source file
    budget_tokens: Option<u32>,    # Token budget for response (default: 500)
) → CompressedOutput

glass_query_trend(
    pattern: Option<String>,       # Command pattern (e.g., "cargo test")
    last_n: Option<u32>,           # Compare last N runs (default: 3)
) → TrendAnalysis
    # "test_auth regressed 2 runs ago, was passing before commit abc123"
    # "build time increased 40% over last 5 runs"

glass_query_drill(
    record_id: i64,                # Specific record from glass_query results
) → FullRecordDetail
    # Returns complete error context, stack trace, surrounding code, etc.
```

### Integration with Existing Systems

- **OutputBuffer** (`output_capture.rs`): Currently captures raw bytes. SOI taps the same captured bytes after `CommandFinished`, before/alongside DB insert.
- **glass_errors**: The existing error extraction crate becomes one parser within SOI's parser registry. Its `StructuredError` maps directly to `OutputRecord::CompilerError`.
- **History DB**: SOI records link to `commands.id` via foreign key — same correlation as existing `pipe_stages`.
- **glass_extract_errors MCP tool**: Remains for backward compatibility but internally delegates to SOI.

---

## Feature 2: Agent Mode

### What It Does

Agent Mode runs a persistent, lightweight AI agent session in the background that watches all terminal activity (via SOI's compressed stream) and proactively proposes actions. The agent uses its own judgment to decide when to act — no regex rules or trigger configs.

### Design Principles

1. **The AI is the trigger engine** — no hardcoded event-to-action mappings
2. **Never modify without approval** — the agent proposes, the user decides
3. **Worktree isolation** — agent work happens in a git worktree, never the user's working directory
4. **Context efficient** — agent receives SOI summaries, not raw output
5. **Cheap to run** — background agent uses a fast/cheap model (Haiku or Sonnet)
6. **Unobtrusive** — status bar indicator, not modal dialogs

### Components

#### 2.1 Activity Stream

A channel that feeds compressed SOI summaries to the background agent session. The agent receives a rolling window of recent activity:

```rust
pub struct ActivityEvent {
    pub timestamp: i64,
    pub event_type: ActivityEventType,
    pub summary: String,                    // SOI one-line summary
    pub severity: Severity,
    pub command_id: Option<i64>,            // For drill-down via glass_query
}

pub enum ActivityEventType {
    CommandCompleted {
        command: String,
        exit_code: Option<i32>,
        duration_ms: u64,
    },
    FileChanged {
        path: String,
        change_type: String,                // created, modified, deleted
    },
    GitEvent {
        action: String,                     // pull, merge, conflict, etc.
    },
    AgentMessage {
        from: String,
        content: String,
    },
}
```

The stream is **budget-constrained**: the agent receives at most N tokens of context per update cycle. Old events roll off. The agent can always drill down via `glass_query` if it needs more detail.

#### 2.2 Agent Runtime

A background process manager that maintains a persistent Claude CLI session:

```rust
pub struct AgentRuntime {
    mode: AgentMode,
    model: String,                          // claude-haiku-4-5, claude-sonnet-4-6, etc.
    process: Option<Child>,                 // Claude CLI process
    activity_rx: Receiver<ActivityEvent>,    // From SOI pipeline
    proposal_tx: Sender<AgentProposal>,     // To approval UI
    cooldown: Duration,                     // Min time between actions
    context_budget: usize,                  // Tokens per activity update
    session_token: Option<String>,          // For resuming sessions
}

pub enum AgentMode {
    /// Watches silently. Only surfaces critical issues.
    /// Speaks when: build fails, test regresses, obvious error.
    Watch,

    /// Actively suggests improvements and catches issues.
    /// Speaks when: anything it thinks the user would want to know.
    Assist,

    /// Proposes fixes and enhancements autonomously.
    /// Acts when: it identifies something actionable.
    /// Still requires approval for file changes.
    Autonomous,
}
```

**How the agent session works**:

1. Glass spawns `claude` CLI as a background process with a system prompt explaining its role
2. The system prompt includes available MCP tools (glass_query, glass_query_drill, etc.)
3. Periodically (every N seconds, or on significant events), Glass feeds the activity stream to the agent's stdin
4. The agent reasons about whether action is needed
5. If yes, the agent produces an `AgentProposal` describing what it wants to do
6. Glass surfaces the proposal through the approval UI

**Agent system prompt** (injected by Glass):
```
You are Glass Agent, a background assistant watching terminal activity in a
development session. You receive compressed activity summaries from the
terminal. Your job is to:

1. Identify problems worth acting on (build failures, test regressions,
   obvious errors, missed issues)
2. Propose specific fixes or actions
3. Stay quiet when everything is fine — don't be noisy

You have access to glass_query() to drill into command output details.
You can propose file edits, commands to run, or informational messages.

Current mode: {mode}
Project: {project_path}
```

#### 2.3 Worktree Isolation

When the agent needs to make code changes, it works in an isolated git worktree:

```rust
pub struct WorktreeManager {
    repo_root: PathBuf,
    worktree_base: PathBuf,              // .glass/agent-worktrees/
}

impl WorktreeManager {
    /// Create isolated worktree for agent work
    pub fn create(&self, branch_name: &str) -> Result<Worktree>;

    /// Generate diff between worktree and main working tree
    pub fn diff(&self, worktree: &Worktree) -> Result<String>;

    /// Apply worktree changes to main working tree
    pub fn apply(&self, worktree: &Worktree) -> Result<()>;

    /// Clean up worktree (on dismiss or after apply)
    pub fn cleanup(&self, worktree: &Worktree) -> Result<()>;
}
```

#### 2.4 Proposal & Approval System

```rust
pub struct AgentProposal {
    pub id: Uuid,
    pub timestamp: i64,
    pub trigger: String,                    // What prompted this proposal
    pub description: String,                // What the agent wants to do
    pub proposal_type: ProposalType,
    pub confidence: f32,                    // 0.0-1.0, affects UI prominence
}

pub enum ProposalType {
    /// Fix code — agent has a diff ready in a worktree
    CodeFix {
        worktree_path: PathBuf,
        files_changed: Vec<String>,
        diff_summary: String,               // "+12 -3 across 2 files"
    },

    /// Run a command (e.g., "npm install missing-package")
    RunCommand {
        command: String,
        rationale: String,
    },

    /// Informational — no action, just a heads-up
    Notification {
        message: String,
    },

    /// Session handoff — agent session is ending, here's context for next
    Handoff {
        summary: String,
        remaining_work: Vec<String>,
        context_snapshot: String,
    },
}
```

#### 2.5 Approval UI

Three layers of UI, from least to most prominent:

**Status bar indicator** (always visible when agent mode is on):
```
┌────────────────────────────────────────────────────────┐
│ 🤖 Agent: watching │ 2 proposals pending              │
└────────────────────────────────────────────────────────┘
```

**Toast notification** (appears for new proposals, auto-dismisses after 10s):
```
┌──────────────────────────────────────────────┐
│ 🤖 Build failed — agent has a fix ready      │
│    Press Ctrl+Shift+A to review              │
└──────────────────────────────────────────────┘
```

**Review overlay** (opened by Ctrl+Shift+A or clicking status bar):
```
┌─────────────────────────────────────────────────────────────┐
│  Agent Proposals                                     [×]    │
│─────────────────────────────────────────────────────────────│
│                                                             │
│  ● Fix: missing import in src/auth.rs                       │
│    Trigger: `cargo build` failed (exit 101)                 │
│    Changes: +1 line in src/auth.rs                          │
│    [View Diff]  [Apply]  [Edit]  [Dismiss]                  │
│                                                             │
│  ○ Info: test_login has failed 3 times in a row             │
│    Pattern detected across last 3 `cargo test` runs         │
│    [Acknowledge]  [Ask Agent to Fix]  [Dismiss]             │
│                                                             │
│  ○ Handoff: agent session ending                            │
│    34/52 components done. Remaining: auth, notifications.   │
│    [Save to clipboard]  [Start new session]  [Dismiss]      │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

#### 2.6 Session Continuity

When an AI agent's context is exhausted (or the agent session ends for any reason), Glass preserves continuity:

```rust
pub struct SessionHandoff {
    pub previous_session_id: String,
    pub ended_at: i64,
    pub reason: String,                     // "context_exhausted", "user_closed", "error"
    pub work_completed: Vec<String>,        // What was done
    pub work_remaining: Vec<String>,        // What's left
    pub key_decisions: Vec<String>,         // Important choices made
    pub current_state: String,              // "build passing", "3 tests failing", etc.
    pub files_modified: Vec<String>,        // Files touched this session
    pub error_context: Option<String>,      // If session ended due to error
}
```

Stored in the history DB. When a new session starts, Glass provides the handoff as initial context:

```sql
CREATE TABLE agent_sessions (
    id              TEXT PRIMARY KEY,        -- UUID
    project         TEXT NOT NULL,
    model           TEXT NOT NULL,
    mode            TEXT NOT NULL,
    started_at      INTEGER NOT NULL,
    ended_at        INTEGER,
    reason          TEXT,                    -- Why it ended
    handoff_json    TEXT                     -- SessionHandoff serialized
);
CREATE INDEX idx_agent_sessions_project ON agent_sessions(project);
```

### Configuration

```toml
[agent]
enabled = false                     # Opt-in
mode = "watch"                      # watch | assist | autonomous
model = "claude-haiku-4-5"         # Model for background agent
cooldown_seconds = 30               # Min time between proposals
context_budget_tokens = 4096        # Tokens per activity update
max_proposals = 10                  # Max queued proposals before oldest dismissed

[agent.permissions]
edit_files = "approve"              # approve | auto | never
run_commands = "approve"            # approve | auto | never
git_operations = "never"            # approve | auto | never
create_worktree = "auto"            # Usually fine to auto-allow

[agent.quiet]
# Commands/patterns the agent should ignore
ignore_commands = ["ls", "cd", "pwd", "clear"]
ignore_exit_0 = false               # If true, only react to failures
```

---

## Phase Breakdown

### Phase 1: SOI Core — Output Classification & Parsing

**Goal**: Build the output classification and parsing pipeline. Given captured command output, produce structured records.

**Work**:
- Create `glass_soi` crate
- Implement `OutputClassifier` — detect output type from command + content
- Port `glass_errors` parsers as first SOI parsers (Rust compiler, generic)
- Add test runner parsers (cargo test, jest, pytest, go test)
- Add package manager parsers (npm, cargo, pip)
- Define `ParsedOutput`, `OutputRecord`, `OutputSummary` types
- Unit tests with real captured output fixtures

**Success criteria**: Given raw output from `cargo test`, `cargo build`, `npm install`, `pytest`, produces correct structured records with one-line summaries.

### Phase 2: SOI Storage — Structured Record DB

**Goal**: Persist parsed output alongside command history in SQLite.

**Work**:
- Add `command_output_records` and `output_records` tables to history DB
- Schema migration (history DB v2 → v3)
- Insert parsed output after command completion
- Query API: by command_id, by severity, by file, by type
- Retention/pruning aligned with existing history retention
- Integration tests with history DB

**Success criteria**: After running commands, structured records are queryable from SQLite. Records survive app restart.

### Phase 3: SOI Pipeline Integration

**Goal**: Wire SOI into the command lifecycle so parsing happens automatically.

**Work**:
- Hook into `CommandFinished` event in main event loop
- Feed captured output (from `OutputBuffer`) through classifier → parser → DB
- Handle async: parsing should not block the event loop (spawn onto Tokio)
- Handle edge cases: no output, alt-screen apps, very large output, binary output
- Emit `AppEvent::SoiReady { command_id }` when parsing completes

**Success criteria**: Every command with captured output gets automatically parsed and stored. No impact on terminal responsiveness.

### Phase 4: SOI Compression Engine

**Goal**: Produce token-budgeted summaries at multiple granularity levels.

**Work**:
- Implement `CompressionEngine` with `OneLine`, `Summary`, `Detailed`, `Full` levels
- Token estimation (simple word/character heuristic, not a tokenizer)
- Smart truncation: prioritize errors over warnings, recent over old
- Drill-down support: return record IDs for expanding specific items
- Diff-aware compression: "compared to last run, 2 new failures"

**Success criteria**: Given a `ParsedOutput` with 50 test results and 3 errors, `compress(budget=100)` returns a useful summary within budget. `compress(budget=10)` returns the one-liner.

### Phase 5: SOI Shell Summary Injection

**Goal**: Inject SOI summaries into terminal output so AI agents see them naturally.

**Work**:
- After SOI parsing completes, write summary line to PTY
- Design visual format (Unicode box drawing, muted colors via ANSI)
- Configurable: on/off, minimum output lines threshold, format
- Ensure it doesn't interfere with shell integration (OSC 133 boundaries)
- Handle timing: summary must appear after command output but before next prompt

**Success criteria**: After `cargo test` with failures, a summary line appears in the terminal. Claude Code's Bash tool captures this line in its output. The line includes a hint to use `glass_query`.

### Phase 6: SOI MCP Tools

**Goal**: Expose SOI data through MCP tools for AI agent consumption.

**Work**:
- `glass_query`: query structured output by command_id/scope/file/budget
- `glass_query_trend`: compare recent runs of same command pattern
- `glass_query_drill`: expand specific record for full detail
- Update `glass_context` and `glass_compressed_context` to include SOI summaries
- Update MCP server instructions to guide agents toward SOI tools
- Integration tests with MCP protocol

**Success criteria**: An AI agent can call `glass_query("last", scope="errors")` and get a structured, compressed summary of the last command's errors. `glass_query_trend("cargo test", last_n=5)` shows test result trends.

### Phase 7: SOI Additional Parsers

**Goal**: Expand parser coverage to common dev tools.

**Work**:
- Git output parser (status, diff stats, merge conflicts, pull results)
- Docker parser (build progress, errors, compose events)
- Generic JSON lines parser (structured logging, NDJSON)
- TypeScript/tsc parser
- Go compiler and test parser
- Kubectl parser (pod status, describe output, apply results)
- Make parser registry extensible for future additions

**Success criteria**: SOI produces useful structured output for at least 10 common developer tools/commands.

### Phase 8: Agent Mode — Activity Stream

**Goal**: Build the activity stream that feeds compressed SOI data to the agent runtime.

**Work**:
- Define `ActivityEvent` and `ActivityEventType`
- Create bounded channel (tokio::sync::mpsc) for event delivery
- Subscribe to SOI completion events, file change events, git events
- Budget-constrained rolling window (last N tokens of activity)
- Deduplication and rate limiting (don't flood on rapid command execution)

**Success criteria**: Activity stream produces a coherent, compressed narrative of terminal activity. Given 20 commands in 5 minutes, the stream is a readable ~2000 token summary.

### Phase 9: Agent Mode — Agent Runtime

**Goal**: Build the background process manager that runs a persistent Claude CLI session.

**Work**:
- Create `glass_agent` crate
- Spawn `claude` CLI as a background process with custom system prompt
- Feed activity stream to agent stdin (as structured messages)
- Parse agent stdout for proposals (JSON protocol)
- Handle agent process lifecycle: start, restart, graceful shutdown
- Implement `AgentMode` (Watch, Assist, Autonomous) affecting system prompt
- Session tracking: store session start/end/handoff in DB
- Implement cooldown timer between proposals

**Success criteria**: Background Claude session receives activity updates, reasons about them, and produces structured proposals. Agent restarts cleanly after crash.

### Phase 10: Agent Mode — Worktree Isolation

**Goal**: Agent code changes happen in isolated git worktrees.

**Work**:
- Implement `WorktreeManager`: create, diff, apply, cleanup
- Auto-create worktree when agent proposes code changes
- Generate unified diff between worktree and main working tree
- Apply changes: copy files from worktree to working tree
- Cleanup: remove worktree after apply or dismiss
- Handle non-git projects: fall back to temp directory with file copies

**Success criteria**: Agent proposes a fix in an isolated worktree. User can view the diff, apply it cleanly, and the worktree is cleaned up.

### Phase 11: Agent Mode — Approval UI

**Goal**: Build the proposal notification and review interface.

**Work**:
- Status bar integration: agent mode indicator, proposal count
- Toast notifications: non-modal, auto-dismiss, with keyboard shortcut hint
- Review overlay: scrollable list of proposals with diff preview
- Keyboard shortcuts: Ctrl+Shift+A (review), Enter (apply), Esc (dismiss)
- Render diff view with syntax-highlighted additions/deletions
- Proposal queue management: max queue size, auto-dismiss old

**Success criteria**: User sees status bar indicator when agent mode is on. Toast appears for new proposals. Review overlay shows diffs and allows apply/dismiss with keyboard.

### Phase 12: Agent Mode — Session Continuity

**Goal**: Preserve context across agent session boundaries for seamless handoff.

**Work**:
- Define `SessionHandoff` structure
- Agent produces handoff summary before session ends (context exhaustion, timeout)
- Store handoff in `agent_sessions` table
- When new session starts, load most recent handoff as initial context
- Combine with SOI history for rich context restoration
- Handle multiple sequential sessions (chain of handoffs)

**Success criteria**: Agent session 1 implements 30 files then exhausts context. Session 2 starts automatically with a compressed summary of session 1's work and continues from where it left off.

### Phase 13: Agent Mode — Configuration & Polish

**Goal**: User-facing configuration, documentation, and edge case handling.

**Work**:
- Full `[agent]` config section in config.toml with hot-reload support
- Permission system: approve/auto/never per action type
- Quiet rules: ignore patterns, exit-0 filtering
- Claude API key management (check for existing `claude` CLI auth)
- Error handling: agent process crashes, API errors, rate limits
- Metrics: track proposals made/applied/dismissed for UX tuning
- Status command: `glass agent status` showing current state

**Success criteria**: User can enable agent mode via config.toml, configure permissions and behavior, and the agent operates within the defined boundaries. Agent mode degrades gracefully when Claude CLI is unavailable.

---

## New Crates

| Crate | Purpose |
|-------|---------|
| `glass_soi` | Output classification, parsing, compression, shell summary injection |
| `glass_agent` | Agent runtime, activity stream, worktree manager, proposal system |

Both depend on `glass_core` (events, config) and `glass_history` (DB access).

---

## Risk & Mitigation

| Risk | Mitigation |
|------|------------|
| **SOI parsers break on unexpected output formats** | Graceful fallback to FreeformChunk — never crash, never lose data |
| **Shell summary injection breaks shell integration** | Summary written after CommandFinished but before next PromptStart; gated by config flag |
| **Agent mode costs money (API calls)** | Default to cheapest model (Haiku); cooldown prevents spam; `watch` mode is very conservative |
| **Agent proposes bad fixes** | Worktree isolation means nothing touches working tree until user approves; diff preview shows exactly what changes |
| **Agent is too noisy** | Conservative defaults (watch mode, 30s cooldown, ignore common commands); user tunes via config |
| **Context exhaustion in long sessions** | SOI compression keeps agent context small; session handoff preserves continuity across resets |
| **Non-git projects** | Worktree manager falls back to temp directory with file copies instead of git worktree |
| **Multiple agents running** | Integrate with existing glass_coordination for lock management |
| **Windows/macOS/Linux differences** | Claude CLI is cross-platform; git worktree is cross-platform; no platform-specific concerns beyond existing Glass platform abstractions |

---

## Success Metrics

### SOI
- **Coverage**: Structured parsing for top 10 developer tools by usage
- **Compression ratio**: 10:1 average token reduction vs raw output
- **Accuracy**: Zero false negatives on errors (may have false positives — better to surface too much than miss something)
- **Latency**: Parsing completes within 100ms for typical output sizes

### Agent Mode
- **Proposal relevance**: >80% of proposals are actionable (not noise)
- **Apply rate**: >50% of proposals are applied by users
- **Cost**: Background agent costs <$0.10/hour on cheapest model
- **Continuity**: Session handoff preserves >90% of relevant context across resets
- **Latency**: Proposal appears within 10s of triggering event
