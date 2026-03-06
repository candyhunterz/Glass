# Feature Research

**Domain:** Cross-platform terminal emulator with tabs/split panes (v2.0 milestone)
**Researched:** 2026-03-06
**Confidence:** HIGH

## Feature Landscape

### Table Stakes (Users Expect These)

#### macOS Platform Support

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Cmd+C/V for copy/paste | Every macOS app uses Cmd, not Ctrl. Terminal users expect Cmd+C to copy (not SIGINT). Ctrl+C must still send SIGINT. | LOW | Map Cmd modifier in winit via `ModifiersState::SUPER`. Already have clipboard via `arboard` (cross-platform). |
| Cmd+Q quit, Cmd+W close tab, Cmd+N new window | Standard macOS app lifecycle shortcuts. Missing these = app feels foreign. | LOW | winit exposes Super modifier. Wire into existing keybinding system. |
| Retina / HiDPI rendering | All modern Macs have Retina (2x+ scaling). Blurry text = unusable terminal. | MEDIUM | wgpu + winit handle `scale_factor()` natively. glyphon text rendering needs scale factor for glyph rasterization. Font size in points, render at device pixel density. |
| Metal GPU backend | macOS deprecated OpenGL in 2018. Metal is the native GPU API. | LOW | wgpu 28.0 auto-selects Metal on macOS. Glass currently forces DX12 -- change to `wgpu::Backends::PRIMARY` or per-platform selection. |
| Native .app bundle | macOS users expect a draggable .app in /Applications. Raw binaries feel wrong. | MEDIUM | Requires Info.plist, icon.icns, code signing. `cargo-bundle` or manual bundle structure. Distribution concern but required for real usage. |
| macOS default shell (zsh) | macOS ships zsh since Catalina (2019). Shell detection must find zsh, not just bash/powershell. | LOW | Auto-detection reads `$SHELL` env var (standard on Unix), then falls back to `/bin/zsh` on macOS. Existing shell override config works unchanged. |
| Shell integration for zsh | OSC 133 sequences need zsh precmd/preexec hooks. Different mechanism than bash PROMPT_COMMAND. | MEDIUM | zsh uses `precmd` and `preexec` hook functions natively. Need new shell integration script alongside existing bash/powershell scripts. Existing glass_core OSC parsing unchanged. |
| Option-as-Meta key | macOS Option key should optionally act as Meta/Alt for terminal escape sequences. Needed for vim, emacs, tmux keybindings. | LOW | Map Option to send ESC prefix. Must be configurable -- some users need Option for accented characters. |
| macOS-standard config/data paths | Config in `~/Library/Application Support/glass/`, not `~/.glass/`. | LOW | The `dirs` crate (already a dependency) returns platform-correct paths. Verify existing code uses `dirs::config_dir()`. |

#### Linux Platform Support

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Wayland + X11 support | Linux is split: Wayland (default GNOME/KDE since ~2022) and X11 (Nvidia, older distros). Must support both. | MEDIUM | winit 0.30 supports both. wgpu uses Vulkan on Linux. Known wgpu Wayland surface sync issues -- test on GNOME and Hyprland. winit falls back to X11 if Wayland init fails. |
| Vulkan GPU backend with GL fallback | Vulkan is standard Linux GPU API. GL needed for VMs and older hardware. | LOW | wgpu 28.0 auto-selects Vulkan on Linux, falls back to GL. Just remove hardcoded DX12 backend selection. |
| XDG Base Directory compliance | Config in `$XDG_CONFIG_HOME/glass/` (~/.config/glass/), data in `$XDG_DATA_HOME/glass/` (~/.local/share/glass/). Linux users notice XDG violations. | LOW | `dirs` crate respects XDG env vars on Linux. Verify current path logic uses `dirs::config_dir()` and `dirs::data_dir()`. |
| PTY via forkpty | Linux/macOS use Unix PTY (forkpty/openpty), not ConPTY. Completely different API. | HIGH | Current code calls `alacritty_terminal::tty::new()` which dispatches to ConPTY on Windows. The same alacritty_terminal crate supports forkpty on Unix behind cfg. This is the single biggest porting task -- need platform abstraction. |
| inotify (Linux) / FSEvents (macOS) file watching | glass_snapshot uses `notify` crate for FS watching. Must work on all platforms. | LOW | `notify` 8.2 (already used) abstracts over inotify, FSEvents, and ReadDirectoryChanges. Should work cross-platform with no code changes. Verify. |
| Shell integration for fish | Fish is popular on Linux. Uses `fish_prompt` and `fish_preexec`/`fish_postexec` events. Not POSIX-compatible. | MEDIUM | Cannot reuse bash integration script. Need separate fish script using fish's native event system for prompt/command lifecycle hooks. |
| Standard Linux shortcuts (Ctrl+Shift+C/V) | Linux terminals use Ctrl+Shift+C/V for copy/paste (Ctrl+C = SIGINT). | LOW | Already implemented for Windows. Same shortcuts work on Linux. No changes needed. |
| .desktop file | Linux app launchers need a .desktop file for application menu integration. | LOW | Simple text file, part of packaging. Not a runtime feature. |

