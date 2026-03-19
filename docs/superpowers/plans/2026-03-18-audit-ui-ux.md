# UI/UX Polish Implementation Plan (Branch 6 of 8)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship onboarding, discoverability, visual polish, and interaction improvements so Glass feels complete to first-time and power users alike.

**Architecture:** Work in four waves — (A) quick surgical fixes that touch one file each, (B) medium features that touch 2-3 files, (C) the theme extraction refactor, (D) low-priority polish. Each wave ends with a CI-clean commit.

**Tech Stack:** Rust, wgpu, winit, glyphon, toml (config), alacritty_terminal

**Branch:** `audit/ui-ux` off `master`

**Spec:** `docs/superpowers/specs/2026-03-18-prelaunch-audit-fixes-design.md` Branch 6

---

### Task 1: Branch setup + search overlay label (UX-6)

**Files:**
- Modify: `crates/glass_renderer/src/search_overlay_renderer.rs:123`

- [ ] **Step 1: Create branch**

```bash
git checkout -b audit/ui-ux master
```

- [ ] **Step 2: Change search label text**

In `crates/glass_renderer/src/search_overlay_renderer.rs:123`, change the format string:

```rust
// BEFORE:
text: format!("Search: {}", query),

// AFTER:
text: format!("Search History: {}", query),
```

- [ ] **Step 3: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add crates/glass_renderer/src/search_overlay_renderer.rs
git commit -m "fix(UX-6): rename search overlay label to 'Search History:'"
```

---

### Task 2: Exit code in badge (UX-8)

**Files:**
- Modify: `crates/glass_renderer/src/block_renderer.rs:104,166-186`

- [ ] **Step 1: Widen badge from 3 to 5 cells**

In `crates/glass_renderer/src/block_renderer.rs`, find both occurrences of:

```rust
let badge_width = self.cell_width * 3.0;
```

Replace with:

```rust
let badge_width = self.cell_width * 5.0;
```

There are two sites: one in `build_block_rects` (~line 104) and one in `build_block_text` (~line 166).

- [ ] **Step 2: Change badge text from "OK"/"X" to "OK"/"E:{code}"**

In `build_block_text` (~line 168-186), change the non-zero branch:

```rust
// BEFORE:
} else {
    (
        "X".to_string(),
        Rgb { r: 255, g: 255, b: 255 },
    )
};

// AFTER:
} else {
    (
        format!("E:{}", exit_code),
        Rgb { r: 255, g: 255, b: 255 },
    )
};
```

- [ ] **Step 3: Adjust badge_x centering**

The badge_x calculation uses `badge_width` to position text. After widening, verify the text is still centered within the badge. The existing formula places text at `badge_x + cell_width` from left edge. With 5-cell width, the text anchor should be approximately `badge_x + cell_width * 0.5` for longer strings, or keep the existing offset and let glyphon handle it.

- [ ] **Step 4: Update `decoration_cluster_width` if it references badge_width**

Search for `decoration_cluster_width` in block_renderer.rs and update any hardcoded badge width of `3.0` to `5.0`.

- [ ] **Step 5: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 6: Commit**

```bash
git add crates/glass_renderer/src/block_renderer.rs
git commit -m "feat(UX-8): show exit code in badge — 'E:1', 'E:127' instead of 'X'

Widen badge from 3 to 5 cells to accommodate exit code text."
```

---

### Task 3: Running command indicator (UX-3)

**Files:**
- Modify: `crates/glass_renderer/src/block_renderer.rs` (build_block_rects + build_block_text)

Currently `build_block_rects` and `build_block_text` only render decorations for blocks with `exit_code` (i.e., Complete state). Executing blocks have no visual indicator.

- [ ] **Step 1: Add elapsed timer rect for Executing blocks**

In `build_block_rects`, after the existing `if let Some(exit_code) = block.exit_code { ... }` block, add a separate branch for executing blocks:

```rust
// Running indicator for blocks in Executing state
if block.exit_code.is_none() && block.state == BlockState::Executing {
    // Pulsing/static indicator badge — same position as exit code badge
    let badge_width = self.cell_width * 5.0;
    let badge_x = viewport_width - badge_width - SCROLLBAR_WIDTH;

    // Opaque background
    let cluster_width = badge_width;
    let padding = self.cell_width;
    let cluster_x = viewport_width - cluster_width - SCROLLBAR_WIDTH - padding;
    rects.push(RectInstance {
        pos: [cluster_x, y, cluster_width + SCROLLBAR_WIDTH + padding, self.cell_height],
        color: [0.102, 0.102, 0.102, 1.0],
    });

    // Blue badge for "running"
    rects.push(RectInstance {
        pos: [badge_x, y, badge_width, self.cell_height],
        color: [30.0 / 255.0, 120.0 / 255.0, 200.0 / 255.0, 1.0],
    });
}
```

This requires importing `BlockState` into block_renderer.rs. Check if the `Block` struct already carries `state` — it should, since `block_manager.rs` defines it.

- [ ] **Step 2: Add elapsed timer text for Executing blocks**

In `build_block_text`, after the exit code label logic, add:

```rust
// Running elapsed timer for Executing blocks
if block.exit_code.is_none() && block.state == BlockState::Executing {
    if let Some(started) = block.started_at {
        let elapsed = started.elapsed();
        let secs = elapsed.as_secs();
        let text = if secs >= 3600 {
            format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
        } else if secs >= 60 {
            format!("{}m{}s", secs / 60, secs % 60)
        } else {
            format!("{}s", secs)
        };

        let badge_width = self.cell_width * 5.0;
        let badge_x = viewport_width - badge_width - SCROLLBAR_WIDTH + self.cell_width * 0.5;
        labels.push(BlockLabel {
            x: badge_x,
            y,
            text,
            color: Rgb { r: 255, g: 255, b: 255 },
        });
    }
}
```

- [ ] **Step 3: Verify Block struct has `started_at` field**

Check `crates/glass_terminal/src/block_manager.rs` for a `started_at: Option<Instant>` field on `Block`. If it exists, use it. If not, add it — set to `Some(Instant::now())` when state transitions to `Executing` (in the `CommandExecuted` OSC handler, ~line 199).

- [ ] **Step 4: Ensure redraw triggers during execution**

The timer updates once per second. Verify that the existing render loop (dirty flag or periodic redraw) fires at least once per second when a command is executing. If not, the orchestrator's silence tracker already fires periodic events, which may suffice. If needed, add a 1-second `request_redraw()` timer for executing blocks.

- [ ] **Step 5: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 6: Commit**

```bash
git add crates/glass_renderer/src/block_renderer.rs crates/glass_terminal/src/block_manager.rs
git commit -m "feat(UX-3): show live elapsed timer for running commands

