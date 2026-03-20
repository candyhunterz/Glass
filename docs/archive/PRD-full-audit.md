# Glass Full Feature Audit

## Goal

Systematically audit all Glass features for bugs, inconsistencies, dead code, missing tests, and config/runtime disconnects. Fix everything found. This is a code-level audit — not a visual/UI audit.

## Approach

For each feature area: read the code, check for logic bugs, verify tests cover the important paths, write tests for gaps, fix bugs found. Run `cargo test --workspace` and `cargo clippy --workspace -- -D warnings` after each fix.

## Audit Areas (in order)

### 1. Config System (`crates/glass_core/src/config.rs`)

Audit:
- Every `#[serde(default)]` has a matching default function that returns a sensible value
- `update_config_field()` correctly handles all section paths used in settings handlers (test with: `agent`, `agent.orchestrator`, `agent.permissions`, `agent.quiet_rules`, `soi`, `snapshot`, `pipes`, `history`)
- Config round-trip: write a field → hot-reload → read it back → value matches
- No serde `rename` mismatches between TOML key names and struct field names
- All `Option<T>` fields behave correctly when absent vs explicitly set to default

Write tests for any gaps found.

### 2. Settings Overlay Sync (`src/main.rs` + `crates/glass_renderer/src/settings_overlay.rs`)

Audit:
- Every `SettingsConfigSnapshot` field reads from the correct `self.config.*` path
- Every snapshot default matches the corresponding serde default in config.rs
- Every `handle_settings_activate()` match arm writes the correct section/key with proper TOML formatting
- Every `handle_settings_increment()` match arm uses the right step size, bounds, and value type
- No fields in `fields_for_section()` that have no corresponding activate/increment handler
- No handlers for field indices that don't exist in `fields_for_section()`

### 3. Orchestrator (`src/orchestrator.rs` + orchestrator handling in `src/main.rs`)

Audit:
- `parse_agent_response()` correctly handles all response types (TypeText, Wait, Checkpoint, Done, Verify) including edge cases (empty strings, malformed JSON, mixed markers)
- `should_auto_checkpoint()` logic is correct (iteration count, timing)
- `should_stop_bounded()` handles all edge cases (None, Some(0), exact boundary)
- `check_regression()` handles edge cases: empty baselines, mismatched lengths, all-None counts
- `update_baseline_if_improved()` only raises the floor when appropriate
- `build_bounded_summary()` format is correct for all cases (with/without metric guard, with/without test counts)
- Stuck detection: `record_response()` correctly detects N identical responses
- Fingerprint: `compute_fingerprint()` is deterministic and meaningful
- The OrchestratorSilence handler in main.rs: verify the flow from silence → context capture → agent send is correct, backpressure works, response_pending is always cleared
- Checkpoint cycle: begin_checkpoint → poll → respawn flow has no race conditions

Write tests for any edge cases not covered.

### 4. Undo System (`crates/glass_snapshot/`)

