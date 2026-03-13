# Getting Started

This page covers first launch, shell integration, command blocks, Structured Output Intelligence, key shortcuts, and where to find configuration.

---

## First Launch

Glass requires no configuration to start. On first launch it auto-detects the default shell from the environment (`$SHELL` on Unix, the user profile default on Windows) and opens a single tab with that shell. Everything works immediately.

If you want to override the shell or set startup options, see [Configuration](./configuration.md). Configuration is optional and hot-reloaded -- you do not need to restart Glass after editing it.

---

## Shell Integration

Glass injects a small integration script into the shell at startup. The script emits OSC 133 sequences that mark command boundaries:

- Prompt start
- Command start (after the user begins typing)
- Command executed (when Enter is pressed)
- Command finished (with exit code)

Shell integration is supported on **bash**, **zsh**, **fish**, and **PowerShell**. No manual installation is required; Glass injects the appropriate script automatically when the shell starts.

You can verify integration is active by running any command and observing the command block that appears around its output. If no block appears, see [Troubleshooting](./troubleshooting.md).

---

## Command Blocks

Every command executed inside Glass is captured as a block. A block displays:

- The command text as entered
- Exit code (green for zero, red for non-zero)
- Execution duration
- Working directory at time of execution

Blocks are discrete units -- they can be collapsed to a single line, individually searched, and referenced by MCP tools using their block ID.

---

## Structured Output Intelligence

After a command completes, Glass classifies its output through the SOI pipeline. The classification appears as a one-line decoration at the bottom of the block -- a concise label such as "2 errors", "JSON object", "warning: deprecated flag", or "success". This runs in the background automatically after each command finishes; no action is required.

The classification is stored in the history database and is accessible via MCP tools, allowing AI agents to query structured output rather than parsing raw text. See [Structured Output Intelligence](./features/soi.md) for the full list of output categories and how the pipeline works.

---

## Key Shortcuts

| Action | Shortcut |
|---|---|
| New tab | `Ctrl+Shift+T` |
| Close tab or pane | `Ctrl+Shift+W` |
| Next tab | `Ctrl+Tab` |
| Previous tab | `Ctrl+Shift+Tab` |
| Split pane horizontally | `Ctrl+Shift+H` |
| Split pane vertically | `Ctrl+Shift+V` |
| Focus next pane | `Ctrl+Shift+]` |
| Focus previous pane | `Ctrl+Shift+[` |
| Open search overlay | `Ctrl+Shift+F` |
| Undo last file modification | `Ctrl+Shift+Z` |
| Open agent review overlay | `Ctrl+Shift+A` |
| Scroll up one block | `Ctrl+Shift+Up` |
| Scroll down one block | `Ctrl+Shift+Down` |

The agent review overlay (`Ctrl+Shift+A`) is relevant only when Agent Mode is active. If no agent session is running, the overlay reports that no agent is present.

---

## Configuration

Configuration lives at `~/.glass/config.toml`. The file is created with defaults on first launch if it does not exist.

Glass watches the file with a filesystem watcher and applies changes immediately -- there is no need to restart. If a change introduces a parse error, Glass logs the error and continues using the last valid configuration.

Key sections in `config.toml`:

- `[font]` -- family, size, line height
- `[shell]` -- override the detected shell, set environment variables
- `[history]` -- retention period, database location
- `[snapshot]` -- which commands trigger snapshots, blob store location
- `[pipes]` -- pipeline capture settings

See [Configuration](./configuration.md) for the full reference with all available keys and their defaults.

---

## Next Steps

- [Structured Output Intelligence](./features/soi.md) -- how Glass classifies command output and what the SOI pipeline produces
- [Agent Mode](./features/agent-mode.md) -- running Claude CLI in a supervised background session with worktree isolation
- [Search](./features/search.md) -- searching current session output and persistent history
- [Undo](./features/undo.md) -- restoring files after destructive commands
- [MCP Server](./mcp-server.md) -- connecting an AI agent to Glass over MCP