Blue badge with elapsed time (e.g., '12s', '3m15s') on blocks in
Executing state. Updates per-second alongside normal redraws."
```

---

### Task 4: CWD dynamic truncation (UX-5)

**Files:**
- Modify: `crates/glass_renderer/src/status_bar.rs:192-196`

- [ ] **Step 1: Pass viewport_width to `build_status_label`**

The function already receives `viewport_height`. Check if `viewport_width` is also available. If not, add it as a parameter.

- [ ] **Step 2: Replace hardcoded 60-char limit with dynamic calculation**

```rust
// BEFORE:
let left_text = if cwd.len() > 60 {
    format!("...{}", &cwd[cwd.len() - 57..])
} else {
    cwd.to_string()
};

// AFTER:
// Dynamic CWD truncation: use roughly half the viewport width for CWD,
// leaving room for right-side elements (git branch, agent cost, etc.).
let max_cwd_chars = ((viewport_width / self.cell_width) as usize / 2).max(20);
let left_text = if cwd.len() > max_cwd_chars {
    format!("...{}", &cwd[cwd.len() - (max_cwd_chars - 3)..])
} else {
    cwd.to_string()
};
```

Note: `cell_width` may not be available on `StatusBarRenderer`. If not, pass it via parameter or store it during construction (it is available in the frame renderer that creates the status bar renderer — see `frame.rs:104`).

- [ ] **Step 3: Handle byte boundary safety**

The existing code uses byte slicing (`&cwd[cwd.len() - 57..]`) which can panic on non-ASCII paths. Fix:

```rust
// Safe truncation for Unicode paths
let left_text = if cwd.chars().count() > max_cwd_chars {
    let skip = cwd.chars().count() - (max_cwd_chars - 3);
    format!("...{}", cwd.chars().skip(skip).collect::<String>())
} else {
    cwd.to_string()
};
```

- [ ] **Step 4: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add crates/glass_renderer/src/status_bar.rs
git commit -m "fix(UX-5): CWD truncation adapts to viewport width

Replace hardcoded 60-char limit with dynamic calculation based on
viewport width. Also fixes potential panic on non-ASCII CWD paths."
```

---

### Task 5: Undo feedback via PTY injection (UX-2)

**Files:**
- Modify: `src/main.rs:4218-4298` (Ctrl+Shift+Z undo handler)

- [ ] **Step 1: Inject visible message after successful undo**

After the undo summary tracing line (~line 4269-4272), inject a message into the PTY so the user sees feedback in the terminal:

```rust
// After the tracing::info! summary line:
let summary = format!(
    "\r\n\x1b[36m[Glass]\x1b[0m Undo: {} restored, {} deleted, {} conflicts, {} errors\r\n",
    restored, deleted, conflicts, errors,
);
let _ = session.pty_sender.send(PtyMsg::InjectOutput(summary.into_bytes()));
```

Check how orchestrator messages are injected into the PTY. The mechanism might use `PtyMsg::InjectOutput` or write directly to the terminal via `term.lock()`. Follow the existing pattern.

If there's no `InjectOutput` variant, write directly to the terminal grid:

```rust
// Alternative: write directly to terminal
let summary_bytes = summary.as_bytes().to_vec();
let mut term = session.term.lock();
for byte in &summary_bytes {
    term.input(*byte);
}
```

Or use the `process_input_bytes` pattern if the terminal supports it.

- [ ] **Step 2: Add status bar flash for "Nothing to undo"**