#### Tabs

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Tab bar with new/close/switch | Every modern terminal (Ghostty, WezTerm, Kitty, Windows Terminal) has tabs. Table stakes for a daily-driver terminal. | HIGH | New UI component: tab bar rendered above terminal content. Each tab owns independent PTY + terminal state + block history. Requires refactoring the single-terminal assumption throughout the codebase. |
| Keyboard shortcuts for tab management | Ctrl+Shift+T (new), Ctrl+Shift+W (close), Ctrl+Tab/Ctrl+Shift+Tab (cycle), Ctrl+1-9 (jump). On macOS: Cmd+T, Cmd+W, Cmd+1-9. | LOW | Standard conventions. Platform-conditional modifier (Ctrl+Shift on Linux/Windows, Cmd on macOS). |
| Tab title showing CWD or process | Users need to identify what each tab is doing. Show CWD basename or running process name. | MEDIUM | Existing OSC 7 CWD tracking provides directory. Surface in tab title. Process name from PTY child. Depends on tab bar UI. |
| Independent PTY per tab | Each tab must be a fully independent terminal session with its own shell, CWD, environment. | HIGH | Currently Glass has a single PTY. Need to manage N PTY instances, each with its own reader thread, terminal state, block list, history context, snapshot context. Major architectural change. |
| Middle-click or X button to close tab | Standard mouse interaction for closing tabs. | LOW | UI event handling on tab bar widget. |
| Tab reordering via drag | Common in browsers and terminals. Widely expected. | MEDIUM | Mouse drag-and-drop on tab bar. Can defer to v2.1 if scope is tight. |

#### Split Panes

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Horizontal and vertical splits | Split current pane into side-by-side or top-bottom. WezTerm, Ghostty, Kitty all have this. | HIGH | Binary tree layout of panes within a tab. Each pane is an independent terminal. Layout engine calculates dimensions. Resize redistributes space. |
| Keyboard shortcuts for splitting | Ctrl+Shift+D vertical, Ctrl+Shift+E horizontal (emerging convention). macOS: Cmd+D, Cmd+Shift+D. | LOW | Wire into split pane manager. |
| Pane focus switching via keyboard | Alt+Arrow keys to move focus between panes. Active pane indicated by border highlight. | MEDIUM | Focus tracking, directional navigation logic (which pane is "to the right" in a tree layout), visual indicator. |
| Pane resize via keyboard | Alt+Shift+Arrow to resize the focused pane. | MEDIUM | Adjust split ratios in layout tree. Re-layout and re-render. |
| Independent PTY per pane | Each pane is a full terminal session. | HIGH | Same architecture as per-tab PTY, but panes within a tab share the tab's lifecycle. Closing a tab closes all its panes. |
| Visual divider between panes | Users need to see pane boundaries. Thin line or gap between panes. | LOW | Render 1-2px divider line. Accent color for focused pane border. |

### Differentiators (Competitive Advantage)

These align with Glass's core value of "passively watching, indexing, and snapshoting everything."

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Per-pane block UI + history | Glass's block UI, command history, and undo work independently in each pane. No competitor has structured command history per-pane. | MEDIUM | Each pane needs its own block list, history DB session, and snapshot context. Extend per-command metadata with pane/tab identifiers. |
| Cross-pane/tab search | Search across all panes/tabs via existing search overlay. Find a command you ran "somewhere" without knowing which tab. | MEDIUM | Extend Ctrl+Shift+F search to query all active sessions. Highlight matching pane and scroll to result. Unique to Glass. |
| New tab/pane inherits CWD | Split or new tab starts shell in current pane's CWD. WezTerm and Ghostty do this; many others do not. | LOW | Read CWD from existing OSC 7 tracking. Pass as working directory to new PTY spawn. |
| Pane zoom (toggle fullscreen for one pane) | Temporarily maximize a single pane to full tab area, hiding others. tmux-like Ctrl+Shift+Z toggle. | LOW | Hide other panes, expand focused pane to full size. Toggle restores layout. Simple state toggle. |
| Mouse resize of pane dividers | Click and drag divider line between panes. More intuitive than keyboard-only. | MEDIUM | Hit-testing on divider regions, mouse drag, continuous re-layout during drag. |
| Broadcast input to all panes | Type in one pane, input sent to all panes simultaneously. Useful for multi-server ops. | LOW | Fan out keyboard input to all PTY writers in current tab. Toggle on/off. Niche but valued. |

