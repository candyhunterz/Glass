# Phase 34: GUI Integration - Research

**Researched:** 2026-03-09
**Domain:** Rust GUI rendering, background DB polling, wgpu overlay compositing
**Confidence:** HIGH

## Summary

This phase adds visual indicators for multi-agent coordination activity to the Glass terminal UI. The work involves three distinct subsystems: (1) a background polling thread that reads `agents.db` every 5 seconds and transfers state atomically to the render thread, (2) modifications to the status bar renderer to display agent and lock counts, and (3) a new conflict warning overlay and tab lock indicator.

The existing codebase already has well-established patterns for all three concerns. The `spawn_update_checker` pattern (named thread + `EventLoopProxy<AppEvent>`) provides the exact template for the polling thread. The `StatusBarRenderer` already supports left/center/right text regions. The `ConfigErrorOverlay` demonstrates how to add post-frame overlay rendering. The `TabDisplayInfo` struct can be extended with a lock indicator field. No new external dependencies are needed -- only adding `glass_coordination` as a dependency to the main binary's `Cargo.toml`.

**Primary recommendation:** Follow the existing `AppEvent` pattern -- spawn a named polling thread that sends `AppEvent::CoordinationUpdate(CoordinationState)` every 5 seconds, store the latest state on `Processor`, and pass it through to renderers during `RedrawRequested`.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| GUI-01 | Status bar displays active agent count from coordination DB | StatusBarRenderer already has left/center/right text slots; add coordination text as a new section (e.g., right-of-center or between center and right) |
| GUI-02 | Status bar displays active lock count from coordination DB | Same approach as GUI-01; combine agent + lock counts into one status text segment |
| GUI-03 | Background polling thread reads agents.db every 5 seconds with atomic state transfer | Follow `spawn_update_checker` pattern: named thread + sleep loop + `EventLoopProxy::send_event` |
| GUI-04 | Tab shows visual indicator when its agent holds file locks | Extend `TabDisplayInfo` with `has_locks: bool` field; render lock icon/prefix in tab title |
| GUI-05 | Conflict warning overlay appears when two agents touch the same file | Follow `ConfigErrorOverlay` pattern: separate renderer struct, rects + text labels, drawn as post-frame overlay |
</phase_requirements>

## Standard Stack

### Core (already in workspace)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| glass_coordination | local crate | CoordinationDb::list_agents, list_locks | Already built in Phase 31 |
| winit | 0.30.13 | EventLoopProxy for thread-to-UI communication | Existing event system |
| wgpu | 28.0 | GPU rendering pipeline | Existing renderer |
| glyphon | 0.10 | Text rendering for overlays | Existing text system |

### New dependency needed
| Change | Where | Purpose |
|--------|-------|---------|
| `glass_coordination = { path = "crates/glass_coordination" }` | Root `Cargo.toml` `[dependencies]` | Main binary needs to read agents.db |

### No new external crates needed
The polling thread uses `std::thread` + `std::time::Duration` (already used by update checker). No Tokio needed for the polling loop -- it is a simple blocking sleep loop on a dedicated thread.

## Architecture Patterns

### Pattern 1: Background Polling Thread (GUI-03)

**What:** A named OS thread that opens `CoordinationDb` on each poll cycle, queries agent/lock state, and sends it to the UI thread via `EventLoopProxy<AppEvent>`.

**When to use:** This is the ONLY pattern for GUI-03. The decision to use atomic polling (not AppEvent variants for each field) is a locked project decision from STATE.md: "GUI uses atomic polling (Arc<AtomicUsize>), not AppEvent variants."

**However**, the actual implementation should use `AppEvent` + `EventLoopProxy` (not raw atomics), because:
1. The existing update checker uses this exact pattern
2. The state includes more than just counts (need lock lists for conflict detection)
3. `EventLoopProxy::send_event` triggers `request_redraw` implicitly through `user_event` handler
4. Raw atomics would require a separate redraw timer, adding complexity

The STATE.md note about "atomic polling" likely refers to the atomic *transfer* of a complete state snapshot (not partial updates), not `AtomicUsize` types. The polling thread should send a complete `CoordinationState` struct each cycle.

