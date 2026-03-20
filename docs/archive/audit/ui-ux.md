# Glass UI/UX Prelaunch Audit

**Date**: 2026-03-18
**Auditor**: Automated codebase review
**Scope**: All user-facing interaction surfaces -- keybindings, visual design, overlays, command blocks, pipe visualization, undo, search, resize, scrollback, first-time experience.
**Perspective**: Developer switching from iTerm2, Windows Terminal, or Alacritty.

---

## Executive Summary

Glass has a surprisingly rich feature set for a terminal emulator: command blocks with exit badges, pipe visualization, undo, full-text search, agent proposals, split panes, and a settings overlay. The core terminal emulation (via alacritty_terminal) is solid and the GPU rendering pipeline is well-structured. However, the UX has significant discoverability problems -- a first-time user will see what looks like a slightly different dark terminal with no indication of Glass's unique capabilities. The shortcut system is extensive but entirely undiscoverable without pressing Ctrl+Shift+, to find the Shortcuts tab. Several interaction patterns are inconsistent or miss standard terminal conventions.

**Critical**: 0 findings
**High**: 6 findings
**Medium**: 11 findings
**Low**: 8 findings

---

## 1. Key Bindings

### HIGH-01: No discoverability for Glass-specific shortcuts
- **Description**: Glass has ~25 unique shortcuts (Ctrl+Shift+Z for undo, Ctrl+Shift+P for pipe view, Ctrl+Shift+F for search, etc.) but there is zero indication these exist when a user first opens the terminal. No tooltip, no status bar hint, no first-run prompt.
- **Current behavior**: The user sees a standard-looking terminal. All Glass features are invisible until the user discovers Ctrl+Shift+, (Settings > Shortcuts tab) or reads external documentation.
- **User impact**: Users coming from other terminals will never discover Glass's differentiating features. The entire value proposition is hidden.
- **Recommendation**: Add a subtle first-run hint in the status bar (e.g., "Press Ctrl+Shift+, for settings & shortcuts") that auto-dismisses after 3 sessions. Alternatively, show the Shortcuts overlay on first launch.

### MEDIUM-02: Ctrl+Shift+C/V may conflict with terminal programs
- **Description**: Copy/paste is bound to Ctrl+Shift+C and Ctrl+Shift+V. While this is standard for Linux terminals (GNOME Terminal, Konsole), users coming from Windows Terminal (which also supports Ctrl+C/V when no selection) or macOS (Cmd+C/V) may find this unfamiliar.
- **Current behavior**: Copy = Ctrl+Shift+C, Paste = Ctrl+Shift+V. On macOS, the code uses Cmd instead (via `is_glass_shortcut`). This is correct but not documented in-app.
- **User impact**: Windows users may try Ctrl+C to copy a selection and accidentally send SIGINT. macOS mapping looks correct.
- **Recommendation**: Consider supporting Ctrl+C for copy when there is an active selection (like Windows Terminal does), falling through to SIGINT only when nothing is selected. Document the platform-specific behavior.

### MEDIUM-03: Ctrl+Shift+D/E for splits is non-standard
- **Description**: Horizontal split is Ctrl+Shift+D, vertical split is Ctrl+Shift+E. Most terminal multiplexers use different conventions: iTerm2 uses Cmd+D/Cmd+Shift+D, tmux uses Ctrl+B %/", Windows Terminal uses Alt+Shift+Plus/Minus.
- **Current behavior**: The D/E choice is arbitrary and undocumented in the UI except in the Shortcuts overlay.
- **User impact**: Users will need to learn a new muscle memory for splits. The "D" and "E" mnemonics are not intuitive (what does "E" stand for?).
- **Recommendation**: Keep the bindings but add tooltips or hints. Consider making "D" = split Down (vertical) and "H" = split Horizontal for better mnemonics, or match iTerm2/Windows Terminal conventions. At minimum, document the rationale.

