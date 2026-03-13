# History

Glass stores your complete command history in a SQLite database with FTS5 full-text indexing. History persists across sessions and is searchable from both the UI and the command line.

## How it works

Every command you run is recorded with:

- The command line text
- Full output (stdout and stderr), up to a configurable size limit
- Exit code, duration, and working directory
- Timestamp
- SOI classification results (e.g., error counts, build status) when available

History is per-project and stored in the `~/.glass/` directory. Each project tracked by Glass maintains its own history database scoped to the project root.

## Searching history

### From the UI

Press **Ctrl+Shift+F** to open the search overlay. See [Search](./search.md) for details.

### From the command line

Glass provides CLI commands for querying history:

```bash
glass history search "query"            # Full-text search across command history
glass history list                      # List recent commands
glass history list --exit 1             # List commands that exited with a specific code
glass history list --after 1h          # List commands from the last hour
glass history list --after 1h --cwd /project  # Filter by time and working directory
```

## Schema

The history database is currently on schema version 3. Version 3 adds a `soi_records` column to store SOI classification results alongside each command entry. Existing databases are migrated automatically on first launch.

## Configuration

History behavior is controlled by the `[history]` section in `~/.glass/config.toml`:

```toml
[history]
max_output_capture_kb = 50    # Maximum output capture per command in KB (default: 50)
```

### Options

| Option | Default | Description |
|---|---|---|
| `max_output_capture_kb` | `50` | Maximum size of captured command output in kilobytes. Output exceeding this limit is truncated. |

## Storage

The history database uses SQLite, chosen for its reliability, zero-configuration operation, and built-in FTS5 full-text search. The database is stored in the `~/.glass/` directory and requires no maintenance.
