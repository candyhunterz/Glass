# Glass Agent Orchestrator

## Problem

Claude Code implements features step by step, but stops after each step to wait for user input. The user must babysit the session — answering questions, nudging it to continue, and verifying work before moving on. This defeats the purpose of an AI coding assistant for multi-step projects.

The current Glass agent mode is an error-watcher that proposes fixes. This is useless — Claude Code already fixes its own errors. The real value is **two AI agents collaborating** — one implementing, one reviewing and guiding — working through a PRD autonomously without human intervention.

### Prior Art

- **Ralph Loop** ([snarktank/ralph](https://github.com/snarktank/ralph)) — Bash while loop that restarts Claude Code each iteration. Simple but blind between iterations — can't answer questions or review mid-work.
- **Claude Code Agent Teams** ([docs](https://code.claude.com/docs/en/agent-teams)) — Coordinates multiple Claude Code instances on parallel tasks. Not designed for one reviewer + one implementer collaborating on the same work.
- **Autoresearch** ([uditgoenka/autoresearch](https://github.com/uditgoenka/autoresearch)) — Autonomous iteration with git-as-memory and mechanical metrics. Single-agent but excellent patterns for structured iteration and state management.

**What we add:** Two agents in a reviewer/implementer collaboration. The Glass Agent doesn't just restart Claude Code — it reads what Claude Code did, makes product decisions, verifies quality, and provides real-time guidance. Like pair programming between two AIs.

## Architecture

Two claude processes, one orchestrator:

```
┌─────────────────────────────────────────────────────┐
│ Glass (Rust)                                        │
│                                                     │
│  ┌──────────────┐    ┌───────────────────────────┐  │
│  │ Glass Agent   │    │ Terminal (PTY)             │  │
│  │ (claude sub-  │    │                           │  │
│  │  process)     │    │  $ claude                 │  │
│  │              │◄───│  [Claude Code running]     │  │
│  │  Reviewer /  │───►│  [receives typed input]    │  │
│  │  Guide       │    │                           │  │
│  └──────────────┘    └───────────────────────────┘  │
│         ▲                                           │
│         │                                           │
│  ┌──────┴───────────────────────────────────────┐   │
│  │ Orchestrator Loop (Rust)                     │   │
│  │  - Silence detection (timer)                 │   │
│  │  - Capture terminal context                  │   │
│  │  - Send to Glass Agent for decision          │   │
│  │  - Parse response and route accordingly      │   │
│  │  - Write text response to PTY                │   │
│  │  - Usage tracking (HTTP poll)                │   │
│  │  - Iteration logging                         │   │
│  └──────────────────────────────────────────────┘   │
│                                                     │
│  ┌──────────────────────────────────────────────┐   │
│  │ Status Bar                                   │   │
│  │  5h: 42% | Orchestrating | iter #12 | ✓18   │   │
│  └──────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────┘
```

- **Claude Code** — the implementer. Interactive CLI running in the terminal, launched with `--dangerously-skip-permissions`. Writes code, runs commands, builds features. Glass treats it as a regular terminal process.
- **Glass Agent** — the reviewer/guide. Background claude subprocess (already exists). Repurposed from error-watcher to collaborator. Reviews Claude Code's work, makes product decisions, ensures quality against the PRD, and guides next steps.
- **Orchestrator Loop** — new Rust module. Handles silence detection, context capture, PTY input, usage tracking, and iteration lifecycle. All AI decisions are delegated to the Glass Agent subprocess; the orchestrator itself is pure Rust (timers, I/O, HTTP calls).

## Structured Iteration Protocol

Inspired by [autoresearch](https://github.com/uditgoenka/autoresearch). Each feature follows a structured cycle rather than freeform conversation:

```
┌─────────────────────────────────────────────┐
│ 1. PLAN — Glass Agent tells Claude Code     │
│    what to build next (from PRD)            │
├─────────────────────────────────────────────┤
│ 2. IMPLEMENT — Claude Code works            │
│    (Glass Agent answers questions mid-work) │
├─────────────────────────────────────────────┤
│ 3. COMMIT — Claude Code commits before      │
│    verification (clean rollback point)      │
├─────────────────────────────────────────────┤
│ 4. VERIFY — Glass Agent tells Claude Code   │
│    to write tests and run them              │
├─────────────────────────────────────────────┤
│ 5. DECIDE — Tests pass → keep               │
│             Tests fail → fix and re-verify  │
│             Stuck → revert and try different │
│             approach                         │
├─────────────────────────────────────────────┤
│ 6. LOG — Orchestrator logs iteration result  │
│    to .glass/iterations.tsv                  │
├─────────────────────────────────────────────┤
│ 7. NEXT — Glass Agent picks next PRD item    │
│    or emits GLASS_CHECKPOINT for context     │
│    refresh if enough work has accumulated    │
└─────────────────────────────────────────────┘
```

### Git as Memory

Adopted from autoresearch. Git is the primary state management mechanism:

- **Commit before verify** — Claude Code commits after implementing, before tests run. This creates a clean rollback point.
- **Revert on failure** — If tests fail after multiple attempts, the Glass Agent can tell Claude Code to `git revert` and try a different approach.
- **Git history as context** — After a context refresh, both agents can read `git log` to understand what's been done. No separate checkpoint file needed for tracking completed work.

### Iteration Log

A structured TSV file at `<project>/.glass/iterations.tsv` tracks every iteration:

```tsv
iteration	commit	feature	metric	status	description
1	a1b2c3d	auth-module	18/18 tests pass	keep	Implemented JWT auth with login/signup
2	e4f5g6h	auth-module	18/18 tests pass	keep	Added password reset flow
3	i7j8k9l	db-schema	0/4 tests pass	revert	Tried raw SQL, migration failed
4	m0n1o2p	db-schema	4/4 tests pass	keep	Used ORM migrations instead
```

The Glass Agent reads this log to learn from past iterations: what approaches worked, what failed, what hasn't been tried.

### Mechanical Metric

Each PRD item should have a testable success criterion. The Glass Agent uses this to make keep/revert decisions:

- **Tests pass** — the primary metric for most features
- **Build succeeds** — minimum bar
- **Custom metric** — if the PRD specifies one (e.g., "page loads in < 2s")

If the PRD doesn't include acceptance criteria, the Glass Agent asks Claude Code to write tests first, then uses those as the metric.

## Orchestrator Loop

The core cycle that enables collaboration:

```
Claude Code outputs text
  → Silence detected (no PTY output for N seconds)
  → Capture terminal context (scrollback + output buffer)
  → Send to Glass Agent with iteration context
  → Glass Agent responds (text / GLASS_CHECKPOINT / GLASS_WAIT)
  → Orchestrator parses response and acts accordingly
  → Log the interaction
  → Reset silence timer
  → Repeat
```

### Idle Detection

Silence-based timeout with the Glass Agent as the decision maker.

**Mechanism:** Track the timestamp of the last PTY output byte. When `now - last_output > threshold`, trigger the orchestrator.

**Threshold:** Configurable, default 30 seconds. Must be long enough that Claude Code's "thinking" pauses (10-20 seconds) don't trigger false positives, but short enough that the user doesn't wait too long.

**False positive handling:** The Glass Agent receives terminal context and decides whether Claude Code is actually waiting for input or still working. If it determines Claude Code is still thinking (e.g., a build is running, a spinner is visible), it responds with `GLASS_WAIT` and the orchestrator resets the timer for another cycle.

**Long-running commands:** If Claude Code kicked off `cargo build` or `npm install`, there will be periodic output (progress bars, compilation messages). The silence timer resets on each output byte, so long-running commands with output won't trigger the orchestrator.

### Context Capture

When the silence timer fires, the orchestrator captures terminal context to send to the Glass Agent.

**Sources (combined):**
- Terminal grid scrollback via `extract_term_lines()` — the last N visible lines
- The PTY `OutputBuffer` raw bytes (if available) — captures content that may have scrolled off the grid

**Target:** Last ~100 lines of meaningful content. If the terminal grid has fewer lines of scrollback, use what's available. ANSI escape sequences are stripped before sending.

### Terminal Input

When the Glass Agent decides what to type, the orchestrator writes it to the PTY.

**Mechanism:** Use the existing `PtyMsg::Input` channel (already used for SOI hint injection). Send the Glass Agent's response as bytes to the PTY master, which feeds into Claude Code's stdin.

**Format:** Plain text followed by a newline (simulating the user pressing Enter). No escape sequences or special formatting.

### Response Parsing

The Glass Agent's response is parsed for structured markers before being typed into the terminal:

| Response | Orchestrator Action |
|----------|-------------------|
| `GLASS_WAIT` | Reset silence timer, do nothing. Claude Code is still working. |
| `GLASS_CHECKPOINT: {"completed": "...", "next": "..."}` | Trigger context refresh cycle (see below). Do NOT type this into the terminal. |
| Any other text | Type it into the PTY as user input to Claude Code. |

### Decision Engine

The Glass Agent collaborates with Claude Code across three interaction modes:

1. **Guide** — Claude Code finished a step or is waiting. The Glass Agent reviews the iteration log, checks the PRD, and gives direction: "Now implement the database layer. Start by creating the schema migrations."

2. **Collaborate** — Claude Code asked a question like "Should I use SQLite or PostgreSQL?" The Glass Agent reads the PRD, considers the project context, and makes a decision with reasoning: "Use SQLite — the PRD says single-server deployment, and it simplifies the stack."

3. **Review** — Claude Code says "I've implemented the auth module." The Glass Agent tells it to commit, write tests, and run them. Based on results, it decides: keep the work, fix failures, or revert and try a different approach.

**System prompt structure:**
```
You are the Glass Agent, collaborating with Claude Code to build a project.
Claude Code is the implementer — it writes code, runs commands, builds features.
You are the reviewer and guide — you make product decisions, ensure quality,
and keep the project moving against the plan.

PROJECT PLAN:
<contents of PRD, truncated to 4000 words, or path if too large>

ITERATION PROTOCOL:
For each feature, guide Claude Code through this cycle:
1. PLAN: Tell Claude Code what to build next and define acceptance criteria
2. IMPLEMENT: Let Claude Code work. Answer its questions with clear decisions.
3. COMMIT: Tell Claude Code to commit before verification ("commit your changes")
4. VERIFY: Tell Claude Code to write tests and run them
5. DECIDE: Tests pass → move to next feature. Tests fail → tell Claude Code to fix.
   Stuck after 3 attempts → tell Claude Code to revert and try a different approach.
6. When a feature is complete and verified, continue to the next PRD item.

ITERATION HISTORY:
<contents of .glass/iterations.tsv if it exists>

CONTEXT REFRESH:
When you've completed 2-3 features and context is getting heavy, emit:
GLASS_CHECKPOINT: {"completed": "<summary>", "next": "<next PRD item>"}
This triggers a context refresh for both agents. Git history and iterations.tsv
persist across refreshes. Do NOT wait until context is exhausted.

RESPONSE FORMAT:
Respond with ONLY one of:
1. Text to type into the terminal (sent as-is to Claude Code)
2. GLASS_WAIT (Claude Code is still working, check again later)
3. GLASS_CHECKPOINT: {"completed": "...", "next": "..."} (trigger context refresh)

No explanations, no meta-commentary. Just the response.
```

## Context Refresh System

Replaces the previous "checkpoint" concept. Uses git as the primary memory.

### When to Refresh

The Glass Agent emits `GLASS_CHECKPOINT` when:
- 2-3 features have been completed (context is getting heavy)
- Proactively, before context exhaustion

### Refresh Cycle

```
1. Orchestrator receives GLASS_CHECKPOINT from Glass Agent

2. Orchestrator types into terminal (via PTY):
   "Commit all pending changes and write a brief status update to
   .glass/checkpoint.md: what you just completed, what's next,
   and any key decisions. Keep it under 500 words."

3. Orchestrator waits for .glass/checkpoint.md to be written
   (poll file mtime every 2 seconds, timeout after 60 seconds)

4. Once file is updated, orchestrator types: /clear
   (Claude Code's context is now fresh)

5. Orchestrator kills the Glass Agent subprocess

6. Orchestrator appends a "refresh" row to .glass/iterations.tsv

7. Orchestrator spawns a fresh Glass Agent subprocess
   - System prompt includes: PRD + checkpoint.md + iterations.tsv
   - Fresh context window for both agents

8. New Glass Agent types:
   "Read .glass/checkpoint.md, review git log --oneline -20,
   and continue with the next item from the project plan."

9. Claude Code reads the file + git history, resumes work
```

### What Persists Across Refreshes

| Persists | Source |
|----------|--------|
| Completed work | Git history (commits) |
| What's done / what's next | `.glass/checkpoint.md` (500 words max) |
| Iteration history | `.glass/iterations.tsv` (structured log) |
| PRD | Original file on disk (unchanged) |
| Codebase state | Files on disk |

No growing memory file problem — checkpoint.md is replaced each time (not appended), iterations.tsv grows slowly (one row per feature attempt), and git handles the rest.

## OAuth Usage Tracking

Glass polls the Anthropic usage API directly from Rust. No tokens consumed.

### Mechanism

1. **Read OAuth token** from `~/.claude/.credentials.json` at path `.claudeAiOauth.accessToken`. Re-read the file on each poll cycle (tokens may be refreshed by Claude Code). On macOS, read from Keychain (`Claude Code-credentials`). On Linux, read from `~/.claude/.credentials.json` (same as Windows).

2. **Poll endpoint** every 60 seconds:
   ```
   GET https://api.anthropic.com/api/oauth/usage
   Headers:
     Authorization: Bearer <token>
     anthropic-beta: oauth-2025-04-20
     Accept: application/json
   ```

3. **Parse response:**
   ```json
   {
     "five_hour": { "utilization": 0.42, "resets_at": "2026-03-14T08:00:00Z" },
     "seven_day": { "utilization": 0.15, "resets_at": "2026-03-20T00:00:00Z" }
   }
   ```

4. **Cache** response for 60 seconds (avoid redundant requests).

5. **Fallback:** If the endpoint returns 401 (token expired), re-read credentials and retry once. If the endpoint returns errors consistently (3+ failures), disable usage display and log a warning. The orchestrator continues without usage-based pausing.

> **Note:** This API endpoint was discovered from a third-party project ([ClaudeCodeStatusLine](https://github.com/daniel3303/ClaudeCodeStatusLine)) and may be undocumented or change. The implementation must handle the endpoint being unavailable gracefully.

### Status Bar Display

Show alongside existing agent info:
```
5h: 42% | 7d: 15% | iter #12 | ✓18 tests    [agent: autonomous]
```

Color coding for usage:
- Green (0-70%)
- Yellow (70-85%)
- Red (85%+)

If usage data is unavailable: show `5h: --% | 7d: --%` in gray.

### Auto-Pause at Threshold

When `five_hour.utilization >= 0.80`:

1. Trigger context refresh cycle (as described above)
2. After refresh completes, kill both subprocesses
3. Status bar shows: `Paused — resumes at <resets_at time>`
4. Continue polling usage API every 60 seconds

### Auto-Resume

When poll shows `five_hour.utilization` has dropped below 0.20 (post-reset):

1. Verify terminal is at a shell prompt (via OSC 133 PromptStart state) before typing anything. If not at a prompt, wait and re-check.
2. Spawn fresh Glass Agent subprocess (with PRD + checkpoint.md + iterations.tsv)
3. Glass Agent types into terminal: `claude --dangerously-skip-permissions` (restart Claude Code)
4. Glass Agent types: "Read .glass/checkpoint.md, review git log, and continue"
5. Orchestrator loop resumes

### Hard Failure (95%+)

At 95% utilization, don't attempt AI-driven checkpoint:

1. Kill both subprocesses immediately
2. Glass (Rust) writes a minimal emergency checkpoint from its own state:
   ```markdown
   # Emergency Checkpoint (written by Glass, not AI)
   Paused at: 2026-03-14T10:30:00Z
   Reason: OAuth usage at 95%
   Last agent action: "Told Claude Code to implement database layer"
   Last terminal lines:
   <last 50 lines of terminal output>
   Working directory: <CWD>
   Resume: run `claude`, then read .glass/checkpoint.md and continue
   ```
3. Status bar: `PAUSED — usage limit (resumes at <time>)`

## User Override

**Toggle:** `Ctrl+Shift+O` toggles orchestration on/off.

- **On:** Glass Agent collaborates with Claude Code. Status bar shows `Orchestrating`.
- **Off:** Normal terminal. You type as usual.

**Behavior when toggling off:**
- Orchestrator loop stops immediately
- Current silence timer is cancelled
- No checkpoint is triggered (you're taking over mid-work)
- Glass Agent subprocess stays alive (in case you toggle back on)

**Behavior when toggling on:**
- Orchestrator loop starts
- Silence timer begins from now
- If Claude Code is already waiting, the agent will respond on the next timeout

**User types while orchestrating:** If user keystrokes are detected while orchestration is active, the orchestrator auto-pauses (same as toggling off). The user can toggle back on with `Ctrl+Shift+O` when done. This prevents interleaved input from both the user and the agent.

## Claude Code Crash Recovery

If Claude Code exits (process ends, shell prompt returns):

1. Orchestrator detects shell prompt via OSC 133 PromptStart
2. Verify checkpoint exists and work remains in the PRD
3. If yes → type `claude --dangerously-skip-permissions` to restart, then "Read .glass/checkpoint.md, review git log, and continue"
4. If no work remains → stop orchestration, notify user "Project complete"

## Stuck Loop Prevention

Circuit breaker to prevent infinite retry loops.

**Mechanism:** Track consecutive Glass Agent responses with exact string matching. If the orchestrator detects 3 consecutive identical responses (same text typed into the terminal):

1. Glass Agent receives: "You've tried this 3 times without success. Revert to the last good commit, log the blocker, and stop."
2. `git revert` to last passing state
3. Orchestrator pauses and notifies user: "Agent stuck on: <description>. Manual intervention needed."

Exact match for MVP. Smarter similarity detection is a future enhancement.

## Orchestrator Logging

Every orchestrator cycle is logged for debugging and post-mortem analysis:

- Terminal context sent to the Glass Agent (truncated to first/last 20 lines in log)
- The Glass Agent's raw response
- How the response was classified (text / checkpoint / wait)
- Whether it was typed into the PTY
- Timestamp and cycle number

Logs go to the standard Glass tracing output (`RUST_LOG=glass=debug`).

## Configuration

New fields in `~/.glass/config.toml`:

```toml
[agent]
mode = "autonomous"

[agent.orchestrator]
enabled = true                    # Master switch for orchestrator loop
silence_timeout_secs = 30         # Seconds of silence before triggering
prd_path = "PRD.md"              # Path to project plan (relative to CWD)
checkpoint_path = ".glass/checkpoint.md"  # Relative to project CWD
usage_pause_threshold = 0.80      # Pause at this 5h utilization level
usage_hard_stop = 0.95            # Hard stop at this level
max_retries_before_stuck = 3      # Circuit breaker threshold
```

Per-project overrides: If a `.glass/config.toml` exists in the project root, its `[agent.orchestrator]` section overrides the global config. This allows different `prd_path` per project.

## Existing Infrastructure Reused

| Component | Current Use | Orchestrator Use |
|-----------|------------|------------------|
| `AgentRuntime` | Error-watching subprocess | Collaborator brain |
| `PtyMsg::Input` | SOI hint injection | Type agent responses into terminal |
| Writer/Reader threads | Activity events → claude | Terminal context → claude |
| `glass_context` MCP tool | Agent queries history | Agent looks up past work |
| `glass_tab_output` MCP tool | Read tab output | Agent reads Claude Code output |
| `extract_proposal` parser | GLASS_PROPOSAL extraction | GLASS_CHECKPOINT extraction |
| Status bar | Agent mode display | + usage % + iteration count |
| Coordination DB | Multi-agent locks | Same (both agents registered) |
| OSC 133 PromptStart | Shell integration | Crash detection / resume safety |

## What Changes

### New Rust modules
- `orchestrator.rs` — Silence timer, response parsing, context refresh lifecycle, PTY input relay, stuck detection, user-input pause, iteration logging
- `usage_tracker.rs` — OAuth token reading (re-read each poll), API polling, threshold logic, auto-pause/resume state machine

### Modified modules
- `src/main.rs` — Wire orchestrator into event loop, `Ctrl+Shift+O` shortcut, user-input detection for auto-pause
- `AgentRuntime` — New system prompt for collaborator role, GLASS_CHECKPOINT/GLASS_WAIT marker extraction in reader thread
- `StatusBarRenderer` — Usage percentage display with color coding, iteration count
- `GlassConfig` — New `[agent.orchestrator]` section with per-project override support

### New files created at runtime
- `<project>/.glass/checkpoint.md` — Brief project status (replaced each refresh, max 500 words)
- `<project>/.glass/iterations.tsv` — Structured iteration log (append-only)
- `~/.glass/usage-cache.json` — Cached usage API response (global)

### Backward compatibility
- The existing activity event pipeline (error-watching mode) is preserved for `mode = "watch"` and `mode = "assist"`. The orchestrator only activates when `[agent.orchestrator] enabled = true` AND `mode = "autonomous"`.
- `GLASS_PROPOSAL` mechanism continues to work for non-orchestrator modes.
