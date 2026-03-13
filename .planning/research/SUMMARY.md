# Project Research Summary

**Project:** Glass v3.0 — SOI & Agent Mode
**Domain:** Structured output intelligence, background AI agent runtime, git worktree isolation
**Researched:** 2026-03-12
**Confidence:** HIGH

## Executive Summary

Glass v3.0 adds two interconnected capabilities on top of a working GPU terminal emulator: Structured Output Intelligence (SOI), which parses and compresses command output into machine-readable records, and Agent Mode, which feeds those records to a background Claude CLI process that watches development activity and proposes code fixes. The research shows a strong existing foundation — output capture, shell integration (OSC 133), a history DB, 25 MCP tools, multi-agent coordination, and a working overlay rendering system — that maps cleanly to the requirements. The recommended build order is SOI first, Agent Mode second, because the agent runtime depends entirely on the compressed activity stream that SOI produces. SOI phases are independently shippable and deliver immediate value to AI assistants querying `glass_query` MCP tools even before any background agent exists.

The technology decisions are minimal: only two new crates (`uuid 1.22`, `git2 0.20`) are required. All parsing, storage, serialization, async process management, and DB access are covered by existing workspace dependencies. `glass_soi` and `glass_agent` are the two new crates to create; seven existing crates require targeted modifications. Architecture research was derived from direct codebase inspection and produces high-confidence integration points: SOI parsing runs in `tokio::task::spawn_blocking` off the main thread, new DB tables co-locate in the existing history DB file via the established open-per-request connection pattern, and Agent Mode's `AgentRuntime` lives as a struct in `Processor` (matching the existing coordination poller pattern) rather than as a second long-running process.

The critical risks are front-loaded: blocking the winit event loop with SOI parsing, injecting summary text into the PTY stream (which races with OSC 133 boundaries), and spawning the Claude CLI subprocess without platform-appropriate process lifecycle management (Windows Job Objects / Unix `prctl`). All three architectural decisions must be settled in the first implementation phase or they require rewrites. Secondary risks include API cost spirals without a default `max_budget_usd` cap, worktree orphan accumulation on crash, and MCP tool token bloat as the tool count approaches 30+.

## Key Findings

### Recommended Stack

The complete new dependency requirement is two crates added to `[workspace.dependencies]`: `uuid = { version = "1", features = ["v4"] }` and `git2 = "0.20"`. Everything else — tokio, regex, serde/serde_json, rusqlite, strip-ansi-escapes, similar, chrono, anyhow, tracing — is already present as workspace dependencies. The rejection list is equally important: no `nom`, `pest`, `tap_parser`, `junit-parser`, `lazy_static`, `once_cell`, `crossbeam-channel`, or `subprocess` crate additions are justified. See STACK.md for the full rationale.

**Core technologies:**
- `tokio::process::Command` — spawn and manage Claude CLI child process — already in workspace as `tokio = { version = "1.50.0", features = ["full"] }`, provides `Stdio::piped()`, `AsyncWriteExt`, `BufReader` for line-by-line reads
- `git2 0.20` — `WorktreeManager`: `worktree_add`, `prune`, `path`, `validate` — chosen over shelling out to `git` (fragile) and `gitoxide` (still maturing for worktree ops); `Worktree` is `!Send` so must use `spawn_blocking` (pattern already established in `glass_mcp`)
- `uuid 1.22` — `AgentProposal.id`, `agent_sessions.id` — pure Rust, no system deps, only `v4` feature needed
- `regex 1.12.3` (workspace) — SOI output classification patterns — use `std::sync::LazyLock<Regex>` or `OnceLock<Regex>` per existing glass_errors pattern; no `lazy_static` or `once_cell` needed
- `serde_json 1.0` (workspace/direct) — JSON wire protocol for Claude CLI stdin/stdout; JSON storage in `detail_json` column; NDJSON parsing (split on `\n`, filter blank, `from_str` per line — no separate ndjson crate needed)

### Expected Features