For the `Ok(None)` branch (~line 4290-4292), add a brief status bar message:

```rust
Ok(None) => {
    tracing::info!("Nothing to undo -- no file-modifying commands found");
    // Flash message in status bar
    ctx.status_message = Some((
        "Nothing to undo".to_string(),
        std::time::Instant::now(),
    ));
}
```

Check if `ctx` (WindowContext) has a `status_message` field for transient messages. If not, add one:
- Add `pub status_message: Option<(String, std::time::Instant)>` to the WindowContext struct
- In the status bar render path, display it and clear after 3 seconds

- [ ] **Step 3: Add status bar flash for undo errors**

For the `Err(e)` branch (~line 4293-4295):

```rust
Err(e) => {
    tracing::error!("Undo failed: {}", e);
    ctx.status_message = Some((
        format!("Undo failed: {}", e),
        std::time::Instant::now(),
    ));
}
```

- [ ] **Step 4: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat(UX-2): visible undo feedback in terminal and status bar

Inject '[Glass] Undo: N restored, M conflicts' into terminal output.
Show 'Nothing to undo' as status bar flash. Show error in status bar."
```

---

### Task 6: Pipeline panel Escape dismiss (UX-4)

**Files:**
- Modify: `src/main.rs` (key handler section, near Escape handlers for search/settings/activity)
- Modify: `crates/glass_renderer/src/block_renderer.rs` or pipeline visualization renderer (hint text)

- [ ] **Step 1: Find where pipeline expansion state is tracked**

The pipeline panel is toggled via `block.toggle_pipeline_expanded()` at `main.rs:4324`. Find the `pipeline_expanded` field on Block. The panel is per-block, not a global overlay.

- [ ] **Step 2: Add Escape handler to collapse expanded pipeline**

In the key handler section, before the fallthrough to normal input (but after settings/activity overlay Escape handlers), add:

```rust
// Escape: collapse any expanded pipeline block
if event.state == ElementState::Pressed {
    if let Key::Named(NamedKey::Escape) = &event.logical_key {
        if let Some(session) = ctx.session_mux.focused_session_mut() {
            let collapsed = session.block_manager.blocks_mut().iter_mut().any(|b| {
                if b.pipeline_expanded {
                    b.pipeline_expanded = false;
                    true
                } else {
                    false
                }
            });
            if collapsed {
                ctx.window.request_redraw();
                return;
            }
        }
    }
}
```

Place this after the search overlay Escape check but before input passthrough. The Escape key should cascade: search overlay > settings overlay > activity overlay > pipeline panel > normal terminal.

- [ ] **Step 3: Add "[Esc] close" hint in pipeline panel header**

Find where the pipeline panel header is rendered. This is likely in `block_renderer.rs` or a pipeline-specific renderer. Add a hint label:

```rust
// In the pipeline expansion header rendering:
labels.push(BlockLabel {
    x: panel_right - self.cell_width * 12.0,
    y: header_y,
    text: "[Esc] close".to_string(),
    color: Rgb { r: 120, g: 120, b: 120 },
});
```

Locate the exact rendering site by searching for `pipeline_expanded` in the renderer crate.

- [ ] **Step 4: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add src/main.rs crates/glass_renderer/src/block_renderer.rs
git commit -m "feat(UX-4): Escape key dismisses expanded pipeline panel

Add Escape handler in key cascade after overlay checks.
Show '[Esc] close' hint in pipeline panel header."
```

---

### Task 7: Active tab accent underline (UX-7)

**Files:**
- Modify: `crates/glass_renderer/src/tab_bar.rs`

- [ ] **Step 1: Add accent underline to active tab**

In the tab bar `build_rects` method (or equivalent), after the active tab background rect is pushed, add a 2px cornflower-blue underline:

```rust
// After the active tab background rect:
if is_active {
    // 2px cornflower blue accent underline
    rects.push(RectInstance {
        pos: [tab_x, tab_y + tab_height - 2.0, tab_width, 2.0],
        color: [100.0 / 255.0, 149.0 / 255.0, 237.0 / 255.0, 1.0], // cornflower blue
    });
}
```

Find the exact rendering loop in `tab_bar.rs` where each tab's background rect is emitted. The active tab uses `ACTIVE_TAB_COLOR` — add the underline right after that rect.

- [ ] **Step 2: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 3: Commit**

```bash
git add crates/glass_renderer/src/tab_bar.rs
git commit -m "feat(UX-7): cornflower blue accent underline on active tab

2px underline at bottom of active tab for clearer visual distinction."
```

---

### Task 8: Shift+Up/Down single-line scrollback (UX-9)

**Files:**
- Modify: `src/main.rs:4775-4790` (scrollback handler)

- [ ] **Step 1: Add Shift+ArrowUp/Down handlers**

In the existing Shift+PageUp/Down handler block (~line 4776-4790), add ArrowUp and ArrowDown:

