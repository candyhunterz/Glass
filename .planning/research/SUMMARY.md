# Project Research Summary

**Project:** Glass — Rust GPU-accelerated terminal emulator (Windows-first)
**Domain:** GPU-accelerated terminal emulator with block-based UI and shell integration
**Researched:** 2026-03-04
**Confidence:** HIGH

## Executive Summary

Glass is a GPU-accelerated terminal emulator built in Rust for Windows-first deployment. The established expert approach for this class of product combines two battle-tested open-source components — `alacritty_terminal` for VTE parsing and PTY management, and `wgpu` (via `glyphon`) for GPU rendering — layered under a `winit`-based event loop. This stack eliminates the two hardest problems (5,000+ lines of ANSI escape code parsing, and cross-backend GPU text rendering) by leveraging mature crates with verified version compatibility. The key differentiator for Glass over existing terminals is its block-based command output UI (grouping each command's prompt, output, exit code, and duration visually), which requires shell integration via OSC 133/OSC 7 sequences — the same mechanism used by WezTerm and Windows Terminal. This is well-understood territory with clear implementation patterns.

The recommended architecture is a Cargo workspace with strict crate boundaries: `glass_core` (shared types), `glass_terminal` (all `alacritty_terminal` interaction, PTY, shell hooks), `glass_renderer` (wgpu/glyphon, receives only a lock-free `GridSnapshot`), and a thin binary that wires them together via winit's `ApplicationHandler` trait. The PTY I/O runs on a dedicated thread and signals the main thread via `EventLoopProxy<AppEvent>`. This separation is mandatory — not optional — because holding the terminal state lock during GPU draw calls causes PTY starvation and visible input lag. The rendering pipeline uses two wgpu passes (rect pipeline for backgrounds/cursor/block UI, text pipeline for glyphon glyph rendering) to avoid pipeline state switching per cell.

The primary risks are concentrated in Phase 0 (scaffold): ConPTY rewrites escape sequences in transit (non-transparent pipe), `alacritty_terminal` has no stable public API and must be pinned and isolated, wgpu surface resize on Windows causes flickering with Vulkan (prefer DX12), and Windows defaults to a legacy OEM code page rather than UTF-8. All four of these must be addressed before building any higher-level feature — they are structural, and retrofitting them after the terminal stack is built is disruptive or impossible. Research gives us concrete mitigations for each.

## Key Findings

### Recommended Stack

The Glass stack is fully determined by research with high confidence. `alacritty_terminal` 0.25.1 handles the entire VT state machine and ConPTY integration on Windows — it must be pinned at an exact version (no `^` or `~`) and isolated behind the `glass_terminal` crate boundary because it carries no API stability guarantee. `wgpu` 28.0.0 provides the GPU surface via DX12 on Windows 11 (auto-selected), paired with `glyphon` 0.10.0 (the only maintained wgpu text renderer; requires exactly `wgpu ^28.0.0`). `winit` 0.30.13 manages the OS window and event loop — the 0.30 API is a complete rewrite from prior versions; all pre-2024 tutorials are broken. `tokio` handles async PTY I/O glue. All version combinations have been verified against crates.io dependency manifests.

**Core technologies:**
- `alacritty_terminal` 0.25.1: VTE/ANSI parsing, terminal grid, ConPTY on Windows — eliminates ~5,000 lines of parser work; must be isolated in `glass_terminal`
- `wgpu` 28.0.0: GPU surface via DX12 (Windows) — auto-selects backend; production-stable DX12; WGSL shaders compiled AOT
- `glyphon` 0.10.0: GPU text rendering (cosmic-text shaping + etagere atlas + wgpu) — the only maintained wgpu text renderer; version-locked to wgpu 28
- `winit` 0.30.13: window creation and OS event loop — 0.30 ApplicationHandler trait is a breaking redesign; create window only inside `resumed()` callback
- `tokio` 1.50.0: async runtime for PTY I/O pipeline — industry standard; required by alacritty_terminal's event loop integration
- `pollster` 0.4.0: blocks on async wgpu init from sync winit callbacks — mandatory pattern; do not make event loop async

**What NOT to use:** `wgpu_glyph` (unmaintained since 2023), `rusttype`/`glyph_brush` (deprecated), `nix` crate (Unix-only), `winpty` (legacy), `log` crate (use `tracing` instead), `openssl` (not needed).

### Expected Features

The Milestone 1 launch bar is "can a developer use Glass as their primary terminal while building Glass?" All table-stakes features must be present, or users cannot work. Block-based output is the competitive differentiator — it is the visible payoff that justifies Glass's existence alongside Alacritty/WezTerm.

**Must have (table stakes for M1 launch):**
- ConPTY PTY spawn with PowerShell — the single highest-risk dependency; without it, nothing works
- Full VT/ANSI escape processing via `alacritty_terminal` — required by every CLI tool
- Truecolor (24-bit) rendering — required by modern tooling (delta, bat, neovim themes); set `COLORTERM=truecolor`
- Keyboard input with modifier keys (Ctrl, Alt, Shift) — vim/fzf/tmux break without correct modifier handling
- Bracketed paste mode — prevents accidental multi-line execution on paste
- Scrollback buffer (10,000 lines configurable) — absence is immediately complained about
- Copy/paste (Ctrl+Shift+C / Ctrl+Shift+V) — universal Windows convention
- Shell integration (OSC 133 + OSC 7) for PowerShell and bash — required for block UI
- Block-based command output — prompt + output + exit code + duration per block
- Status bar — CWD (from OSC 7) + git branch (async subprocess)
- TOML config — font family, size, shell override
- GPU-accelerated rendering — `<5ms` input latency, `<200ms` cold start
- Window resize handling — PTY resize on dimension change

**Should have (M1.x polish after daily-driver validation):**
- Config hot reload — saves restart cycles
- Collapsible blocks — architecture must support this from day one even if not shipped in M1
- URL detection and Ctrl+click to open
- Bash shell integration (WSL/Git Bash)
- Block keyboard navigation (jump to previous/next block)

**Defer (Phase 2+):**
- Structured history DB (Phase 2) — queryable scrollback; requires block data pipeline first
- MCP server (Phase 2) — exposes history to AI tools
- Command-level undo (Phase 3) — Glass's flagship future feature
- Pipe visualization (Phase 4)
- Tabs/split panes — requires multiplexer architecture; separate milestone
- Font ligatures — HarfBuzz shaping; M2+ rendering enhancement
- Image protocols (Kitty, Sixel)
- Searchable scrollback UI

**Anti-features to avoid building in M1:** tabs/splits, built-in AI, plugin system, cloud sync, cmd.exe support, font ligatures, theme marketplace, Ctrl+Z undo, image protocols, scrollback search, right-click context menu.

### Architecture Approach

The architecture follows Alacritty's proven pattern, extended with Glass's block UI layer. The critical design constraint is the lock-minimizing grid snapshot: lock `Term<EventProxy>` briefly to copy a `GridSnapshot`, drop the lock immediately, then render from the snapshot. This prevents PTY starvation during GPU draw calls. The thread model has two threads: the main thread runs the winit event loop and all rendering; a dedicated PTY I/O thread (not a Tokio task) does blocking reads and signals the main thread via `EventLoopProxy<AppEvent>`. OSC sequences (133 and 7) are intercepted via a `ShellIntegrationHandler<H>` wrapper around the vte `Handler` trait, routing hook events to a `BlockManager` state machine that tracks command boundaries.

**Major components:**
1. `glass_core` — shared event types (`AppEvent`), config structs (`GlassConfig`), error types; no deps on other Glass crates
2. `glass_terminal` — wraps `alacritty_terminal`; owns PTY spawn, VTE grid, OSC 133/7 parsing, `BlockManager`; only crate that imports `alacritty_terminal`
3. `glass_renderer` — wgpu surface, `TextAtlas`/`TextRenderer` (glyphon), `RectPipeline` (cell backgrounds, cursor, block borders), `frame.rs` orchestration; takes `GridSnapshot`, not live `Term<T>`
4. `glass_app` binary — thin winit `ApplicationHandler` implementation; wires terminal to renderer; routes keyboard to PTY
5. Stub crates (`glass_history`, `glass_snapshot`, `glass_pipes`, `glass_mcp`) — exist from day one for clean workspace structure; filled in per milestone

### Critical Pitfalls

1. **ConPTY rewrites escape sequences in transit** — Not a transparent byte pipe; `ESC[49m` collapses to `ESC[m`, curly underlines stripped, keyboard sequences rewritten unless `ENABLE_VIRTUAL_TERMINAL_INPUT` is set. Mitigation: enable the flag immediately after ConPTY creation; test with escape sequence fixtures before building any rendering layer on top. Address in Phase 0.

2. **`alacritty_terminal` has no stable embedding API** — Breaking changes ship with every Alacritty release; no semver guarantee; maintainers explicitly do not support third-party embedders. Mitigation: pin exact version (`= "0.25.1"`, not `^`); isolate all `alacritty_terminal` types behind `glass_terminal` crate boundary so the rest of the workspace never imports it directly. Address in Phase 0.

3. **wgpu surface resize causes flickering and hangs on Windows** — White rectangles appear during drag-resize; `surface.configure()` can block 100–150ms on Vulkan. Mitigation: prefer DX12 backend (wgpu auto-selects it on Windows); debounce resize events; handle `SurfaceError::Outdated`/`Lost` gracefully without panic. Address in Phase 0.

4. **PTY reader thread blocking the render thread** — Blocking PTY reads on the main thread freeze the UI during idle shell and bursts. Mitigation: dedicated `std::thread::spawn` for PTY reads (not a Tokio task — PTY reads are blocking); bound the mpsc channel (16 entries); send dirty notification only, not full state. Address in Phase 0.

5. **Windows UTF-8 code page not set by default** — ConPTY inherits the parent process code page (OEM 437 or locale default), causing mojibake on non-ASCII output. Mitigation: call `SetConsoleCP(65001)` and `SetConsoleOutputCP(65001)` as the first thing in `main()` before any PTY creation. Address in Phase 0.

6. **Shell integration fragile under prompt customization** — OSC 133 breaks with Oh My Posh/Starship if integration overwrites rather than wraps the `prompt` function in PowerShell. Mitigation: save and wrap the existing prompt function; use PSReadLine `PreExecution` hook for `OSC 133;D`; implement a fallback indicator if marks not received within 5s. Address in Phase 1.

7. **Font atlas overflow causes silent glyph drops** — Default atlas size insufficient for CJK-heavy terminal sessions. Mitigation: use 2048×2048+ atlas; pre-warm with full printable ASCII at startup; separate atlas pools for monochrome and colored glyphs. Address in Phase 1.

## Implications for Roadmap

The research makes the phase structure clear: there is a strict dependency chain driven by the architecture. Shell integration cannot work until PTY is correct; block UI cannot work until shell integration is correct; the Phase 2+ features cannot exist without block data. Every Phase 0 pitfall, if left unaddressed, requires a disruptive retrofit.

### Phase 0: Foundation Scaffold

**Rationale:** Four of the nine documented pitfalls must be addressed here or they require structural retrofitting later. The winit ApplicationHandler pattern, wgpu surface init, PTY thread architecture, UTF-8 code page, and `alacritty_terminal` isolation boundary are all zero-cost to get right now and extremely costly to fix after the feature stack is built. No user-visible feature work should begin until this phase passes its verification checklist.

**Delivers:** Cargo workspace with all crates (including stubs), winit window with wgpu clear-to-color, PTY spawning PowerShell with keyboard round-trip, UTF-8 code page set, ConPTY escape sequence fixture tests passing, drag-resize stability confirmed.

**Addresses:** Non-broken shell launching (table stakes baseline), correct process architecture
**Avoids:** PTY-reader-on-render-thread freeze, wgpu resize crash, alacritty_terminal API leakage, UTF-8 mojibake, winit 0.30 API misuse

**Research flag:** Standard patterns — do not need `/gsd:research-phase`. Architecture, stack, and pitfall mitigations are fully documented.

### Phase 1: Basic Terminal (Table Stakes)

**Rationale:** Once the scaffold is correct, wire VTE parsing to the rendering pipeline to produce a functional basic terminal. The dependencies are: `glass_terminal` (VTE grid) → `GridSnapshot` → `glass_renderer` (text + rect pipeline) → visible output. This is the longest phase because it delivers the entire baseline feature set. Every table-stakes feature must ship here.

**Delivers:** Fully functional terminal — VT/ANSI processing, truecolor, all keyboard input, bracketed paste, scrollback (10k lines), copy/paste, cursor rendering, window resize, TOML config (font family/size/shell).

**Uses:** `alacritty_terminal` Term+Grid pipeline, `glyphon` TextAtlas (2048×2048, ASCII pre-warm), two wgpu render pipelines (RectPipeline + TextPipeline), `bytemuck` for GPU buffer uploads.

**Avoids:** Font atlas overflow (size correctly from start), wide character misalignment (use alacritty_terminal's placeholder cell model, skip `WIDE_CHAR_SPACER` cells in renderer), glyph atlas rebuild per frame (persist `TextAtlas` across frames).

**Research flag:** Standard patterns — wgpu text rendering with glyphon is well-documented. Wide character handling and atlas sizing have documented solutions. May need targeted research on the `ShellIntegrationHandler` wrapper API depending on exact `alacritty_terminal` 0.25.1 trait structure.

### Phase 2: Shell Integration and Block UI

**Rationale:** Shell integration (OSC 133 + OSC 7) is the architectural foundation for blocks, and blocks are the core Glass differentiator. These ship together because they are tightly coupled: OSC parsing feeds the `BlockManager`, which feeds the block renderer. Exit code badges and command duration are zero-marginal-effort once blocks exist. Status bar (CWD + git branch) depends on OSC 7 CWD events and can ship in the same phase.

**Delivers:** PowerShell integration script (wrapping prompt function), bash/Git Bash integration script, `BlockManager` state machine (OSC 133 A/B/C/D), block rendering in `glass_renderer` (separator lines, header rows, exit code badge, duration display), status bar (CWD + async git branch via subprocess), block UI visible in daily use.

**Uses:** `ShellIntegrationHandler<H>` OSC passthrough wrapper, `BlockManager` state machine, `RectPipeline` for block borders and badges, `notify` crate for future config hot-reload groundwork.

**Avoids:** Shell integration breaking with Oh My Posh (wrap existing prompt, don't replace; test against Oh My Posh before declaring done), `OSC 133;D` exit code attribution error (send D before A, not after; use PSReadLine PreExecution hook in PowerShell).

**Research flag:** Needs `/gsd:research-phase` for PSReadLine 2.x hook integration in PowerShell (exact API for `PreExecution` and prompt function wrapping). The bash side is standard `PROMPT_COMMAND`/`PS0`. Oh My Posh compatibility testing approach needs documentation.

### Phase 3: Polish and M1.x Features

**Rationale:** After Phase 2, Glass is daily-drivable. This phase addresses quality-of-life improvements that meaningfully improve the experience but don't unblock daily use. Collapsible blocks must be done before this point architecturally (the rendering layer must not preclude it), but the collapse/expand UX ships here.

**Delivers:** Config hot reload (via `notify` watcher), collapsible block UI (fold long output to one-line header), URL detection with Ctrl+click, bash shell integration (WSL/Git Bash), block keyboard navigation (jump to prev/next block with keyboard shortcut).

**Uses:** `notify` 8.2.0 for filesystem watching, existing `BlockManager` extended with collapse state, existing rendering pipeline extended with collapsed view mode.

**Avoids:** DPI change handling regression (ensure `ScaleFactorChanged` events propagate to wgpu surface reconfiguration), cursor blink as full-frame redraw (blink only cursor cell region).

**Research flag:** Standard patterns — collapsible blocks are a rendering mode addition, not an architecture change if Phase 2 left room for it. Config hot reload with `notify` is well-documented.

### Phase 4: Structured History DB (Phase 2 per PROJECT.md)

**Rationale:** Block UI in Phase 2/3 produces command boundary events that need a durable store. SQLite via `rusqlite` (bundled) is the answer — no external dependency on Windows. This phase builds the history pipeline and exposes it for Phase 5 MCP server.

**Delivers:** `glass_history` crate (SQLite via `rusqlite`), command history recording (command text, CWD, exit code, duration, start time), queryable scrollback (replace in-memory scrollback beyond N lines), env var secret filtering (`*_TOKEN`, `*_SECRET`, `*_KEY`, `*_PASSWORD` blocked).

**Uses:** `rusqlite` 0.38.0 with `bundled` feature, WAL mode SQLite for concurrent reads from MCP server.

**Research flag:** Needs `/gsd:research-phase` for SQLite WAL mode schema design and concurrent read patterns. Also needs research on what history query interface the MCP server requires.

### Phase 5: MCP Server (Phase 2 per PROJECT.md)

**Rationale:** The history DB is the data source; MCP is the protocol that exposes it to external AI tools. This is explicitly the Phase 2 goal from PROJECT.md. The `glass_mcp` stub crate becomes real here.

**Delivers:** `glass_mcp` crate, MCP server exposing history, command lookup, and CWD tracking to external AI/tool integrations. Runs on a separate thread (already blocked out in the thread model).

**Research flag:** Needs `/gsd:research-phase` — MCP is a relatively new protocol (Anthropic). API surface, Rust SDK availability, and session/transport mechanism need verification.

### Phase 6+: Command-Level Undo, Pipe Visualization (Phases 3 and 4 per PROJECT.md)

**Rationale:** Glass's flagship future features. These depend on the history DB, snapshot infrastructure, and a mature block UI. Not M1 scope. Stub crates (`glass_snapshot`, `glass_pipes`) exist to avoid future restructuring.

**Research flag:** Both need `/gsd:research-phase` when their milestone approaches. Filesystem snapshotting on Windows (NTFS shadow copy vs. copy-on-write vs. Git-style content addressing) is a non-trivial design decision. Pipe visualization requires a command parser for pipeline detection.

### Phase Ordering Rationale

- **Phase 0 before anything:** Four critical pitfalls (ConPTY, wgpu resize, PTY threading, UTF-8) must be structurally correct before any feature work. These are architectural — not fixable with a wrapper later.
- **Phase 1 before Phase 2:** Block UI requires correct VTE rendering as its substrate. There is no shortcut.
- **Phase 2 before Phase 4:** History DB records block-boundary events. Blocks must exist before they can be recorded.
- **Phase 4 before Phase 5:** MCP server reads from history DB. No DB = no MCP.
- **Stub crates from day one:** `glass_history`, `glass_snapshot`, `glass_pipes`, `glass_mcp` compile as empty libs immediately. This prevents future workspace restructuring when their milestones arrive.
- **Collapsible block architecture in Phase 2:** Even though the collapse/expand UX ships in Phase 3, the block rendering layer must not foreclose it. Leave room in the `BlockManager` and renderer during Phase 2 or Phase 3 becomes a refactor.

### Research Flags

Phases requiring `/gsd:research-phase` during planning:
- **Phase 2:** PSReadLine 2.x hook API for PowerShell prompt wrapping; Oh My Posh/Starship compatibility testing approach
- **Phase 4:** SQLite WAL schema for command history; concurrent read pattern for MCP + write from terminal thread
- **Phase 5:** MCP protocol spec; Rust SDK availability and transport mechanism
- **Phase 6+:** Windows filesystem snapshot mechanisms for command-level undo; pipe/command parser for pipe visualization

Phases with well-documented patterns (skip `/gsd:research-phase`):
- **Phase 0:** All patterns and mitigations fully documented in research
- **Phase 1:** wgpu + glyphon text rendering, alacritty_terminal grid consumption, wide char handling — all documented
- **Phase 3:** Config hot reload with `notify`, collapsible block rendering — standard patterns

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | All versions verified via crates.io API; version compatibility matrix confirmed (glyphon 0.10 → wgpu 28 → winit 0.30 chain verified against dependency manifests) |
| Features | HIGH | Verified against Warp, WezTerm, Ghostty, Alacritty official docs and changelogs; OSC 133 spec from Microsoft Learn |
| Architecture | HIGH | Primary sources: Alacritty source code, winit/wgpu official docs, Microsoft shell integration docs; patterns confirmed against cosmic-term reference implementation |
| Pitfalls | HIGH (ConPTY, wgpu, winit, PTY threading), MEDIUM (shell integration), LOW (alacritty_terminal API stability guarantees) | ConPTY/wgpu pitfalls backed by tracked GitHub issues with reproducible evidence; shell integration fragility is MEDIUM because the exact PSReadLine hook API needs verification |

**Overall confidence:** HIGH

### Gaps to Address

- **`alacritty_terminal` 0.25.1 OSC dispatch API:** Research notes the `ShellIntegrationHandler<H>` wrapper approach but flags that "exact trait names may differ from vte's `Perform` trait." Verify the precise handler trait interface against actual 0.25.1 crate docs before implementing. Low-risk gap — isolated to `glass_terminal`.

- **PSReadLine `PreExecution` hook availability:** The PowerShell shell integration requires PSReadLine 2.x for the `PreExecution` hook. Verify this hook is available in the PowerShell 7 + PSReadLine 2.x combination that ships with Windows 11 by default. If unavailable, the fallback is a less reliable `$PROFILE` mutation pattern.

- **Collapsible block rendering approach:** Research identifies that block collapsing is "additive" to the rendering pipeline, but does not specify the exact rendering mechanism (shrink scrollback rows, overlay collapsed header, or virtual row model). This needs a design decision before Phase 2 rendering is locked in.

- **`alacritty_terminal` scrollback API at 10k lines:** Research warns against pre-allocating full scrollback buffers (191MB for 10k lines is a documented Alacritty issue). Verify that `alacritty_terminal` 0.25.1's `Storage` type uses lazy/partial allocation and does not require Glass to manage scrollback size independently.

- **MCP protocol and Rust SDK:** Research explicitly notes this needs `/gsd:research-phase` when Phase 5 approaches. No research was done into MCP transport, session model, or Rust library availability.

## Sources

### Primary (HIGH confidence)
- crates.io API — Version verification for all crates (fetched 2026-03-04); glyphon 0.10.0 → wgpu ^28.0.0 dependency confirmed
- alacritty/alacritty GitHub source — Event loop, renderer, ConPTY source code reviewed
- Microsoft Learn: Windows Terminal Shell Integration — OSC 133 spec, PowerShell hooks, OSC 9;9 format
- winit 0.30 changelog (docs.rs) — ApplicationHandler migration, WindowBuilder deprecation
- wgpu GitHub issues #5374, #7447 — Surface resize flickering on Windows (DX12 vs Vulkan)
- microsoft/terminal GitHub issues #12166, #362 — ConPTY escape sequence rewriting documented
- grovesNL/glyphon GitHub — Architecture (cosmic-text + etagere + wgpu pipeline)
- pop-os/cosmic-term — Reference implementation using this exact stack in production

### Secondary (MEDIUM confidence)
- Warp Terminal blog: "How Warp Works" — Block architecture and shell integration patterns
- WezTerm shell integration docs — OSC 7/133/1337 implementation reference
- Ghostty features docs — Feature set baseline; GPU rendering approach
- Alacritty README + issue tracker — Minimal terminal philosophy; what's intentionally excluded
- DEV Community: "Taming Windows Terminal's win32-input-mode" — `ENABLE_VIRTUAL_TERMINAL_INPUT` flag requirement
- Warp blog: "Adventures in Text Rendering" — Glyph atlas design patterns
- hy2k.dev: "Fixing Mojibake from UTF-8 Tools in PowerShell on Windows" (2025) — UTF-8 code page issue

### Tertiary (LOW confidence / needs validation)
- PSReadLine 2.x `PreExecution` hook availability in Windows 11 default PowerShell 7 — assumed available, not verified against installed version
- `alacritty_terminal` 0.25.1 OSC handler trait interface — code pattern documented but exact trait names need verification against actual crate docs

---
*Research completed: 2026-03-04*
*Ready for roadmap: yes*
