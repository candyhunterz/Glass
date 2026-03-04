# Feature Research

**Domain:** GPU-accelerated terminal emulator (Rust, Windows-first, block-based UI)
**Researched:** 2026-03-04
**Confidence:** HIGH (verified against Warp, WezTerm, Ghostty, Alacritty official docs and changelogs)
**Milestone Scope:** Phase 0-1 — foundation terminal with block UI and shell integration

---

## Feature Landscape

### Table Stakes (Users Expect These)

Features that are invisible when present but immediately disqualifying when absent. Missing any of these means users cannot do their job.

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| VT/ANSI escape sequence processing | Every CLI tool — git, vim, ls, compilers — emits escape codes. Without them, output is garbage. | HIGH | alacritty_terminal handles this. Core dependency for everything else. |
| Truecolor (24-bit RGB) support | Modern shells, neovim, bat, delta, and most CLI tools emit 24-bit color. 256-color is the floor. | MEDIUM | Must set `COLORTERM=truecolor` in env. alacritty_terminal provides this natively. |
| Keyboard input forwarding | Ctrl+C, Ctrl+D, arrow keys, modifier combinations — any mismatch breaks vim/emacs/fzf immediately. | MEDIUM | Includes Ctrl, Alt, Shift modifiers. Kitty keyboard protocol is a plus but not required for M1. |
| Bracketed paste mode | Without it, pasting multi-line code or scripts triggers immediate execution — data loss risk. | LOW | Protocol: `ESC[?2004h`. alacritty_terminal handles. Must be wired up in input pipeline. |
| Scrollback buffer | Every user scrolls up to read output. Absence is the most complained-about missing feature. | MEDIUM | Need configurable line limit. 10,000 lines is a reasonable default. |
| Copy/paste (keyboard) | Ctrl+Shift+C / Ctrl+Shift+V is universal convention on Windows. Absence is immediately felt. | LOW | Clipboard integration via winit/arboard. Primary selection not expected on Windows. |
| Font configuration (family + size) | Monospace font preference is personal. Fixed font is a dealbreaker for most developers. | LOW | TOML config: `font.family`, `font.size`. Hot reload is a quality-of-life addition. |
| Correct cursor rendering | Cursor must be visible, blink optional, and move correctly. Cursor bugs make editing unusable. | MEDIUM | Block, beam, underline shapes. Blink controlled by escape sequences. |
| Window resize handling | Resizing the window must reflow terminal content — every program assumes this works. | LOW | PTY `SIGWINCH` equivalent on Windows (ConPTY handles resize notification). |
| Working directory inheritance | Terminal must launch shell in a useful CWD (home or configurable). | LOW | Standard PTY setup, set `cwd` in spawn config. |
| Exit code visibility | Knowing whether the last command succeeded or failed. Block UI makes this prominent. | MEDIUM | Requires OSC 133 shell integration (D sequence carries exit code). |
| Non-broken shell launching | Shell must start, prompt must appear, typing must work. The baseline functional test. | HIGH | ConPTY + alacritty_terminal on Windows. Hardest platform-specific problem in M1. |

### Differentiators (Competitive Advantage for Milestone 1)

Features that distinguish Glass's foundation from a bare-bones terminal. These should be buildable within M1 scope without depending on Phase 2+ features.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Block-based command output | Each command's output is visually grouped with its prompt, exit code badge, and duration. Users can see at a glance what ran and whether it succeeded. Warp proved the paradigm — users say it "changes how you think about terminal history." | HIGH | Core Glass differentiator. Requires OSC 133 shell integration for semantic boundaries. Blocks are the visible payoff of shell integration work. |
| Exit code badge per block | Red/green visual indicator directly on the block header. Removes need to `echo $?` constantly. | LOW | Dependent on: shell integration (OSC 133;D). Zero extra work once blocks work. |
| Command duration per block | Shows wall-clock time for each command inline. Immediately useful for build times, test runs. | LOW | Dependent on: shell integration start/end events. Timestamp delta between OSC 133;C and OSC 133;D. |
| Shell integration (OSC 133 + OSC 7) | Semantic prompt marking, command boundary detection, CWD tracking. Enables blocks, exit codes, duration, and future features. WezTerm and Warp both use this. | HIGH | Needs PowerShell integration script (custom `$PROMPT` function) and bash script (via `PROMPT_COMMAND`/`precmd`). Standard OSC 7 for CWD, OSC 133;A/B/C/D for command lifecycle. |
| Status bar (CWD + git branch) | Always-visible context strip showing where you are and what branch you're on. Eliminates `pwd` and `git branch` invocations. Warp, iTerm2, and WezTerm all ship variants of this. | MEDIUM | CWD from OSC 7 events. Git branch via `git rev-parse` subprocess or parsing `.git/HEAD` directly. Git dirty count needs `git status --porcelain` — can be async. |
| GPU-accelerated rendering | Visibly smoother scrolling and text under load vs. non-accelerated terminals. Performance under large output (e.g., `cargo build`) matters. Ghostty and Alacritty both demonstrate this. | HIGH | wgpu + custom glyph atlas. This is the rendering architecture, not an add-on. |
| Collapsible blocks (deferred to M1.x) | Long outputs (test runs, build logs) become one-liner headers. Users reclaim screen real estate. | HIGH | Architecture must support it from day one — collapsing is a rendering mode change, not a rearchitect. Flag for phase-specific research. |