```rust
// Shift+PageUp/Down: scrollback
if modifiers.shift_key() && !modifiers.control_key() && !modifiers.alt_key() {
    match &event.logical_key {
        Key::Named(NamedKey::PageUp) => {
            ctx.session().term.lock().scroll_display(Scroll::PageUp);
            ctx.window.request_redraw();
            return;
        }
        Key::Named(NamedKey::PageDown) => {
            ctx.session().term.lock().scroll_display(Scroll::PageDown);
            ctx.window.request_redraw();
            return;
        }
        // UX-9: Single-line scrollback
        Key::Named(NamedKey::ArrowUp) => {
            ctx.session().term.lock().scroll_display(Scroll::Delta(1));
            ctx.window.request_redraw();
            return;
        }
        Key::Named(NamedKey::ArrowDown) => {
            ctx.session().term.lock().scroll_display(Scroll::Delta(-1));
            ctx.window.request_redraw();
            return;
        }
        _ => {}
    }
}
```

Note: `Scroll::Delta` takes an `i32` — positive scrolls up, negative scrolls down. Verify this matches alacritty_terminal's convention. If the API differs, check the `Scroll` enum in alacritty_terminal.

Note: `ctx.session()` may return `Option` if the C-2 refactor from Branch 1 has landed. If so, guard with `if let Some(session) = ctx.session()`.

- [ ] **Step 2: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat(UX-9): Shift+Up/Down for single-line scrollback