### MEDIUM-04: Alt+Arrow pane focus conflicts with readline
- **Description**: Alt+Arrow is used for moving focus between split panes. However, Alt+Left/Right is used in bash/zsh readline for word navigation (move cursor forward/backward by word).
- **Current behavior**: Alt+Arrow only triggers pane focus when `active_tab_pane_count() > 1`, so single-pane users are unaffected. But once a user creates splits, they lose Alt+Arrow word navigation in all panes.
- **User impact**: Developers who habitually use Alt+Left/Right for word jumping will be frustrated when splits are active. This is a common and important editing shortcut.
- **Recommendation**: Use a different modifier combination for pane focus (e.g., Ctrl+Alt+Arrow or a configurable key). Or only intercept Alt+Arrow when the terminal is in non-application mode.

### LOW-05: Ctrl+1-9 tab jumping overlaps with Ctrl+digit in some shells
- **Description**: Ctrl+1 through Ctrl+9 jumps to tab N. Some terminal programs (e.g., screen, certain TUI apps) use Ctrl+digit for their own purposes.
- **Current behavior**: These shortcuts use `is_action_modifier(modifiers)` which is just Ctrl on Win/Linux. They are consumed before being forwarded to the PTY.
- **User impact**: Minimal -- most shells don't use Ctrl+digit. But it is different from the convention of using Ctrl+Shift+1-9 (which would be more consistent with the other Glass shortcuts).
- **Recommendation**: Consider using Ctrl+Shift+1-9 for consistency, or make this configurable.

### LOW-06: No user-configurable key bindings
- **Description**: All key bindings are hardcoded in `src/main.rs`. There is no `[keybindings]` section in config.toml.
- **Current behavior**: The settings overlay can change config values for features but not key bindings.
- **User impact**: Power users cannot remap shortcuts. Users with non-US keyboard layouts may find some shortcuts awkward or impossible to type.
- **Recommendation**: Add a `[keybindings]` config section for at least the most common shortcuts. This is a standard feature in Alacritty, Kitty, and Windows Terminal.

---

## 2. Visual Design

### MEDIUM-07: Dark-only color scheme with no theme support
- **Description**: The terminal background is hardcoded to `Rgb { r: 26, g: 26, b: 26 }` (near-black). Tab bar, status bar, block separators, and overlays all use hardcoded dark colors. There is no light theme or custom theme support.
- **Current behavior**: `default_bg` is set in `FrameRenderer::new()` to `(26, 26, 26)`. Status bar background is `(38, 38, 38)`. Tab bar is `(30, 30, 30)`. All hardcoded constants.
- **User impact**: Users who prefer light themes or custom color schemes (common with Alacritty, Kitty users) cannot adapt Glass to their preferences. The hardcoded values also mean Glass ignores terminal color scheme OSC sequences for the chrome.
- **Recommendation**: Extract all chrome colors into a theme structure loaded from config.toml. Ship with at least dark and light presets. The terminal grid already respects ANSI colors from the shell; only the chrome needs theming.

### MEDIUM-08: Tab bar has no overflow handling
- **Description**: When many tabs are open, tab width clamps to `MIN_TAB_WIDTH` (60px) but the total tab width can exceed the viewport. Tests explicitly confirm that 50 tabs at 60px overflow 1920px.
- **Current behavior**: Tabs render past the right edge of the viewport. There is no scroll, no collapse, no overflow indicator. The `+` new tab button moves offscreen.
- **User impact**: Users with many tabs cannot see all tabs and lose access to the `+` button. This breaks usability for heavy tab users.
- **Recommendation**: Implement tab bar scrolling (left/right arrows at edges) or a tab overflow dropdown. Alternatively, shrink tab labels more aggressively or show a tab count badge.

