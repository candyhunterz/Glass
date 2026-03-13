# Feature Research

**Domain:** Structured Output Intelligence (SOI) + Agent Mode for GPU terminal emulator
**Researched:** 2026-03-12
**Confidence:** HIGH (SOI patterns), MEDIUM (Agent Mode UX patterns — emerging space)

---

## Context: What Already Exists

These features are DONE. Research below covers only NEW work for v3.0.

- Output capture (50KB, ANSI-stripped, alt-screen filtered) — `glass_terminal`
- `glass_errors` crate — Rust JSON, Rust human, and generic `file:line:col` parsers
- `glass_extract_errors` MCP tool — structured error extraction
- `glass_compressed_context` MCP tool — budget-aware context compression
- `glass_context` MCP tool — full context assembly
- 25 MCP tools total — agents can already query history, snapshots, pipe stages
- Command block lifecycle — `PromptActive -> InputActive -> Executing -> Complete`
- `glass_coordination` — multi-agent registry, advisory locks, inter-agent messaging

The v3.0 work builds ABOVE this foundation, not beside it.

---

## Feature Landscape

### Table Stakes — SOI

Features that agents and power users will expect from any "AI-native terminal" claim.
Missing these makes the entire SOI value proposition hollow.

| Feature | Why Expected | Complexity | Existing Foundation |
|---------|--------------|------------|---------------------|
| Output classifier — detect output type from command + content | Agents can't use structured data they don't know exists. Classification is the entry point. Every structured output tool (Pare, IDE agents) classifies before parsing. | MEDIUM | `glass_errors` already classifies Rust vs generic. Extend to test runners, package managers, git, JSON. |
| Parser for Rust/cargo test results | `cargo test` is the #1 command run in a Rust project. Fail to parse it and SOI is useless for the target audience. | LOW | `glass_errors` already parses Rust compiler. Test result format (`test X ... ok/FAILED`) is simple. |
| Parser for Rust/cargo compiler errors | Already partially in `glass_errors`. Port and normalize into SOI's `OutputRecord` type. | LOW | Direct port from `glass_errors`. |
| One-line compressed summary per command | The headline feature. "3 errors, 247 passed" in 10 tokens vs 2000 raw. Industry standard for AI-optimized output (Pare does this, OpenAI structured outputs do this). | LOW | Compression logic already in `glass_compressed_context`. Adapt for per-command summaries. |
| Structured record storage in SQLite | Summaries are worthless if they don't persist and remain queryable. AI agent workflow requires lookup-by-command and lookup-by-severity. | MEDIUM | History DB already has schema migration path. Add `command_output_records` and `output_records` tables. |
| `glass_query` MCP tool | Agents need a single tool to query structured output. Without it, they have to re-parse raw captured output via `glass_history`. | MEDIUM | IPC channel and MCP server already exist. Add tool that queries new tables. |
| Auto-parse on `CommandFinished` | SOI is invisible machinery. Users and agents should never have to manually trigger parsing. | LOW | `CommandFinished` event already exists in `main.rs` event loop. Hook it. |
| Token-budget-aware drill-down | Agents have finite context windows. Summary at 10 tokens, detailed at 500 tokens, full at 1000+ — agent chooses what fits. | MEDIUM | `glass_compressed_context` has budget logic. Generalize it to per-command structured data. |

### Table Stakes — Agent Mode

Features that make Agent Mode credible as a "proactive development partner."
Missing these makes it a glorified notification system.