Complements existing Shift+PageUp/Down with line-by-line scrolling."
```

---

### Task 9: Alt+Arrow conflict guard (UX-11)

**Files:**
- Modify: `src/main.rs:4820-4875`

- [ ] **Step 1: Add alternate screen mode check**

The spec says: only intercept Alt+Arrow for pane focus when pane count > 1 AND not in alternate screen mode. The pane count check already exists at line 4853. Add the alternate screen check:

```rust
// BEFORE (line 4851-4853):
} else {
    // Alt+Arrow: move focus
    if ctx.session_mux.active_tab_pane_count() > 1 {

// AFTER:
} else {
    // Alt+Arrow: move focus (only when multi-pane and not in alternate screen)
    let in_alt_screen = ctx.session_mux.focused_session()
        .map(|s| s.term.lock().mode().contains(alacritty_terminal::term::TermMode::ALT_SCREEN))
        .unwrap_or(false);
    if ctx.session_mux.active_tab_pane_count() > 1 && !in_alt_screen {
```

Check the exact import path for `TermMode::ALT_SCREEN`. It may be `alacritty_terminal::vte::ansi::Mode` or similar. Search for `ALT_SCREEN` in the codebase.

- [ ] **Step 2: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "fix(UX-11): don't intercept Alt+Arrow in single-pane or alternate screen mode

Prevents conflict with readline Alt+Arrow word movement and
vim/tmux/etc. navigation inside alternate screen apps."
```

---

### Task 10: Config error path in banner (UX-12)

**Files:**
- Modify: `crates/glass_core/src/config.rs:441-448` (error branch)
- Modify: `src/main.rs` (config error display, if applicable)

- [ ] **Step 1: Include path in config parse error message**

In `crates/glass_core/src/config.rs`, the `load_from_str` method (~line 458) already logs the parse error. But the user-facing error banner (if one exists) should include the config file path.

Check how config errors are surfaced to the user. If the config watcher sends `ConfigReloaded { error: Some(...) }`, ensure the error string includes the path:

```rust
// In config_watcher.rs or config.rs where the error is formatted:
Err(err) => {
    let error_msg = format!(
        "Config error in {}: {}",
        config_path.display(),
        err
    );
    tracing::warn!("{}", error_msg);
    // Send error_msg to UI
}
```

- [ ] **Step 2: In main.rs, show config path in any error banner/status bar text**

Search for where config errors are displayed to the user (likely in the `ConfigReloaded` event handler). Ensure the path `~/.glass/config.toml` is included.

- [ ] **Step 3: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add crates/glass_core/src/config.rs crates/glass_core/src/config_watcher.rs src/main.rs
git commit -m "fix(UX-12): include config file path in error banner

Shows '~/.glass/config.toml' in error messages so users know which
file to fix."
```

---

### Task 11: Tab bar overflow with scroll arrows (UX-10)

**Files:**
- Modify: `crates/glass_renderer/src/tab_bar.rs`

- [ ] **Step 1: Add scroll state to TabBarRenderer**

```rust
pub struct TabBarRenderer {
    // existing fields...
    /// First visible tab index when tabs overflow
    pub scroll_offset: usize,
}
```

- [ ] **Step 2: Detect overflow in build_rects**

In the tab rendering loop, calculate total tab width. If it exceeds `viewport_width - NEW_TAB_BUTTON_WIDTH - arrow_width * 2`, enable overflow mode:

```rust
let total_tabs_width = tab_count as f32 * (tab_width + TAB_GAP);
let available_width = viewport_width - NEW_TAB_BUTTON_WIDTH - 2.0 * ARROW_BUTTON_WIDTH;
let overflow = total_tabs_width > available_width;
```

- [ ] **Step 3: Render scroll arrows when overflowing**

When overflow is detected, render left/right arrow buttons:

```rust
if overflow && self.scroll_offset > 0 {
    // Left arrow: "<" button at x=0
    rects.push(RectInstance {
        pos: [0.0, 0.0, ARROW_BUTTON_WIDTH, tab_height],
        color: [45.0 / 255.0, 45.0 / 255.0, 45.0 / 255.0, 1.0],
    });
}
if overflow && (self.scroll_offset + visible_count) < tab_count {
    // Right arrow: ">" button at right edge before "+"
    let arrow_x = viewport_width - NEW_TAB_BUTTON_WIDTH - ARROW_BUTTON_WIDTH;
    rects.push(RectInstance {
        pos: [arrow_x, 0.0, ARROW_BUTTON_WIDTH, tab_height],
        color: [45.0 / 255.0, 45.0 / 255.0, 45.0 / 255.0, 1.0],
    });
}
```

Add constant: `const ARROW_BUTTON_WIDTH: f32 = 24.0;`

- [ ] **Step 4: Only render visible tabs**

Modify the tab rendering loop to skip tabs before `scroll_offset` and stop once the available width is exhausted.

- [ ] **Step 5: Keep "+" button always visible at right edge**

Ensure the "+" new tab button is always rendered at `viewport_width - NEW_TAB_BUTTON_WIDTH`, regardless of scroll state.

- [ ] **Step 6: Add hit-test for arrow buttons**

In the `hit_test` method, add `TabHitResult::ScrollLeft` and `TabHitResult::ScrollRight` variants (or handle internally by adjusting `scroll_offset`).

- [ ] **Step 7: Wire scroll arrow clicks in main.rs**

In the mouse click handler for tab bar hits, handle the new arrow variants by adjusting `tab_bar.scroll_offset`.

- [ ] **Step 8: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 9: Commit**

```bash
git add crates/glass_renderer/src/tab_bar.rs src/main.rs
git commit -m "feat(UX-10): tab bar overflow with scroll arrows

When tabs exceed viewport width, show left/right scroll arrows.
'+' new tab button stays visible at right edge."
```

---

### Task 12: First-run onboarding (UX-1)

**Files:**
- Modify: `crates/glass_core/src/config.rs` (first-run detection)
- Create: state tracking in `~/.glass/state.toml`
- Modify: `src/main.rs` (show overlay, status bar hint)
- Modify: `crates/glass_renderer/src/frame.rs` or new overlay renderer (welcome overlay)

- [ ] **Step 1: Add state.toml loading**

In `crates/glass_core/src/config.rs` (or a new `state.rs` module), add:

```rust
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct GlassState {
    /// Number of sessions launched
    pub session_count: u32,
}

impl GlassState {
    pub fn state_path() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".glass").join("state.toml"))
    }

    pub fn load() -> Self {
        let Some(path) = Self::state_path() else { return Self::default() };
        match std::fs::read_to_string(&path) {
            Ok(contents) => toml::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let Some(path) = Self::state_path() else { return };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(contents) = toml::to_string_pretty(self) {
            let _ = std::fs::write(&path, contents);
        }
    }

    pub fn is_first_run(&self) -> bool {
        self.session_count == 0
    }

    pub fn should_show_hint(&self) -> bool {
        self.session_count < 5
    }
}
```

Add `serde` derive if not already available in glass_core.

- [ ] **Step 2: Load state at startup and increment session count**

In `src/main.rs` app initialization:

```rust
let mut glass_state = GlassState::load();
let is_first_run = glass_state.is_first_run();
let show_hint = glass_state.should_show_hint();
glass_state.session_count += 1;
glass_state.save();
```

Store `show_hint` in the App struct for status bar rendering.

- [ ] **Step 3: Show welcome overlay on first run**

If `is_first_run`, set a flag that causes a simple overlay to render:

```rust
if is_first_run {
    self.welcome_overlay_visible = true;
}
```

Render a centered semi-transparent overlay with text:
- "Welcome to Glass"
- "Press Ctrl+Shift+, for settings & shortcuts"
- "[Enter] to dismiss"

Implement as a simple rect + text labels in the frame renderer, similar to the search overlay.

- [ ] **Step 4: Dismiss welcome overlay on Enter or Escape**

In the key handler:

```rust
if self.welcome_overlay_visible && event.state == ElementState::Pressed {
    match &event.logical_key {
        Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Escape) => {
            self.welcome_overlay_visible = false;
            ctx.window.request_redraw();
            return;
        }
        _ => { return; } // Swallow all keys while overlay is shown
    }
}
```

- [ ] **Step 5: Show status bar hint for first 5 sessions**

In the status bar rendering path, if `show_hint` is true, append a hint to the center text:

```rust
// In status bar label building:
if self.show_settings_hint {
    // Show as center text or append to coordination text area
    center_text = Some("Tip: Ctrl+Shift+, = settings & shortcuts".to_string());
}
```

- [ ] **Step 6: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 7: Commit**

```bash
git add crates/glass_core/src/config.rs crates/glass_core/src/lib.rs src/main.rs crates/glass_renderer/src/frame.rs
git commit -m "feat(UX-1): first-run onboarding overlay and settings hint

