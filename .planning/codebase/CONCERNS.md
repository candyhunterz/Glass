# Codebase Concerns

**Analysis Date:** 2026-03-08

## Tech Debt

**Monolithic `src/main.rs` (2,537 lines):**
- Issue: All window event handling, session lifecycle, rendering orchestration, shell event routing, keyboard shortcuts, mouse handling, clipboard, and CLI dispatch live in a single file.
- Files: `src/main.rs`
- Impact: Difficult to navigate, test, or refactor individual concerns. The `window_event` match arm alone spans ~1,050 lines. Adding new keyboard shortcuts or event handlers requires understanding the entire file.
- Fix approach: Extract logical modules: `src/keybindings.rs`, `src/event_handler.rs`, `src/session_lifecycle.rs`, `src/clipboard.rs`. Keep `Processor` and `ApplicationHandler` impl in `main.rs` but delegate to extracted modules.

**Duplicate `ShellEvent` to `OscEvent` conversion:**
- Issue: `OscEvent` (in `glass_terminal`) and `ShellEvent` (in `glass_core`) are structurally identical enums. Two manual conversion functions exist: `shell_event_to_osc()` in `src/main.rs` (line 181) and `convert_osc_to_shell()` in `crates/glass_terminal/src/pty.rs` (line 94). Every variant must be kept in sync across both enums and both converters.
- Files: `src/main.rs`, `crates/glass_terminal/src/pty.rs`, `crates/glass_terminal/src/osc_scanner.rs`, `crates/glass_core/src/event.rs`
- Impact: Adding a new shell integration event requires changes in 4 files. Risk of silent breakage if a variant is added to one enum but not the other.
- Fix approach: Unify into a single event type in `glass_core::event`, or use `From` trait implementations to automate conversion.

**Duplicated terminal size calculation logic:**
- Issue: Terminal column/line computation from pixel dimensions appears in at least 4 places with slightly different formulations: `create_session()`, `resumed()`, `Resized` handler single-pane path, and `resize_all_panes()`.
- Files: `src/main.rs` (lines 304, 360-362, 500-503, 877-879)
- Impact: Easy to introduce off-by-one errors when adjusting chrome reservations (tab bar, status bar lines). One path subtracts 2, another subtracts `1 + tab_bar_lines`.
- Fix approach: Create a `compute_grid_dimensions(window_size, cell_size, chrome_lines) -> (cols, lines)` helper and use it everywhere.

**`Session` struct has too many public fields (17 fields, all `pub`):**
- Issue: `Session` in `crates/glass_mux/src/session.rs` is a flat struct with 17 public fields and no methods. It acts as a bag of state with no encapsulation.
- Files: `crates/glass_mux/src/session.rs`
- Impact: Any code with a `&mut Session` can modify any field. Business logic for session lifecycle (command tracking, snapshot management) is scattered across `src/main.rs` rather than being co-located with the data.
- Fix approach: Add methods to `Session` for common operations (start_command, finish_command, set_cwd). Make internal tracking fields private.

**Prompt prefix stripping is fragile:**
- Issue: Pipeline command text extraction strips the prompt prefix by searching for `"> "` (line 1934 of `src/main.rs`). This is a PowerShell-specific heuristic that will break for other shells or custom prompts.
- Files: `src/main.rs` (lines 1934-1938)
- Impact: Pipeline stage command text may include prompt prefix on non-PowerShell shells, or incorrectly strip content containing `"> "`.
- Fix approach: Use shell integration OSC sequences to precisely delimit command input boundaries rather than heuristic stripping.

## Known Bugs

**Dynamic DPI / scale factor changes not supported:**
- Symptoms: Moving Glass between monitors with different DPI scaling produces incorrect font rendering. A warning is logged: "Dynamic scale factor update not yet supported; restart Glass to apply new DPI settings."
- Files: `src/main.rs` (lines 930-941)
- Trigger: Drag window between monitors with different scale factors.
- Workaround: Restart Glass after moving to a different-DPI monitor.

**Mouse selection not accounting for multi-pane offsets:**
- Symptoms: In split-pane mode, mouse selection coordinates use global window position but the grid renderer expects pane-local coordinates. Selection may land on wrong cells in non-primary panes.
- Files: `src/main.rs` (lines 1418-1455, 1517-1549)
- Trigger: Click and drag to select text in a split pane that is not at the top-left origin.
- Workaround: None currently; selection works correctly only in single-pane mode and the primary pane of splits.

## Security Considerations

**Auto-update downloads and executes MSI without signature verification:**
- Risk: The update mechanism in `apply_update()` downloads an MSI from the GitHub release URL and launches `msiexec /i /passive` without verifying any code signature, checksum, or TLS certificate pinning. A compromised GitHub account or MITM attack could deliver malicious installers.
- Files: `crates/glass_core/src/updater.rs` (lines 131-162)
- Current mitigation: HTTPS transport (TLS via ureq). User must explicitly press Ctrl+Shift+U to trigger.
- Recommendations: Add SHA256 checksum verification against a signed manifest. Consider code-signing the MSI and verifying before execution. At minimum, prompt the user for confirmation before launching the installer.

