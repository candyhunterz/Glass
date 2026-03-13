# Pitfalls Research

**Domain:** SOI (Structured Output Intelligence) output parsing + Agent Mode background AI agent capabilities added to existing Rust terminal emulator
**Researched:** 2026-03-12
**Confidence:** HIGH (verified against Glass codebase architecture, VSCode terminal integration race conditions, git worktree orphan issues, MCP tool token bloat research, Claude agent cost management docs, Rust subprocess management patterns)

---

## Critical Pitfalls

### Pitfall 1: SOI Parser Blocks the PTY Reader Thread

**What goes wrong:**
The PTY reader thread is a `std::thread` (not Tokio) that does blocking I/O. If SOI parsing runs inline on this thread after a `CommandFinished` event, the thread stalls waiting for parser completion before reading the next PTY bytes. With large outputs (50KB cap, or longer before truncation), a regex-heavy or multi-pass parser takes 10–100ms. During that window, the PTY buffer fills up and the shell blocks on write, causing visible input lag even on an idle session.

**Why it happens:**
The existing shell event handler at `AppEvent::Shell` in `src/main.rs` (lines 1732–2188) already runs 456 lines of sequential logic on the main thread. Appending SOI classification inline to the `CommandFinished` arm feels natural — it's where all other post-command work happens — but that arm already does history DB insert, snapshot comparison, and FS watcher drain. Adding a parser here multiplies synchronous work on the hot path.

**How to avoid:**
SOI parsing must happen off the main thread. Pattern: at `CommandFinished`, enqueue the captured output bytes into a bounded channel (capacity ~8) and spawn a dedicated Tokio task (or a `std::thread` worker pool) to classify and parse. The parsed result is written to SQLite via a separate `soi_records` table. The main event loop gets a lightweight `AppEvent::SoiComplete(session_id, command_id, SoiRecord)` callback to update the block's UI decoration after the fact. Never put parser execution between the PTY read loop and the next `term.process()` call.

**Warning signs:**
- Input latency rises from 3–7µs baseline to >5ms after commands with large output
- `cargo build` output causes visible input stuttering
- Benchmark: `criterion` `input_latency` test regresses under SOI

**Phase to address:**
SOI Phase 1 (classification pipeline) — design the async dispatch path before writing any parser logic. Make the async boundary the first thing built, not retrofitted.

---

### Pitfall 2: ANSI Escape Sequences Corrupt SOI Parser Input

**What goes wrong:**
Command output stored in `OutputBuffer` is already ANSI-stripped by the existing `strip_ansi` function in `glass_history`. However, the strip function uses a regex, and regex-based ANSI stripping is provably incomplete: OSC sequences with non-standard terminators (`ST` vs `BEL`), DCS sequences, private-use sequences, and sequences split across read boundaries all have edge cases the regex misses. When a SOI parser (e.g., the JSON classifier) receives output with residual escape bytes, `serde_json::from_str` returns errors on what should be clean JSON, the git log parser matches wrong field boundaries, and the TypeScript error parser counts line numbers incorrectly.

**Why it happens:**
The existing ANSI stripping was designed for readability and history search, not for machine parsing. It tolerates imperfect stripping because humans can still read the text. SOI parsers are strict and fail on unexpected bytes. The test suite for `OutputBuffer` likely uses synthetic output without complex escape sequences. Production shells (especially those running Starship, Oh My Posh, or LSP-enhanced editors) emit heavy escape traffic.