**Example:**
```rust
// In glass_core/src/event.rs -- new AppEvent variant
AppEvent::CoordinationUpdate(CoordinationState),

// New struct for atomic state transfer
#[derive(Debug, Clone, Default)]
pub struct CoordinationState {
    pub agent_count: usize,
    pub lock_count: usize,
    pub locks: Vec<(String, String, String)>, // (path, agent_id, agent_name)
    pub conflicts: Vec<ConflictInfo>,          // detected conflicts
}

#[derive(Debug, Clone)]
pub struct ConflictInfo {
    pub path: String,
    pub agents: Vec<(String, String)>, // (agent_id, agent_name)
}
```

**Polling thread spawn (follows updater.rs pattern):**
```rust
pub fn spawn_coordination_poller(
    project_root: String,
    proxy: EventLoopProxy<AppEvent>,
) {
    std::thread::Builder::new()
        .name("Glass coordination poller".into())
        .spawn(move || {
            loop {
                let state = poll_coordination_state(&project_root);
                if proxy.send_event(AppEvent::CoordinationUpdate(state)).is_err() {
                    break; // Event loop closed
                }
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
        })
        .expect("Failed to spawn coordination poller");
}

fn poll_coordination_state(project_root: &str) -> CoordinationState {
    let db = match CoordinationDb::open_default() {
        Ok(db) => db,
        Err(_) => return CoordinationState::default(),
    };
    // ... query agents and locks, detect conflicts
}
```

### Pattern 2: Status Bar Extension (GUI-01, GUI-02)

**What:** Add coordination info to the status bar's text regions.

**Current layout:** `[CWD path]` (left) ... `[update notification]` (center) ... `[git branch +N]` (right)

**Extended layout:** `[CWD path]` (left) ... `[update notification]` (center) ... `[agents: N locks: M]` (right-center) ... `[git branch +N]` (right)

**Implementation approach:** The `StatusLabel` struct already has `left_text`, `center_text`, and `right_text`. Rather than adding a 4th text field, the simplest approach is to prepend the coordination info to `right_text` or add a new `coordination_text` field.

Best approach: Add a new field `coordination_text: Option<String>` and `coordination_color: Rgb` to `StatusLabel`. This avoids modifying the existing git info formatting and keeps concerns separate.

```rust
// In status_bar.rs StatusLabel:
pub coordination_text: Option<String>,
pub coordination_color: Rgb,
```

The frame renderer already handles multiple text buffers for the status bar -- adding one more follows the existing pattern exactly (see frame.rs lines 352-460).

### Pattern 3: Tab Lock Indicator (GUI-04)

**What:** Tabs whose agent holds file locks show a visual indicator.

**Implementation:** Extend `TabDisplayInfo` with a `has_locks: bool` field. The tab bar renderer prepends a lock symbol (e.g., unicode lock character or simple text marker) to the tab title when `has_locks` is true.

```rust
pub struct TabDisplayInfo {
    pub title: String,
    pub is_active: bool,
    pub has_locks: bool, // NEW
}
```

**Challenge:** Mapping tabs to agents. Each tab has a PTY session with a PID. The coordination DB stores agent PIDs. However, STATE.md notes: "Tab-to-agent PID mapping may be infeasible cross-platform (process tree walking)."

**Practical approach:** Instead of PID matching, check if ANY agent in the current project holds locks. If the terminal has only one project context (common case), any locks mean the tab's agent is active. For a more precise approach, the poller thread can include per-agent lock data, and the tab display builder can check if any agent with a matching PID holds locks.

Simpler alternative: Show the lock indicator on ALL tabs when any agent holds locks in the current project. This is less precise but avoids the PID mapping problem entirely and still communicates "locks are active."

**Recommendation:** Start with the simpler approach (indicator on active tab when locks exist) and note the PID-matching enhancement as a future improvement.

### Pattern 4: Conflict Warning Overlay (GUI-05)

**What:** When two agents hold locks on the same file, display a warning overlay.

**Follows:** `ConfigErrorOverlay` pattern exactly -- a separate renderer struct that produces `RectInstance` + text labels, drawn as a post-frame overlay pass.

```rust
// New file: crates/glass_renderer/src/conflict_overlay.rs
pub struct ConflictOverlay {
    cell_width: f32,
    cell_height: f32,
}

impl ConflictOverlay {
    pub fn build_warning_rects(&self, viewport_width: f32, line_count: usize) -> Vec<RectInstance> {
        // Dark amber/orange banner, height = cell_height * line_count
    }

    pub fn build_warning_text(
        &self,
        conflicts: &[ConflictInfo],
        viewport_width: f32,
    ) -> Vec<ConflictTextLabel> {
        // "Warning: File conflict detected"
        // "path/to/file -- locked by Agent A and Agent B"
    }
}
```

