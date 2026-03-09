# Feature Landscape: Multi-Agent Coordination

**Domain:** Agent orchestration layer for a terminal emulator — enabling multiple AI coding agents (Claude Code, Cursor, Copilot, etc.) to register, coordinate, claim files, and communicate through shared infrastructure
**Researched:** 2026-03-09
**Confidence:** MEDIUM-HIGH (Claude Code Agent Teams, Warp 2.0, Overstory, and mcp_agent_mail provide strong prior art; core patterns are converging but the domain is still young)

## Table Stakes

Features that are baseline expectations for any agent coordination system in 2026. Without these, agents running in parallel will produce conflicts and wasted work. These map directly to the problems the design document identifies.

| Feature | Why Expected | Complexity | Depends On | Notes |
|---------|--------------|------------|------------|-------|
| Agent registration and discovery | Agents must know about each other to coordinate. Claude Code Agent Teams, Overstory, and mcp_agent_mail all implement agent registries as their foundation. Without this, no other coordination feature works. | LOW | New `glass_coordination` crate | UUID-based identity. Design doc covers this well. SQLite schema is straightforward. |
| Advisory file locking | The single most valuable coordination primitive. Prevents two agents from editing the same file simultaneously. Every multi-agent system implements this: Claude Code uses task-claiming with file locking, Overstory uses git worktrees for physical isolation, mcp_agent_mail uses "file reservations/leases." Glass's advisory lock approach matches mcp_agent_mail's pattern. | MEDIUM | Agent registry | Atomic all-or-nothing lock acquisition eliminates TOCTOU. Path canonicalization critical for cross-platform correctness. |
| Heartbeat-based liveness detection | Agents crash, users close tabs, processes get killed. Without stale detection, ghost locks block real agents indefinitely. Every coordination system implements this. The AgentHeartbeat pattern recommends 60-90s intervals with 5min timeout, matching the design doc. | LOW | Agent registry | PID fallback is important: if process is dead, prune immediately without waiting for timeout. Design doc already includes this. |
| Inter-agent messaging (broadcast + directed) | Agents need to communicate: "I'm done with X", "please unlock Y", "starting work on Z." Claude Code Agent Teams uses a mailbox system. Overstory uses a SQLite mail system. mcp_agent_mail has inbox/outbox with threading. This is expected infrastructure. | MEDIUM | Agent registry | Design doc's structured message types (info, conflict_warning, task_complete, request_unlock) are well-chosen. Read-once semantics are appropriate for coordination signals. |
| MCP tool exposure | The interface between agents and the coordination layer. Agents interact with Glass exclusively through MCP tools. The existing 5 tools (GlassHistory, GlassContext, GlassUndo, GlassFileDiff, GlassPipeInspect) prove the pattern works. | MEDIUM | glass_coordination crate, glass_mcp crate | 11 new tools per design doc. Follows existing rmcp tool_router pattern. spawn_blocking wrapping for synchronous SQLite ops. |
| Project scoping | Agents on unrelated projects must not interfere. Lock visibility and agent listing should be scoped by project root. Overstory scopes by repository. mcp_agent_mail scopes by project directory. | LOW | Agent registry | Design doc's `project` field on agent registration handles this. |
| Lock conflict reporting | When a lock attempt fails, the agent must know WHO holds the lock and WHY, so it can decide what to do (wait, ask, work on something else). Every system returns conflict details. | LOW | File locking | Design doc returns `{ path, held_by, reason }` on conflict. This is the right shape. |
| CLAUDE.md integration instructions | Claude Code reads CLAUDE.md for project-specific behavior. Adding coordination instructions there is how agents learn to self-coordinate. This is the "glue" that makes the system work without human intervention. | LOW | MCP tools working | Design doc has a good draft. Must be tested with real Claude Code sessions. |

## Differentiators

Features that go beyond what other tools offer. Not expected, but these would make Glass uniquely valuable as an agent orchestration layer.