Audit:
- `command_parser.rs`: all destructive command patterns are correct (rm, mv, sed -i, chmod, chown, truncate, etc.)
- No false positives (e.g., `rm` inside a string argument shouldn't trigger)
- No false negatives (common destructive commands that are missing)
- `undo.rs`: undo_latest() correctly restores files from the blob store
- Blob store: content-addressed hashing is correct (blake3)
- Pruning: retention_days and max_count/max_size limits work correctly
- File watcher integration: snapshot is taken BEFORE the command executes, not after

Write tests for destructive command detection edge cases.

### 5. Pipe System (`crates/glass_pipes/`)

Audit:
- Pipeline detection: correctly identifies pipe operators (`|`) in command strings
- Handles edge cases: pipes inside quotes, escaped pipes, subshells, backgrounded pipelines
- Stage capture: each pipeline stage's output is captured independently
- Stage count matches the number of pipe operators + 1

Write tests for edge cases (quoted pipes, nested commands).

### 6. History System (`crates/glass_history/`)

Audit:
- SQLite schema is consistent with query code
- FTS5 search works correctly (test with special characters, unicode, empty queries)
- Pruning respects max_entries and retention settings
- Concurrent access (WAL mode) doesn't corrupt data
- `max_output_capture_kb` truncation is applied correctly

Write tests for edge cases.

### 7. SOI System (`crates/glass_soi/`)

Audit:
- Classifier: every `OutputType` variant has a corresponding parser dispatch
- No unhandled variants that silently fall through
- All parsers handle empty input gracefully (return freeform, don't panic)
- Token estimates are reasonable (not wildly off)
- ANSI stripping is applied consistently before parsing

Write tests for any gaps.

### 8. Shell Integration (`shell-integration/`)

Audit:
- OSC 133 sequences are correctly formed in all 4 shell scripts (bash, zsh, fish, PowerShell)
- Prompt/command/execute/finish markers are consistent across shells
- Pipeline stage markers (133;S and 133;P) are present in all shells

This is a read-only audit — report findings but don't modify shell scripts unless there's an obvious bug.

### 9. MCP Tools (`crates/glass_mcp/`)

Audit:
- All 31 tools are registered in the tool dispatch
- Tool parameter validation: required params are checked, optional params have defaults
- Error responses are properly formatted
- No tools that silently return empty results on valid input

### 10. Performance Audit

Glass must feel instant. Every user-facing operation should complete in under 16ms (one frame at 60fps) unless it involves I/O.

Audit:
- **Unnecessary allocations in hot paths**: Look for `.clone()`, `.to_string()`, `format!()`, `Vec::new()` inside per-frame code, event handlers, and PTY read loops. Replace with borrows, `Cow`, or pre-allocated buffers where possible.
- **Regex compilation**: Find any `Regex::new()` called repeatedly (not cached with `OnceLock`/`LazyLock`). Every regex must be compiled once and reused. Check `src/main.rs` (especially `parse_test_counts_from_output`), all SOI parsers, and `command_parser.rs`.
- **Blocking the event loop**: Find any `std::process::Command` or file I/O that runs synchronously on the main thread (the winit event loop in `main.rs`). These must be on background threads. Check: git commands, config writes, checkpoint file reads, iteration log writes.
- **SQLite on the hot path**: Verify that SQLite queries (history, SOI, coordination) use `spawn_blocking` or run on a background thread, never synchronously on the event loop.
- **Unnecessary work per frame**: Check the frame rendering path — is `SettingsConfigSnapshot` rebuilt every frame even when settings overlay is closed? Are there expensive computations that could be cached?
- **String formatting in logging**: Replace `tracing::info!("... {}", expensive_computation)` with lazy evaluation where the computation is only needed at debug level.
- **Large clones**: Find `.clone()` on large structs (config, baseline results, terminal content). Use references or `Arc` where ownership isn't needed.

Fix:
- Cache regexes with `OnceLock` or `std::sync::LazyLock`
- Move blocking I/O off the event loop onto background threads
- Replace hot-path clones with borrows
- Add `#[inline]` to small frequently-called functions in the rendering path
- Remove dead allocations

Write benchmarks (using `#[cfg(test)]` timing assertions) for any performance-critical function where the fix isn't obviously correct.

### 11. Cross-Crate Consistency

Audit:
- Event types in `glass_core/src/event.rs` all have handlers in `main.rs`
- No `AppEvent` variants that are defined but never matched
- No match arms that reference removed or renamed variants
- `SessionId` is used consistently (not mixed with raw u64)

## Rules

- Run `cargo test --workspace` after EACH fix (not at the end)
- Run `cargo clippy --workspace -- -D warnings` after EACH fix
- Commit each fix separately: `fix(crate): description of what was wrong`
- Do NOT refactor or restructure — only fix bugs and add tests
- Do NOT add features
- Do NOT modify shell integration scripts unless there's a clear bug
- If you find a bug that's risky to fix (could break runtime behavior), write a test that demonstrates the bug and commit it as `test(crate): expose bug in X` — don't fix it
- Prioritize: performance issues blocking the event loop > bugs that cause incorrect behavior > missing tests > code style issues

## Success Criteria

- All audit areas reviewed
- All bugs found are either fixed (with tests) or documented (with failing test)
- `cargo test --workspace` passes
- `cargo clippy --workspace -- -D warnings` clean
- No regressions in existing tests