| Feature | Why Expected | Complexity | Existing Foundation |
|---------|--------------|------------|---------------------|
| Background agent process that watches terminal activity | The core promise. Without it, Agent Mode is just SOI + alerts. Copilot, Cursor, and Claude Code all ship background agents (2025 baseline). | HIGH | `glass_coordination` has agent registry. SOI provides the compressed activity stream it needs. |
| Worktree isolation for agent code changes | Industry consensus (2025): agents must NEVER touch the working tree directly. git worktrees are the standard pattern. Copilot, ccswarm, Claude Code all use worktrees. Non-negotiable for trust. | HIGH | Glass already uses `git` integration (git branch in status bar). No worktree management exists yet. |
| Proposal approval UI | Users must see what the agent wants to do BEFORE it happens. Standard pattern across all AI IDEs. Without this, no developer will trust or enable Agent Mode. | HIGH | Status bar, overlay rendering already exist. No proposal data model exists yet. |
| Agent activity feed to AI runtime | The agent can't act on things it doesn't know about. The feed turns SOI summaries into an agent-readable stream. | MEDIUM | SOI pipeline provides compressed events. Need a channel and subscription mechanism. |
| Configurable autonomy levels (Watch / Assist / Autonomous) | Developers have different risk tolerances. Copilot and Cursor both ship "auto-approve off by default" with escalating autonomy tiers. Single hardcoded behavior is a dealbreaker for cautious teams. | LOW | Config system (hot-reload) already exists. Add `[agent]` section. |
| Default opt-in to conservative mode | Copilot, Cursor, and Aider all default to requiring approval. Trust must be earned incrementally. Autonomous-by-default is a user trust killer. | LOW | Config default: `mode = "watch"`, `edit_files = "approve"`. |
| Session handoff on context exhaustion | Long-running agent sessions inevitably hit context limits. Without handoff, each new session starts blind. Industry consensus (2025): structured handoff documents are essential for continuity. | HIGH | `glass_history` DB can store handoff records. `glass_coordination` has session tracking. |

### Differentiators — SOI

Features that distinguish Glass SOI from existing tools like Pare or MCP wrappers.
Glass's advantage: it's IN the terminal loop, not wrapping CLI calls from outside.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Shell summary injection into terminal output | Claude Code's Bash tool captures the summary naturally in its output — no agent workflow change required. Agents using any MCP tool see SOI hints inline. Pare and MCP wrappers can't do this because they're outside the PTY. | MEDIUM | Write summary line to PTY after `CommandFinished`. OSC 133 boundary awareness required to avoid breaking shell integration. |
| Trend analysis across runs of same command | "test_login regressed 2 runs ago" — only possible because Glass has ALL historical command output in SQLite. External tools parse one invocation at a time. `glass_query_trend` is genuinely unique. | MEDIUM | History DB FTS5 already indexes commands. Query last-N runs of same command pattern, compare structured records. |
| Drill-down from summary to specific record | Hierarchical: one-liner → error list → full error context. Minimizes agent token usage while preserving access to full detail when needed. Pattern validated by OpenAI structured outputs and Pare's compact mode. | LOW | `glass_query_drill(record_id)` completes the loop started by `glass_query`. Record IDs returned in summaries. |
| Parsers for 10+ dev tools beyond errors | Cargo test, jest, pytest, go test, git, docker, kubectl, tsc, npm, generic JSON lines. The more parsers, the wider the value. Pare covers 222 CLI tools; Glass covers the subset humans actually run in a dev session. | HIGH | Each parser is independent. Prioritize by dev-tool ubiquity. Rust/npm/git first (aligned with target user). |
| SOI integrated with existing pipe stage data | Pipe stages already captured per-stage. SOI can parse each stage independently. No other terminal does per-stage structured output. | LOW | `pipe_stages` table already exists. Add SOI parsing per stage in Phase 7+. |

### Differentiators — Agent Mode

Features that make Glass Agent Mode better than running `claude --dangerously-skip-permissions` in a separate terminal.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Agent sees COMPRESSED terminal activity, not raw output | Raw output is 10-100x more tokens than SOI summaries. Agent with SOI summaries can watch 10x more commands per context window. No other tool provides pre-compressed, structured activity feeds to background agents. | LOW | SOI pipeline is already the compression layer. Activity stream subscribes to `SoiReady` events. |
| Agent uses SAME MCP tools as the human's AI assistant | `glass_query`, `glass_history`, `glass_snapshot`, `glass_pipes` — same 25+ tools. Agent and human assistant share context infrastructure. No duplication. | LOW | Agent is spawned with same MCP server config. No new MCP work needed for agent tool access. |
| Multi-agent coordination with existing `glass_coordination` | If user is also running Claude Code or another agent, the background Glass Agent coordinates via advisory locks. No conflicting edits. Unique to Glass because coordination infrastructure already exists. | LOW | Wire `glass_agent` through `glass_coordination` on session start. |
| Proposal from worktree means zero working-tree contamination until approval | User can review, reject, and retry without any uncommitted files in their working tree. Copilot creates draft PRs (requires GitHub); Glass does it locally with `git worktree`. | HIGH | `WorktreeManager` is new. `git worktree add/remove` cross-platform. Non-git fallback to temp dir. |

