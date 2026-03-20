# Glass Robustness & Edge Case Audit

## Goal

Harden Glass for open-source release. Focus on error handling, edge cases, resource safety, and cross-platform correctness. The previous audit (PRD-full-audit.md) covered feature correctness and test gaps. This audit focuses on what breaks under stress, unusual inputs, and real-world usage patterns.

## Approach

For each area: read the code, identify edge cases that could crash/hang/corrupt, write tests that exercise those edges, fix issues found. Run `cargo test --workspace` and `cargo clippy --workspace -- -D warnings` after each fix.

## Rules

- Do NOT break existing tests. Test count must only go up.
- Every fix must have a test that would have caught the bug.
- Commit after each fix with a descriptive message.
- If you find a known issue that's too risky to fix, document it in `.glass/known-issues.md` with the file, line, and description.

## Audit Areas (in order)

### 1. MCP Tool Robustness (`crates/glass_mcp/src/tools.rs`)

This is the primary attack surface when agents interact with Glass. Every tool is callable by external AI agents.

Audit:
- Every tool handler validates its inputs (missing params, wrong types, empty strings, very long strings)
- `glass_tab_send` with binary/control characters doesn't corrupt terminal state
- `glass_tab_output` with massive terminal buffers doesn't OOM
- `glass_tab_close` on invalid/already-closed tab IDs returns a clean error
- `glass_history_search` with adversarial FTS5 queries (SQL injection, very long queries, special characters) is safe
- `glass_undo` on a command ID that doesn't exist returns a clean error
- `glass_file_diff` with paths outside the project root is handled
- `glass_pipe_inspect` with invalid stage indices returns a clean error
- All coordination tools (`glass_agent_register`, `glass_agent_lock`, etc.) handle concurrent access correctly
- No tool handler panics on any input — all return proper MCP error responses

Write tests for any gaps found.

### 2. Shell Integration Scripts (`shell-integration/`)

These run inside user shells. Bugs here break the user's terminal, not just Glass.

Audit:
- `glass.bash`: precmd/preexec hooks don't break if $TERM is unusual or if running inside tmux/screen
- `glass.zsh`: hook registration doesn't conflict with powerlevel10k, oh-my-zsh, or other popular frameworks
- `glass.fish`: function definitions don't shadow common fish functions
- `glass.ps1`: works in both PowerShell 5.1 and PowerShell 7+
- All scripts: OSC 133 sequences are properly terminated (ST vs BEL)
- All scripts: handling of multiline commands, heredocs, and command substitution
- All scripts: no performance impact on shell startup (measure with `time`)
- Pipeline capture (OSC 133;S/P) handles pipes with >10 stages gracefully
- CWD reporting (OSC 7) handles paths with spaces, unicode, and special characters

### 3. Block Manager Edge Cases (`crates/glass_terminal/src/block_manager.rs`)

The block manager is the heart of Glass's command-level awareness.

Audit:
- Rapid shell events (PromptStart → CommandStart → Executed → Finished in <1ms) don't corrupt state
- Out-of-order events (e.g., Finished before Executed) are handled gracefully
- Missing events (PromptStart without a following CommandStart — user presses Enter on empty prompt)
- Very long commands (>10KB of input text) don't cause allocation issues
- Block lifecycle state machine: every transition is valid, invalid transitions are logged and ignored
- `visible_blocks()` with thousands of blocks performs acceptably
- Exit codes: negative exit codes, exit code 128+signal on Unix, very large exit codes
- SOI hint line rendering with very long summaries

Write tests for edge cases not covered.

### 4. PTY & Terminal Resilience (`crates/glass_terminal/src/pty.rs`, `output_capture.rs`)

Audit:
- PTY spawn failure paths: what happens if the shell binary doesn't exist?
- PTY read with very large output bursts (e.g., `cat /dev/urandom | head -c 10M`) — does the output buffer cap work?
- Output capture: binary data (null bytes, invalid UTF-8) doesn't panic
- Output capture: alt-screen detection works with tmux, vim, less, htop
- Shell integration injection: what if the shell rc file is read-only?
- ConPTY resize during active output doesn't crash (Windows-specific)
- PTY cleanup: child process is killed when session is closed (no zombie processes)

### 5. History Database Safety (`crates/glass_history/`)

Audit:
- SQLite WAL mode: concurrent reads during write don't block
- FTS5 index corruption recovery: what happens if the DB is corrupted?
- Very long command text (>100KB) — does insert truncate or fail?
- Very long output (>1MB) stored in history — memory pressure on query
- Retention pruning with >10K entries — is it O(n) or O(1)?
- Search with empty query, single character query, query with only special characters
- Database migration path: what happens if the schema changes between versions?
- Database locked by another process (e.g., two Glass instances)

### 6. Snapshot/Undo Safety (`crates/glass_snapshot/`)

Audit:
- Undo after the file has been modified by another process since the snapshot
- Undo of a file that was moved/renamed after snapshot
- Snapshot of symlinks — does it follow or snapshot the link?
- Snapshot of files in directories that are deleted mid-snapshot
- Blob store with very large files (>100MB) — is streaming used or full-memory load?
- File watcher: what happens if >1000 files change simultaneously (e.g., `git checkout` to a different branch)?
- Pruning race: pruning runs while a new snapshot is being written
- Content-addressed store: hash collision handling (blake3 makes this theoretical but worth checking)
- Command parser: piped commands like `cat foo | sed 's/x/y/' > bar` — is `bar` detected as a target?

### 7. Renderer Stability (`crates/glass_renderer/src/frame.rs`)

Audit:
- Zero-size window (minimized to taskbar) doesn't crash the renderer
- Very small window (<100px wide) — do overlays, tab bar, status bar handle it?
- Font not found: fallback behavior when configured font doesn't exist
- Very large font size (100pt) — does the grid renderer handle it?
- >20 tabs open simultaneously — does the tab bar overflow gracefully?
- Split panes: very deep split tree (10+ levels) — does layout computation overflow?
- Activity overlay with >500 events — scrolling performance
- Orchestrator overlay with 1000 events in the buffer — rendering performance

### 8. Error Propagation Audit (workspace-wide)

Audit:
- Search for `.unwrap()` in non-test code. Each one is a potential panic. Classify as:
  - Safe (value is guaranteed by prior check)
  - Unsafe (could panic on unexpected input) — fix with proper error handling
- Search for `panic!()` and `unreachable!()` in non-test code — verify these are actually unreachable
- Search for `.expect()` — verify the messages are descriptive and the panics are intentional

Focus on `src/main.rs` (14 unwraps) and `crates/glass_mcp/src/tools.rs` (high-value target for agent-triggered panics).
