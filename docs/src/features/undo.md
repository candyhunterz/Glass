# Undo

Glass can undo file modifications made by terminal commands. Before a command modifies a file, Glass snapshots the original content, allowing you to restore it with a single keystroke.

## Using undo

Press **Ctrl+Shift+Z** to undo the last file modification.

When you trigger undo:

1. Glass identifies the most recent file modification from the last command.
2. The file is restored to its pre-command state from the snapshot.
3. A notification confirms which file was restored.

## How snapshots work

Glass monitors file system changes during command execution. Before a file is modified, its contents are captured and stored in a local SQLite database. This means:

- Snapshots are taken **automatically** -- no manual action required
- Only files that are actually modified are snapped
- Snapshots persist across sessions

## Configuration

Snapshot behavior is controlled by the `[snapshot]` section in `~/.glass/config.toml`:

```toml
[snapshot]
enabled = true          # Enable/disable snapshot capture (default: true)
max_count = 1000        # Maximum number of snapshots to retain (default: 1000)
max_size_mb = 500       # Maximum total snapshot storage in MB (default: 500)
retention_days = 30     # Number of days to keep snapshots (default: 30)
```

### Options

| Option | Default | Description |
|---|---|---|
| `enabled` | `true` | Whether to capture file snapshots before commands modify files |
| `max_count` | `1000` | Maximum number of snapshots stored. Oldest are pruned first |
| `max_size_mb` | `500` | Maximum total storage size for snapshot blobs in megabytes |
| `retention_days` | `30` | Snapshots older than this many days are automatically pruned |

## Limitations

- Undo restores the file to its state before the **last** command that modified it
- Binary files are snapped but may not be meaningfully restorable in all cases
- Snapshot pruning runs automatically based on count, size, and age limits
