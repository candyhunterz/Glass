# Getting Started

## First launch

When you open Glass for the first time, it automatically detects and launches your default shell. No configuration is required -- Glass works out of the box.

## Shell integration

Glass automatically detects command boundaries using shell integration. Each command you run appears as a distinct **block** in your scrollback, showing:

- **Exit code** -- Green checkmark for success (exit 0), red X for failure
- **Duration** -- How long the command took to execute
- **Working directory** -- The directory the command ran in

You interact with Glass just like any other terminal. The structured features work passively in the background.

## Basic navigation

- **Scroll** -- Mouse wheel or trackpad scrolls through your command history
- **Command blocks** -- Each command is visually separated, making it easy to find specific output
- **Search** -- Press **Ctrl+Shift+F** to search across your entire command history

## Key shortcuts

| Shortcut | Action |
|---|---|
| Ctrl+Shift+T | Open a new tab |
| Ctrl+Shift+W | Close current tab or pane |
| Ctrl+Shift+D | Split pane vertically |
| Ctrl+Shift+F | Open search overlay |
| Ctrl+Shift+Z | Undo last file modification |
| Ctrl+Shift+U | Check for updates |

## Configuration

Glass stores its configuration at `~/.glass/config.toml`. The file is created on first edit (Glass does not generate it automatically). See the [Configuration](./configuration.md) reference for all available options.

Changes to `config.toml` are applied immediately via hot-reload -- no restart needed.

## Next steps

- Learn about [Command Blocks](./features/blocks.md) and structured scrollback
- Set up [Search](./features/search.md) to find commands across sessions
- Configure [Undo](./features/undo.md) for file safety
- Connect an AI assistant via the [MCP Server](./mcp-server.md)