### LOW-09: Status bar information density varies wildly
- **Description**: The status bar shows CWD (left), git branch+dirty (right), and optionally: update notification (center), coordination text, agent cost, agent mode, proposal count. When agent features are active, the bar becomes very crowded.
- **Current behavior**: Up to 7 different text segments compete for space in a single line. A two-line mode exists when agents are active.
- **User impact**: In non-agent mode, the status bar is clean and useful (CWD + git). In agent mode, it becomes dense and hard to parse quickly.
- **Recommendation**: Prioritize information by truncating or hiding less important items. Consider icons or color-coded indicators instead of text for agent status.

### LOW-10: Block separator lines are very subtle
- **Description**: Command block separators are 1px tall, `(60, 60, 60)` gray -- barely distinguishable from the `(26, 26, 26)` background.
- **Current behavior**: Separator lines are intentionally subtle but may be invisible on some displays, especially with low contrast settings.
- **User impact**: Users may not notice command boundaries, defeating the purpose of the block UI.
- **Recommendation**: Make separator thickness/color configurable. A 2px line at `(80, 80, 80)` would be more visible while remaining unobtrusive.

### LOW-11: No padding between terminal content and window edges
- **Description**: Terminal cells start at `x=0, y=tab_bar_height`. There is no inner padding (gutter) between the terminal grid and the window borders.
- **Current behavior**: Text renders edge-to-edge horizontally. The only vertical spacing comes from the tab bar (top) and status bar (bottom).
- **User impact**: Text feels cramped against window edges, especially on high-DPI displays. Alacritty and Kitty both support configurable padding.
- **Recommendation**: Add configurable `padding.x` and `padding.y` in config.toml (default 4-8px) to give terminal content breathing room.

---

## 3. Tab/Pane Management

### MEDIUM-12: No visual indicator of active tab beyond color
- **Description**: Active tab is `(50, 50, 50)`, inactive is `(35, 35, 35)`. The color difference is subtle -- only 15 units apart on a 0-255 scale.
- **Current behavior**: No underline, no bold text, no accent color for the active tab. Only a slight brightness difference.
- **User impact**: With many tabs, it is hard to quickly identify which tab is active, especially in peripheral vision.
- **Recommendation**: Add an accent color underline (2px colored bar) on the active tab, or use a more distinct background color. iTerm2 uses a colored tab indicator; Windows Terminal uses a colored accent line.

### MEDIUM-13: No tab title auto-update from shell CWD
- **Description**: Tab titles appear to be set at creation time. The `TabDisplayInfo.title` comes from the session, but it is unclear if it dynamically updates from OSC 2 (window title) or the current working directory.
- **Current behavior**: The title is pulled from `session` state at render time. If the shell emits OSC 2 sequences, these should update the title. However, the shell integration scripts only emit OSC 133 (command boundaries) and OSC 7 (CWD), not OSC 2.
- **User impact**: Tab titles may become stale, showing the initial directory rather than the current working directory.
- **Recommendation**: Auto-set tab title from the session's current working directory (last path component). Allow OSC 2 overrides. Show the command being executed in the title when a command is running.

### LOW-14: No tab context menu (right-click)
- **Description**: Tabs support click, close button, and drag-reorder. There is no right-click context menu for rename, duplicate, move to new window, etc.
- **Current behavior**: Right-clicking on tabs does nothing special.
- **User impact**: Minor for launch. Context menus are expected in modern terminals but not critical.
- **Recommendation**: Post-launch feature. Add right-click menu with: Rename, Duplicate, Close Other Tabs, Move to New Window.

### LOW-15: Pane split depth limit is documented in code only
- **Description**: `MAX_SPLIT_DEPTH` is 8, defined in `crates/glass_mux/src/split_tree.rs`. When reached, `split_pane` returns `None` and the key press is silently ignored.
- **Current behavior**: No feedback to the user when max split depth is reached. The shortcut simply does nothing.
- **User impact**: Users may think the split shortcut is broken if they hit the limit.
- **Recommendation**: Show a brief toast or status bar message: "Maximum split depth reached."

---

## 4. Search Overlay