### Anti-Features (Commonly Requested, Often Problematic)

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| Tmux/screen integration mode | "I already use tmux for splits" | Duplicates tab/split functionality. Two layers of multiplexing creates keybinding conflicts, rendering issues. Tmux redraws entire screen which defeats Glass's block UI. | Let tmux work inside a pane passively. Glass's native tabs/splits replace tmux for Glass users. |
| Detachable/reattachable sessions | "I want persistent sessions like tmux" | Requires daemon architecture (terminal server + client). Massive complexity. Changes entire process model. | Defer indefinitely. Recommend tmux inside a Glass pane for this use case. |
| Session save/restore across restarts | "Remember my tab layout when I restart" | Complex state serialization. Easy to produce broken restored states with stale CWDs or dead processes. | Provide a "startup layout" config option for fixed arrangements. Don't try to serialize live state. |
| Native platform tab bar (NSTabView on macOS, GTK tabs on Linux) | "Use system tab bar for native look" | Requires Objective-C/Swift interop on macOS, GTK on Linux. Breaks cross-platform rendering model. Ghostty does this but maintains two separate frontends. | Render tab bar with wgpu. Match platform visual style via theming. This is the WezTerm/Kitty approach and works well. |
| Per-pane shell picker GUI | "Let me pick bash for tab 1, fish for tab 2" | Adds UI complexity for a niche need. | Config default shell + `glass --shell /bin/fish` for specific instances. No GUI picker needed. |
| Unlimited layout nesting | "Arbitrarily complex split layouts" | Deep nesting creates unusably small panes. Layout algorithms become complex. Kitty has 7+ layout modes -- overkill. | Cap split depth at ~4 levels. Binary tree splits cover 95% of real use cases. |
| Tab groups / workspaces | "Organize tabs into named groups" | Significant UX and state management complexity for a v2.0. | Defer. Tab titles + CWD display provide enough context initially. |

## Feature Dependencies

```
[Platform PTY Abstraction]
    |
    +--requires--> [macOS Support] --requires--> [zsh Shell Integration]
    |                                         +--requires--> [fish Shell Integration]
    |
    +--requires--> [Linux Support] --requires--> [Wayland + X11 windowing]
                                              +--requires--> [XDG directory paths]

[wgpu Backend Auto-Selection]
    |
    +--required by--> [macOS Rendering (Metal)]
    +--required by--> [Linux Rendering (Vulkan/GL)]
    +--required by--> [HiDPI / Retina support]

[Renderer Viewport Refactor]
    |
    +--required by--> [Tab Bar UI]
    +--required by--> [Split Pane Rendering]
    |
    (Currently renderer assumes single full-window viewport.
     Must support rendering into sub-regions before tabs or splits work.)

[Tab Manager]
    |
    +--requires--> [Multi-PTY Management] --requires--> [Platform PTY Abstraction]
    +--requires--> [Tab Bar UI] --requires--> [Renderer Viewport Refactor]
    +--requires--> [Per-Tab State Isolation] (blocks, history, snapshots)

[Split Pane Manager]
    |
    +--requires--> [Tab Manager] (panes live within tabs)
    +--requires--> [Layout Engine] (binary tree of pane regions)
    +--requires--> [Multi-PTY Management]
    +--requires--> [Renderer Viewport Refactor]
```

### Dependency Notes

- **Platform PTY Abstraction is the foundation:** Everything else depends on being able to spawn terminals on all platforms. Must come first.
- **wgpu backend auto-selection is a prerequisite for any cross-platform rendering.** Currently hardcoded to DX12. Trivial change but blocks all non-Windows testing.
- **Renderer viewport refactor is cross-cutting:** The renderer assumes a single full-window terminal. Must support sub-regions before tab bar or split panes can display. This is the key architectural unlock.
- **Tab Manager must exist before Split Panes:** Panes live within tabs. Build tabs first, splits second.
- **Shell integration scripts (zsh, fish) are independent:** Can be written in parallel with platform porting. No code dependency on tabs/splits.
- **HiDPI depends on correct backend selection + scale factor plumbing.** wgpu provides the surface, winit provides the scale factor. glyphon needs the scale factor for glyph sizing.