| Feature | Value Proposition | Complexity | Depends On | Notes |
|---------|-------------------|------------|------------|-------|
| GUI agent status in status bar | No other terminal emulator shows agent coordination state in its GUI. Warp 2.0 has agent status icons per tab, but Glass would be the first to show cross-tab agent awareness in the status bar. Developers get at-a-glance visibility into how many agents are active, who holds locks, and what each is doing. | MEDIUM | Agent registry, StatusBarRenderer | Extends existing status bar (left: CWD, right: git info) with agent count/status. Pattern: `[2 agents] [3 locks]` or similar compact indicator. Requires polling agents.db periodically from the GUI process. |
| Tab-level agent indicators | Each tab shows whether its session has an active agent, what it's working on, and whether it holds file locks. Warp 2.0 does this with per-tab status icons (in-progress, completed, error, idle). Glass can match and exceed this by showing lock counts and task descriptions. | MEDIUM | Agent registry, TabBarRenderer | Extend TabDisplayInfo with optional agent badge. Requires mapping SessionId to agent_id (currently separate per design doc — may need a bridge). |
| Conflict warning overlay | When two agents attempt to touch the same file, Glass shows a visual warning overlay before damage is done. No other tool does this proactively in the terminal UI. Conflict resolution today is reactive (merge conflicts after the fact). | HIGH | File locking, FrameRenderer | Requires real-time monitoring: the GUI process would need to watch agents.db for conflict events and trigger overlays. Complex because the overlay system needs new event types. Defer to Phase 4 per design doc. |
| Agent status broadcasting via task description | Agents set their current task ("refactoring auth module", "running tests"), visible to other agents and the GUI. This goes beyond simple liveness — it's activity awareness. Claude Code Agent Teams has this via task lists. | LOW | Agent registry | Design doc's `set_status(agent_id, status, task)` handles this. The `task` field is free-text, which is flexible. |
| Structured message types for programmatic triage | Messages carry a `msg_type` field (info, conflict_warning, task_complete, request_unlock) so agents can programmatically decide what to do without parsing natural language. mcp_agent_mail uses importance levels but not structured types. This is a practical improvement. | LOW | Messaging | Already in design doc. The four types cover the most common coordination signals. |
| Automatic stale agent cleanup with lock cascade | When an agent is detected as stale (heartbeat timeout or dead PID), its locks are automatically released and other agents are unblocked. This is self-healing coordination. Most systems require manual cleanup or restart. | LOW | Liveness detection, file locking | Design doc handles this via `ON DELETE CASCADE` in SQLite schema and `prune_stale()` auto-called on list operations. Elegant approach. |
| Atomic multi-file lock acquisition | Lock multiple files in a single atomic operation — all succeed or all fail with conflict details. Prevents partial-lock deadlocks. Most systems offer per-file locking only. | LOW | File locking | Design doc specifies this. SQLite transaction provides atomicity. This is a genuine advantage over check-then-lock patterns. |

## Anti-Features

Features to explicitly NOT build. These are tempting but would add complexity without proportionate value, or would conflict with Glass's design philosophy.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| Enforced file locking (blocking writes) | Would require intercepting file writes at the PTY level or OS level. Fragile, platform-specific, and breaks normal terminal behavior. AI agents follow instructions — advisory locks work because the agents read CLAUDE.md and cooperate. Overstory explicitly warns that "agent swarms are not a universal solution" and enforcement adds more problems than it solves. | Advisory locks with clear CLAUDE.md instructions. Agents that ignore locks get conflicts reported to them. |
| Full A2A protocol support | Google's Agent2Agent protocol (150+ orgs, HTTP/SSE/JSON-RPC) is designed for enterprise agent interoperability across vendors. Way too heavy for local terminal coordination. Glass agents communicate through shared SQLite, which is simpler, faster, and requires zero network stack. | SQLite WAL for local coordination. A2A is for cloud-scale agent ecosystems, not local dev tools. |
| Durable message queue (audit log) | Messages are coordination signals, not audit trails. Making them durable adds storage management complexity (retention policies, indexing, querying) without clear value. mcp_agent_mail stores messages in git for audit, but Glass's coordination messages are ephemeral by nature. | Read-once semantics with message preservation for unread recipients. Pruned when recipient is pruned. |
| Agent permission system / RBAC | Tempting to add "which agent can lock which files" rules, but this adds a configuration surface with no clear benefit. AI agents should coordinate, not compete. Permission enforcement belongs at the OS/git level, not in the coordination layer. | All agents in the same project are peers. Trust is established by being in the same CLAUDE.md scope. |
| Built-in conflict resolution / auto-merge | When two agents edit the same file despite locks, attempting to auto-merge is the IDE's job (or git's). Glass should detect and report conflicts, not resolve them. Overstory's 4-tier merge resolution is interesting but complex — and still requires human review for semantic conflicts. | Report conflicts clearly. Let agents and humans resolve them using existing tools (git merge, manual review). |
| Network-based agent discovery | Agents running on different machines discovering each other. Out of scope per PROJECT.md ("MCP over network transport — stdio sufficient for local AI; network adds security concerns"). | All agents share a filesystem. SQLite WAL requires same-host access. This is a feature, not a limitation. |
| Real-time WebSocket notifications | Push notifications when locks change or messages arrive. Would require a long-lived connection between MCP server processes. SQLite polling on `read_messages` is simpler and sufficient for 60-second heartbeat intervals. | Agents poll via `glass_agent_messages` on their heartbeat cycle. Latency of 0-60s is acceptable for coordination signals. |
| Thread-based message conversations | mcp_agent_mail supports threading with `thread_id`. For Glass's coordination scope, flat messages with `msg_type` are sufficient. Threading adds complexity for a feature that's mostly used for lock negotiation. | Flat messages with structured types. An agent that needs to discuss can send multiple messages. |
| Git worktree isolation | Overstory gives each agent its own git worktree to physically prevent file conflicts. This is the nuclear option — it works but requires a complex merge queue with tiered conflict resolution. Glass's advisory locks are much lighter weight. | Advisory locks. If users want physical isolation, they can use git worktrees manually alongside Glass coordination. |