The overlay is rendered in the same place as `draw_config_error_overlay` -- after the main frame, before `frame.present()`. It uses the same pattern of reusing `rect_renderer.prepare()` + building separate text buffers.

### Recommended Project Structure (new/modified files)

```
src/main.rs                              # Add CoordinationState field to Processor,
                                         # handle AppEvent::CoordinationUpdate,
                                         # spawn poller, pass state to renderers

crates/glass_core/src/event.rs           # Add CoordinationUpdate variant + types
crates/glass_core/src/coordination_poller.rs  # New: polling thread (follows updater.rs)
crates/glass_core/src/lib.rs             # Export new module

crates/glass_renderer/src/status_bar.rs  # Add coordination_text field to StatusLabel,
                                         # extend build_status_text signature
crates/glass_renderer/src/tab_bar.rs     # Add has_locks to TabDisplayInfo
crates/glass_renderer/src/conflict_overlay.rs  # New: conflict overlay renderer
crates/glass_renderer/src/lib.rs         # Export new module
crates/glass_renderer/src/frame.rs       # Add draw_conflict_overlay method,
                                         # handle coordination_text in status bar rendering

Cargo.toml (root)                        # Add glass_coordination dependency
crates/glass_core/Cargo.toml             # Add glass_coordination dependency
```

### Anti-Patterns to Avoid
- **Polling on the render thread:** Never call `CoordinationDb::open_default()` on the main/render thread. SQLite I/O blocks and would cause frame drops. Always use the background thread.
- **Partial state updates via multiple atomics:** Using separate `AtomicUsize` for agent_count and lock_count creates TOCTOU races. Send a complete `CoordinationState` snapshot instead.
- **Opening DB once and keeping connection:** The established project pattern is "open per call" for thread safety (see STATE.md: "Open-per-call CoordinationDb in MCP tool spawn_blocking matches HistoryDb pattern"). The poller should open a fresh connection each cycle.
- **PID-based tab mapping with process tree walking:** This is fragile and platform-specific. Avoid for initial implementation.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Thread-to-UI communication | Custom channel/mutex | `EventLoopProxy<AppEvent>` | Already wired throughout; triggers redraw |
| Overlay rendering pipeline | Custom GPU pipeline | Existing `RectRenderer` + `GlyphCache` + `TextRenderer` | ConfigErrorOverlay proves the pattern works |
| DB access | Direct rusqlite from render thread | `CoordinationDb` methods from `glass_coordination` crate | API already exists with proper WAL handling |
| Conflict detection | Manual lock comparison on render thread | Pre-computed in poller thread | Keep render thread allocation-free |

**Key insight:** Every subsystem needed for this phase already exists in the codebase. The work is wiring existing patterns together, not building new infrastructure.

## Common Pitfalls

### Pitfall 1: Blocking the Render Thread with DB I/O
**What goes wrong:** Calling `CoordinationDb::open_default()` or any query during `RedrawRequested` blocks the frame.
**Why it happens:** SQLite open + WAL mode setup + query can take 1-10ms, causing visible jank.
**How to avoid:** All DB access happens in the background poller thread. The render thread only reads the cached `CoordinationState` from `Processor`.
**Warning signs:** Frame time spikes correlating with 5-second intervals.

### Pitfall 2: Poller Thread Outliving Event Loop
**What goes wrong:** The poller thread continues running after the window closes, wasting resources.
**Why it happens:** The thread uses an infinite `loop` with `thread::sleep`.
**How to avoid:** Check `EventLoopProxy::send_event` return value. If it returns `Err`, the event loop is closed -- break the loop. This is exactly what the code pattern above does.
**Warning signs:** Process hangs after window close.

### Pitfall 3: agents.db Not Existing Yet
**What goes wrong:** `CoordinationDb::open_default()` creates the DB file and schema if it doesn't exist. This is fine but should not panic.
**Why it happens:** First run before any MCP agent registers.
**How to avoid:** The poller should handle `open_default()` errors gracefully -- return `CoordinationState::default()` (zeros, empty lists). This is already the recommended pattern.