### MEDIUM-16: Search only queries history database, not current terminal buffer
- **Description**: The search overlay (`Ctrl+Shift+F`) searches the history database (SQLite FTS5 on past commands), not the current terminal scrollback buffer. There is no way to search visible terminal text.
- **Current behavior**: `SearchOverlay` queries `CommandRecord` entries from `HistoryDb`. It shows command text, exit code, timestamp, and output preview. Enter jumps to the matched block. This is useful but different from what users expect from "terminal search."
- **User impact**: Users pressing Ctrl+Shift+F expect to search the current terminal output (like Ctrl+Shift+F in iTerm2 or Ctrl+F in Windows Terminal). Finding it searches a command database instead of visible text is confusing.
- **Recommendation**: Rename this to "Search History" in the overlay header (currently shows "Search: "). Add a separate Ctrl+F (or Ctrl+Shift+/) for scrollback text search. Both are valuable but serve different use cases.

### LOW-17: No cursor blinking in search input
- **Description**: The search overlay shows "Search: {query}" as static text with no visible cursor. There is no blinking cursor or caret indicator.
- **Current behavior**: Text is appended character by character but the user has no visual indication of the insertion point. The cursor is always at the end (no left/right navigation within the query).
- **User impact**: Minor -- the search input is simple enough that cursor position is obvious. But it feels less polished than expected.
- **Recommendation**: Add a blinking pipe character at the end of the query text, or an underscore cursor.

---

## 5. Status Bar / Tab Bar

### HIGH-18: Status bar CWD truncation is too aggressive
- **Description**: CWD is truncated to 60 characters with `...` prefix. On a 1920px-wide terminal, this wastes significant horizontal space.
- **Current behavior**: `if cwd.len() > 60 { format!("...{}", &cwd[cwd.len()-57..]) }`. The 60-char limit is hardcoded regardless of viewport width.
- **User impact**: Users with deep directory structures see truncated paths even when there is plenty of horizontal space available.
- **Recommendation**: Calculate available width dynamically based on viewport width minus the right-side elements (git info, agent cost, etc.). Only truncate when necessary.

---

## 6. Command Blocks

### HIGH-19: No visual cue for executing commands
- **Description**: Blocks transition through states: PromptActive -> InputActive -> Executing -> Complete. The block renderer only shows visual decorations (separator, badge, duration) for Complete blocks with exit codes. Executing blocks have no visual indicator.
- **Current behavior**: While a command is running, there is no spinner, progress indicator, elapsed timer, or color change. The block appears identical to idle state until it completes.
- **User impact**: Users cannot visually distinguish between "command still running" and "command finished with no output." This is especially problematic for long-running commands.
- **Recommendation**: Add a pulsing or animated indicator on the block separator for Executing state. Show a live elapsed timer. Even a simple "running..." label would help.

### MEDIUM-20: Exit badge text "OK" and "X" lacks context
- **Description**: Successful commands show a green badge with "OK", failed commands show a red badge with "X". The actual exit code is not shown for failures.
- **Current behavior**: `build_block_text` returns `"OK"` for exit_code==0 and `"X"` for non-zero. The numeric exit code is only visible in search results (as "X:127").
- **User impact**: Users cannot see the actual exit code (e.g., 127 = command not found, 137 = killed by signal) without searching history. This is important debugging information.
- **Recommendation**: Show the exit code number in the badge for non-zero: e.g., "X:1" or "E:127". The badge width (3 cells = ~24px) may need expanding to fit longer codes.

---

## 7. Pipe Visualization

### HIGH-21: Pipeline panel has no close/dismiss button
- **Description**: The pipeline panel opens when toggling `Ctrl+Shift+P` and must be dismissed with the same shortcut. There is no X button, no Escape to close, no visual hint about how to dismiss it.
- **Current behavior**: `Ctrl+Shift+P` toggles `pipeline_expanded` on the most recent pipeline block. The panel renders at the bottom of the viewport above the status bar. It persists until toggled off.
- **User impact**: Users who accidentally open the pipeline panel (or open it on purpose and forget the shortcut) have no obvious way to close it. There is no close hint in the panel UI.
- **Recommendation**: Add an "Esc to close" hint in the panel header. Also support Escape key to dismiss the pipeline panel (currently Escape only closes search overlay and settings).