## MVP Definition

### Launch With (v2.0)

- [ ] **Platform PTY abstraction** -- forkpty on macOS/Linux, existing ConPTY on Windows, behind a unified trait
- [ ] **wgpu backend auto-selection** -- Metal on macOS, Vulkan/GL on Linux, DX12 on Windows
- [ ] **macOS Cmd key mappings** -- Cmd+C/V, Cmd+Q, Cmd+T, Cmd+W, Cmd+N, Cmd+1-9
- [ ] **Option-as-Meta** -- configurable in config.toml
- [ ] **HiDPI / Retina rendering** -- scale_factor plumbing through renderer and text
- [ ] **Platform config/data paths** -- XDG on Linux, ~/Library on macOS via `dirs` crate
- [ ] **zsh shell integration** -- OSC 133 via precmd/preexec hooks
- [ ] **Unix shell detection** -- `$SHELL` env var, fallback to /bin/zsh (macOS) or /bin/bash (Linux)
- [ ] **Tab bar with new/close/switch** -- wgpu-rendered, keyboard shortcuts, per-tab PTY/state
- [ ] **Horizontal and vertical splits** -- binary tree layout, keyboard create/navigate/resize
- [ ] **Independent PTY + state per pane** -- terminal, blocks, history session, snapshot context per pane
- [ ] **Pane focus indicator** -- colored border on active pane
- [ ] **Pane dividers** -- thin visual separators
- [ ] **Cross-platform CI** -- build and test on Windows, macOS, Linux

### Add After Validation (v2.x)

- [ ] **fish shell integration** -- lower priority than zsh/bash, add when fish users request it
- [ ] **Tab reordering via drag** -- polish feature
- [ ] **Mouse resize of pane dividers** -- keyboard resize sufficient for launch
- [ ] **Pane zoom toggle** -- low complexity tmux-like feature
- [ ] **Cross-pane search** -- extend existing search across all sessions
- [ ] **New tab/pane inherits CWD** -- easy win once CWD tracking is reliable per-pane
- [ ] **Broadcast input** -- niche power-user feature
- [ ] **macOS .app bundle** -- needed for real distribution, not for dev testing
- [ ] **Linux packaging (.deb, .rpm, Flatpak)** -- distribution concern

### Future Consideration (v3+)

- [ ] **Detachable/reattachable sessions** -- daemon architecture, massive scope
- [ ] **Session save/restore** -- complex state serialization
- [ ] **Startup layout config** -- define default tab/pane arrangement in TOML
- [ ] **Tab groups / workspaces** -- organizational layer above tabs

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| Platform PTY abstraction | HIGH | HIGH | P1 |
| wgpu backend auto-selection | HIGH | LOW | P1 |
| macOS Cmd key mappings | HIGH | LOW | P1 |
| Retina/HiDPI rendering | HIGH | MEDIUM | P1 |
| XDG / macOS config paths | MEDIUM | LOW | P1 |
| zsh shell integration | HIGH | MEDIUM | P1 |
| Unix shell detection | HIGH | LOW | P1 |
| Tab bar UI + management | HIGH | HIGH | P1 |
| Horizontal/vertical splits | HIGH | HIGH | P1 |
| Independent PTY per pane | HIGH | HIGH | P1 |
| Pane focus indicator | HIGH | LOW | P1 |
| Pane dividers | MEDIUM | LOW | P1 |
| Cross-platform CI | HIGH | MEDIUM | P1 |
| Option-as-Meta key | MEDIUM | LOW | P1 |
| fish shell integration | MEDIUM | MEDIUM | P2 |
| Tab drag reorder | LOW | MEDIUM | P2 |
| Mouse pane resize | MEDIUM | MEDIUM | P2 |
| Pane zoom toggle | MEDIUM | LOW | P2 |
| Cross-pane search | MEDIUM | MEDIUM | P2 |
| CWD inheritance for new panes | MEDIUM | LOW | P2 |
| macOS .app bundle | MEDIUM | MEDIUM | P2 |
| Broadcast input | LOW | LOW | P3 |
| Session persistence | MEDIUM | HIGH | P3 |

## Competitor Feature Analysis

