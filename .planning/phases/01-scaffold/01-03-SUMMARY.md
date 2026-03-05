---
phase: 01-scaffold
plan: "03"
subsystem: terminal
tags: [conpty, alacritty_terminal, winit, keyboard, pty, utf8, codepage, wgpu]

# Dependency graph
requires:
  - phase: 01-scaffold-01
    provides: Cargo workspace with all 7 crates, glass_core AppEvent types
  - phase: 01-scaffold-02
    provides: wgpu DX12 GPU surface, winit Processor/WindowContext structure

provides:
  - ConPTY PTY spawning via alacritty_terminal 0.25.1 with dedicated reader thread
  - EventProxy bridging PTY events (Wakeup, Title, Exit) to winit AppEvent
  - Keyboard input forwarding from winit KeyboardInput to PTY stdin
  - Window title updates from PTY SetTitle events (PowerShell prompt path)
  - TerminalExit handling — shell exit closes the application
  - Window resize notifications forwarded to PTY via Msg::Resize
  - Automated escape sequence fixture tests (cargo test -p glass_terminal -- escape_seq)
  - UTF-8 codepage 65001 assertion test (cargo test -p glass -- codepage)
  - Full keyboard round-trip: winit key -> PTY stdin -> PowerShell -> PTY stdout -> TerminalDirty

affects: [02-rendering, 03-shell-integration, 04-intelligence]

# Tech tracking
tech-stack:
  added:
    - alacritty_terminal 0.25.1 (exact pin) — PTY spawning, Term grid, FairMutex, event_loop
    - ConPTY (Windows) — spawned via alacritty_terminal::tty::new()
    - windows-sys 0.59 (Win32_System_Console) — SetConsoleCP/GetConsoleCP calls
  patterns:
    - Dedicated PTY reader thread via event_loop.spawn() (NOT a Tokio task) — avoids blocking async runtime
    - EventProxy pattern: PTY events -> winit EventLoopProxy<AppEvent> -> ApplicationHandler::user_event
    - Lock-minimizing access: Arc<FairMutex<Term<EventProxy>>> for terminal state
    - Minimal keyboard forwarding (ASCII text via event.text) with Phase 2 plan to add full escape encoding

key-files:
  created:
    - crates/glass_terminal/src/event_proxy.rs
    - crates/glass_terminal/src/pty.rs
    - crates/glass_terminal/src/tests.rs
    - src/tests.rs
  modified:
    - crates/glass_terminal/src/lib.rs
    - src/main.rs
    - Cargo.toml

key-decisions:
  - "Keyboard forwarding in scaffold is ASCII-only (event.text) — Phase 2 Plan 03 handles full escape sequence encoding for Ctrl/Alt/arrows/function keys"
  - "PTY reader thread uses std::thread via event_loop.spawn() not tokio::spawn — avoids blocking async executor with blocking I/O"
  - "EventProxy derives Clone because both Term::new() and PtyEventLoop::new() consume the listener by value — two instances sharing same EventLoopProxy (which is Clone+Send)"
  - "Terminal grid logging added in RedrawRequested handler for scaffold verification — removed in Phase 2 when rendering is added"

patterns-established:
  - "EventProxy pattern: implement EventListener on a struct holding EventLoopProxy<AppEvent> + WindowId, forward Wakeup/Title/Exit to AppEvent variants"
  - "PTY lifecycle: spawn in resumed()/can_create_surfaces(), store Sender<Msg> in WindowContext, forward keyboard in window_event, handle AppEvent in user_event"

requirements-completed: [CORE-01]

# Metrics
duration: 45min
completed: 2026-03-04
---

# Phase 1 Plan 03: PTY Keyboard Round-Trip Summary

**PowerShell spawns via ConPTY with a dedicated alacritty_terminal reader thread, keyboard input forwarded from winit to PTY stdin, completing the full scaffold keyboard round-trip with verified escape-sequence and UTF-8 codepage tests.**

## Performance

- **Duration:** ~45 min
- **Started:** 2026-03-04 (continuation from 01-02 completion)
- **Completed:** 2026-03-04
- **Tasks:** 4 (including human-verify checkpoint — Task 3)
- **Files modified:** 7

## Accomplishments

