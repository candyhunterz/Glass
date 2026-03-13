# Agent Mode

Agent Mode turns Glass into a proactive development partner by running a background Claude CLI process that watches terminal activity and proposes code fixes. All proposals are isolated in git worktrees and require explicit user approval before any file in the working tree is modified.

---

## Overview

When Agent Mode is enabled, Glass monitors SOI activity events produced by completed commands and forwards compressed summaries to a background Claude CLI process. When the agent determines a fix is warranted, it makes changes inside an isolated git worktree. Glass displays a proposal notification and waits for the user to accept or dismiss it. The working tree is never touched without approval.

---

## Autonomy Modes

Three autonomy levels control when the agent reacts:

| Mode | Behavior |
|---|---|
| `watch` | Reacts only to critical failures (error-severity SOI events) |
| `assist` | Suggests fixes for non-critical events as well (warnings, test failures) |
| `autonomous` | Proposes fixes proactively without waiting for failures |

The active mode is shown in the status bar (e.g., `[agent: watch]`).

---

## Activity Stream

SOI summaries are forwarded to the agent through a bounded channel. Three mechanisms prevent noise and excessive API usage:

- **Deduplication** — Repetitive success events for the same command are collapsed so the agent does not see a stream of identical passing test results.
- **Rate limiting** — Rapid successive command completions are batched rather than forwarded individually.
- **Rolling budget window** — A configurable token limit (default 4096 tokens) governs how much context is sent per window. Older events are dropped when the window fills.

---

## Agent Runtime

The background process is a standard Claude CLI invocation. Glass communicates with it over stdio using a JSON lines protocol: activity events on stdin, proposals on stdout.

Key runtime properties:

- **Platform-safe lifecycle** — On Windows, the child process is assigned to a Job Object so it cannot outlive the Glass process. On Unix, `prctl(PR_SET_PDEATHSIG)` achieves the same effect. Orphaned agent processes are not possible under normal operation.
- **Cooldown timer** — A configurable delay (default 30 seconds) enforces a minimum gap between consecutive proposals, preventing the agent from flooding the review queue.
- **Budget cap** — API spend is tracked per session. When the cap is reached (default $1.00), the agent suspends until the next session. Current spend is shown in the status bar.

---

## Worktree Isolation

Agent changes land in git worktrees registered under `~/.glass/worktrees/`, never directly in the working tree.

**Lifecycle of a proposal:**

1. Agent produces a set of file edits.
2. Glass creates a git worktree at `~/.glass/worktrees/<id>` and applies the edits there.
3. The proposal is registered in the `pending_worktrees` SQLite table.
4. A toast notification appears prompting the user to review.
5. The user opens the review overlay (Ctrl+Shift+A) and inspects the unified diff.
6. **Accept** — Changed files are copied from the worktree to the working tree. The worktree is removed.
7. **Dismiss** — The worktree is removed without touching the working tree.

**Crash recovery** — On startup, Glass prunes any `pending_worktrees` rows whose worktrees still exist on disk, preventing stale entries from accumulating.

**Non-git fallback** — When the project directory is not a git repository, Glass uses a temporary directory copy instead of a worktree. The same accept/dismiss flow applies.

---

## Approval UI

The terminal remains fully interactive while proposals are pending. Agent Mode UI is non-blocking by design.

**Status bar** — Displays the active autonomy mode and the count of pending proposals, for example: `[agent: watch] 2 pending`.

**Toast notification** — When a new proposal arrives, a toast appears at the bottom of the window. It auto-dismisses after 30 seconds and shows the keyboard shortcut for opening the review overlay.

**Review overlay** — Ctrl+Shift+A opens a scrollable list of pending proposals. Each entry shows the agent's rationale, the list of affected files, and a unified diff preview. Keyboard shortcuts accept or reject individual proposals without leaving the overlay.

---

## Session Continuity

Agent context is preserved across Glass sessions:

1. When a session ends, the agent produces a structured handoff summary covering open issues, proposed fixes, and their outcomes.
2. The summary is stored in the `agent_sessions` SQLite table alongside the session timestamp and project root.
3. When a new session starts with Agent Mode enabled, Glass loads the most recent handoff as the agent's initial context.
4. Successive sessions form a chain. Older entries are compacted to stay within the rolling context budget.

---

## Configuration

```toml
[agent]
enabled = false          # Enable Agent Mode (default: false)
mode = "watch"           # watch, assist, or autonomous
max_budget_usd = 1.0     # Maximum API spend per session
cooldown_secs = 30       # Minimum seconds between proposals

[agent.permissions]
edit_files = "approve"   # approve, auto, or never
run_commands = "never"   # approve, auto, or never
git_operations = "never" # approve, auto, or never

[agent.quiet_rules]
ignore_patterns = []     # Command name patterns to suppress (glob syntax)
ignore_exit_zero = false # Suppress all events for commands that exit 0
```

`edit_files = "auto"` allows the agent to accept its own proposals without user review. This is not recommended unless the project has comprehensive test coverage and a clean git working tree.

---

## Requirements

Claude CLI must be installed and available on `PATH`. If Glass cannot locate it at startup, Agent Mode disables gracefully and logs a hint:

```
agent mode disabled: claude CLI not found — install from https://claude.ai/download or add to PATH
```

No other configuration is required. Glass manages the subprocess lifecycle, worktree cleanup, and session storage automatically.