Detect first launch via ~/.glass/state.toml session counter.
Show welcome overlay with shortcut hint on first run.
Show 'Ctrl+Shift+, = settings' hint in status bar for first 5 sessions."
```

---

### Task 13: Theme support — extract hardcoded chrome colors (UX-13)

**Files:**
- Modify: `crates/glass_core/src/config.rs` (add `[theme]` section)
- Modify: `crates/glass_renderer/src/frame.rs:106-110` (terminal bg)
- Modify: `crates/glass_renderer/src/status_bar.rs:114-119` (status bar bg)
- Modify: `crates/glass_renderer/src/block_renderer.rs:99` (block separator)
- Modify: `crates/glass_renderer/src/tab_bar.rs:49-84` (tab colors)
- Modify: `crates/glass_renderer/src/search_overlay_renderer.rs:61-64` (search overlay)

This is the largest single task. It touches every renderer component.

- [ ] **Step 1: Define ThemeConfig in config.rs**

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ThemeConfig {
    /// Preset name: "dark" (default) or "light"
    pub preset: String,

    // Terminal
    pub terminal_bg: [u8; 3],

    // Tab bar
    pub tab_bar_bg: [u8; 3],
    pub tab_active_bg: [u8; 3],
    pub tab_inactive_bg: [u8; 3],
    pub tab_accent: [u8; 3],

    // Status bar
    pub status_bar_bg: [u8; 3],

    // Block decorations
    pub block_separator: [u8; 3],
    pub badge_success: [u8; 3],
    pub badge_error: [u8; 3],
    pub badge_running: [u8; 3],

    // Search overlay
    pub search_backdrop: [f32; 4],  // RGBA with alpha
    pub search_input_bg: [u8; 3],
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self::dark()
    }
}

impl ThemeConfig {
    pub fn dark() -> Self {
        Self {
            preset: "dark".to_string(),
            terminal_bg: [26, 26, 26],
            tab_bar_bg: [30, 30, 30],
            tab_active_bg: [50, 50, 50],
            tab_inactive_bg: [35, 35, 35],
            tab_accent: [100, 149, 237],
            status_bar_bg: [38, 38, 38],
            block_separator: [60, 60, 60],
            badge_success: [40, 160, 40],
            badge_error: [200, 50, 50],
            badge_running: [30, 120, 200],
            search_backdrop: [0.05, 0.05, 0.05, 0.85],
            search_input_bg: [56, 56, 56],
        }
    }

    pub fn light() -> Self {
        Self {
            preset: "light".to_string(),
            terminal_bg: [250, 250, 250],
            tab_bar_bg: [235, 235, 235],
            tab_active_bg: [255, 255, 255],
            tab_inactive_bg: [225, 225, 225],
            tab_accent: [70, 130, 210],
            status_bar_bg: [230, 230, 230],
            block_separator: [200, 200, 200],
            badge_success: [50, 180, 50],
            badge_error: [220, 60, 60],
            badge_running: [40, 140, 220],
            search_backdrop: [0.95, 0.95, 0.95, 0.9],
            search_input_bg: [240, 240, 240],
        }
    }

    /// Convert a [u8; 3] color to wgpu-style [f32; 4] with alpha 1.0
    pub fn to_f32_rgba(color: [u8; 3]) -> [f32; 4] {
        [
            color[0] as f32 / 255.0,
            color[1] as f32 / 255.0,
            color[2] as f32 / 255.0,
            1.0,
        ]
    }
}
```

- [ ] **Step 2: Add `[theme]` to GlassConfig**

```rust
pub struct GlassConfig {
    // existing fields...
    #[serde(default)]
    pub theme: ThemeConfig,
}
```

Handle preset loading: if `preset` is set to "light", apply light defaults for any unspecified fields.

- [ ] **Step 3: Thread ThemeConfig through to renderers**

The theme needs to reach each renderer. Options:
1. Pass `&ThemeConfig` to each `build_rects` / `build_text` call
2. Store a `ThemeConfig` clone on each renderer struct, updated on config reload

Option 2 is cleaner. Add `theme: ThemeConfig` to `FrameRenderer` and propagate to sub-renderers.

In `FrameRenderer::new()`, accept a `ThemeConfig` parameter and pass it down:

```rust
pub fn new(/* existing params */, theme: ThemeConfig) -> Self {
    let default_bg = Rgb {
        r: theme.terminal_bg[0],
        g: theme.terminal_bg[1],
        b: theme.terminal_bg[2],
    };
    // ...
}
```

- [ ] **Step 4: Replace hardcoded colors in frame.rs**

```rust
// BEFORE:
let default_bg = Rgb { r: 26, g: 26, b: 26 };

// AFTER:
let default_bg = Rgb {
    r: self.theme.terminal_bg[0],
    g: self.theme.terminal_bg[1],
    b: self.theme.terminal_bg[2],
};
```

- [ ] **Step 5: Replace hardcoded colors in status_bar.rs**

```rust
// BEFORE:
[38.0 / 255.0, 38.0 / 255.0, 38.0 / 255.0, 1.0]

// AFTER:
ThemeConfig::to_f32_rgba(self.theme.status_bar_bg)
```

- [ ] **Step 6: Replace hardcoded colors in block_renderer.rs**

