# Configuration

Glass is configured through a TOML file at `~/.glass/config.toml`. The file is optional -- Glass works with sensible defaults out of the box. Glass does not create this file automatically; create it manually when you want to customize settings.

**Changes are applied immediately** via hot-reload. You do not need to restart Glass after editing the config file.

## Complete example

```toml
# Font settings
font_family = "JetBrains Mono"
font_size = 16.0

# Shell override (auto-detected if omitted)
# shell = "/usr/bin/zsh"

# Command history settings
[history]
max_output_capture_kb = 50

# File snapshot settings
[snapshot]
enabled = true
max_count = 1000
max_size_mb = 500
retention_days = 30

# Pipeline inspection settings
[pipes]
enabled = true
max_capture_mb = 10
auto_expand = true
```

## Top-level options

| Option | Type | Default | Description |
|---|---|---|---|
| `font_family` | string | Platform default | Font face for terminal text. Defaults to Consolas (Windows), Menlo (macOS), or Monospace (Linux). |
| `font_size` | float | `14.0` | Font size in points. |
| `shell` | string | Auto-detected | Path to shell executable. When omitted, Glass detects your default shell automatically. |

### Platform font defaults

| Platform | Default font |
|---|---|
| Windows | Consolas |
| macOS | Menlo |
| Linux | Monospace |

## `[history]` section

Controls command history capture and storage.

| Option | Type | Default | Description |
|---|---|---|---|
| `max_output_capture_kb` | integer | `50` | Maximum output captured per command in kilobytes. Output exceeding this limit is truncated. |

## `[snapshot]` section

Controls the file undo snapshot system. See [Undo](./features/undo.md) for usage details.

| Option | Type | Default | Description |
|---|---|---|---|
| `enabled` | boolean | `true` | Whether to capture file snapshots before commands modify files. |
| `max_count` | integer | `1000` | Maximum number of snapshots to retain. Oldest snapshots are pruned first. |
| `max_size_mb` | integer | `500` | Maximum total storage size for snapshot blobs in megabytes. |
| `retention_days` | integer | `30` | Snapshots older than this many days are automatically pruned. |

## `[pipes]` section

Controls pipeline stage capture. See [Pipe Inspection](./features/pipes.md) for usage details.

| Option | Type | Default | Description |
|---|---|---|---|
| `enabled` | boolean | `true` | Whether to capture intermediate pipeline stage output. |
| `max_capture_mb` | integer | `10` | Maximum data captured per pipeline stage in megabytes. |
| `auto_expand` | boolean | `true` | Automatically expand pipeline blocks when any stage fails. |

## Config file location

The config file path is:

| Platform | Path |
|---|---|
| All platforms | `~/.glass/config.toml` |

Glass resolves `~` to your home directory using the system's standard home directory lookup.

## Error handling

If `config.toml` contains invalid TOML or type errors, Glass displays an **error overlay** showing:

- The error message
- The line and column where the error was found
- A snippet of the problematic line

Glass continues running with the previous valid configuration (or defaults if no valid config was ever loaded). Fix the error in your config file and save -- the hot-reload will pick up the correction automatically.