**`std::mem::forget(temp_dir)` leaks temporary MSI file:**
- Risk: The downloaded MSI is intentionally leaked via `std::mem::forget(temp_dir)` so it persists for `msiexec`. This leaves an unmanaged executable file in the temp directory indefinitely.
- Files: `crates/glass_core/src/updater.rs` (line 143)
- Current mitigation: None. The file persists until OS temp cleanup.
- Recommendations: Use `into_path()` instead of `forget()` to take ownership of the path without dropping the dir. Consider cleanup on next launch.

**Shell integration injection via PTY input:**
- Risk: Shell integration scripts are auto-injected by sending `source '<path>'\r\n` to the PTY as raw input (line 396 of `src/main.rs`). If the script path contains shell metacharacters (unlikely but possible via crafted filesystem paths), this could execute unintended commands.
- Files: `src/main.rs` (lines 384-398)
- Current mitigation: Path comes from the executable's own directory, which is typically controlled by the installer.
- Recommendations: Validate the path contains no shell metacharacters, or use a safer injection mechanism.

**History database uses `dirs::home_dir().expect()` which panics:**
- Risk: If the home directory cannot be determined (rare but possible in containerized environments), the entire application panics on history DB path resolution.
- Files: `crates/glass_history/src/lib.rs` (line 36), `crates/glass_snapshot/src/lib.rs` (line 93)
- Current mitigation: None. The expect message describes the issue but the app crashes.
- Recommendations: Return `Result` instead of panicking. The caller already handles `None` for `history_db` gracefully.

## Performance Bottlenecks

**Glyph cache unbounded growth:**
- Problem: The `GlyphCache` wraps glyphon's `TextAtlas` which grows as new glyphs are rasterized but is only trimmed via `trim()` at end of each frame. There is no upper bound on atlas size.
- Files: `crates/glass_renderer/src/glyph_cache.rs`, `crates/glass_renderer/src/frame.rs` (line 857: `trim()` call)
- Cause: Terminal output can contain arbitrary Unicode characters, each requiring atlas space. Long-running sessions with diverse output (e.g., CJK, emoji, math symbols) accumulate atlas entries.
- Improvement path: Monitor atlas memory usage and implement a hard cap or LRU eviction strategy. Profile with `GLASS_LOG=glass_renderer=debug` to track atlas size.

**O(n^2) divider computation for split panes:**
- Problem: `compute_dividers()` compares every pair of pane layouts with a nested loop, checking 4 adjacency conditions per pair.
- Files: `src/main.rs` (lines 208-273)
- Cause: Brute-force pairwise comparison instead of spatial indexing.
- Improvement path: For typical pane counts (2-8), this is negligible. Only becomes an issue if deeply nested splits are supported in the future. No action needed currently.

**Git status query spawns a new thread per CWD change:**
- Problem: Every `ShellEvent::CurrentDirectory` event spawns a new OS thread to run `query_git_status()`, which shells out to `git status --porcelain`.
- Files: `src/main.rs` (lines 2168-2183)
- Cause: No thread pool or debouncing. Rapid `cd` commands spawn many threads.
- Improvement path: Use a dedicated background thread with a channel, or debounce CWD changes before spawning the query.

**Full filesystem watcher per command execution:**
- Problem: A `notify::RecommendedWatcher` is created for every command execution and watches the entire CWD recursively. In large directories (e.g., monorepos with thousands of files), this can be expensive.
- Files: `src/main.rs` (lines 1953-1968), `crates/glass_snapshot/src/watcher.rs`
- Cause: Recursive watch on potentially large directory trees. The watcher is created at CommandExecuted and drained at CommandFinished.
- Improvement path: Consider rate-limiting watcher creation for rapid command sequences, or skip watching for read-only commands (already parsed by `command_parser`).

## Fragile Areas

**Keyboard shortcut handling in `window_event`:**
- Files: `src/main.rs` (lines 945-1417)
- Why fragile: Keyboard input handling is a deeply nested chain of `if`/`match` blocks with multiple early returns. The ordering matters (search overlay intercept must come first, then Glass shortcuts, then PTY forwarding). Adding a new shortcut requires understanding the full precedence chain.
- Safe modification: When adding shortcuts, add them to the existing `Ctrl+Shift` match block (lines 1166-1302). Always test that the shortcut does not conflict with the search overlay intercept (lines 950-1029). Never add shortcuts before the overlay check.
- Test coverage: Only CLI parsing is tested (`src/tests.rs`). No tests for keyboard shortcut routing.

**Shell event routing in `user_event` (AppEvent::Shell handler):**
- Files: `src/main.rs` (lines 1732-2188)
- Why fragile: The `AppEvent::Shell` handler is 456 lines of sequential logic that must maintain careful ordering: block manager update, snapshot creation, command text extraction, pipeline parsing, history DB insert, watcher drain, git query spawn. Borrow checker constraints require splitting session access across multiple scopes.
- Safe modification: Follow the existing pattern of borrowing `session` in a block, extracting needed data, dropping the borrow, then using the data. Do not hold `session` across operations that need `ctx` or `self`.
- Test coverage: No integration tests for the shell event lifecycle.