```rust
// Separator (line 99):
color: ThemeConfig::to_f32_rgba(self.theme.block_separator),

// Badge success (line 124-125):
color: ThemeConfig::to_f32_rgba(self.theme.badge_success),

// Badge error (line 128):
color: ThemeConfig::to_f32_rgba(self.theme.badge_error),
```

- [ ] **Step 7: Replace hardcoded colors in tab_bar.rs**

Replace the constants `BAR_BG_COLOR`, `ACTIVE_TAB_COLOR`, `INACTIVE_TAB_COLOR` with theme-derived values. Since constants can't reference struct fields, either:
- Remove the constants and compute inline from `self.theme`
- Or keep the constants as fallbacks and override from theme

- [ ] **Step 8: Replace hardcoded colors in search_overlay_renderer.rs**

```rust
// Backdrop (line 63):
color: self.theme.search_backdrop,

// Input box (line 74):
color: ThemeConfig::to_f32_rgba(self.theme.search_input_bg),
```

- [ ] **Step 9: Handle theme hot-reload**

In the `ConfigReloaded` event handler in `src/main.rs`, propagate the new theme to the frame renderer:

```rust
// After applying new config:
ctx.frame_renderer.update_theme(config.theme.clone());
```

Add `update_theme(&mut self, theme: ThemeConfig)` to `FrameRenderer` that propagates to sub-renderers.

- [ ] **Step 10: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 11: Add unit test for theme preset loading**

```rust
#[test]
fn test_theme_presets() {
    let dark = ThemeConfig::dark();
    assert_eq!(dark.terminal_bg, [26, 26, 26]);

    let light = ThemeConfig::light();
    assert_eq!(light.terminal_bg, [250, 250, 250]);
}

#[test]
fn test_theme_from_config_toml() {
    let toml = r#"
    [theme]
    preset = "light"
    "#;
    let config: GlassConfig = toml::from_str(toml).unwrap();
    // Verify light preset values applied
}
```

- [ ] **Step 12: Commit**

```bash
git add crates/glass_core/src/config.rs crates/glass_renderer/src/frame.rs \
    crates/glass_renderer/src/status_bar.rs crates/glass_renderer/src/block_renderer.rs \
    crates/glass_renderer/src/tab_bar.rs crates/glass_renderer/src/search_overlay_renderer.rs \
    src/main.rs
git commit -m "feat(UX-13): extract hardcoded chrome colors into [theme] config

Add ThemeConfig with 'dark' (default) and 'light' presets.
All chrome colors (terminal bg, tab bar, status bar, block decorations,
search overlay) now read from config. Hot-reloadable."
```

---

### Task 14: Low-priority polish — Ctrl+C copy, docs, misc (UX-14, UX-15, UX-16, UX-18, UX-19, UX-20)

**Files:**
- Modify: `src/main.rs` (Ctrl+C, split depth feedback, tab title)
- Modify: `crates/glass_renderer/src/block_renderer.rs` (separator config)
- Modify: `crates/glass_core/src/config.rs` (padding, separator config)

- [ ] **Step 1: UX-14 — Ctrl+C copy with selection**

Find the Ctrl+C handler in `src/main.rs`. Currently it likely always sends SIGINT. Change to:

```rust
// Ctrl+C handler:
if modifiers.control_key() && !modifiers.shift_key() {
    if let Key::Character(c) = &event.logical_key {
        if c.as_str().eq_ignore_ascii_case("c") {
            if let Some(session) = ctx.session_mux.focused_session() {
                let has_selection = session.term.lock().selection_to_string().is_some();
                if has_selection {
                    // Copy to clipboard
                    if let Some(text) = session.term.lock().selection_to_string() {
                        ctx.clipboard.set_contents(text).ok();
                        // Clear selection after copy
                        session.term.lock().selection = None;
                        ctx.window.request_redraw();
                        return;
                    }
                }
            }
            // No selection — fall through to send SIGINT as normal
        }
    }
}
```

Check how clipboard access works — it may use `arboard` or `winit`'s clipboard. Follow the existing copy pattern (likely Ctrl+Shift+C already copies).

- [ ] **Step 2: UX-15 — Document D/E split mnemonic**

In the settings/shortcuts overlay renderer, find where keyboard shortcuts are listed. Add tooltip or description text:

```
Ctrl+Shift+D  Split pane Down (horizontal)
Ctrl+Shift+E  Split pane East/right (vertical)
```

- [ ] **Step 3: UX-16 — Tab title from CWD**

Find where tab titles are set/displayed. When a session's CWD changes (detected via OSC 7 or shell integration), update the tab title:

```rust
// In the CWD update handler:
if let Some(tab) = ctx.session_mux.active_tab_mut() {
    let dir_name = cwd.rsplit('/').next().or_else(|| cwd.rsplit('\\').next()).unwrap_or(&cwd);
    tab.title = dir_name.to_string();
}
```

- [ ] **Step 4: UX-18 — Configurable block separator**

