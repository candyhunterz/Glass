# AI Integration

Glass is a universal enhancer for AI coding tools. Any AI CLI launched inside Glass — Claude Code, Codex, Aider, Cursor, Gemini — automatically gets capabilities it wouldn't have in a regular terminal.

---

## Persistent Ground Truth

Every command, its output, exit code, duration, and working directory is recorded in a queryable SQLite database with full-text search. AI agents can look up what actually happened — across sessions, across context resets, across different AI tools.

The model doesn't remember; it looks things up. That's more reliable than memory.

For example, an agent can ask:
- "What command did I use to fix that Docker issue last week?" — FTS5 search finds it
- "What was the output of the last `cargo test` run?" — exact output, not a hallucinated guess
- "I tried something similar before and it broke, what happened?" — full command + exit code + output

This works across sessions, across AI tools, and across context resets. Claude Code gets `/clear`ed, Cursor starts a new session, a different model picks up — they all query the same history DB and get the same ground truth.

See [History](./history.md) for details on the history system.

---

## Structured Understanding

Glass doesn't just store raw text. 19 format-specific parsers (SOI) extract test counts, compiler errors, container states, and more into structured records. When an AI asks "what failed?", it gets parsed data, not 500 lines of scrollback.

See [Structured Output Intelligence](./soi.md) for the full list of supported formats.

---

## Safety Net

Every command gets a pre-execution filesystem snapshot. AI agents make destructive mistakes — Glass catches them. Undo is one MCP call away, regardless of which AI tool made the change.

See [Undo](./undo.md) for details on the snapshot system.

---

## Zero Setup

Glass auto-registers its MCP server with installed AI tools on first launch. No manual configuration needed — open Glass, start your AI tool, and it already has access to history, context, undo, and 30 other tools.

### How it works

Glass exposes its capabilities via [MCP (Model Context Protocol)](https://modelcontextprotocol.io/). AI tools connect to `glass mcp serve` and gain access to 33 tools spanning history, context, undo, diffs, pipe inspection, and more. See [MCP Server](../mcp-server.md) for the full tool list.

### Auto-registration

On first launch, Glass detects installed AI tools and writes its MCP server entry into their configuration:

| Tool | Config written |
|------|---------------|
| Claude Code | `~/.claude/settings.local.json` |
| Cursor | `~/.cursor/mcp.json` |
| Windsurf | `~/.codeium/windsurf/mcp_config.json` |
| Any MCP-aware tool | `.mcp.json` in project root |

Auto-registration is:
- **Non-destructive** — only adds the Glass entry, never modifies existing MCP server configs
- **Idempotent** — running multiple times produces the same result
- **Automatic** — happens at Glass startup and when the orchestrator activates

To verify registration status, run `glass check` — it reports which tools were detected and registered.

To register manually, add `glass mcp serve` as a stdio MCP server in your tool's configuration.
