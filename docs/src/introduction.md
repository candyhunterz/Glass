# Introduction

Glass is a GPU-accelerated terminal emulator built in Rust. It looks and behaves like a normal terminal, but it understands command structure -- tracking boundaries, classifying output, snapshotting affected files, and indexing everything into a queryable store.

This makes Glass useful to two distinct audiences at the same time. Humans see rendered output in a structured, navigable interface. AI agents get structured, compressed, queryable intelligence -- not raw scrollback.

---

## For Humans

Glass adds durable structure to the command-line workflow without changing how you use the terminal.

- **Command blocks** -- each command occupies a discrete block with exit code, duration, and working directory. Blocks can be collapsed, individually searched, and navigated by keyboard.
- **Undo** -- before executing a destructive command (`rm`, `mv`, `sed -i`, and others), Glass snapshots affected files into a content-addressed store. A single command restores them.
- **Pipe inspection** -- multi-stage pipelines are captured per stage, letting you inspect intermediate data without re-running the pipeline.
- **Tabs and panes** -- a binary split tree supports arbitrary horizontal and vertical splits within a session, alongside a full tab bar.
- **Search** -- an overlay search across current session output and persistent FTS5-indexed history spanning all past sessions.
- **Scrollbar** -- a context-aware scrollbar with block markers for quick orientation in long sessions.

---

## For AI Agents

Glass exposes 31 MCP tools over an embedded MCP server, providing structured access to everything the terminal has seen.

- **Structured Output Intelligence (SOI)** -- after a command completes, Glass classifies its output (error, warning, success, structured data, and others) and stores a structured representation alongside the raw text. Agents receive pre-classified output rather than raw terminal scrollback.
- **31 MCP tools** -- covering history queries, context retrieval, undo, file diffs, pipe inspection, snapshot access, and multi-agent coordination. The MCP server is embedded in the Glass process; no separate setup is required.
- **Multi-agent coordination** -- a shared SQLite database in WAL mode provides an agent registry, advisory file locks, and inter-agent messaging. Multiple agents working on the same project can coordinate without race conditions or conflicting edits.
- **Token-efficient tools** -- compressed context summaries, diff-only file representations, and per-block output access keep MCP call payloads small.

---

## Agent Mode

Agent Mode runs a Claude CLI process in the background, isolated to a Git worktree, and surfaces its proposed changes through an approval UI inside the terminal. Changes are visible before they are applied. The approval overlay is accessible at any time with `Ctrl+Shift+A`.

Agent Mode is designed for tasks that are too large or too risky to execute without review -- refactors, multi-file changes, or exploratory edits -- while keeping the human in control at whatever granularity they choose.

---

## Core Design

Glass passively watches, indexes, and snapshots everything. It does not require configuration to provide value and does not change how you interact with the shell. Structure is captured as a side effect of normal terminal use and surfaced only when needed -- as a decoration on a block, a diff on hover, or a structured query result from an MCP call.

---

## Next Steps

If you are new to Glass, start with [Getting Started](./getting-started.md) to install Glass, verify shell integration, and run your first commands.

If you are integrating an AI agent, see [MCP Server](./mcp-server.md) for the full tool reference, or [Multi-Agent Coordination](./agent-coordination.md) for the agent registry and locking protocol.