### MEDIUM-22: Pipeline stage expand/collapse is keyboard-only
- **Description**: Pipeline stages show [+]/[-] indicators but clicking them does nothing. Expansion uses `expanded_stage_index` which appears to only change via the toggle shortcut.
- **Current behavior**: The pipeline panel shows stage rows with [+] indicators. There are no mouse click handlers for the pipeline panel; only `Ctrl+Shift+P` toggles overall expansion.
- **User impact**: Users see [+] indicators suggesting they are clickable, but they are not. This creates a false affordance.
- **Recommendation**: Add click handlers for pipeline stage rows to expand/collapse individual stages. Add mouse interaction for the [+]/[-] indicators.

---

## 8. Undo Feature

### HIGH-23: Undo feedback is log-only with no visual indication
- **Description**: When `Ctrl+Shift+Z` undoes a command, the results are logged via `tracing::info!` and `tracing::error!`. The only visual feedback is removing the `[undo]` label from the undone block.
- **Current behavior**: The undo operation outputs results to the tracing log (not the terminal). The user sees the `[undo]` label disappear but gets no summary of what files were restored, deleted, or had conflicts. If nothing is undoable, a log message says "Nothing to undo" but the user sees nothing.
- **User impact**: Users have no idea what the undo actually did. Did it restore 1 file or 50? Were there conflicts? They must check the tracing log (if they even know it exists) to find out.
- **Recommendation**: Inject a visible summary into the terminal via PTY (similar to how orchestrator messages are shown). Example: `[GLASS] Undo: 3 files restored, 1 conflict (see ~/.glass/undo.log)`. Show a brief toast or status bar message for "nothing to undo."

### MEDIUM-24: [undo] label positioning overlaps terminal content
- **Description**: The `[undo]` label is rendered as a floating overlay label positioned to the left of the duration text and exit badge. It uses an opaque background rect to cover terminal content underneath.
- **Current behavior**: The `decoration_cluster_width` calculation reserves space for badge + duration + [undo]. An opaque background rect at `(26, 26, 26)` covers terminal content beneath the label cluster.
- **User impact**: If command output happens to extend to the right edge of the terminal, the opaque background will hide a few characters of actual output. This is a tradeoff for readability but may confuse users.
- **Recommendation**: This is acceptable for now but should be noted in documentation. Consider making the [undo] label only appear on hover, or positioning it on a separate visual line.

---

## 9. History Query

### MEDIUM-25: CLI history query is powerful but UI integration is minimal
- **Description**: The `glass history search` and `glass history list` CLI commands provide rich filtering (--exit, --after, --before, --cwd, --limit) but the in-app search overlay only supports basic text search.
- **Current behavior**: The search overlay does a FTS5 query on the history database with the typed query string. There is no way to filter by exit code, time range, or CWD from the overlay.
- **User impact**: Power users must exit the overlay and use the CLI for advanced queries. The overlay feels like a minimal subset of the available functionality.
- **Recommendation**: Add filter toggles in the search overlay: e.g., prefix query with `exit:0` or `after:1h` for inline filtering. Or add filter buttons/tabs.

---

## 10. Error/Feedback States

### MEDIUM-26: Config error overlay is display-only with no guidance
- **Description**: When config.toml has a parse error, a dark red banner appears at the top of the viewport showing the error message with line/column info. The user cannot interact with it.
- **Current behavior**: Banner shows `"Config error (line X, col Y): message"`. It persists until the config file is fixed (hot-reload via file watcher). There is no button to open the config file, no suggestion on how to fix it.
- **User impact**: Helpful for identifying the error, but users need guidance on where the config file is and how to edit it. New users may not know the path is `~/.glass/config.toml`.
- **Recommendation**: Add the config file path to the error message: `"Config error in ~/.glass/config.toml (line 3, col 5): expected string"`. Consider adding a "Press Ctrl+Shift+, to open settings" hint.