### Anti-Features (Do Not Build in Milestone 1)

Features that look like reasonable scope expansions but would compromise delivery or are explicitly out of scope.

| Feature | Why Requested | Why Problematic in M1 | What to Do Instead |
|---------|---------------|----------------------|-------------------|
| Tabs and split panes | Every popular terminal has them. Users ask immediately. | Requires multiplexer architecture — non-trivial window management, layout engine, PTY-per-pane. Easily doubles M1 scope. Blocks can span the full window first. | Defer to a dedicated milestone. Users accept "use multiple windows" while blocks are novel enough to be interesting. |
| Built-in AI features | Warp 2.0 shows AI integration. Developers associate modern terminals with AI. | Glass's value is as an MCP data source, not an AI chat interface. Premature AI locks in UI/UX patterns before the data layer (Phase 2) exists. | MCP server in Phase 2 exposes history to external AI tools. Do not embed chat. |
| Plugin/extension system | Power users want to extend everything. | API surface before core is stable leads to breaking changes and maintaining compatibility. Core must be solid first (PROJECT.md explicitly says so). | Stabilize internals first. Plugin system is a v2+ concern. |
| Cloud sync / remote config | Users want settings on multiple machines. | History and snapshots stay local (by design). Cloud sync requires auth, encryption, conflict resolution — massive scope. | Document config file location clearly. Manual dotfile management is acceptable for M1. |
| cmd.exe support | Windows users expect cmd.exe to work. | ConPTY works with cmd.exe but shell integration (OSC 133) requires shell-side scripting. cmd.exe has no equivalent hook mechanism. | PowerShell and bash (WSL or Git Bash) only for M1. Explicitly documented. |
| Font ligature support | FiraCode, Cascadia Code — ligatures are expected by many developers. | Requires grapheme-aware glyph shaping pipeline (HarfBuzz or equivalent). Alacritty famously omitted this for years due to complexity. Ghostty uses platform shaping. | Basic glyph rendering works for all code. Flag ligatures as M2+ rendering enhancement. |
| Theme marketplace / custom color schemes | Developers love theming terminals. | Config format not stable in M1. Theming before rendering pipeline is solid adds churn. | Ship one dark theme, one light theme (PROJECT.md). Add color scheme config in M2. |
| Ctrl+Z undo | Users will press Ctrl+Z expecting command undo (the core Glass feature). | Phase 3 feature. Undo doesn't exist yet — wiring a broken stub confuses users. | Pass Ctrl+Z through to shell normally (suspend process, standard Unix behavior). Document intentionally. |
| Image / graphics protocol (Sixel, Kitty) | Ghostty ships with Kitty graphics. Modern TUIs render images. | Image protocol requires separate rendering layer. No CLI workflows during M1 depend on images. | VTE escape pass-through means tools won't crash, they just won't render images. Add in M2+. |
| Search within scrollback | Finding text in long outputs is essential long-term. | Requires text indexing on the scrollback buffer, regex matching UI, and highlight rendering. Non-trivial to do well. | Command-level block structure mitigates scrollback searching — you can identify which block's output to read. Defer. |
| Right-click context menu | GUI users expect right-click to copy/paste/open-URL. | Custom context menu requires OS integration (Windows HMENU or equivalent). Low ROI for developer audience using keyboard shortcuts. | Ctrl+Shift+C/V covers copy/paste. URL opening via Ctrl+click is acceptable M1 UX. |

---

## Feature Dependencies

```
[PTY spawn / ConPTY]
    └──required by──> [Keyboard input forwarding]
    └──required by──> [VT/ANSI processing]
    └──required by──> [Shell launch]

[VT/ANSI processing]
    └──required by──> [Truecolor rendering]
    └──required by──> [Cursor rendering]
    └──required by──> [Bracketed paste]
    └──required by──> [Scrollback buffer]

[Shell integration (OSC 133 + OSC 7)]
    └──required by──> [Block-based command output]
    └──required by──> [Exit code badge]
    └──required by──> [Command duration display]
    └──required by──> [Status bar CWD tracking]
    └──enables (future)──> [Collapsible blocks]
    └──enables (future)──> [Structured history DB (Phase 2)]
    └──enables (future)──> [Command-level undo (Phase 3)]

[Block-based command output]
    └──enhances──> [Exit code badge]
    └──enhances──> [Command duration display]
    └──required by (future)──> [Collapsible blocks]

[GPU rendering pipeline (wgpu)]
    └──required by──> [All visual output]
    └──required by (future)──> [Collapsible block animation]
    └──required by (future)──> [Image protocol]

[Font loading / glyph atlas]
    └──required by──> [GPU rendering pipeline]
    └──required by (future)──> [Font ligature support]

[Status bar]
    └──depends on──> [Shell integration (OSC 7 for CWD)]
    └──depends on──> [Git subprocess (branch / dirty count)]
```