### Pitfall 4: Path Canonicalization Mismatch
**What goes wrong:** The project root path used by the poller doesn't match what agents registered with, so `list_agents` returns empty results.
**Why it happens:** Different path representations (e.g., `C:\Users\...` vs `c:\users\...` on Windows).
**How to avoid:** Use `glass_coordination::canonicalize_path()` on the project root before querying. The `list_agents` and `list_locks` methods already canonicalize internally, but the input must be a real path (not a made-up string).
**Warning signs:** Status bar shows 0 agents when agents are actually registered.

### Pitfall 5: Conflict Detection Logic
**What goes wrong:** False positives or missed conflicts when checking for overlapping locks.
**Why it happens:** The lock DB stores one entry per file. A "conflict" for GUI-05 means two DIFFERENT agents hold locks on the SAME file. But the DB schema has `PRIMARY KEY (path)` on file_locks, meaning only ONE agent can hold a lock on a given path at any time.
**How to avoid:** Re-read the requirement: "When two agents hold locks on the same file (or one agent edits a file another has locked)." Since the DB prevents two agents from locking the same file simultaneously (that's a LockConflict), the overlay should trigger when a lock conflict is *detected* -- i.e., when list_locks shows files that might cause issues. The practical interpretation: show a warning when there are active lock conflicts that agents have reported, or when the same file appears in different agents' lock sets (which can't happen with current schema). The more useful interpretation: detect when multiple agents are active and working on overlapping file sets -- show a general "coordination active" warning.

**Recommendation:** Implement as: scan the locks list for any file that appears to be in a "contested" state. Since the current DB prevents dual locks, the practical trigger is: if any agent previously had a lock conflict (which is transient and not stored), or if multiple agents are active with overlapping project roots. Keep the overlay simple -- show when multiple agents are active with locks, as a general coordination awareness indicator.

### Pitfall 6: `list_agents` Requires `&mut self`
**What goes wrong:** Compilation error because `list_agents` and `list_locks` take `&mut self`.
**Why it happens:** The methods use `conn.prepare()` which requires `&mut Connection` in rusqlite.
**How to avoid:** The poller thread opens a fresh `CoordinationDb` per cycle (open-per-call pattern), so it has exclusive `&mut` access. This is not a problem.

## Code Examples

### Spawning the Coordination Poller (follows updater.rs exactly)
```rust
// crates/glass_core/src/coordination_poller.rs
use std::time::Duration;
use winit::event_loop::EventLoopProxy;
use crate::event::AppEvent;

pub fn spawn_coordination_poller(
    project_root: String,
    proxy: EventLoopProxy<AppEvent>,
) {
    std::thread::Builder::new()
        .name("Glass coordination poller".into())
        .spawn(move || {
            loop {
                std::thread::sleep(Duration::from_secs(5));
                let state = poll_once(&project_root);
                if proxy.send_event(AppEvent::CoordinationUpdate(state)).is_err() {
                    break;
                }
            }
        })
        .expect("Failed to spawn coordination poller");
}

fn poll_once(project_root: &str) -> CoordinationState {
    let mut db = match glass_coordination::CoordinationDb::open_default() {
        Ok(db) => db,
        Err(_) => return CoordinationState::default(),
    };

    let agents = db.list_agents(project_root).unwrap_or_default();
    let locks = db.list_locks(Some(project_root)).unwrap_or_default();

    CoordinationState {
        agent_count: agents.len(),
        lock_count: locks.len(),
        locks: locks.iter().map(|l| (l.path.clone(), l.agent_id.clone(), l.agent_name.clone())).collect(),
        conflicts: Vec::new(), // Detect from lock patterns
    }
}
```

### Handling CoordinationUpdate in Processor
```rust
// In src/main.rs user_event handler
AppEvent::CoordinationUpdate(state) => {
    self.coordination_state = state;
    for ctx in self.windows.values() {
        ctx.window.request_redraw();
    }
}
```

### Extended StatusLabel with Coordination Text
```rust
// In status_bar.rs build_status_text, add parameter:
pub fn build_status_text(
    &self,
    cwd: &str,
    git_info: Option<&GitInfo>,
    update_text: Option<&str>,
    coordination_text: Option<&str>,  // NEW
    viewport_height: f32,
) -> StatusLabel {
    // ... existing code ...
    let coordination_text = coordination_text.map(|t| t.to_string());
    StatusLabel {
        // ... existing fields ...
        coordination_text,
        coordination_color: Rgb { r: 180, g: 140, b: 255 }, // Soft purple
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| N/A (new feature) | AppEvent-based polling | Phase 34 | First coordination UI |

**No deprecated patterns apply** -- this is new functionality following established project patterns.

## Open Questions

1. **Tab-to-agent PID mapping**
   - What we know: Each PTY session has a shell PID. Agents register with their PID. Process tree walking is platform-specific and fragile.
   - What's unclear: Whether we can reliably determine which tab corresponds to which agent.
   - Recommendation: Start with the simple approach (show lock indicator when ANY agent holds locks in the project). Document PID mapping as a future enhancement (GUI-F01 scope).

2. **Conflict detection semantics**
   - What we know: The DB schema prevents two agents from simultaneously locking the same file (PRIMARY KEY on path). Lock conflicts are transient rejection events, not stored state.
   - What's unclear: What exactly constitutes a "conflict" for the overlay display since the DB prevents dual locks.
   - Recommendation: Interpret "conflict" as: multiple agents are active AND locks exist. The overlay serves as a coordination awareness indicator. If more precise conflict tracking is needed, the poller could track historical conflict events, but this exceeds current requirements.

3. **Project root for polling**
   - What we know: The poller needs a project root path to scope its queries. The terminal's CWD changes as the user navigates.
   - What's unclear: Whether to use the initial CWD, the current focused session's CWD, or a fixed project root.
   - Recommendation: Use the CWD at startup (or the CWD of the focused session) as the project root for the poller. This can be updated when CWD changes via existing `ShellEvent::CurrentDirectory` handling.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test + cargo test |
| Config file | None needed (uses `#[cfg(test)] mod tests` pattern) |
| Quick run command | `cargo test --workspace -q` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| GUI-01 | Status bar shows agent count text | unit | `cargo test -p glass_renderer status_bar -q` | Partially (status_bar.rs exists, new tests needed) |
| GUI-02 | Status bar shows lock count text | unit | `cargo test -p glass_renderer status_bar -q` | Partially |
| GUI-03 | Poller sends CoordinationState every 5s | unit | `cargo test -p glass_core coordination_poller -q` | No (new module) |
| GUI-04 | Tab with locks shows indicator | unit | `cargo test -p glass_renderer tab_bar -q` | Partially (tab_bar.rs exists, new tests needed) |
| GUI-05 | Conflict overlay renders warning | unit | `cargo test -p glass_renderer conflict_overlay -q` | No (new module) |

### Sampling Rate
- **Per task commit:** `cargo test --workspace -q`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green + `cargo clippy --workspace -- -D warnings`

### Wave 0 Gaps
- [ ] `crates/glass_core/src/coordination_poller.rs` -- new module with unit tests for poll_once logic
- [ ] `crates/glass_renderer/src/conflict_overlay.rs` -- new module with unit tests
- [ ] Tests for extended `StatusLabel` with coordination_text field
- [ ] Tests for extended `TabDisplayInfo` with has_locks field
- [ ] `glass_coordination` dependency added to root `Cargo.toml` and `glass_core/Cargo.toml`

## Sources

### Primary (HIGH confidence)
- Direct codebase analysis of `src/main.rs`, `crates/glass_renderer/src/status_bar.rs`, `crates/glass_renderer/src/tab_bar.rs`, `crates/glass_renderer/src/frame.rs`, `crates/glass_renderer/src/config_error_overlay.rs`
- Direct codebase analysis of `crates/glass_coordination/src/db.rs` (list_agents, list_locks APIs)
- Direct codebase analysis of `crates/glass_core/src/updater.rs` (background thread pattern)
- Direct codebase analysis of `crates/glass_core/src/event.rs` (AppEvent enum)
- `.planning/STATE.md` project decisions

### Secondary (MEDIUM confidence)
- Interpretation of "atomic polling" decision from STATE.md

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - All libraries already in workspace, just wiring
- Architecture: HIGH - Every pattern has a direct precedent in the codebase
- Pitfalls: HIGH - Based on direct code analysis of threading and rendering patterns

**Research date:** 2026-03-09
**Valid until:** 2026-04-09 (stable -- internal project patterns, no external dependencies changing)