---

## Anti-Features

Features that seem good but create real problems. Explicitly out of scope.

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| Auto-apply agent code changes without approval | "Faster workflow" | Single bad suggestion trashes the working tree. Zero users will trust or enable Agent Mode after first bad experience. Copilot and Cursor explicitly default to approval-required after learning this lesson. | Keep `edit_files = "approve"` as default. Power users can flip to `"auto"`. |
| FTS5 full-text search on raw command output | "Find anything in terminal history" | Storage explosion — 50KB per command × 10,000 commands = 500MB index. Already deferred in PROJECT.md. | SOI structured records ARE the searchable layer. Search errors by file, messages by severity. Raw output FTS5 is future work. |
| Built-in AI chat in the terminal | "One tool for everything" | Explicitly out of scope (PROJECT.md). Glass exposes data TO AI assistants, it's not an AI. Shipping chat requires model access, key management, and UX that dilutes the terminal focus. | MCP server serves as the bridge. Users keep their preferred AI assistant. |
| Agent that runs continuously 24/7 | "Always watching" | API cost explodes. Users get proposal fatigue. Copilot agents are task-scoped, not perpetual. | Activity-driven polling with cooldown. Agent activates on events, not on a heartbeat. Cooldown default: 30 seconds. |
| Real-time streaming SOI parsing during command execution | "See structure as it emerges" | PTY output is streamed in small chunks. Parsers need full output to produce accurate summaries. Incremental parsing is error-prone and complex. | Parse on `CommandFinished` when full output is available. Latency is ~100ms — invisible to users. |
| Parser for every CLI tool | "100% coverage" | Long tail of tools has diminishing returns. Pare built 222 parsers as a dedicated product. Glass builds parsers for tools developers actually run daily. | Graceful `FreeformChunk` fallback for unrecognized output. Add parsers incrementally. Prioritize by command frequency in history DB. |
| Agent permission to push branches or open PRs | "Close the loop" | Cross-project repo hosting assumptions. GitHub-only feature. Requires OAuth. | Worktree diff + apply brings changes to local working tree. User runs `git push` manually. |
| Networked/cloud MCP transport | "Remote agent access" | Security surface expansion. PROJECT.md explicitly defers. | stdio MCP sufficient for local AI assistants. |

---

## Feature Dependencies

```
[SOI Output Classifier]
    └──requires──> [Format-Specific Parsers]
                       └──produces──> [ParsedOutput / OutputRecord types]
                                          └──requires──> [SOI Storage Schema (SQLite)]
                                                             └──enables──> [glass_query MCP tool]
                                                             └──enables──> [glass_query_trend MCP tool]
                                                             └──enables──> [glass_query_drill MCP tool]

[CommandFinished event] ──triggers──> [SOI Auto-Parse Pipeline]
    └──uses──> [OutputBuffer (existing, 50KB capture)]
    └──fires──> [SoiReady { command_id }] ──feeds──> [Activity Stream]

[SOI Compression Engine]
    └──requires──> [ParsedOutput types]
    └──enables──> [Shell Summary Injection]
    └──enables──> [Activity Stream token budgeting]

[Activity Stream]
    └──requires──> [SOI Compression Engine]
    └──requires──> [SoiReady events]
    └──feeds──> [Agent Runtime]

[Agent Runtime]
    └──requires──> [Activity Stream]
    └──requires──> [glass_query MCP tool] (for drill-down)
    └──produces──> [AgentProposal]
    └──requires──> [Worktree Isolation] (for CodeFix proposals)

[Worktree Isolation]
    └──independent of SOI (git operation, not output parsing)
    └──requires──> [AgentProposal data model]
    └──feeds──> [Approval UI]

[Approval UI]
    └──requires──> [AgentProposal data model]
    └──requires──> [Worktree diff output]
    └──uses──> [existing overlay rendering infrastructure]
    └──uses──> [existing status bar rendering]

[Session Continuity]
    └──requires──> [Agent Runtime] (session tracking)
    └──requires──> [SOI historical data] (for context reconstruction)
    └──uses──> [glass_history DB] (handoff storage)

[glass_errors (existing)]
    └──refactors-into──> [SOI Parser Registry] (backward-compatible)
    └──glass_extract_errors MCP tool delegates to SOI internally
```