### Dependency Notes

- **Shell integration must precede blocks:** There is no reliable way to detect command boundaries without shell cooperation. Regex-matching the prompt is fragile and breaks on custom prompts. OSC 133 is the right path — Warp, WezTerm, and VS Code all use it.
- **PTY spawn is the hardest Windows-specific problem:** ConPTY is the only supported mechanism on Windows. alacritty_terminal's ConPTY support must be verified before committing to the embedding approach. This is the single highest-risk dependency in M1.
- **Block rendering does not require collapsibility:** Block grouping (visual separation, headers, badges) can ship without collapse/expand. Collapsibility is additive — architecture just needs to not preclude it.
- **Git status in status bar must be async:** Synchronous `git status` blocks rendering on large repos. Run as a background subprocess with a brief staleness window. WezTerm's status bar uses event-driven refresh.

---

## MVP Definition

### Launch With (Milestone 1 — Daily Drivable)

The bar is: "Can a developer use Glass as their primary terminal while building Glass?" Every item below is required to meet that bar.

- [ ] ConPTY PTY spawn with PowerShell as default shell — without this, nothing works
- [ ] Full VT/ANSI escape processing via alacritty_terminal — required for any real CLI tool
- [ ] Truecolor rendering — required for modern tooling (delta, bat, neovim themes)
- [ ] Keyboard input with modifier keys — Ctrl, Alt, Shift combinations; vim/tmux/fzf require this
- [ ] Bracketed paste mode — safe multi-line paste without accidental execution
- [ ] Scrollback buffer — at least 10,000 lines; configurable
- [ ] Copy/paste (Ctrl+Shift+C / Ctrl+Shift+V) — clipboard integration
- [ ] Shell integration scripts for PowerShell and bash — OSC 133 + OSC 7
- [ ] Block-based command output — prompt + output + exit code + duration per block
- [ ] Status bar — CWD (from OSC 7) + git branch (async subprocess)
- [ ] TOML config — `font.family`, `font.size`, `shell` override
- [ ] GPU-accelerated rendering via wgpu — <5ms input latency, <200ms cold start
- [ ] Window resize handling — PTY resize on window dimension change

### Add After Validation (Milestone 1.x — Quality Polish)

Once M1 is daily-drivable, these meaningfully improve the experience:

- [ ] Config hot reload — saves restart cycles while tweaking config
- [ ] Collapsible blocks — fold long output to one-line header; high UX value, architecture must support
- [ ] URL detection and Ctrl+click to open — expected by developers; moderate complexity
- [ ] Bash shell integration (WSL / Git Bash) — expands shell support; PowerShell is the M1 priority
- [ ] Block keyboard navigation — jump to previous/next block with keyboard shortcut

### Future Consideration (Phase 2+)

Deferred by design per PROJECT.md. Do not attempt in M1.

- [ ] Structured history DB (Phase 2) — queryable scrollback; requires block data pipeline first
- [ ] MCP server (Phase 2) — exposes history to AI tools; requires history DB
- [ ] Command-level undo (Phase 3) — Glass's flagship feature; requires filesystem snapshotting
- [ ] Pipe visualization (Phase 4) — visual pipe debugging; requires command parser
- [ ] Tabs and split panes — multiplexer architecture; separate milestone
- [ ] Font ligatures — HarfBuzz shaping pipeline; rendering enhancement milestone
- [ ] Image protocols (Kitty, Sixel) — separate rendering layer
- [ ] Searchable scrollback UI — block structure partially mitigates this need