### LOW-27: No loading state for agent operations
- **Description**: Agent proposals, worktree operations, and orchestrator actions have no loading indicators. When a proposal is being applied or a worktree is being created, there is no spinner or progress indication.
- **Current behavior**: Operations are either instant (file operations) or happen in background threads with eventual event delivery. The UI does not show intermediate states.
- **User impact**: Minor -- most operations are fast. But worktree apply/dismiss could take noticeable time on large repos.
- **Recommendation**: Add a brief loading indicator or status bar text ("Applying proposal...") for operations that may take more than 100ms.

---

## 11. Resize Behavior

### LOW-28: Background tabs are resized with full-window dimensions
- **Description**: When the window is resized, background tabs are resized assuming full-window dimensions rather than their actual split-tree layout (since they are not visible).
- **Current behavior**: `src/main.rs` line 3714: background tabs get `num_cols` and `num_lines` based on full window dimensions. When switched to, if they have splits, these dimensions may be wrong until the next resize event.
- **User impact**: Background tabs with split panes may briefly show incorrect sizing when activated after a resize. The next redraw should fix it.
- **Recommendation**: Re-compute split layouts for the newly activated tab when switching tabs, or defer background tab resize until activation.

---

## 12. Scrollback

### MEDIUM-29: Scrollback only works with Shift+PageUp/Down and mouse wheel
- **Description**: Scrollback navigation uses Shift+PageUp/Down for keyboard and raw mouse wheel for trackpad/mouse. There is no Shift+Up/Down for line-by-line scrolling.
- **Current behavior**: `Shift+PageUp` = page up, `Shift+PageDown` = page down. Mouse wheel scrolls by line delta. No line-by-line keyboard scrolling.
- **User impact**: Users who want to scroll up by one or two lines must use the mouse wheel or scrollbar. There is no keyboard equivalent for fine-grained scrollback. iTerm2, Alacritty, and Kitty all support Shift+Arrow for line-by-line scrolling.
- **Recommendation**: Add Shift+Up/Down for single-line scrollback. This is a very common expectation.

---

## 13. First-Time User Experience

### HIGH-24b: No onboarding, no help, no feature discovery
- **Description**: Glass launches as a plain dark terminal. There is no welcome message, no feature tour, no help command, no README integration. The only way to discover features is to accidentally press Ctrl+Shift+, or read external documentation.
- **Current behavior**: `glass` launches directly into the shell with no introduction. The `--help` flag shows CLI subcommands (history, undo, mcp, profile) but not the interactive features.
- **User impact**: This is the single biggest UX risk for launch. Users will try Glass, see a dark terminal, not discover any features, and leave thinking "it's just another terminal emulator."
- **Recommendation** (multi-step):
  1. **First-run**: On first launch (detect via `~/.glass/config.toml` absence), show a brief overlay: "Welcome to Glass. Press Ctrl+Shift+, for settings. Press Ctrl+Shift+F to search history."
  2. **Status bar hint**: Show "Ctrl+Shift+, = settings" in the status bar for the first 5 sessions.
  3. **`glass --features` command**: Add a CLI command that lists all features with their shortcuts.
  4. **About tab**: The About tab already exists with version/platform info. Add a "Key Features" section with brief descriptions.

---

## Priority Fix List

### Must-fix before launch