### Dependency Notes

- **SOI must ship before Agent Mode:** Activity stream requires `SoiReady` events and `glass_query`. Agent Mode without SOI is a blind watcher.
- **Compression Engine before Shell Summary Injection:** Injection uses compression output. Both in same SOI crate.
- **Worktree Isolation before Proposal Approval UI:** Approval UI renders worktree diffs. Can stub worktree for basic UI, but full flow requires both.
- **Session Continuity last:** Requires runtime (to detect context exhaustion) and SOI history (for context reconstruction). Most complex, least critical for initial release.
- **`glass_errors` refactor is non-breaking:** Port parsers INTO SOI; `glass_extract_errors` becomes a thin delegate. Existing MCP tool behavior unchanged.

---

## MVP Definition

### Launch With (v3.0 core — SOI)

Minimum to deliver the "AI agents get structured, compressed, queryable intelligence" value.

- [ ] SOI classifier + parsers for cargo build, cargo test, npm install, pytest, jest — covers 80% of commands for target users
- [ ] SOI storage schema (two new SQLite tables, schema migration)
- [ ] SOI auto-parse on `CommandFinished` (non-blocking, Tokio spawn)
- [ ] Compression engine with OneLine / Summary / Detailed levels
- [ ] Shell summary injection (configurable, off by default to start)
- [ ] `glass_query` MCP tool (query by command_id, scope, budget)
- [ ] `glass_query_trend` MCP tool (compare last-N runs of same command)
- [ ] `glass_query_drill` MCP tool (expand specific record)

### Launch With (v3.0 core — Agent Mode)

Minimum to deliver "proactive development partner" value.

- [ ] Activity stream (subscribes to `SoiReady`, budget-constrained rolling window)
- [ ] Agent runtime (background Claude CLI process, system prompt, proposal output protocol)
- [ ] Worktree isolation (create/diff/apply/cleanup via `git worktree`)
- [ ] Approval UI (status bar indicator, toast notifications, review overlay with diff)
- [ ] Configuration (`[agent]` section, autonomy levels, permission matrix)
- [ ] Agent mode integrated with `glass_coordination` (register on start, deregister on stop)

### Add After Validation (v3.x)

- [ ] Git, Docker, kubectl, tsc, Go parsers — expand SOI coverage to devops/infrastructure tools
- [ ] Session continuity / handoff across context resets
- [ ] Generic JSON lines parser (structured logging, NDJSON)
- [ ] SOI per-stage parsing for pipe stages
- [ ] Agent quiet rules (ignore patterns, exit-0 filtering)
- [ ] `glass agent status` CLI subcommand

### Future Consideration (v4+)

- [ ] SOI parser plugin system (user-defined parsers)
- [ ] SOI trend anomaly detection (e.g., build time increasing over last 10 runs)
- [ ] Agent Mode multi-model routing (Haiku for watch, Sonnet for autonomous)
- [ ] Agent PR/branch creation (requires GitHub integration)
- [ ] FTS5 on structured record messages (after storage impact measured)

---

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| SOI classifier + core parsers (cargo, npm, pytest) | HIGH | MEDIUM | P1 |
| SOI storage schema + auto-parse pipeline | HIGH | LOW | P1 |
| `glass_query` MCP tool | HIGH | MEDIUM | P1 |
| Compression engine | HIGH | MEDIUM | P1 |
| Shell summary injection | HIGH | MEDIUM | P1 |
| `glass_query_trend` + `glass_query_drill` | HIGH | LOW | P1 |
| Activity stream | HIGH | MEDIUM | P1 |
| Agent runtime (background Claude CLI) | HIGH | HIGH | P1 |
| Worktree isolation | HIGH | HIGH | P1 |
| Approval UI (status bar + toast + overlay) | HIGH | HIGH | P1 |
| Agent configuration + permissions | MEDIUM | LOW | P1 |
| Git, Docker, kubectl, tsc parsers | MEDIUM | MEDIUM | P2 |
| Session continuity / handoff | MEDIUM | HIGH | P2 |
| Generic JSON lines parser | MEDIUM | LOW | P2 |
| SOI per-stage pipe parsing | LOW | LOW | P3 |
| Agent CLI status command | LOW | LOW | P3 |

