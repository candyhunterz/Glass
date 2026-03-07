# Pipe Inspection

Glass captures intermediate output at each stage of a pipeline, letting you see what data flows between commands in a pipe chain.

## How it works

When you run a pipeline like:

```bash
cat data.csv | grep "error" | sort | uniq -c
```

Glass captures the output at each stage:

1. `cat data.csv` -- Full file contents
2. `grep "error"` -- Filtered lines
3. `sort` -- Sorted output
4. `uniq -c` -- Final deduplicated counts

You can inspect any stage to see exactly what data was passed to the next command.

## Auto-expand on failure

When a pipeline fails (non-zero exit code), Glass automatically expands the pipeline block to show all intermediate stages. This makes it easy to identify which stage in the pipeline produced unexpected output or failed.

This behavior can be disabled in configuration.

## Viewing pipe stages

Pipeline blocks display a visual indicator showing the number of stages. Click or expand the block to see the intermediate output at each stage.

## Configuration

Pipe inspection is controlled by the `[pipes]` section in `~/.glass/config.toml`:

```toml
[pipes]
enabled = true          # Enable/disable pipe capture (default: true)
max_capture_mb = 10     # Maximum capture size per stage in MB (default: 10)
auto_expand = true      # Auto-expand pipeline blocks on failure (default: true)
```

### Options

| Option | Default | Description |
|---|---|---|
| `enabled` | `true` | Whether to capture intermediate pipeline stage output |
| `max_capture_mb` | `10` | Maximum data captured per pipeline stage in megabytes |
| `auto_expand` | `true` | Automatically expand pipeline blocks when any stage fails or there are many stages |