---

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| PTY spawn (ConPTY) | HIGH | HIGH | P1 |
| VT/ANSI processing (alacritty_terminal) | HIGH | MEDIUM (embedding) | P1 |
| Keyboard input forwarding | HIGH | MEDIUM | P1 |
| GPU rendering pipeline (wgpu) | HIGH | HIGH | P1 |
| Shell integration (OSC 133 + OSC 7) | HIGH | HIGH | P1 |
| Block-based command output | HIGH | HIGH | P1 |
| Truecolor rendering | HIGH | LOW (after VT/ANSI) | P1 |
| Scrollback buffer | HIGH | MEDIUM | P1 |
| Copy/paste | HIGH | LOW | P1 |
| Bracketed paste | HIGH | LOW | P1 |
| TOML config (font, size, shell) | MEDIUM | LOW | P1 |
| Cursor rendering | HIGH | MEDIUM | P1 |
| Window resize | HIGH | LOW | P1 |
| Status bar (CWD + git) | MEDIUM | MEDIUM | P1 |
| Exit code badge | HIGH | LOW (after blocks) | P1 |
| Command duration | MEDIUM | LOW (after blocks) | P1 |
| Config hot reload | MEDIUM | LOW | P2 |
| Collapsible blocks | HIGH | HIGH | P2 |
| URL detection + Ctrl+click | MEDIUM | MEDIUM | P2 |
| Block keyboard navigation | MEDIUM | LOW | P2 |
| Bash shell integration | MEDIUM | MEDIUM | P2 |
| Font ligatures | MEDIUM | HIGH | P3 |
| Search within scrollback | MEDIUM | HIGH | P3 |
| Tabs and splits | HIGH | HIGH | P3 (separate milestone) |
| Image protocols | LOW | HIGH | P3 |

**Priority key:**
- P1: Must have for Milestone 1 launch
- P2: Should have, add after M1 core works
- P3: Nice to have, future milestone or consideration

---

## Competitor Feature Analysis

| Feature | Alacritty | Warp | Ghostty | WezTerm | Glass M1 Approach |
|---------|-----------|------|---------|---------|-------------------|
| GPU rendering | YES (OpenGL) | YES (Metal/custom) | YES (Metal/OpenGL) | YES (DX12/Vulkan/Metal) | YES — wgpu auto-selects DX12/Vulkan |
| Shell integration | NO | YES (DCS-based, proprietary) | NO | YES (OSC 7/133/1337) | YES — OSC 133 + OSC 7, standard |
| Block-based output | NO | YES (core differentiator) | NO | NO | YES — core Glass differentiator |
| Exit code display | NO | YES (per block) | NO | Partial (mark in scrollback) | YES — per block badge |
| Status bar | NO | YES | NO | YES (configurable Lua) | YES — CWD + git branch |
| Config format | TOML | GUI + YAML | Simple text | Lua | TOML (minimal) |
| Config hot reload | YES | N/A | YES | YES | P2 (not M1 blocker) |
| Font ligatures | NO | YES | YES | YES | NO — deferred to M2+ |
| Tabs/splits | NO | YES | YES | YES | NO — deferred |
| Truecolor | YES | YES | YES | YES | YES |
| Collapsible output | NO | NO | NO | NO | PLANNED M1.x — unique |
| URL detection | YES | YES | YES | YES | P2 |
| Scrollback search | NO (vi mode) | YES | NO | YES | NO — deferred |
| Windows support | YES | YES (2025) | NO (macOS/Linux) | YES | YES — first-class |

**Key takeaway:** Glass matches the performance baseline (GPU, truecolor, keyboard) while adding the block UI layer that only Warp offers — but without Warp's proprietary protocol or electron/web-tech stack. Ghostty and Alacritty prove users accept minimal config. WezTerm's shell integration approach (OSC 133/7) is the right model.

---

## Sources

- [Warp Terminal — How Warp Works](https://www.warp.dev/blog/how-warp-works) — Block architecture and shell integration via DCS
- [WezTerm Shell Integration](https://wezterm.org/shell-integration.html) — OSC 7, OSC 133, OSC 1337 implementation reference
- [WezTerm Features](https://wezterm.org/features.html) — Feature baseline for daily-drivable terminal
- [Ghostty Features](https://ghostty.org/docs/features) — Feature set and what ships out-of-the-box
- [Alacritty GitHub](https://github.com/alacritty/alacritty) — Minimal terminal philosophy, what's intentionally excluded
- [State of Terminal Emulators 2025 — Jeff Quast](https://www.jeffquast.com/post/state-of-terminal-emulation-2025/) — Compliance gaps, Unicode width issues, performance baseline
- [Shell Integration in Windows Terminal — Microsoft](https://devblogs.microsoft.com/commandline/shell-integration-in-the-windows-terminal/) — OSC 133 + OSC 9;9 on Windows, PowerShell integration patterns
- [OSC 133 Shell Integration — Contour Terminal](https://contour-terminal.org/vt-extensions/osc-133-shell-integration/) — Sequence specification: A/B/C/D lifecycle
- [VS Code Terminal Shell Integration](https://code.visualstudio.com/docs/terminal/shell-integration) — Reference implementation for PowerShell and bash hooks
- [termstandard/colors](https://github.com/termstandard/colors) — Truecolor standard, COLORTERM env variable convention

---

*Feature research for: Glass terminal emulator — Milestone 1 (Phase 0-1)*
*Researched: 2026-03-04*