- ConPTY spawns PowerShell (pwsh) via alacritty_terminal 0.25.1 with a dedicated std::thread reader (not Tokio)
- Full keyboard round-trip verified: winit KeyboardInput -> PTY stdin -> PowerShell processes -> PTY stdout -> TerminalDirty event -> debug logging
- Window title updates from PTY SetTitle events (shell sets the title to its prompt path)
- Three automated tests added and passing: conpty_spawns_and_wakeup_fires, pty_keyboard_round_trip, test_utf8_codepage_65001_active
- Human-verify checkpoint approved: window launched, keyboard round-trip confirmed, all 3 tests green, PowerShell 5.1 spawned correctly

## Task Commits

Each task was committed atomically:

1. **Task 0: Create Wave 0 test files for escape sequences and UTF-8 codepage** - `192b83d` (test)
2. **Task 1: Implement EventProxy and spawn_pty in glass_terminal** - `e564f48` (feat)
3. **Task 2: Wire PTY into main.rs Processor and add keyboard input forwarding** - `19f42d7` (feat)
4. **Task 3: Verify PowerShell spawns and keyboard round-trip works** - (human-verify checkpoint — no code commit)

**Plan metadata:** (docs commit — this summary)

## Files Created/Modified

- `crates/glass_terminal/src/event_proxy.rs` — EventProxy struct implementing alacritty_terminal EventListener, forwards Wakeup/Title/Exit to AppEvent via EventLoopProxy<AppEvent>
- `crates/glass_terminal/src/pty.rs` — spawn_pty() function: creates ConPTY with TtyOptions, spawns dedicated reader thread, returns (Sender<Msg>, Arc<FairMutex<Term<EventProxy>>>)
- `crates/glass_terminal/src/tests.rs` — Two ConPTY fixture tests: conpty_spawns_and_wakeup_fires (Wakeup fires after spawn), pty_keyboard_round_trip (keyboard input produces more output)
- `crates/glass_terminal/src/lib.rs` — Public module declarations: pub mod event_proxy, pub mod pty, pub use re-exports, #[cfg(test)] mod tests
- `src/tests.rs` — UTF-8 codepage 65001 assertion test using windows-sys GetConsoleCP/GetConsoleOutputCP
- `src/main.rs` — Updated Processor: WindowContext now holds pty_sender + term; keyboard input forwarding in window_event; TerminalDirty/SetTitle/TerminalExit handling in user_event; resize forwarded to PTY; terminal grid debug logging; #[cfg(test)] mod tests
- `Cargo.toml` — Added alacritty_terminal dependency if not already present from 01-01

## Decisions Made

- **ASCII-only keyboard forwarding for scaffold:** Used `event.text` to extract typed characters and send as bytes. Full escape sequence encoding (Ctrl+C -> ^C, arrow keys, function keys) deferred to Phase 2 Plan 03 where font metrics are available for proper terminal sizing.
- **std::thread for PTY reader:** event_loop.spawn() uses a dedicated OS thread. This is intentional — PTY I/O is blocking and would block the Tokio executor if run as an async task. Research Pitfall 4 explicitly called this out.
- **EventProxy derives Clone:** alacritty_terminal's PtyEventLoop::new() and Term::new() both consume the listener by value. Deriving Clone on EventProxy (which wraps EventLoopProxy, itself Clone+Send) allows creating two independent instances that both funnel to the same winit event loop.
- **Terminal grid logging in scaffold:** Debug logs in RedrawRequested confirm PTY output reaches the Term grid. These logs will be replaced by actual GPU text rendering in Phase 2.

## Deviations from Plan

None — plan executed exactly as written. The alacritty_terminal API matched research notes with only minor adaptation (SizeInfo constructor parameter order verified at compile time, adapted to actual crate signature).

## Issues Encountered

None. The scaffold compiled on first attempt after adapting SizeInfo parameters from compiler feedback. All three automated tests passed. Human-verify confirmed full end-to-end operation including PowerShell 5.1 spawn and keyboard round-trip.

## User Setup Required

None — no external service configuration required. ConPTY and pwsh are standard on Windows 11.

## Next Phase Readiness

- Phase 1 complete: all three scaffold plans done, all success criteria met
- Phase 2 (Rendering) can begin: wgpu surface is ready, Term grid is being updated by PTY, text rendering (glyphon) just needs to read from the grid
- Phase 2 Plan 03 (keyboard encoding) has its requirements defined: full Kitty protocol / VT escape sequence encoding for Ctrl/Alt/arrow/function keys
- Blocker noted: PSReadLine 2.x PreExecution hook API for Phase 3 still needs verification

---
*Phase: 01-scaffold*
*Completed: 2026-03-04*