Add to config:

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct BlockConfig {
    pub separator_thickness: f32,  // default 2.0
    // separator color is in ThemeConfig
}
```

In `block_renderer.rs`, use `self.separator_thickness` instead of hardcoded `1.0`:

```rust
pos: [0.0, y, viewport_width, self.separator_thickness],
```

- [ ] **Step 5: UX-19 — Terminal content padding**

Add to config:

```rust
pub struct TerminalConfig {
    pub padding_x: f32,  // default 4.0
    pub padding_y: f32,  // default 4.0
}
```

Apply in the grid renderer or frame composition where terminal content offset is calculated.

- [ ] **Step 6: UX-20 — Max split depth feedback**

Find the split handler in main.rs. When the max depth is reached (currently silently refuses), show a status bar message:

```rust
if !ctx.session_mux.can_split() {
    ctx.status_message = Some((
        "Maximum split depth reached".to_string(),
        std::time::Instant::now(),
    ));
    ctx.window.request_redraw();
    return;
}
```

- [ ] **Step 7: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 8: Commit**

```bash
git add src/main.rs crates/glass_renderer/src/block_renderer.rs crates/glass_core/src/config.rs
git commit -m "feat: low-priority UX polish batch

UX-14: Ctrl+C copies when selection active, SIGINT otherwise
UX-15: D/E mnemonic in shortcuts overlay
UX-16: auto-update tab title from CWD
UX-18: configurable block separator thickness
UX-19: terminal content padding config fields
UX-20: status bar message when max split depth reached"
```

---

### Task 15: Pipeline click-to-expand and tab context menu (UX-17, UX-21)

**Files:**
- Modify: `src/main.rs` (mouse handlers)
- Modify: `crates/glass_renderer/src/block_renderer.rs` (click indicators)
- Modify: `crates/glass_renderer/src/tab_bar.rs` (context menu)

- [ ] **Step 1: UX-17 — Pipeline stage click-to-expand**

In the mouse click handler, add hit-testing for pipeline [+]/[-] indicators:

1. In block_renderer, add `hit_test_pipeline(x, y, blocks, display_offset) -> Option<(block_index, stage_index)>`
2. In the mouse handler in main.rs, call the hit test and toggle the relevant stage's expanded state

This requires rendering clickable [+]/[-] indicators next to each pipeline stage. Add them in `build_block_text`.

- [ ] **Step 2: UX-21 — Tab context menu on right-click**

Add right-click detection on tab bar:

1. In the mouse handler, detect right-click on a tab
2. Show a context menu with: Rename, Duplicate, Close Others
3. Implement as a simple overlay similar to the settings overlay

For MVP, implement as keyboard shortcuts shown in a tooltip, or a minimal rect+text popup.

Context menu state:

```rust
struct TabContextMenu {
    visible: bool,
    tab_index: usize,
    x: f32,
    y: f32,
    selected_item: usize,
}
```

Handle Enter to execute selected action, Escape to dismiss.

- [ ] **Step 3: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add src/main.rs crates/glass_renderer/src/block_renderer.rs crates/glass_renderer/src/tab_bar.rs
git commit -m "feat: pipeline click-to-expand and tab context menu

UX-17: click [+]/[-] indicators to expand/collapse pipeline stages
UX-21: right-click tab for Rename, Duplicate, Close Others context menu"
```

---

### Task 16: Final verification and clippy

- [ ] **Step 1: Run clippy**

```bash
cargo clippy --workspace -- -D warnings 2>&1
```

Fix any warnings.

- [ ] **Step 2: Run fmt**

```bash
cargo fmt --all -- --check 2>&1
```

Fix any formatting issues.

- [ ] **Step 3: Run full test suite**

```bash
cargo test --workspace 2>&1
```

- [ ] **Step 4: Commit any cleanup**

```bash
git add -A
git commit -m "chore: clippy and fmt cleanup for ui-ux branch"
```

- [ ] **Step 5: Summary — verify all items addressed**

Check off against the spec:
- [x] UX-1: First-run onboarding (Task 12)
- [x] UX-2: Undo feedback (Task 5)
- [x] UX-3: Running command indicator (Task 3)
- [x] UX-4: Pipeline panel dismiss (Task 6)
- [x] UX-5: CWD dynamic truncation (Task 4)
- [x] UX-6: Search overlay label (Task 1)
- [x] UX-7: Active tab indicator (Task 7)
- [x] UX-8: Exit code in badge (Task 2)
- [x] UX-9: Shift+Up/Down scrolling (Task 8)
- [x] UX-10: Tab bar overflow (Task 11)
- [x] UX-11: Alt+Arrow conflict (Task 9)
- [x] UX-12: Config error path (Task 10)
- [x] UX-13: Theme support (Task 13)
- [x] UX-14: Ctrl+C copy (Task 14)
- [x] UX-15: D/E split mnemonic (Task 14)
- [x] UX-16: Tab title from CWD (Task 14)
- [x] UX-17: Pipeline click-to-expand (Task 15)
- [x] UX-18: Block separator config (Task 14)
- [x] UX-19: Terminal padding config (Task 14)
- [x] UX-20: Max split depth feedback (Task 14)
- [x] UX-21: Tab context menu (Task 15)