## Feature Dependencies

```
Agent Registry (register, deregister, heartbeat, list, status)
    |
    +---> File Locking (lock, unlock, list_locks)
    |         |
    |         +---> Lock Conflict Reporting (returned by lock attempt)
    |         |
    |         +---> Tab Agent Indicators (GUI reads lock count per agent)
    |         |
    |         +---> Conflict Warning Overlay (GUI watches for conflicts)
    |
    +---> Inter-Agent Messaging (broadcast, send, read_messages)
    |         |
    |         +---> Structured Message Types (info, conflict_warning, task_complete, request_unlock)
    |
    +---> Liveness Detection (heartbeat timeout + PID check)
    |         |
    |         +---> Stale Agent Cleanup (cascade lock release)
    |
    +---> Status Bar Agent Indicator (GUI reads agent count/status)
    |
    +---> CLAUDE.md Instructions (references all MCP tools)

MCP Tool Exposure (wraps all coordination API methods)
    |
    +---> Depends on glass_coordination crate being complete
    +---> Follows existing rmcp tool_router pattern from glass_mcp
```

Key ordering constraint: the coordination crate (pure library) must be built and tested before MCP tools can wrap it. GUI integration can happen independently once the DB schema is stable, since the GUI process reads agents.db directly.

## MVP Recommendation

### Must-have for v2.2 (this milestone)

Prioritize in this order:

1. **Agent registry with heartbeat/liveness** — Foundation for everything. Without this, no coordination is possible. (Table stakes)
2. **Advisory file locking with atomic acquisition** — The highest-value coordination primitive. This is why agents need Glass. (Table stakes)
3. **Inter-agent messaging** — Enables lock negotiation ("please unlock X") and task announcements. (Table stakes)
4. **11 MCP tools** — The interface. Agents can't use coordination without MCP exposure. (Table stakes)
5. **CLAUDE.md integration instructions** — The "glue" that makes it all work automatically. (Table stakes)
6. **Status bar agent indicator** — At-a-glance awareness. Low complexity, high visibility. (Differentiator, but low-hanging fruit)

### Should-have for v2.2

7. **Tab-level agent indicators** — Visual enhancement showing which tabs have active agents. Moderate complexity. (Differentiator)
8. **Agent status task descriptions** — Agents announce what they're working on. Trivially extends registry. (Differentiator)

### Defer to future milestone

9. **Conflict warning overlay** — HIGH complexity, requires new overlay event types and real-time DB monitoring. The design doc correctly defers this to Phase 4. (Differentiator)

### What NOT to build

Everything in the Anti-Features table. Especially resist the temptation to enforce locks, add network transport, or build conflict resolution.

## Competitive Landscape Summary