**Priority key:**
- P1: Required for v3.0 milestone completion
- P2: Adds significant value, deliver in v3.x patch releases
- P3: Nice to have, defer to v4.0

---

## Competitor / Ecosystem Comparison

| Feature | Pare (MCP wrappers) | GitHub Copilot Agent | Aider | Glass v3.0 Approach |
|---------|---------------------|---------------------|-------|---------------------|
| Structured CLI output | 222 CLI tools, MCP-only, external to PTY | IDE-native, not terminal | None — raw git/test output | In-PTY parsing, 10+ tools, shell summary injected into output stream |
| Token compression | Compact mode auto-switch | None exposed | None | 4-level compression engine, budget-aware, per-command |
| Historical trend analysis | None — single-invocation | None | None | `glass_query_trend` across full command history DB |
| Background agent watching | None | Yes (GitHub Actions) | No | Yes — background Claude CLI process fed SOI activity stream |
| Worktree isolation | N/A | GitHub sandbox (remote) | Optional `--dirty` | Local `git worktree`, non-git fallback to temp dir |
| Approval UI | N/A | PR review (GitHub) | Chat-based confirmation | Status bar + toast + review overlay (keyboard-driven) |
| Context continuity | N/A | PR-scoped | Session-scoped | Cross-session handoff stored in SQLite |
| Agent coordination | N/A | None | None | `glass_coordination` advisory locks |
| Works offline | Yes | No (requires GitHub) | Yes | Yes (local SQLite, local git) |

---

## Sources

- [Pare: Structured Output for AI Coding Agents](https://dev.to/dave_london_d0728737f5d67/structured-output-for-ai-coding-agents-why-i-built-pare-2k5f) — validates SOI approach; Glass's advantage is in-PTY injection
- [Pare GitHub](https://github.com/Dave-London/Pare) — 222 tools, Zod schemas, compact-mode auto-switch pattern
- [GitHub Copilot Agent Mode 2025](https://redmonk.com/kholterhoff/2025/12/22/10-things-developers-want-from-their-agentic-ides-in-2025/) — approval-gate pattern, autonomy spectrum
- [Git Worktrees for AI Agents — Nick Mitchinson](https://www.nrmitchi.com/2025/10/using-git-worktrees-for-multi-feature-development-with-ai-agents/) — worktree isolation as industry standard
- [ccswarm: Multi-agent worktree isolation](https://github.com/nwiizo/ccswarm) — parallel agent + worktree pattern in Rust
- [AI Agent Handoff — xtrace.ai](https://xtrace.ai/blog/ai-agent-handoff-why-context-gets-lost-between-agents-and-how-to-fix-it) — structured handoff beats summarization
- [Session Handoff Protocol — Blake Link](https://blakelink.us/posts/session-handoff-protocol-solving-ai-agent-continuity-in-complex-projects/) — handoff document pattern; "30/90 tests passing" specificity requirement
- [Context Compression for AI Agents — Medium](https://medium.com/ai-artistry/context-compression-for-ai-agents-why-structured-memory-beats-aggressive-truncation-0b03596caa5b) — structured memory beats truncation
- [Agentic IDE expectations 2025 — Redmonk](https://redmonk.com/kholterhoff/2025/12/22/10-things-developers-want-from-their-agentic-ides-in-2025/) — approval gates, fine-grained permissions, audit trails
- [Git Worktrees Parallel Agents — Upsun](https://devcenter.upsun.com/posts/git-worktrees-for-parallel-ai-coding-agents/) — shared DB/port conflict pitfalls

---

*Feature research for: Glass v3.0 SOI + Agent Mode*
*Researched: 2026-03-12*