**Must have — SOI (table stakes for v3.0):**
- Output classifier — detects output type from command + content; entry point for all SOI value
- Parsers for cargo build, cargo test, npm, pytest, jest — covers 80%+ of commands for the target audience
- Per-command compressed summaries — one-line, summary, detailed, full — at 4 token-budget levels
- SQLite storage schema — `command_output_records` and `output_records` tables in existing history DB
- Auto-parse on `CommandFinished` — invisible machinery, no user trigger required
- `glass_query`, `glass_query_trend`, `glass_query_drill` MCP tools — agents need queryable structured output
- SOI summary rendered as block decoration (NOT injected into PTY stream — see Pitfall 3)

**Must have — Agent Mode (table stakes for v3.0):**
- Background Claude CLI process watching compressed activity stream
- Worktree isolation — agents never touch working tree directly; `git worktree` create/diff/apply/cleanup
- Proposal approval UI — non-blocking toast + hotkey pattern (NOT a modal overlay — see Pitfall 8)
- Configurable autonomy levels: Watch / Assist / Autonomous; default `mode = "watch"`, `edit_files = "approve"`
- `max_budget_usd` with a non-unlimited default (1.0 USD) — required field, not optional
- Integration with `glass_coordination` for advisory lock management

**Should have — competitive differentiators:**
- Shell summary hint line written to PTY (visible to Claude Code's Bash tool output capture)
- `glass_query_trend` — regression detection across historical runs (unique to Glass)
- Expanded parsers: git, docker, kubectl, tsc, Go — adds devops tool coverage
- Session continuity / handoff JSON on context exhaustion
- Activity stream noise filtering — deduplicate, collapse repetitive success events, default to `"important"` verbosity

**Defer to v3.x / v4+:**
- SOI per-stage pipe parsing
- Generic JSON lines parser
- Session continuity (v3.x — most complex, least critical for initial release)
- Agent CLI status subcommand
- SOI parser plugin system, trend anomaly detection, multi-model routing

### Architecture Approach

Glass v3.0 adds two new crates (`glass_soi`, `glass_agent`) and modifies seven existing ones (`glass_core`, `glass_history`, `glass_mcp`, `glass_renderer`, `src/main.rs`, and optionally `glass_terminal/block_manager.rs`). All integration flows through the existing `AppEvent` / `EventLoopProxy` communication pattern: SOI parsing spawns as a `spawn_blocking` task from the `CommandFinished` handler in `main.rs`, emits `AppEvent::SoiReady` when complete, which then feeds the `AgentRuntime` activity channel. The agent runtime is a struct in `Processor` (not a separate process), manages the Claude CLI child internally, and communicates back via `AppEvent::AgentProposal`. New DB tables co-locate in the existing history DB file using the open-per-request connection pattern. Rendering follows the existing overlay pattern (stateless renderers passed data from main.rs, additional draw calls after the main frame).

**Major components:**
1. `glass_soi::OutputClassifier` + Parser Registry — classifies command output type, routes to format-specific parsers (rust.rs, test_runners.rs, pkg_mgr.rs, devops.rs, structured.rs); runs off main thread via `spawn_blocking`
2. `glass_soi::CompressionEngine` — 4-level token-budgeted summaries (OneLine / Summary / Detailed / Full); feeds block decoration and activity stream
3. `glass_soi::SoiDb` — writes `command_output_records` and `output_records` to the history DB file via its own open-per-request connection; does NOT import `glass_history`
4. `glass_agent::AgentRuntime` — manages Claude CLI child process via `tokio::process::Command`; reads proposals from stdout (JSON lines); writes activity events to stdin; emits `AppEvent::AgentProposal`
5. `glass_agent::WorktreeManager` — `git2`-based worktree create/diff/apply/cleanup with SQLite-backed cleanup registration to handle crash recovery
6. `glass_agent::ActivityStream` — bounded mpsc channel with rolling budget window; deduplicates and filters SOI events before forwarding to agent
7. Toast + `AgentOverlayRenderer` — non-blocking proposal review (hotkey-driven, auto-dismiss); modeled after existing overlays but explicitly non-modal

### Critical Pitfalls

1. **SOI parser blocking the PTY thread / event loop** — Use `tokio::task::spawn_blocking` for all SOI parsing; emit `AppEvent::SoiReady` asynchronously; never call `pipeline::run()` inline in the `CommandFinished` arm of main.rs. Validate with the existing criterion `input_latency` benchmark — must not regress.

2. **Shell summary injection racing with OSC 133 prompt boundary** — Do NOT write summary text to the PTY byte stream. Render summaries as host-side `BlockRenderer` decorations (the `Block` struct gains `soi_summary: Option<String>`). This eliminates the VSCode-documented 80% failure rate on this race condition and prevents SOI text from appearing in the history DB's output column.

3. **Agent subprocess orphaned on crash (Windows Job Objects / Unix prctl)** — Implement a `glass_agent::spawn` platform-abstraction module in Phase 1 of agent work. Windows: `CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP` + Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. Unix: `process_group(0)` + `prctl(PR_SET_PDEATHSIG, SIGTERM)`. Store PIDs in the existing `~/.glass/agents.db` (glass_coordination pattern) for startup cleanup scan.

4. **Git worktree orphan accumulation on crash** — Register worktree in SQLite (`pending_worktree` row) BEFORE calling `git worktree add`; update to `active` on success; run cleanup on Glass startup for any `pending` rows. Cap per-repo worktree count at 3 (configurable). This pattern is validated by opencode PR #14649.

5. **API cost spirals without budget cap** — `max_budget_usd` must be a required config field with a non-unlimited default (1.0 USD). Surface real-time cost in the status bar. Implement `max_turns` hard limit. Ship these in Agent Mode Phase 1 — adding them later risks user financial harm and cannot be treated as polish.

6. **Approval UI blocking terminal interaction** — Do NOT reuse the existing modal `SearchOverlay` pattern for agent proposals. Design: toast notification at bottom of active pane (hotkeys: Alt+A accept, Alt+R reject, auto-dismiss after 30s), side-panel review overlay that does not capture keyboard focus from the terminal.

7. **MCP tool token bloat at 30+ tools** — Before adding SOI MCP tools, audit token footprint of all 25 existing tools. Target: ≤100 tokens per tool description, ≤15K total. Consider collapsing `glass_query` / `glass_query_trend` / `glass_query_drill` into one tool with a `mode` parameter. Anthropic's own testing showed 58 tools consuming ~55K tokens before any conversation content.

## Implications for Roadmap

Based on the architecture's build-order constraints (glass_soi must precede glass_agent; SOI phases 1-3 are independently shippable), the following phase structure is recommended. This maps directly to the 13-phase build order documented in ARCHITECTURE.md.

### Phase 1: SOI Foundation — Classifier and Parser Crate
**Rationale:** All downstream work (storage, MCP tools, agent activity stream) depends on the `ParsedOutput` and `OutputRecord` types and the `Parser` trait. This phase has zero new crate dependencies and can be built and tested in full isolation. The async dispatch boundary and the classification rejection strategy (aggressive `None` default, ≥3 matching lines before committing) must be settled here — not retrofitted later.
**Delivers:** `glass_soi` crate with `OutputClassifier`, `Parser` trait, parsers for cargo build/test/clippy, jest, pytest, npm; `ParsedOutput`, `OutputRecord`, `OutputType`, `Severity` public types.
**Addresses:** Table-stakes SOI features; establishes the correct `spawn_blocking` + `EventLoopProxy` async pattern.
**Avoids:** PTY blocking (Pitfall 1), ANSI residue corruption (Pitfall 2), binary/alt-screen misclassification (Pitfall 4).

### Phase 2: SOI Storage Schema and DB Extension
**Rationale:** Parsers are useless without storage. Schema migration from v2 to v3 must happen before any tool or agent can query structured records. Follows the existing `PRAGMA user_version` bump + `migrate()` pattern in `glass_history`.
**Delivers:** `command_output_records` and `output_records` tables in the history DB; `SoiDb` struct in `glass_soi` using open-per-request connections (does NOT import `glass_history`); schema v3 migration.
**Uses:** `rusqlite` (workspace), `serde_json` for `detail_json` column.
**Implements:** Open-per-request SQLite pattern (Architecture Pattern 2).

### Phase 3: SOI Pipeline Integration into main.rs
**Rationale:** First end-to-end SOI flow. After this phase, every completed command produces a `SoiReady` event and a record in SQLite. This is the integration phase where `AppEvent::SoiReady` is added to `glass_core/event.rs` and `main.rs` gets the `spawn_blocking` dispatch in the `CommandFinished` handler.
**Delivers:** Automatic SOI parsing on every `CommandFinished`; `AppEvent::SoiReady { command_id, summary, severity }` variant; `pending_soi_output` stash in session state.
**Avoids:** Blocking the event loop (Pitfall 1 prevention confirmed here via criterion benchmark); shell summary injection race (architecture decision: overlay, not PTY write — Pitfall 3).

### Phase 4: SOI Compression Engine
**Rationale:** Summaries are the user-visible output of SOI and the input to the agent activity stream. Four budget levels (OneLine / Summary / Detailed / Full) serve both the terminal decoration and agent context efficiently. Internal to `glass_soi` — no new crate changes.
**Delivers:** `CompressionEngine` in `glass_soi/compression.rs`; token-budgeted summaries at 4 levels; `SoiStore` high-level API.

### Phase 5: SOI Block Decoration
**Rationale:** Makes SOI visible to users as a terminal UI feature. Block struct gains `soi_summary: Option<String>`; `BlockRenderer` renders it as a muted decoration after command output. Depends on Phase 3 (SoiReady event) and Phase 4 (summaries exist).
**Delivers:** SOI one-liner visible in terminal after every classified command; no PTY stream pollution; `OutputBuffer` does NOT contain summary text.
**Avoids:** PTY injection race (Pitfall 3 fully closed here).

### Phase 6: SOI MCP Tools
**Rationale:** Unlocks the primary AI assistant use case. Agents can query structured output via `glass_query`, `glass_query_trend`, `glass_query_drill`. Requires Phase 2 (storage queryable) and Phase 4 (CompressedOutput type). Conduct token audit of all 25 existing tools in this phase before adding new ones.
**Delivers:** Three new MCP tools in `glass_mcp/tools.rs`; token audit result; tool descriptions ≤100 tokens each.
**Avoids:** MCP token bloat (Pitfall 9).

### Phase 7: Additional SOI Parsers
**Rationale:** Expands SOI coverage to devops tools. Independent of agent work — each parser is a self-contained addition to the parser registry. Git, docker, kubectl, tsc, Go parsers are the P2 targets.
**Delivers:** Extended coverage for devops/infrastructure commands; generic JSON lines / NDJSON fallback.

### Phase 8: Agent Activity Stream
**Rationale:** First agent mode work. Creates the `glass_agent` crate with `ActivityStream` — a bounded mpsc channel with rolling budget window and noise filtering. Depends on Phase 3 (`SoiReady` emits `ActivityEvent`). Activity stream noise filtering (deduplication, collapse of repetitive success events) must be part of this phase — not added later.
**Delivers:** `glass_agent` crate with `ActivityStream`; filtered/deduplicated event feed; configurable `activity_stream_verbosity`.
**Avoids:** Activity stream noise overwhelming agent context (Pitfall 11).

### Phase 9: Agent Runtime — Claude CLI Background Process
**Rationale:** Core agent behavior. `AgentRuntime` spawns Claude CLI as a child process, writes activity events to stdin (JSON lines protocol), reads `AgentProposal` from stdout, emits `AppEvent::AgentProposal`. Platform subprocess management (Job Objects / prctl) must be implemented here — not retrofitted.
**Delivers:** `glass_agent::AgentRuntime` with lifecycle management; `AppEvent::AgentProposal` variant; `agent_runtime` field in `Processor`; `max_budget_usd` and `max_turns` config defaults enforced.
**Avoids:** Zombie processes on crash (Pitfall 5), Windows subprocess console windows (Pitfall 10), API cost spirals (Pitfall 7).

### Phase 10: Worktree Isolation
**Rationale:** Establishes safe code isolation for agent code changes. SQLite-backed cleanup registration must be the first thing built in this phase (register `pending_worktree` before creating, update on success, prune on startup). `git2`'s `Worktree` is `!Send` — use `spawn_blocking`.
**Delivers:** `glass_agent::WorktreeManager`; worktree create/diff/apply/cleanup; crash recovery via startup prune; per-repo cap of 3 worktrees.
**Avoids:** Worktree orphan accumulation (Pitfall 6).

### Phase 11: Approval UI — Toast, Status Bar, Review Overlay
**Rationale:** Makes agent proposals visible and actionable without blocking terminal use. Non-modal design is non-negotiable. Toast + hotkeys (Alt+A / Alt+R) + side-panel diff overlay. Status bar shows agent mode indicator and pending proposal count.
**Delivers:** `ToastRenderer`, `AgentOverlayRenderer`, updated `StatusBarRenderer`; `Ctrl+Shift+A` review overlay; non-blocking approval flow.
**Avoids:** Approval UI blocking terminal interaction (Pitfall 8).

### Phase 12: Session Continuity
**Rationale:** Required for multi-hour agent sessions. Persistent session state JSON (`~/.glass/agent-sessions/<id>.json`) records original goal, modified files, approval decisions, worktree branch. Injected as system prompt prefix on context compaction. Most complex, least critical for initial v3.0 release — this is a strong v3.x candidate.
**Delivers:** `SessionStore` in `glass_agent`; `agent_sessions` table; handoff JSON on context exhaustion; restored context on session resume.
**Avoids:** Context window exhaustion with no recovery (Pitfall 12).

### Phase 13: Configuration and Polish
**Rationale:** Completes the `[soi]` and `[agent]` config sections, permission matrix, graceful degradation paths, and CI coverage for agent mode (Windows subprocess tests). Final integration testing.
**Delivers:** Full `config.toml` support for SOI and agent mode; `#[cfg(target_os = "windows")]` agent subprocess tests; graceful degradation when `claude` binary not found.

### Phase Ordering Rationale

- Phases 1-7 (SOI) are independently shippable and deliver immediate value to AI assistants without any Agent Mode work. SOI is the foundation the agent depends on, not an optional add-on.
- Phases 8-13 follow a strict dependency order: activity stream before runtime, runtime before worktrees, worktrees before approval UI, all of them before continuity.
- Critical architectural decisions (async dispatch boundary, overlay vs PTY injection for summaries, non-modal approval) are locked in Phases 1-3 because retrofitting them costs a full rewrite of the affected layers.
- Phase 12 (session continuity) is the most complex and least critical for initial release — it is the strongest candidate to slip to v3.x if schedule pressure exists.

### Research Flags

Phases likely needing deeper `/gsd:research-phase` during planning:
- **Phase 9 (Agent Runtime):** Claude CLI JSON protocol details (exact stdin/stdout schema, session token format, `--resume` flag behavior, compaction detection) may need validation against current Claude CLI docs at planning time. The protocol is a moving target.
- **Phase 10 (Worktree Isolation):** Cross-platform `git2` worktree behavior on Windows (path separators, branch naming constraints) and the non-git fallback strategy need validation on the target platform.
- **Phase 11 (Approval UI):** Hotkey conflict analysis against existing Glass shortcuts needs a full audit before finalizing Alt+A / Alt+R bindings.

Phases with standard patterns (research not needed):
- **Phase 1 (SOI Crate):** Regex pattern matching, trait-based parser registry — well-established Rust patterns.
- **Phase 2 (DB Schema):** SQLite schema migration — established pattern already used in glass_history.
- **Phase 3 (Pipeline Integration):** `spawn_blocking` + `EventLoopProxy` — pattern already present in glass_mcp and glass_core pollers.
- **Phase 6 (MCP Tools):** Adding tools to GlassServer — established pattern in existing glass_mcp.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | All decisions verified against existing Cargo.toml and docs.rs. Only 2 new crates required, both well-established. |
| Features | HIGH (SOI), MEDIUM (Agent Mode) | SOI feature set is well-defined with established patterns (Pare, glass_errors). Agent Mode UX patterns are 2025 conventions but the space is still evolving — approval UX specifics may need iteration. |
| Architecture | HIGH | Derived from direct codebase inspection of main.rs, glass_mcp, glass_history, glass_renderer. Integration points are concrete line numbers, not speculation. |
| Pitfalls | HIGH | 12 pitfalls with verified sources (VSCode issue tracker, opencode PRs, Trail of Bits security research, Anthropic Claude cost docs, Glass codebase line references). |

**Overall confidence:** HIGH

### Gaps to Address

- **Claude CLI JSON wire protocol:** The exact format of the JSON activity stream written to claude's stdin and the `AgentProposal` JSON read from stdout needs validation against the current Claude CLI release before Phase 9 implementation. The spec in `SOI_AND_AGENT_MODE.md` defines the schema but the CLI's actual parsing behavior may differ.
- **Token budget measurement baseline:** The current MCP tool token footprint (25 tools) has not been measured against a live Claude session. Phase 6 should begin with a measurement step before any description rewrites.
- **git2 Windows path behavior:** git2 0.20 on Windows with paths containing spaces or non-ASCII characters in the Glass worktrees directory (`~/.glass/worktrees/`) has not been explicitly tested. Phase 10 should include a Windows-specific test for this.
- **Activity stream verbosity defaults:** Proposed values (`"important"`, 20-event window, 30s cooldown) are based on Copilot/Cursor behavioral documentation; optimal values will emerge from real usage and may need tuning in v3.x.

## Sources

### Primary (HIGH confidence)
- `C:/Users/nkngu/apps/Glass/Cargo.toml` — verified existing workspace deps
- `C:/Users/nkngu/apps/Glass/SOI_AND_AGENT_MODE.md` — feature spec with OutputRecord types, AgentProposal struct, WorktreeManager API
- `C:/Users/nkngu/apps/Glass/src/main.rs` (lines 2664, 2882) — CommandFinished and CommandOutput handler locations
- `C:/Users/nkngu/apps/Glass/crates/glass_core/src/event.rs` — AppEvent variants
- `C:/Users/nkngu/apps/Glass/crates/glass_history/src/db.rs` — open-per-request DB pattern
- `C:/Users/nkngu/apps/Glass/crates/glass_mcp/src/lib.rs` — spawn_blocking pattern for MCP tools
- [docs.rs/git2/latest — Worktree struct](https://docs.rs/git2/latest/git2/struct.Worktree.html) — worktree_add, prune, path methods confirmed
- [docs.rs/uuid/latest](https://docs.rs/uuid/latest/uuid/) — v4 feature confirmed
- [tokio::process docs](https://docs.rs/tokio/latest/tokio/process/index.html) — Stdio::piped, AsyncWriteExt, BufReader

### Secondary (MEDIUM confidence)
- [Pare: Structured Output for AI Coding Agents](https://dev.to/dave_london_d0728737f5d67/structured-output-for-ai-coding-agents-why-i-built-pare-2k5f) — validates SOI approach and compression level patterns
- [Git Worktrees for AI Agents — Nick Mitchinson](https://www.nrmitchi.com/2025/10/using-git-worktrees-for-multi-feature-development-with-ai-agents/) — worktree isolation as industry standard
- [ccswarm: Multi-agent worktree isolation](https://github.com/nwiizo/ccswarm) — Rust parallel agent + worktree pattern
- [opencode orphaned worktrees issue #14648 + PR #14649](https://github.com/anomalyco/opencode/issues/14648) — SQLite-backed cleanup registration pattern validated
- [Manage costs effectively — Claude Code Docs](https://code.claude.com/docs/en/costs) — max_budget_usd, 7x agent team token multiplier
- [Tool-space interference in the MCP era — Microsoft Research](https://www.microsoft.com/en-us/research/blog/tool-space-interference-in-the-mcp-era-designing-for-agent-compatibility-at-scale/) — tool token overhead at scale
- [MCP bloated workflows — DomAIn Labs](https://www.domainlabs.dev/blog/agent-guides/mcp-bloated-workflows-skills-architecture) — "58 tools, ~55K tokens"
- [Destroying child processes when parent exits — Old New Thing](https://devblogs.microsoft.com/oldnewthing/20131209-00/?p=2433) — Windows Job Object pattern
- [Claude agent SDK windowsHide issue (December 2025)](https://github.com/anthropics/claude-agent-sdk-typescript/issues/103) — CREATE_NO_WINDOW requirement
- [VSCode terminal integration race condition #237208](https://github.com/microsoft/vscode/issues/237208) — OSC 633;D timing, 80% failure rate

### Tertiary (LOW confidence — needs validation during implementation)
- Claude CLI JSON wire protocol schema — inferred from SOI_AND_AGENT_MODE.md spec; needs validation against current CLI release
- git2 0.20 Windows path handling — assumed correct; needs empirical test with non-ASCII paths
- Activity stream verbosity defaults — proposed values based on Copilot/Cursor behavioral documentation; optimal values will emerge from real usage

---
*Research completed: 2026-03-12*
*Ready for roadmap: yes*