| Feature | Alacritty | Ghostty | Kitty | WezTerm | Glass v2.0 Plan |
|---------|-----------|---------|-------|---------|-----------------|
| Tabs | No (delegates to tmux) | Yes (native per-platform) | Yes (built-in) | Yes (built-in) | Yes (wgpu-rendered) |
| Split panes | No | Yes (native per-platform) | Yes (7+ layout modes) | Yes (binary tree) | Yes (binary tree) |
| macOS native feel | Partial (no tabs) | Excellent (Swift/AppKit) | Good (custom-rendered) | Good (custom-rendered) | Good (wgpu + Cmd shortcuts) |
| GPU backend macOS | OpenGL (deprecated) | Metal | OpenGL | OpenGL/Metal | Metal via wgpu |
| GPU backend Linux | OpenGL | OpenGL | OpenGL | OpenGL | Vulkan via wgpu (GL fallback) |
| Shell integration | No | Yes (OSC 133) | Yes (custom protocol) | Yes (OSC 133) | Yes (OSC 133, already built) |
| Wayland | Yes | Yes | Yes | Yes | Yes (via winit) |
| Structured command blocks | No | No | No | No | **Yes (unique)** |
| Command undo | No | No | No | No | **Yes (unique)** |
| Per-pane history/search | No | No | No | No | **Yes (unique)** |
| Pipe visualization | No | No | No | No | **Yes (unique, v1.3)** |
| Configuration | TOML | Custom | conf file | Lua | TOML (already built) |
| Layout modes | N/A | Splits only | 7+ modes | Splits only | Splits (binary tree) |
| Pane zoom | N/A | No | Yes (Stack layout) | Yes | Planned (v2.x) |

### Key Competitive Observations

1. **Alacritty deliberately has no tabs/splits** -- delegates to tmux/Zellij. Glass should not follow this path; built-in multiplexing is expected by users who do not want tmux.

2. **Ghostty uses native platform UI** (Swift on macOS, GTK on Linux) for tabs/splits. Best native feel but requires two separate frontend codebases. Glass should use wgpu-rendered UI for consistency, matching WezTerm/Kitty.

3. **Kitty has the most layout flexibility** (7+ modes). Glass does not need this -- binary tree splits cover 95% of use cases. Complexity is not a differentiator for Glass.

4. **No competitor has Glass's structured block model.** Making blocks, undo, history, and pipe visualization work correctly per-pane is what matters most. This is Glass's real competitive advantage.

5. **wgpu gives Glass the strongest GPU backend story.** Metal on macOS and Vulkan on Linux, while Alacritty and Kitty remain on deprecated OpenGL. Ghostty uses Metal on macOS but OpenGL on Linux.

## Sources

- [Ghostty features documentation](https://ghostty.org/docs/features)
- [Ghostty 1.2 release notes](https://ghostty.org/docs/install/release-notes/1-2-0)
- [Ghostty keybind reference](https://ghostty.org/docs/config/keybind/reference)
- [WezTerm multiplexing and tab management](https://wezterm.com/how-does-wezterm-support-multiplexing-and-tab-management/)
- [WezTerm SplitPane documentation](https://wezterm.org/config/lua/keyassignment/SplitPane.html)
- [WezTerm features](https://wezterm.org/features.html)
- [WezTerm shell integration](https://wezterm.org/shell-integration.html)
- [Kitty overview](https://sw.kovidgoyal.net/kitty/overview/)
- [Kitty layouts](https://sw.kovidgoyal.net/kitty/layouts/)
- [Alacritty key bindings config](https://alacritty.org/config-alacritty-bindings.html)
- [portable-pty crate](https://lib.rs/crates/portable-pty)
- [pseudoterminal crate](https://github.com/michaelvanstraten/pseudoterminal)
- [wgpu backends documentation](https://docs.rs/wgpu/latest/wgpu/struct.Backends.html)
- [winit documentation](https://docs.rs/winit/latest/winit/)
- [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir/latest/)
- [iTerm2 Shell Integration Protocol](https://gist.github.com/tep/e3f3d384de40dbda932577c7da576ec3)
- [Apple Terminal keyboard shortcuts](https://support.apple.com/guide/terminal/keyboard-shortcuts-trmlshtcts/mac)
- [Windows Terminal panes](https://learn.microsoft.com/en-us/windows/terminal/panes)

---
*Feature research for: Glass v2.0 cross-platform terminal with tabs/split panes*
*Researched: 2026-03-06*
