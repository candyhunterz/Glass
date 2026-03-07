# Introduction

Glass is a GPU-accelerated terminal emulator that treats your terminal output as structured data, not just text. It looks and feels like a normal terminal but passively watches, indexes, and snapshots everything -- surfacing intelligence only when you need it.

## What makes Glass different

Traditional terminals display a flat stream of characters. Glass understands command boundaries: every command you run is rendered as a distinct **block** with its exit code, duration, and working directory. Your scrollback becomes a structured, searchable history rather than an anonymous wall of text.

## Core capabilities

- **Command blocks** -- Each command appears as a visual block showing exit status (green checkmark or red X), execution duration, and working directory.
- **Structured scrollback** -- Navigate command-by-command instead of line-by-line. Every block is individually addressable.
- **File undo** -- Glass snapshots files before commands modify them. Press Ctrl+Shift+Z to restore a file to its pre-command state.
- **Pipe inspection** -- View intermediate output at each stage of a pipeline. Failed pipelines auto-expand to show where things went wrong.
- **Full-text search** -- Ctrl+Shift+F searches across your entire command history with SQLite FTS5, persisting across sessions.
- **Tabs and panes** -- Split your workspace with tabs (Ctrl+Shift+T) and panes (Ctrl+Shift+D), each running its own shell session.
- **MCP server** -- Expose your terminal history and undo capabilities to AI assistants via the Model Context Protocol.

## Next steps

Head to [Getting Started](./getting-started.md) to install Glass and learn the basics.
