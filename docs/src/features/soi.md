# Structured Output Intelligence (SOI)

SOI is Glass's system for classifying, parsing, and compressing command output into machine-readable structured records. Every command's output is automatically classified and parsed when it finishes. Humans see a one-line summary decoration on command blocks. AI agents get structured, token-budgeted access to the same data via MCP tools.

---

## How It Works

When a command finishes (a `CommandFinished` event fires), Glass runs SOI parsing off the main thread using `spawn_blocking` to avoid blocking the event loop. The pipeline has three stages:

1. **Classification** — The command name and output content are examined by a set of pattern sniffers to determine which format-specific parser to invoke.
2. **Parsing** — The matched parser extracts structured records. Each record carries fields such as file, line, severity, and message.
3. **Storage** — Records are written to SQLite (schema v3, tables: `command_output_records`, `output_records`). A `SoiReady` event fires with the `command_id`, a short summary string, and an aggregate severity level.

If no parser matches, output is stored as `Freeform` with no extracted structure. SOI never produces false positives by forcing unrecognized output into a structured format.

---

## Supported Parsers

Glass ships 12 format-specific parsers:

| Domain | Parser | Recognizes |
|---|---|---|
| Rust | `cargo-build` / `cargo-clippy` | Compiler errors and warnings |
| Rust | `cargo-test` | Test results (passed, failed, ignored) |
| JavaScript | `npm` | Package install/update events |
| JavaScript | `jest` | Test suite results |
| Python | `pytest` | Test results |
| TypeScript | `tsc` | Compiler errors |
| Go | `go-build` | Compiler errors |
| Go | `go-test` | Test results |
| DevOps | `git` | Status, diff summary, log entries |
| DevOps | `docker` | Build steps, compose service events |
| DevOps | `kubectl` | Apply events, resource listings |
| Generic | `json-lines` | NDJSON / structured log streams |

Unrecognized output falls through to `Freeform`.

---

## Compression Engine

AI agents access SOI records through a token-budgeted compression engine. Four budget levels are available:

| Level | Approximate tokens | Content |
|---|---|---|
| `OneLine` | ~10 | Error count and first error file |
| `Summary` | ~100 | Key findings across all severities |
| `Detailed` | ~500 | Prioritized errors, then warnings, then info |
| `Full` | Unlimited | Complete record set |

Two additional modes are available at any budget level:

- **Diff-aware**: Returns a "compared to last run" change summary rather than an absolute snapshot, useful for detecting regressions between successive runs of the same command.
- **Drill-down**: Accepts a `record_id` returned by a previous query and expands that single record to full detail regardless of the active budget level.

---

## Display

**Block decoration** — When a command block transitions to `Complete`, a muted one-line label is rendered beneath the block output. Examples:

```
2 errors, 1 warning  src/main.rs
14 passed, 0 failed
```

**Shell hint lines** — When `soi.shell_summary = true` is set in config, a hint line is injected into the terminal output stream. This line is visible to AI agents using the Bash tool, allowing them to detect structured outcomes without MCP access.

---

## MCP Tools

Four MCP tools expose SOI data to AI agents:

**`glass_query`** — Returns structured records for a given `command_id` at a requested token budget level. Accepts `budget` as `one_line`, `summary`, `detailed`, or `full`.

**`glass_query_trend`** — Compares the last N runs of the same command. Returns a diff-aware summary highlighting regressions and improvements across runs.

**`glass_query_drill`** — Expands a specific `record_id` to full detail. Used after a `Summary` or `Detailed` query identifies a record worth investigating.

**`glass_context` / `glass_compressed_context`** — The general-purpose context tools include SOI summaries for recently completed commands in the returned workspace snapshot.

---

## Configuration

```toml
[soi]
enabled = true           # Enable/disable SOI (default: true)
shell_summary = false    # Inject hint lines into terminal output (default: false)
min_lines = 5            # Minimum output lines to trigger SOI (default: 5)
```

`min_lines` prevents SOI from running on trivial one-line outputs. Commands whose output is shorter than this threshold are stored as `Freeform` without classification.