**Pipeline stage data flow:**
- Files: `src/main.rs` (lines 1783-1829), `crates/glass_pipes/src/types.rs`, `crates/glass_terminal/src/block_manager.rs`
- Why fragile: Pipeline data flows through: shell integration OSC -> BlockManager (allocates stages) -> main.rs (reads temp files, processes through StageBuffer, stores in block) -> history DB (serializes to PipeStageRow). A bug in any step silently produces empty or corrupt stage data.
- Safe modification: Always test pipeline functionality end-to-end after changes to any file in this chain.
- Test coverage: `glass_pipes` has unit tests for parsing and buffering. No integration test for the full pipeline data flow.

## Scaling Limits

**SQLite history database (single-writer):**
- Current capacity: Handles thousands of commands efficiently with WAL mode and FTS5.
- Limit: SQLite's single-writer model means concurrent sessions writing to the same DB could block each other. Currently each session opens its own `HistoryDb` connection, but they share the same database file per working directory.
- Scaling path: The `PRAGMA busy_timeout = 5000` setting provides 5s of retry. For heavy concurrent use, consider a shared writer thread or separate DBs per session.

**Single-window architecture:**
- Current capacity: One OS window with multiple tabs and split panes.
- Limit: `Processor.windows` is a HashMap but `resumed()` exits early if any window exists (line 466). Only one window is ever created.
- Scaling path: Remove the early return guard and handle per-window GPU resource creation.

## Dependencies at Risk

**`alacritty_terminal` pinned to exact version `=0.25.1`:**
- Risk: Exact version pin means no automatic patch updates. The alacritty terminal library is the core of the terminal emulation and changes frequently. API breaks require careful migration.
- Impact: Security patches to the terminal emulator require manual version bump and verification.
- Migration plan: Periodically check for new releases. The pin is intentional to avoid surprise breaks, but should be reviewed quarterly.

## Missing Critical Features

**No right-click context menu:**
- Problem: Terminal emulators conventionally offer right-click context menu for copy/paste/search. Glass has no context menu at all.
- Blocks: Users accustomed to right-click paste (common on Windows terminals) have no alternative.

**No font fallback chain:**
- Problem: `GlassConfig.font_family` is a single string. If the font lacks glyphs for certain characters, rendering falls back to whatever glyphon selects internally, which may not be ideal.
- Blocks: CJK characters, emoji, and special symbols may render as boxes if the primary font lacks them.

**No per-session history isolation:**
- Problem: All sessions in the same CWD share the same SQLite history database. Commands from different tabs/panes interleave in the same DB.
- Blocks: History search results mix commands from different sessions with no way to filter by session.

## Test Coverage Gaps

**No tests for `src/main.rs` event handling (2,537 lines):**
- What's not tested: Window event handling, keyboard shortcuts, mouse input, shell event routing, session lifecycle, rendering orchestration. The only test in `src/tests.rs` covers CLI argument parsing (180 lines of parse tests).
- Files: `src/main.rs`, `src/tests.rs`
- Risk: Any refactoring of the main event loop could silently break keyboard shortcuts, clipboard, tab management, split pane focus, or pipeline interactions. This is the most critical untested code.
- Priority: High

**No tests for GPU rendering pipeline:**
- What's not tested: `glass_renderer` crate has test modules in `block_renderer.rs`, `config_error_overlay.rs`, `search_overlay_renderer.rs`, and `tab_bar.rs`, but these test only rect/layout computation, not actual GPU rendering. No tests verify that rendered output is visually correct.
- Files: `crates/glass_renderer/src/frame.rs` (1,272 lines), `crates/glass_renderer/src/grid_renderer.rs` (391 lines)
- Risk: Visual regressions in terminal rendering (wrong colors, misaligned text, missing cursor) go undetected.
- Priority: Medium (visual testing is inherently hard; screenshot comparison tests would help)

**No tests for PTY event loop:**
- What's not tested: `glass_pty_loop()`, `pty_read_with_scan()`, `pty_write()` in `crates/glass_terminal/src/pty.rs`. These are the core I/O functions bridging the shell process to the terminal grid.
- Files: `crates/glass_terminal/src/pty.rs` (527 lines)
- Risk: Regressions in PTY read/write, synchronized update handling, or output capture could cause data loss or hangs.
- Priority: High

**Single integration test file (`tests/mcp_integration.rs`):**
- What's not tested: No integration tests for the terminal GUI workflow, history recording end-to-end, snapshot/undo lifecycle, or multi-tab/pane behavior.
- Files: `tests/mcp_integration.rs`
- Risk: Cross-crate interactions (glass_terminal -> glass_history -> glass_snapshot) are only tested via individual crate unit tests, not as an integrated system.
- Priority: Medium

---

*Concerns audit: 2026-03-08*