| Tool | Agent Coordination Approach | File Conflict Strategy | GUI Awareness |
|------|---------------------------|----------------------|---------------|
| **Claude Code Agent Teams** | Shared task list, TeammateTool, mailbox messaging | Task claiming with file locking; "avoid same-file edits" guidance | In-process teammate cycling or tmux split panes |
| **Warp 2.0** | Agent Management Panel, per-tab agent status | No explicit file locking; relies on agent isolation | Status icons per tab (in-progress, completed, error, idle), notification dots, toast alerts |
| **Overstory** | Git worktrees + SQLite mail system | Physical isolation via worktrees; 4-tier merge queue for conflicts | TUI-based (tmux), no rich GUI |
| **mcp_agent_mail** | MCP-exposed inbox/outbox, agent identities | Advisory file reservations/leases with TTLs | Web UI at /mail for humans |
| **gptme (Bob)** | File leases, message bus, work claiming | File lease reservations | Terminal-based, no rich GUI |
| **Glass (proposed)** | Shared SQLite DB, MCP tools, CLAUDE.md instructions | Atomic advisory locks with path canonicalization | GPU-rendered status bar + tab indicators (unique) |

Glass's unique position: it is the only tool that combines a GPU-rendered terminal GUI with an agent coordination layer. Claude Code Agent Teams coordinates agents but has no GUI awareness. Warp 2.0 has GUI indicators but ties them to its own agent runtime, not to arbitrary MCP-connected agents. Glass's approach of exposing coordination via MCP and displaying state via GPU rendering is architecturally distinct.

## Complexity Assessment by Phase

| Phase (per design doc) | Features | Estimated Complexity | Risk |
|------------------------|----------|---------------------|------|
| Phase 1: Coordination Crate | Agent registry, file locks, messaging, liveness, SQLite schema | MEDIUM — pure library, no async, well-scoped SQLite operations. Main risk: path canonicalization edge cases on Windows (UNC paths, junction points). | LOW |
| Phase 2: MCP Tools | 11 new tool handlers, CoordinationDb wiring | MEDIUM — follows established rmcp patterns from existing 5 tools. Main risk: spawn_blocking ergonomics for synchronous DB access. | LOW |
| Phase 3: Integration & Testing | CLAUDE.md instructions, integration tests, manual multi-agent testing | LOW — mostly documentation and testing. Main risk: verifying that real Claude Code sessions actually follow the CLAUDE.md instructions reliably. | MEDIUM (behavioral, not technical) |
| Phase 4: GUI Integration | Status bar, tab indicators, conflict overlay | MEDIUM-HIGH — status bar and tab indicators are extensions of existing renderers. Conflict overlay is new overlay type requiring event plumbing from DB to renderer. | MEDIUM |

## Sources

- [Claude Code Agent Teams documentation](https://code.claude.com/docs/en/agent-teams) — official Anthropic docs on multi-agent coordination, task lists, mailbox messaging, teammate management. HIGH confidence.
- [Warp 2.0 Agentic Development Environment](https://www.warp.dev/blog/reimagining-coding-agentic-development-environment) — Warp's agent status indicators, management panel, notification system. MEDIUM confidence (feature descriptions from marketing + docs).
- [Warp Agent Management](https://docs.warp.dev/agents/using-agents/managing-agents) — per-tab status icons, agent management panel, notification dots and toasts. MEDIUM confidence.
- [Overstory multi-agent orchestration](https://github.com/jayminwest/overstory) — git worktree isolation, SQLite mail system, 4-tier merge queue. MEDIUM confidence.
- [mcp_agent_mail](https://github.com/Dicklesworthstone/mcp_agent_mail) — MCP-exposed agent mail with file reservations/leases, SQLite+FTS5, advisory locking. MEDIUM confidence.
- [Google A2A Protocol](https://developers.googleblog.com/en/a2a-a-new-era-of-agent-interoperability/) — agent-to-agent protocol specification, 150+ orgs. HIGH confidence (for understanding what NOT to build at Glass's scale).
- [Building a C compiler with parallel Claudes](https://www.anthropic.com/engineering/building-c-compiler) — real-world 16-agent coordination stress test, 100K-line output. HIGH confidence.
- [SQLite WAL mode documentation](https://sqlite.org/wal.html) — concurrent reader/writer semantics, same-host requirement, checkpoint behavior. HIGH confidence (official SQLite docs).
- [The Heartbeat Pattern for AI Agents](https://dev.to/askpatrick/the-heartbeat-pattern-how-to-keep-ai-agents-alive-between-tasks-2b0p) — heartbeat intervals, silent-by-default principle, stale detection. LOW confidence (blog post, not authoritative).
- [Kestra liveness and heartbeat mechanism](https://kestra.io/blogs/2024-04-22-liveness-heartbeat) — liveness coordinator pattern, timeout-based state transitions. MEDIUM confidence.