| # | Severity | Finding | Effort |
|---|----------|---------|--------|
| 1 | HIGH | HIGH-01: No feature discoverability -- add first-run hint and status bar hint | Small |
| 2 | HIGH | HIGH-24b: No onboarding -- at minimum, show settings shortcut on first launch | Small |
| 3 | HIGH | HIGH-23: Undo feedback is invisible -- inject summary into terminal | Medium |
| 4 | HIGH | HIGH-19: No visual cue for executing commands -- add running indicator | Medium |
| 5 | HIGH | HIGH-21: Pipeline panel has no dismiss hint -- add "Esc to close" | Small |
| 6 | HIGH | HIGH-18: CWD truncation too aggressive -- make dynamic | Small |

### Should-fix before launch

| # | Severity | Finding | Effort |
|---|----------|---------|--------|
| 7 | MEDIUM | MEDIUM-16: Search overlay should be labeled "Search History" | Trivial |
| 8 | MEDIUM | MEDIUM-04: Alt+Arrow conflicts with readline word nav | Medium |
| 9 | MEDIUM | MEDIUM-08: Tab bar overflow not handled | Medium |
| 10 | MEDIUM | MEDIUM-12: Active tab needs stronger visual indicator | Small |
| 11 | MEDIUM | MEDIUM-20: Show exit code in badge | Small |
| 12 | MEDIUM | MEDIUM-29: No Shift+Arrow for line-by-line scrolling | Small |
| 13 | MEDIUM | MEDIUM-07: No theme/color customization | Large |
| 14 | MEDIUM | MEDIUM-26: Config error needs file path in message | Trivial |

### Nice-to-have for launch

| # | Severity | Finding | Effort |
|---|----------|---------|--------|
| 15 | MEDIUM | MEDIUM-02: Support Ctrl+C for copy with active selection | Medium |
| 16 | MEDIUM | MEDIUM-03: Document D/E split mnemonic | Trivial |
| 17 | MEDIUM | MEDIUM-13: Tab title from CWD | Small |
| 18 | MEDIUM | MEDIUM-22: Pipeline stage click-to-expand | Medium |
| 19 | MEDIUM | MEDIUM-25: Search overlay advanced filters | Large |
| 20 | LOW | LOW-06: Configurable key bindings | Large |
| 21 | LOW | LOW-09: Status bar density management | Medium |
| 22 | LOW | LOW-10: Block separator visibility | Trivial |
| 23 | LOW | LOW-11: Terminal content padding | Small |
| 24 | LOW | LOW-14: Tab context menu | Medium |
| 25 | LOW | LOW-15: Max split depth user feedback | Trivial |

---

## Positive Observations

These aspects are well-implemented and should be highlighted in launch materials:

1. **Settings overlay** (Ctrl+Shift+,): Three-tab layout with Settings, Shortcuts, About is well-designed. Navigation hints in the footer are excellent. The sidebar + fields layout is intuitive. Live config editing that writes to config.toml is a great feature.

2. **Shell integration**: OSC 133 support across bash, zsh, fish, and PowerShell is thorough. The auto-injection approach (via PTY spawner) means zero setup for users.

3. **Scrollbar**: Proportional thumb, hover/drag states, smooth dragging, min-thumb-height guarantee. The 8px width is unobtrusive but functional.

4. **Selection**: Blue highlight (semi-transparent), copy-on-release, multi-line support, wide-char handling. Standard and correct.

5. **Cursor rendering**: Block, beam, underline, hollow-block shapes are all implemented. Wide character handling is correct.

6. **Config hot-reload**: File watcher on config.toml provides instant feedback for manual edits. Combined with the settings overlay for GUI editing, this covers both power users and casual users.

7. **Drag-and-drop**: Tab reorder via drag, file drop into PTY with path quoting for spaces. Nice attention to detail.

8. **Platform awareness**: macOS Cmd vs Win/Linux Ctrl+Shift handling via `is_glass_shortcut()`. Default shell detection (pwsh/powershell on Windows). Config/data directory per-platform.

9. **Error handling in config**: Structured `ConfigError` with line/column info and persistent banner display. The overlay does not block terminal use.

10. **Focused pane border**: Cornflower blue (100, 149, 237) accent border on the focused pane in multi-pane layouts. Subtle but effective.