**How to avoid:**
Before SOI classification, pass output through a state-machine-based ANSI stripper rather than the regex. The `anstyle-parse` crate (Rust, implements Paul Williams' ANSI parser state machine) or `strip_ansi_escapes` crate are correct choices. State machine parsers handle split-buffer sequences and all OSC terminators correctly. Add a fuzzing test that feeds SOI parsers real captured output from `cargo build`, `git log --oneline`, `tsc --noEmit`, and `docker build` runs. Verify zero residual escape bytes in the parser's input.

**Warning signs:**
- SOI parser failure rate >5% on real command output
- JSON classifier triggers on `cargo build` output (which is not JSON)
- Git log parser matches wrong fields on colorized `git log` output

**Phase to address:**
SOI Phase 1 — establish a clean parser input pipeline before writing any format-specific parsers. A single correct stripping layer prevents all downstream format parsers from having to defensively handle ANSI bytes.

---

### Pitfall 3: Shell Summary Injection Races with OSC 133 Prompt Boundary

**What goes wrong:**
SOI shell summary injection writes a formatted summary line into the PTY's input side (simulating shell output) immediately after `CommandFinished`. The OSC 133;D sequence that fires `CommandFinished` and the injection of the summary line are on different code paths. If the summary line arrives before the shell renders the next OSC 133;A (PromptStart), `BlockManager` assigns the summary text to the current block's output instead of treating it as inter-block content. The summary appears inside the wrong block, distorts the block's output capture, and the history DB stores it as part of the command's real output.

**Why it happens:**
VSCode has a documented 80% failure rate on an equivalent race condition in their terminal integration (OSC 633;D vs AsyncIterable consumer). The root cause is that OSC sequences and PTY writes share the same byte stream but the consumers (OSC parser, terminal grid, block manager) observe them with different latencies. Writing synthetic bytes to the PTY from the host side races with the shell's natural sequence of events.

**How to avoid:**
Do NOT inject summary text into the PTY byte stream. Instead, render the summary as a host-side overlay or decoration — an additional rendered line in `BlockRenderer` that reads from `SoiRecord`, similar to how exit code badges and duration labels already work as host-side UI, not injected terminal text. This keeps the PTY stream clean, avoids all race conditions with OSC sequences, and means summary text is never stored in history output or captured by OutputBuffer. The Block struct gains an optional `soi_summary: Option<String>` field that BlockRenderer renders after the output section.

**Warning signs:**
- SOI summaries appear inside command output instead of after it
- History DB records contain SOI text mixed with real command output
- `cargo test` block shows TypeScript summary from previous command
- Starship/Oh My Posh prompts render before the summary line appears

**Phase to address:**
SOI Phase 5 (shell summary injection) — but the architectural decision (overlay vs PTY injection) must be made in Phase 1. If the wrong architecture is chosen in Phase 1, Phase 5 requires a rewrite.

---

### Pitfall 4: Binary and Alt-Screen Output Misclassified as Structured Data

**What goes wrong:**
The SOI classifier runs heuristics on command output to determine format (JSON, git log, TypeScript errors, Docker build output, etc.). Commands that produce binary output (`xxd`, `cat /dev/random`, `openssl rand -base64`, `tar -cv`) or alt-screen TUI output (`vim`, `htop`, `less`) produce output that triggers false-positive classification. The JSON classifier triggers on hex dump lines that start with `{`. The TypeScript parser triggers on Vim error messages with `file.ts:N:M` format. Alt-screen output is garbage bytes that should never reach a SOI parser but may if alt-screen detection has edge cases (the existing alt-screen detection is `AppEvent::CommandOutput` gated on `not alt_screen`).

**Why it happens:**
Heuristic classifiers optimize for recall over precision. Test suites use clean, real command output as fixtures, so edge cases from binary and TUI output are never covered. The existing `OutputBuffer` already has binary detection, but it uses a byte-frequency heuristic that may pass through binary output that happens to look textual.

**How to avoid:**
Classify `None` (no SOI) as the preferred default. The classifier must reject output aggressively: require ≥3 consecutive lines matching the target format before committing to a classification. For JSON, the first byte must be `{` or `[` with no leading whitespace from ANSI residue; validate fully with `serde_json`. For git log, require the `commit [0-9a-f]{40}` anchor. Extend the existing `OutputBuffer` binary test to explicitly gate SOI classification — if `is_binary()` returns true, skip SOI entirely. Alt-screen output should already be excluded by the existing `CommandOutput` gating, but add an explicit `output_was_alt_screen` flag to `CommandFinished` and short-circuit SOI in the `CommandFinished` handler.

**Warning signs:**
- `vim` sessions produce spurious SOI records after exit
- `curl https://api.example.com/data` triggers JSON parser on error HTML responses
- SOI classification rate above 40% of all commands (most commands produce unstructured output)

**Phase to address:**
SOI Phase 1 (classification) — the classifier's rejection threshold is the most important design decision. Over-classify later through refinement; do not start permissive.

---

### Pitfall 5: Agent Mode Background Process Leaves Zombies on Crash

**What goes wrong:**
Agent Mode spawns a Claude CLI subprocess (`claude` binary) per background agent session. If the Glass process crashes, is killed with `SIGKILL`, or the user force-quits the window, the Claude CLI process becomes orphaned. On Windows, orphaned processes are not automatically cleaned up unless they are in a Job Object attached to the parent. On Unix, the orphaned `claude` process continues consuming API quota, accrues costs, and may write files to the worktree indefinitely with no owner to approve or reject its proposals.

**Why it happens:**
Rust's `std::process::Child` drop handler sends SIGTERM on Unix but does nothing on Windows (by design — Windows has no SIGTERM). In practice, a crashed parent never drops `Child`, so the child is never signaled at all. The Glass process is also a GUI process; it can be killed by the OS (low-memory killer, Windows task kill) without running destructors.

**How to avoid:**
On Windows: create a Windows Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and assign each spawned agent process to the job. The job handle lives for the Glass process lifetime. When Glass terminates for any reason (including `TerminateProcess`), the OS closes the job handle and kills all member processes. On Unix: use `prctl(PR_SET_PDEATHSIG, SIGTERM)` in the child process (or via `Command::pre_exec`) to ensure the child receives SIGTERM when the parent dies. Store agent PIDs in a persistent SQLite table (`~/.glass/agents.db`, which already exists for `glass_coordination`). On Glass startup, scan this table for PIDs that are no longer live and clean up their worktrees.

**Warning signs:**
- `claude` processes visible in Task Manager / `ps aux` after Glass exits
- API billing shows charges after Glass is closed
- Git worktrees accumulate in `~/.glass/worktrees/` across restarts

**Phase to address:**
Agent Mode Phase 1 (agent runtime) — process lifecycle management must be part of the first implementation, not added later. Retrofitting Job Objects after subprocess spawning is architectural, not cosmetic.

---

### Pitfall 6: Git Worktree Cleanup Fails on Crash, Accumulates Disk Usage

**What goes wrong:**
Agent Mode creates git worktrees for safe code isolation (`git worktree add ~/.glass/worktrees/<id> -b agent-<id>`). If the Glass process crashes during worktree creation, or during agent execution, or if the agent proposes changes that are never applied, the worktree directory and git's administrative files (`<repo>/.git/worktrees/<id>/`) become orphaned. Git's `worktree prune` only removes entries whose directories no longer exist — it does not clean up directories that exist but whose associated branch was force-deleted. On a monorepo with a large `node_modules`, each orphaned worktree can consume hundreds of MB.

**Why it happens:**
This is a documented real-world bug in at least one production AI coding agent (opencode issue #14648). Bootstrap failures that `return` early after partial worktree creation leave the directory tree and the git administrative entry in inconsistent states. Repeated agent retries accumulate orphans with no upper bound.

**How to avoid:**
Worktree creation must be atomic with cleanup registration. The pattern: (1) write a `pending_worktree` row to SQLite before `git worktree add`; (2) on success, update to `active`; (3) on failure, run `git worktree remove --force <path>` and delete the row. On Glass startup, scan for `pending_worktree` rows and clean them up. Add a periodic prune that runs `git worktree prune` in each registered repo. Cap total worktree count per repo at a configurable limit (default: 3). Surface worktree disk usage in the Agent Mode configuration UI so users have visibility before disk fills.

**Warning signs:**
- `git worktree list` shows stale entries after repeated Glass restarts
- `~/.glass/worktrees/` grows beyond 1GB without active agent sessions
- `git worktree add` fails with "already exists" on new agent starts

**Phase to address:**
Agent Mode Phase 3 (worktree isolation) — the SQLite-backed cleanup registration pattern must be the first thing implemented in that phase, before any worktree creation logic.

---

### Pitfall 7: Claude CLI API Costs Spiral Without Budget Caps

**What goes wrong:**
Agent Mode runs a background Claude CLI session that autonomously executes tools, reads files, and proposes changes. Without explicit budget constraints, a single open-ended prompt ("fix all the bugs") can consume $20–$50 of API quota in one session. An agent team (multiple Claude instances) consumes approximately 7× more tokens than a single session. The Cursor community has documented cases where automated "long-context mode" triggering ran up unexpected charges on Bedrock.

**Why it happens:**
The Claude SDK's agent loop runs until Claude decides it is done, not until a budget is hit. Open-ended prompts combined with large SOI context (the activity stream feeding compressed SOI data to the agent) rapidly saturate the context window, trigger compaction (which itself costs tokens), and restart the loop with a fresh window.

**How to avoid:**
Expose `max_budget_usd` as a required configuration field (not optional, no default of "unlimited"). The Glass `[agent]` config section must require `max_budget_usd = 1.0` (reasonable default) and `max_turns = 20`. Surface real-time cost tracking in the Agent Mode status bar display. Implement hard cutoffs: when `max_budget_usd` is reached, the agent sends a final "stopped: budget exceeded" message and terminates. Add a `monthly_budget_usd` cap at the Glass level to prevent abuse across multiple sessions. Document clearly that per-session and monthly caps are independent.

**Warning signs:**
- Agent sessions running longer than 10 minutes without human approval events
- Status bar showing turn count above 15 without progress
- Claude requesting the same files repeatedly (context compaction loop)

**Phase to address:**
Agent Mode Phase 2 (agent runtime) — budget enforcement must be part of the initial runtime implementation, not added as a polish step. Shipping without it risks user financial harm.

---

### Pitfall 8: Approval UI Blocks Terminal Interaction

**What goes wrong:**
Agent Mode's approval UI displays a review overlay when the agent proposes a command or file change. If this overlay is modal (captures all keyboard input while visible), the user cannot type in the terminal, run commands in other tabs, or dismiss the overlay with terminal shortcuts. This is the most commonly cited frustration with AI coding agent UX in 2025: agents "frequently pause to ask for human review" and "users cannot wander off while the assistant works."

**Why it happens:**
The existing overlay pattern in Glass (SearchOverlay, ConfigErrorOverlay) intercepts all keyboard input while active — that is by design for those use cases. Reusing the same overlay pattern for approval UI would make it modal. Agent proposals arrive asynchronously; if the agent sends multiple proposals rapidly, a stack of modal overlays would make the terminal completely unusable.

**How to avoid:**
Agent approval must be non-blocking. Design: proposals appear in a toast notification at the bottom of the active pane (similar to the update notification in the status bar). The toast shows a one-line summary, expires after 30 seconds of no interaction (auto-rejected), and can be accepted/rejected with dedicated hotkeys (e.g., Alt+A to accept, Alt+R to reject) that do not conflict with terminal input. A separate Agent Review overlay (non-modal, side panel) shows the full diff for careful review but does not capture keyboard focus from the terminal. The agent queue is visible but does not block terminal usage.

**Warning signs:**
- User cannot type in terminal when an approval is pending
- Agent proposals pile up and become a multi-level overlay stack
- Escape key dismisses approval instead of going to the terminal

**Phase to address:**
Agent Mode Phase 4 (approval UI) — the non-blocking design must be in the spec for this phase. Review the existing overlay architecture before implementation and explicitly decide NOT to reuse the modal overlay pattern for approvals.

---

### Pitfall 9: MCP Tool Proliferation Degrades Agent Context Quality

**What goes wrong:**
Glass currently has 25 MCP tools. Adding SOI tools (glass_query, glass_query_trend, glass_query_drill) and Agent Mode tools brings the total to ~30+. Each MCP tool's schema description consumes tokens before the agent writes a single line. Anthropic's own testing showed 58 tools consuming ~55K tokens of context before any conversation content. At 30 tools with verbose descriptions, Glass burns ~25K–35K tokens per agent session just on tool metadata — reducing effective working memory for code analysis and proposals.

**Why it happens:**
MCP tools are registered globally. There is no conditional registration or lazy loading. Every tool's full description is serialized into the context window at session start. Tool descriptions written for human readability (verbose parameter documentation) cost more tokens than terse descriptions.

**How to avoid:**
Audit all 25 existing MCP tools and aggressively compress their descriptions. Target: ≤100 tokens per tool description. Group related tools into families and use compact parameter naming. For SOI tools specifically, consider whether all three query variants (glass_query, glass_query_trend, glass_query_drill) need to be separate tools or whether one glass_query tool with a `mode` parameter achieves the same with one schema entry. Before adding Agent Mode tools, measure the current tool token footprint with `claude --mcp-debug` or equivalent. Set a project policy: total MCP tool token budget ≤ 15K tokens.

**Warning signs:**
- Agent sessions exhausting context window faster than previous sessions
- Agent "forgetting" earlier context (more frequent compaction)
- `glass_context` tool returning truncated data due to token budget pressure

**Phase to address:**
SOI Phase 6 (MCP tools) — conduct a token audit of all existing tools as part of this phase before adding new ones. Also applicable when designing Agent Mode tools.

---

### Pitfall 10: Windows Process Spawning Differences Break Agent Subprocess Management

**What goes wrong:**
Agent Mode spawns the Claude CLI as a subprocess. On Windows with ConPTY (the platform Glass primarily targets), subprocess spawning has different semantics than Unix: there is no `SIGTERM` equivalent, process group killing requires Job Objects, and subprocess console windows may appear unless `CREATE_NO_WINDOW` flag is set. If the agent subprocess is spawned with a visible console window, users see a phantom terminal appear alongside Glass. More subtly, `Child::kill()` on Windows sends `TerminateProcess` which does not give the child a chance to flush its output buffer, potentially truncating the final proposal before Glass reads it.

**Why it happens:**
`std::process::Command` abstracts over platforms but does not set Windows-specific flags by default. The Claude agent SDK TypeScript issue tracker documents a known issue (December 2025) about needing `windowsHide: true` to suppress console windows. Glass already uses `windows-sys` for ConPTY and has platform-gated code, but new subprocess spawning in Agent Mode will not automatically inherit those conventions.

**How to avoid:**
Wrap all agent subprocess spawning in a platform-abstraction module `glass_agent::spawn`. On Windows: use `CommandExt::creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP)` and assign to a Job Object. On Unix: use `CommandExt::process_group(0)` so the entire process group can be killed with `kill(-pgid, SIGTERM)`. Before calling `Child::kill()`, send a graceful shutdown signal (ETX byte to stdin, the same mechanism Glass uses for `cancel_command`) and wait up to 5 seconds for the process to exit cleanly before force-terminating. Test agent spawn and cleanup explicitly with `#[cfg(target_os = "windows")]` integration tests.

**Warning signs:**
- Console windows appear when Agent Mode activates on Windows
- Agent subprocess output is truncated on termination
- `cargo test --workspace` fails with process management tests on Linux CI but passes on Windows

**Phase to address:**
Agent Mode Phase 1 (agent runtime) — platform-specific subprocess management must be implemented in the first phase alongside the Job Object cleanup from Pitfall 5.

---

### Pitfall 11: SOI Activity Stream Overwhelms Agent Context with Noise

**What goes wrong:**
The Agent Mode activity stream feeds compressed SOI data to the agent runtime. If every completed command produces a SOI record fed to the agent, a user running `cargo watch` (which rebuilds on every save) generates hundreds of SOI events per session. The agent context fills with redundant "build succeeded" summaries, leaving no room for meaningful code analysis. The agent begins ignoring the stream (effectively context-washing the useful entries) or the context compaction loop activates constantly, costing tokens and losing precision.

**Why it happens:**
The natural design is "feed all SOI records to the agent stream" — it's simple and ensures completeness. But terminal sessions are high-frequency environments. A developer working on a Rust project easily runs 50–100 commands per hour, many of them repetitive (cargo build, cargo test, git status). Each produces a SOI record. Without filtering, the stream is low signal-to-noise.

**How to avoid:**
The activity stream must be filtered before feeding to the agent: (1) deduplicate consecutive identical command+result pairs; (2) collapse N identical "build succeeded" events into "build succeeded 5 times in the last 10 minutes"; (3) always prioritize error/failure events over success events; (4) cap the stream window to the last 20 non-duplicate events. Expose stream verbosity as a user config (`agent.activity_stream_verbosity = "errors_only" | "important" | "all"`). Default to `"important"` (errors + commands the agent explicitly invoked).

**Warning signs:**
- Agent sessions with `cargo watch` running exhaust context in <5 minutes
- Agent proposals repeat the same suggestion multiple times (context noise causing loss of earlier conversation state)
- Activity stream has >80% repeated entries in a typical development session

**Phase to address:**
Agent Mode Phase 1 (activity stream feeding) — stream filtering is a data pipeline concern, not a UI concern. Design the filter layer in Phase 1 alongside the stream architecture.

---

### Pitfall 12: Context Window Exhaustion Has No Graceful Recovery

**What goes wrong:**
Claude CLI sessions have a context window limit. When exhausted, the SDK performs automatic compaction by summarizing older history. If the compaction summary loses critical context (e.g., the user's original goal, the files that were already modified, the approval decisions already made), the resumed agent session may re-propose changes that were already applied, creating duplicate edits or conflicting with the human's manual changes made since the compaction.

**Why it happens:**
Context compaction is a lossy operation by design. The SDK's server-side compaction prioritizes recent exchanges but can lose specific constraints stated early in a conversation. Long agent sessions (multi-hour coding tasks) are particularly vulnerable.

**How to avoid:**
At the Glass layer, maintain a persistent session state file (`~/.glass/agent-sessions/<id>.json`) separate from the Claude SDK's internal state. This file records: (1) the original user goal; (2) which files were modified by the agent and at which git SHAs; (3) approval decisions already made; (4) the current worktree branch. When context compaction occurs (detectable via SDK callbacks or turn count heuristics), Glass injects the session state file as a system prompt prefix for the resumed session. This ensures continuity even after compaction. Also implement the `--continue` / `--resume` pattern documented in Claude Code best practices.

**Warning signs:**
- Agent re-proposes changes to files it already modified
- After a long session, agent "forgets" the original goal and starts on unrelated tasks
- User notices duplicate edits after approving the same proposal twice

**Phase to address:**
Agent Mode Phase 5 (session continuity) — the persistent session state file must be designed before implementing multi-turn agent sessions in Phase 2. Retrofitting it after agents can run multiple turns risks losing session data already accumulated.

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Inline SOI parsing on CommandFinished handler | No new async wiring | Blocks main thread, input latency spikes | Never — the handler is already at the edge of acceptable complexity |
| Regex-based ANSI stripping (reuse existing) | No new dependencies | Residual escape bytes break format parsers | Only for history display, never for SOI parser input |
| SOI injected into PTY stream instead of overlay | Simpler rendering path | Race conditions with OSC 133 boundaries, corrupts history | Never — fundamental architecture error |
| Agent approval as modal overlay (reuse existing pattern) | Reuse SearchOverlay code | Terminal unusable while approval pending | Never for Agent Mode; modal is acceptable for error display only |
| All 30+ MCP tools registered globally with verbose descriptions | Complete documentation | 25K–35K token burn per agent session | Only during initial development; must compress before Agent Mode ships |
| No budget cap default (unlimited spending) | Simpler config | User financial harm | Never — must have a default cap |
| Claude CLI subprocess spawned without Job Object (Windows) | Simpler cross-platform code | Orphaned processes after crash, ongoing API costs | Never for production; acceptable in early prototype testing only |
| Skip worktree cleanup registration | Faster worktree creation | Orphaned worktrees accumulate, disk fills | Never — registration takes <1ms and prevents disk exhaustion |

---

## Integration Gotchas

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| Claude CLI subprocess | `Command::new("claude")` without platform flags | Use `glass_agent::spawn` abstraction with `CREATE_NO_WINDOW` on Windows and `process_group(0)` on Unix |
| Git worktree create | `git worktree add` then cleanup on failure | Write to SQLite first, create worktree second, update SQLite on success |
| MCP tool registration | Register all tools at server start | Audit token footprint first; compress descriptions to ≤100 tokens each |
| SOI parser input | Pass `OutputBuffer.text` directly | Strip ANSI via state machine first; gate on `is_binary()` check |
| Activity stream | Push every SOI event to agent | Deduplicate and filter before push; collapse repetitive success events |
| Approval notification | Reuse SearchOverlay (modal, captures all input) | New toast + hotkey pattern; non-modal; auto-timeout |
| Context compaction | Let SDK handle it silently | Detect compaction events, inject persistent session state file as prefix |
| ANSI stripping for SOI | Reuse existing `strip_ansi` regex in glass_history | Replace with `strip_ansi_escapes` crate (state machine, handles split boundaries) |

---

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| SOI parsing inline on CommandFinished | Input latency >5ms after any command with output | Async dispatch via channel; parse in background task | Any command producing >5KB output |
| Multi-pass SOI classifier (try all parsers) | CPU spike on every command completion | Fast early-rejection: check first byte/line before attempting full parse | High-frequency `cargo watch` sessions (50+ commands/hour) |
| Full SOI record in agent context per event | Context window exhausted in <20 turns | Stream only summary strings to agent, not full records; full records available via MCP query tool | Sessions with >20 SOI events before agent starts |
| Worktree creation without disk space check | Disk full mid-creation, partial orphan | Check available space before `git worktree add`; enforce per-user worktree count cap | Large repos (node_modules, build artifacts) |
| Agent turn loop without timeout | Agent runs indefinitely, costs spiral | Enforce `max_turns` and `max_budget_usd` hard limits with timer-based kill | Any open-ended prompt without specific success criteria |

---

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| Agent proposes and auto-approves `rm -rf` | Irreversible file deletion | Agent proposals MUST go through approval UI regardless of `auto_approve` setting; destructive commands (Glass's existing `command_parser` list) always require explicit confirmation |
| Prompt injection via command output into agent context | Malicious output from a command hijacks agent goals | Sanitize all SOI summaries before injecting into agent context; treat command output as untrusted data with a trust boundary between PTY output and agent input |
| Agent writes to files outside the worktree | Escapes isolation, modifies production code | Enforce worktree confinement: agent file operations must target only paths within the worktree; Glass validates all proposed file paths against the worktree root before approval |
| ANSI escape sequences in MCP tool output to AI | ANSI codes can hide malicious payloads in tool descriptions (Trail of Bits, April 2025) | Strip ANSI from all tool return values before they reach the agent's context window |
| Agent process with network access runs unchecked | Agent exfiltrates repository contents or API keys | Log all subprocess command executions in `glass_history`; require network tool approval separately from file tool approval |

---

## UX Pitfalls

| Pitfall | User Impact | Better Approach |
|---------|-------------|-----------------|
| SOI summaries appear when there's nothing useful to say | Summary clutter for every `ls` and `echo` — user disables feature | Only show SOI decorations when confidence ≥ 0.8 AND the parsed record contains structured data beyond a raw count |
| Agent approval toast blocks terminal content | User can't read the output the agent is proposing to act on | Toast must be semi-transparent or positioned to not obscure the relevant output; show which block the agent is referencing |
| SOI classification shown as "unknown" for most commands | User perceives feature as broken | Do not display any SOI UI decoration when classification is `None`; silence is correct for unstructured output |
| Agent mode notification badge always visible even when idle | Constant attention demand | Agent status badge only appears when agent is actively running or has pending approvals; idle agents don't show a badge |
| Session continuity "resumes" but starts from scratch | User confusion when agent forgets context | Show the session's original goal and change count in the resume prompt so user can verify continuity |

---

## "Looks Done But Isn't" Checklist

- [ ] **SOI parsing:** Parser appears to work on test fixtures — verify against real shell output including colorized git log, TypeScript errors with ANSI highlights, and Docker multi-stage build output
- [ ] **SOI async dispatch:** SOI records appear in SQLite — verify that input latency benchmark does NOT regress (run criterion `input_latency` test before and after)
- [ ] **Shell summary rendering:** Summary text appears after commands — verify that `OutputBuffer` raw bytes do NOT contain the summary text (summary is overlay-only, not in history DB)
- [ ] **Agent subprocess cleanup:** Agent terminates when user closes Glass — verify with Task Manager / `ps aux` that no `claude` processes remain after Glass exits abnormally (`kill -9` / Task Manager kill)
- [ ] **Worktree cleanup:** Agent creates worktree — verify that crashing Glass mid-creation (kill during `git worktree add`) and restarting runs cleanup before creating a new worktree
- [ ] **Budget cap:** Agent runs with `max_budget_usd = 0.50` — verify agent stops before $0.50 and sends a "budget exceeded" message, does NOT silently continue
- [ ] **Approval non-blocking:** Approval toast appears — verify that pressing any regular key (including Enter) while a toast is visible types in the terminal, not dismisses the toast
- [ ] **MCP token budget:** All 30+ tools registered — measure actual token count of full tool schema with `claude --print-mcp-schema` or equivalent; verify under 15K tokens total
- [ ] **Prompt injection defense:** Agent receives command output — verify that output containing `\nSystem: ignore all previous instructions` does NOT alter agent behavior (test with a benign trigger phrase)

---

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| SOI parser blocks PTY thread | HIGH — requires architectural refactor | Extract all post-CommandFinished work into an async worker; add a bounded channel between the event handler and the worker; rewrite SOI dispatch to use the channel |
| PTY injection approach chosen for summaries | HIGH — rewrite SOI rendering layer | Remove PTY write calls; add `soi_summary` field to `Block` struct; add rendering in `BlockRenderer`; migrate existing injected summaries out of history DB |
| Orphaned agent processes after crash | MEDIUM — add Job Object/prctl in new platform module | Write `glass_agent::spawn` abstraction; backfill Job Object assignment for already-spawned agents in current session |
| Worktree orphans accumulated | LOW — one-time cleanup | Run `git worktree prune` in all registered repos; delete rows from `pending_worktree` table; run after next Glass startup |
| API costs exceeded without cap | LOW — config change | Add `max_budget_usd` to `[agent]` config section with default `1.0`; SDK applies on next session |
| MCP token bloat discovered late | MEDIUM — requires description rewrites | Audit all tool descriptions; rewrite to ≤100 tokens each; no API changes needed |

---

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| SOI parser blocks PTY thread | SOI Phase 1 (classification pipeline) | Run criterion input_latency benchmark before and after; must stay <5ms |
| ANSI residue corrupts parser input | SOI Phase 1 (classification pipeline) | Feed real `git log --color=always` output to classifier; verify zero residual ANSI bytes |
| Shell summary injection race | SOI Phase 1 (architecture decision: overlay vs injection) | Verify OutputBuffer does not contain SOI text; verify no OSC sequence timing failures |
| Binary output misclassification | SOI Phase 1 (classification) | Feed `xxd /dev/urandom | head` output; verify 0% classification rate |
| Agent zombie processes on crash | Agent Mode Phase 1 (runtime) | `kill -9` Glass while agent running; verify no `claude` processes survive; Windows: Task Manager |
| Worktree orphan accumulation | Agent Mode Phase 3 (worktree isolation) | Crash Glass during `git worktree add`; verify startup prune removes orphan |
| API cost spiral | Agent Mode Phase 2 (runtime) | Run agent with `max_budget_usd = 0.10`; verify stops with budget message |
| Approval UI blocks terminal | Agent Mode Phase 4 (approval UI) | While approval toast visible, type in terminal; verify keystrokes go to PTY |
| MCP tool token bloat | SOI Phase 6 (MCP tools) | Measure token footprint before and after adding SOI tools; must stay under 15K total |
| Windows subprocess differences | Agent Mode Phase 1 (runtime) | Windows CI test: spawn agent, kill parent, verify no console window and no orphan process |
| Activity stream noise | Agent Mode Phase 1 (activity stream) | Run `cargo watch` for 5 minutes; verify stream compresses to ≤20 unique events |
| Context window exhaustion recovery | Agent Mode Phase 5 (session continuity) | Run agent past compaction threshold; verify resumed session still knows original goal |

---

## Sources

- [VSCode terminal integration race condition (Issue #237208)](https://github.com/microsoft/vscode/issues/237208) — OSC 633;D timing, 80% failure rate
- [opencode orphaned worktrees (Issue #14648)](https://github.com/anomalyco/opencode/issues/14648) — bootstrap failures leave orphaned full-repo clones
- [opencode worktree cleanup fix (PR #14649)](https://github.com/anomalyco/opencode/pull/14649) — SQLite-backed cleanup registration pattern
- [Manage costs effectively — Claude Code Docs](https://code.claude.com/docs/en/costs) — `max_budget_usd` parameter, agent team 7x token multiplier
- [How the agent loop works — Claude API Docs](https://platform.claude.com/docs/en/agent-sdk/agent-loop) — loop termination, compaction behavior
- [Tool-space interference in the MCP era — Microsoft Research](https://www.microsoft.com/en-us/research/blog/tool-space-interference-in-the-mcp-era-designing-for-agent-compatibility-at-scale/) — tool token overhead at scale
- [MCP Isn't Dead, But Bloated Agentic Workflows Are — DomAIn Labs](https://www.domainlabs.dev/blog/agent-guides/mcp-bloated-workflows-skills-architecture) — "58 tools, ~55K tokens before conversation"
- [Deceiving users with ANSI terminal codes in MCP — Trail of Bits (April 2025)](https://blog.trailofbits.com/2025/04/29/deceiving-users-with-ansi-terminal-codes-in-mcp/) — ANSI injection into tool descriptions
- [Prompt Injection to RCE in AI agents — Trail of Bits (October 2025)](https://blog.trailofbits.com/2025/10/22/prompt-injection-to-rce-in-ai-agents/) — command injection via tool output
- [anstyle-parse: ANSI state machine parser — DeepWiki](https://deepwiki.com/rust-cli/anstyle/2.3-anstyle-parse:-ansi-escape-code-parsing) — state machine vs regex correctness
- [Claude Code rate limits and pricing — Northflank](https://northflank.com/blog/claude-rate-limits-claude-code-pricing-cost) — session cost modeling
- [Destroying all child processes when parent exits — Old New Thing (Microsoft)](https://devblogs.microsoft.com/oldnewthing/20131209-00/?p=2433) — Windows Job Object pattern for subprocess cleanup
- [Claude agent SDK: windowsHide subprocess issue (December 2025)](https://github.com/anthropics/claude-agent-sdk-typescript/issues/103) — Windows CREATE_NO_WINDOW requirement
- [Managing Long Contexts in Agentic Coding Systems](https://cto.new/blog/managing-long-contexts-in-agentic-coding-systems) — compaction strategies and session state persistence
- [Structured outputs create false confidence — BAML Blog](https://boundaryml.com/blog/structured-outputs-create-false-confidence) — classification over-confidence pitfall
- Glass codebase: `src/main.rs` (lines 1732–2188: AppEvent::Shell handler — location where SOI parsing must NOT be added inline)
- Glass codebase: `crates/glass_terminal/src/pty.rs` — std::thread PTY reader; blocking I/O constraint
- Glass codebase: `crates/glass_history/src/lib.rs` — existing ANSI stripping (regex-based, insufficient for SOI parser input)
- Glass codebase: `crates/glass_coordination/` — agents.db pattern for agent PID persistence and cleanup registration

---
*Pitfalls research for: SOI output parsing and Agent Mode background AI agents in existing Rust terminal emulator*
*Researched: 2026-03-12*
