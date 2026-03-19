# Bugs & Error Handling Implementation Plan (Branch 1 of 8)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate all panics reachable from user input, add graceful error handling for PTY/GPU/DB failures, and surface errors visibly to users.

**Architecture:** Work bottom-up: quick one-liner fixes first (stabilize base), then PTY error handling refactor, then the large `session()` Option refactor, then GPU recovery, then DB/init hardening.

**Tech Stack:** Rust, parking_lot (new dep for non-poisoning mutex), winit, wgpu, rusqlite, alacritty_terminal

**Branch:** `audit/bugs-error-handling` off `master`

**Spec:** `docs/superpowers/specs/2026-03-18-prelaunch-audit-fixes-design.md` Branch 1

---

### Task 1: Branch setup + quick one-liner fixes (C-1, H-3, H-4, H-6)

**Files:**
- Modify: `src/main.rs:2289,4749` (C-1 checkpoint unwrap)
- Modify: `crates/glass_snapshot/src/pruner.rs:50` (H-3 SystemTime)
- Modify: `crates/glass_history/src/retention.rs:16` (H-3 SystemTime)
- Modify: `crates/glass_core/src/config_watcher.rs:98` (H-4 thread expect)
- Modify: `src/main.rs:438-439` (H-6 child kill/wait)

- [ ] **Step 1: Create branch**

```bash
git checkout -b audit/bugs-error-handling master
```

- [ ] **Step 2: Fix C-1 — checkpoint path unwrap (line 2289)**

In `src/main.rs`, find `cp_path.parent().unwrap()` at line ~2289 and replace:

```rust
// BEFORE:
let _ = std::fs::create_dir_all(cp_path.parent().unwrap());

// AFTER:
if let Some(parent) = cp_path.parent() {
    let _ = std::fs::create_dir_all(parent);
}
```

- [ ] **Step 3: Fix C-1 — checkpoint path unwrap (line 4749)**

Same fix at the second location in `src/main.rs` around line 4749.

- [ ] **Step 4: Fix H-3 — SystemTime unwrap in pruner.rs:50**

```rust
// BEFORE:
let now = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap()
    .as_secs() as i64;

// AFTER:
let now = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap_or(std::time::Duration::ZERO)
    .as_secs() as i64;
```

- [ ] **Step 5: Fix H-3 — SystemTime unwrap in retention.rs:16**

Same pattern in `crates/glass_history/src/retention.rs`.

- [ ] **Step 6: Fix H-4 — config watcher thread expect**

In `crates/glass_core/src/config_watcher.rs:98`:

```rust
// BEFORE:
.expect("Failed to spawn config watcher thread")

// AFTER:
.ok(); // Config hot-reload is nice-to-have, not essential
// Add before the .ok():
if let Err(e) = std::thread::Builder::new().name("glass-config-watcher".into()).spawn(move || { ... }) {
    tracing::warn!("Failed to spawn config watcher thread: {e}");
}
```

Restructure so the `.spawn()` result is checked with `if let Err`.

- [ ] **Step 7: Fix H-6 — child kill/wait logging**

In `src/main.rs:438-439`:

```rust
// BEFORE:
let _ = child.kill();
let _ = child.wait();

// AFTER:
if let Err(e) = child.kill() {
    tracing::debug!("Agent child kill: {e}");
}
if let Err(e) = child.wait() {
    tracing::warn!("Agent child wait failed: {e}");
}
```

- [ ] **Step 8: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 9: Commit**

```bash
git add src/main.rs crates/glass_snapshot/src/pruner.rs crates/glass_history/src/retention.rs crates/glass_core/src/config_watcher.rs
git commit -m "fix: eliminate trivial panics in checkpoint paths, pruner, config watcher

C-1: guard cp_path.parent() with if-let at both sites
H-3: SystemTime unwrap_or(ZERO) in pruner and retention
H-4: config watcher thread spawn -> warn on failure
H-6: log child kill/wait errors instead of silencing"
```

---

### Task 2: Add `parking_lot` dependency for non-poisoning mutex (M-13)

**Files:**
- Modify: `Cargo.toml` (root)
- Modify: `src/main.rs:1413` (agent stdin mutex)

- [ ] **Step 1: Add parking_lot to root Cargo.toml**

```toml
[dependencies]
parking_lot = "0.12"
```

- [ ] **Step 2: Replace std::sync::Mutex with parking_lot::Mutex**

In `src/main.rs:1412-1413`:

```rust
// BEFORE:
let shared_writer = std::sync::Arc::new(std::sync::Mutex::new(BufWriter::new(stdin)));

// AFTER:
let shared_writer = std::sync::Arc::new(parking_lot::Mutex::new(BufWriter::new(stdin)));
```

Update all `.lock()` call sites on `shared_writer` and `writer_clone`. The current code uses `if let Ok(mut w) = writer_clone.lock()` — change to `let mut w = writer_clone.lock();` since `parking_lot::Mutex::lock()` returns the guard directly (no `Result`).

- [ ] **Step 3: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock src/main.rs
git commit -m "fix(M-13): switch agent stdin to parking_lot::Mutex to prevent poisoning"
```

---

### Task 3: PTY error handling — return Result from spawn (H-1/H-8)

**Files:**
- Modify: `crates/glass_terminal/Cargo.toml` (add anyhow dependency)
- Modify: `crates/glass_terminal/src/pty.rs:200-258` (spawn_pty function)
- Modify: `src/main.rs` (call site of spawn_pty)

- [ ] **Step 0: Add anyhow to glass_terminal**

In `crates/glass_terminal/Cargo.toml` under `[dependencies]`, add:
```toml
anyhow.workspace = true
```

- [ ] **Step 1: Change spawn_pty return type to Result**

In `crates/glass_terminal/src/pty.rs`, change the 4 `expect()` calls to `?`:

```rust
// Line 200: PTY creation
let mut pty = tty::new(&options, window_size, 0)
    .map_err(|e| anyhow::anyhow!("Failed to spawn PTY: {e}"))?;

// Line 221: Poller
let poll: Arc<polling::Poller> = polling::Poller::new()
    .map_err(|e| anyhow::anyhow!("Failed to create poller: {e}"))?;

// Line 235: Register
unsafe {
    pty.register(&poll, interest, poll_opts)
        .map_err(|e| anyhow::anyhow!("Failed to register PTY with poller: {e}"))?;
}

// Line 258: Thread spawn
std::thread::Builder::new()
    .name("Glass PTY reader".into())
    .spawn(move || { /* ... */ })
    .map_err(|e| anyhow::anyhow!("Failed to spawn PTY reader thread: {e}"))?;
```

Update the function signature to return `anyhow::Result<(PtySender, ...)>` (match current return tuple).

- [ ] **Step 2: Handle error at call site in main.rs**

Where `spawn_pty` is called (in session creation), handle the Result:

```rust
match spawn_pty(/* args */) {
    Ok((pty_sender, term, ...)) => { /* existing code */ }
    Err(e) => {
        tracing::error!("PTY spawn failed: {e}");
        // Show error in status bar or create an error session
        return;
    }
}
```

- [ ] **Step 3: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add crates/glass_terminal/Cargo.toml crates/glass_terminal/src/pty.rs src/main.rs
git commit -m "fix(H-1/H-8): PTY spawn returns Result instead of panicking

Convert 4 expect() calls to ? propagation. Call site handles
error gracefully instead of crashing the app."
```

---

### Task 4: PTY death feedback via existing TerminalExit event (H-7)

**Files:**
- Modify: `crates/glass_core/src/event.rs` (add exit_code to TerminalExit)
- Modify: `crates/glass_terminal/src/event_proxy.rs:54-58` (pass exit code)
- Modify: `src/main.rs:5943` (display message before closing pane)

**Note:** `AppEvent::TerminalExit` already exists and fires on child exit via `EventProxy`. We extend it with `exit_code` rather than adding a duplicate event.

- [ ] **Step 1: Add exit_code field to TerminalExit**

In `crates/glass_core/src/event.rs`, find the `TerminalExit` variant and add:

```rust
// BEFORE:
TerminalExit { window_id: WindowId, session_id: u64 },

// AFTER:
TerminalExit { window_id: WindowId, session_id: u64, exit_code: Option<i32> },
```

- [ ] **Step 2: Pass exit code from EventProxy**

In `crates/glass_terminal/src/event_proxy.rs`, in the `Event::ChildExit(code)` handler (~line 54), pass the code through:

```rust
Event::ChildExit(code) => {
    let _ = self.proxy.send_event(AppEvent::TerminalExit {
        window_id: self.window_id,
        session_id: self.session_id,
        exit_code: Some(code),
    });
}
```

- [ ] **Step 3: Display message in TerminalExit handler**

In `src/main.rs:5943`, in the `AppEvent::TerminalExit` handler, before closing the pane, inject a visible message:

```rust
AppEvent::TerminalExit { window_id, session_id, exit_code } => {
    if let Some(ctx) = self.windows.get_mut(&window_id) {
        // Show exit message to user before closing
        let msg = match exit_code {
            Some(0) => None, // Don't show message for clean exit
            Some(code) => Some(format!("\r\n[Glass] Shell exited with code {code}\r\n")),
            None => Some("\r\n[Glass] Shell process terminated\r\n".to_string()),
        };
        if let Some(msg) = msg {
            if let Some(session) = ctx.session_mux.session_mut(*session_id) {
                let _ = session.pty_sender.send(PtyMsg::Input(Cow::Owned(msg.into_bytes())));
            }
        }
        // ... existing pane close logic ...
    }
}
```

Fix all other `TerminalExit` pattern matches to include the new `exit_code` field.

- [ ] **Step 4: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add crates/glass_core/src/event.rs crates/glass_terminal/src/event_proxy.rs src/main.rs
git commit -m "feat(H-7): show visible message when shell process exits

Extend TerminalExit with exit_code. Display '[Glass] Shell exited
with code X' before closing the pane for non-zero exit codes."
```

---

### Task 5: PTY send error detection (H-5)

**Files:**
- Modify: `src/main.rs` (13+ `let _ =` sites on pty_sender.send)

- [ ] **Step 1: Create a helper function for PTY sends**

Add near the top of `src/main.rs` (or in a helpers section):

```rust
/// Send a message to the PTY, logging if the channel is dead.
fn pty_send(sender: &std::sync::mpsc::Sender<PtyMsg>, msg: PtyMsg) -> bool {
    match sender.send(msg) {
        Ok(()) => true,
        Err(_) => {
            tracing::debug!("PTY channel closed — shell has exited");
            false
        }
    }
}
```

- [ ] **Step 2: Replace `let _ = session.pty_sender.send(...)` calls**

Go through each site identified (lines ~617, 697, 723, 783, 3705, 3738, 3788, 3820, 4399, 4411, 7128, 8165, 8415) and replace:

```rust
// BEFORE:
let _ = session.pty_sender.send(PtyMsg::Input(Cow::Owned(bytes)));

// AFTER:
pty_send(&session.pty_sender, PtyMsg::Input(Cow::Owned(bytes)));
```

For the resize calls (617, 697, 3705, 3738, 3788, 3820), the return value can be ignored since resize failures on a dead session are harmless. But use `pty_send` for consistency.

For the input sends (723, 4399, 4411, 7128, 8165, 8415), check the return value where it matters — e.g., in the orchestrator loop, a false return should break the orchestrator cycle.

- [ ] **Step 3: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "fix(H-5): detect dead PTY channels instead of silently discarding errors

Add pty_send() helper that logs on closed channel.
Replace 13+ let _ = pty_sender.send() sites."
```

---

### Task 6: Session Option refactor (C-2) — the big one

**Files:**
- Modify: `src/main.rs:231-242` (session/session_mut definitions)
- Modify: `src/main.rs` (all call sites — ~100+)

- [ ] **Step 1: Change session() to return Option**

In `src/main.rs:231-242`:

```rust
// BEFORE:
fn session(&self) -> &Session {
    self.session_mux.focused_session().expect("no focused session")
}
fn session_mut(&mut self) -> &mut Session {
    self.session_mux.focused_session_mut().expect("no focused session")
}

// AFTER:
fn session(&self) -> Option<&Session> {
    self.session_mux.focused_session()
}
fn session_mut(&mut self) -> Option<&mut Session> {
    self.session_mux.focused_session_mut()
}
```

- [ ] **Step 2: Fix all compilation errors**

This will break ~100+ call sites. The strategy:

**Pattern A — guard with `if let` (most common):**
```rust
// BEFORE:
let session = ctx.session();
do_something(session);

// AFTER:
if let Some(session) = ctx.session() {
    do_something(session);
}
```

**Pattern B — early return in functions:**
```rust
// BEFORE:
let session = ctx.session_mut();

// AFTER:
let Some(session) = ctx.session_mut() else { return; };
```

**Pattern C — chains that use session briefly:**
```rust
// BEFORE:
ctx.session().some_field

// AFTER:
ctx.session().map(|s| s.some_field).unwrap_or_default()
```

Work through compilation errors iteratively. Run `cargo build` after every 10-15 fixes to check progress.

- [ ] **Step 3: Build (iterate until clean)**

```bash
cargo build 2>&1
```

Fix remaining errors. This may take several iterations.

- [ ] **Step 4: Run full test suite**

```bash
cargo test --workspace 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "fix(C-2): session()/session_mut() return Option instead of panicking

Eliminates the single biggest crash vector — any session management
bug previously caused an app-wide panic. Now gracefully no-ops when
no session is focused."
```

---

### Task 7: GPU device-lost recovery (H-9)

**Files:**
- Modify: `crates/glass_renderer/src/surface.rs` (add recovery logic)

- [ ] **Step 1: Add consecutive failure counter to GlassRenderer**

In the renderer struct (surface.rs or wherever `GlassRenderer` is defined), add:

```rust
consecutive_surface_failures: u32,
```

Initialize to `0` in `new()`.

- [ ] **Step 2: Track failures in draw()**

In the `draw()` method's error handling (~line 98-100):

```rust
// Current code handles Lost/Outdated by reconfiguring.
// Add: if reconfigure also fails, increment counter.
Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
    self.configure_surface();
    self.consecutive_surface_failures += 1;
    if self.consecutive_surface_failures >= 3 {
        tracing::warn!("3 consecutive surface failures — attempting full GPU reinit");
        // Trigger reinit (may need to send an event to main.rs)
        // For now, log and reset counter
        self.consecutive_surface_failures = 0;
    }
    return;
}
```

On successful frame:
```rust
self.consecutive_surface_failures = 0;
```

- [ ] **Step 3: Add reinit method**

```rust
pub fn reinit(&mut self, window: Arc<winit::window::Window>) {
    // Full GPU reinitialization
    match pollster::block_on(Self::try_new(window)) {
        Ok(new_renderer) => {
            *self = new_renderer;
            tracing::info!("GPU reinitialized successfully after device loss");
        }
        Err(e) => {
            tracing::error!("GPU reinit failed: {e}");
        }
    }
}
```

This requires a `try_new` variant that returns Result instead of panicking — which aligns with the setup branch P-4 fix. For now, add the counter and logging; full reinit can be wired up when P-4 lands.

- [ ] **Step 4: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add crates/glass_renderer/src/surface.rs
git commit -m "fix(H-9): track consecutive GPU surface failures for device-lost recovery

Count consecutive surface acquisition failures. Log warning at 3.
Prepares for full GPU reinit when setup branch converts init to Result."
```

---

### Task 8: Init failure dialogs (M-2, M-3, M-4)

**Files:**
- Modify: `Cargo.toml` (add Win32_UI_WindowsAndMessaging feature to windows-sys)
- Modify: `src/main.rs:9802` (event loop expect)
- Modify: `src/main.rs:9893` (run_app expect)
- Modify: `src/main.rs:2324` (window creation expect)
- Modify: `src/main.rs:2335` (font thread join)
- Modify: `src/main.rs:9981,10008` (dirs::home_dir expect)

- [ ] **Step 0: Add Windows feature flag**

In root `Cargo.toml`, find the `windows-sys` dependency and add `"Win32_UI_WindowsAndMessaging"` to the features list.

- [ ] **Step 1: Add show_fatal_error helper**

Near the top of `src/main.rs`. Note: `windows-sys 0.59` uses `0` as null HWND (isize), not a pointer:

```rust
/// Show a fatal error message and exit. On Windows (where stderr is hidden
/// due to windows_subsystem="windows"), uses a native message box.
fn show_fatal_error(msg: &str) -> ! {
    eprintln!("Glass fatal error: {msg}");
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        let wide_msg: Vec<u16> = msg.encode_utf16().chain(std::iter::once(0)).collect();
        let wide_title: Vec<u16> = "Glass Error".encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            MessageBoxW(0, wide_msg.as_ptr(), wide_title.as_ptr(), MB_ICONERROR | MB_OK);
        }
    }
    std::process::exit(1);
}
```

- [ ] **Step 2: Replace event loop expect (M-2)**

```rust
// BEFORE (line 9802):
.expect("Failed to create event loop");

// AFTER:
.unwrap_or_else(|e| show_fatal_error(&format!("Failed to create event loop: {e}")));
```

- [ ] **Step 3: Replace run_app expect (M-2, line 9893)**

```rust
// BEFORE (line 9893):
event_loop.run_app(&mut processor).expect("Event loop exited with error");

// AFTER:
if let Err(e) = event_loop.run_app(&mut processor) {
    show_fatal_error(&format!("Event loop error: {e}"));
}
```

- [ ] **Step 4: Replace window creation expect (M-2)**

```rust
// BEFORE (line 2324):
.expect("Failed to create window"),

// AFTER:
.unwrap_or_else(|e| show_fatal_error(&format!("Failed to create window: {e}")));
```

- [ ] **Step 5: Handle font thread panic (M-3)**

```rust
// BEFORE (line 2335):
let font_system = font_handle.join().expect("Font system thread panicked");

// AFTER:
let font_system = font_handle.join().unwrap_or_else(|_| {
    tracing::warn!("Font system thread panicked, using default");
    FontSystem::new()
});
```

- [ ] **Step 6: Fix dirs::home_dir expects (M-4)**

```rust
// BEFORE (lines 9981, 10008):
dirs::home_dir().expect("Could not determine home directory")

// AFTER:
dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?
```

- [ ] **Step 7: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml src/main.rs
git commit -m "fix(M-2/M-3/M-4): show native error dialog on init failure

Add show_fatal_error() with Windows MessageBoxW for init failures
that happen before the window exists (event loop, window creation, run_app).
Handle font thread panic with fallback. CLI dirs::home_dir returns error."
```

---

### Task 9: SQLite corruption detection (M-8)

**Files:**
- Modify: `crates/glass_history/src/db.rs:52`
- Modify: `crates/glass_snapshot/src/db.rs:19`
- Modify: `crates/glass_coordination/src/db.rs:26`

- [ ] **Step 1: Add integrity check helper**

Create a shared pattern (or implement in each crate). In each `open()` function, after the connection is opened and PRAGMAs are set:

```rust
// After Connection::open and PRAGMA setup:
match conn.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0)) {
    Ok(result) if result == "ok" => {}
    Ok(result) => {
        tracing::warn!("Database integrity check failed: {result}");
        drop(conn);
        let backup = path.with_extension("db.corrupt");
        tracing::warn!("Renaming corrupt DB to {}", backup.display());
        let _ = std::fs::rename(path, &backup);
        // Reopen fresh
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA busy_timeout = 5000; PRAGMA foreign_keys = ON;")?;
        Self::create_schema(&conn)?;
        // Return fresh DB (skip migrate since schema is new)
        return Ok(Self { conn, /* fields */ });
    }
    Err(e) => {
        tracing::warn!("Integrity check query failed (possibly corrupt): {e}");
        // Same rename-and-recreate as above
    }
}
```

- [ ] **Step 2: Apply to glass_history/src/db.rs**

Add after line 62 (after PRAGMA setup, before create_schema).

- [ ] **Step 3: Apply to glass_snapshot/src/db.rs**

Add after line 29.

- [ ] **Step 4: Apply to glass_coordination/src/db.rs**

Add after line 36.

- [ ] **Step 5: Write test for corruption recovery**

In each crate's test module, add:

```rust
#[test]
fn test_corrupt_db_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.db");
    // Write garbage
    std::fs::write(&path, b"not a sqlite database").unwrap();
    // Should recover by renaming and creating fresh
    let db = HistoryDb::open(&path); // (or SnapshotDb, CoordinationDb)
    assert!(db.is_ok());
    // Corrupt file should be renamed
    assert!(path.with_extension("db.corrupt").exists());
}
```

- [ ] **Step 6: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 7: Commit**

```bash
git add crates/glass_history/src/db.rs crates/glass_snapshot/src/db.rs crates/glass_coordination/src/db.rs
git commit -m "fix(M-8): detect corrupt SQLite databases and auto-recover

Run PRAGMA integrity_check on open. If corrupt, rename to .db.corrupt
and create fresh database. Prevents permanent breakage from disk errors."
```

---

### Task 10: Remaining fixes (M-1, M-5, L-1, L-2)

**Files:**
- Modify: `crates/glass_snapshot/src/watcher.rs:36` (M-5)
- Modify: `crates/glass_core/src/config_watcher.rs:67` (L-1)
- Modify: `crates/glass_feedback/src/lib.rs:374` (L-2)

- [ ] **Step 1: Fix M-5 — watcher channel backpressure warning**

In `crates/glass_snapshot/src/watcher.rs:36`:

```rust
// BEFORE:
tx.send(res).ok();

// AFTER:
if tx.send(res).is_err() {
    // Channel disconnected — receiver dropped
    tracing::debug!("Snapshot watcher event dropped (receiver closed)");
}
```

- [ ] **Step 2: Fix L-1 — config watcher keeps current config on error**

In `crates/glass_core/src/config_watcher.rs:65-69`:

The `ConfigReloaded` event needs to support sending only an error without a new config. Two options:
- Change `config` field to `Option<Box<GlassConfig>>`
- Or send a separate error-only event

Simpler: change to `Option`:

```rust
// In event.rs, ConfigReloaded:
ConfigReloaded {
    config: Option<Box<GlassConfig>>,  // None = keep current
    error: Option<String>,
},

// In config_watcher.rs error branch:
Err(err) => {
    let _ = proxy_clone.send_event(AppEvent::ConfigReloaded {
        config: None,  // Keep current config
        error: Some(err),
    });
}
```

Update the handler in `src/main.rs` to only apply config when `Some`.

- [ ] **Step 3: Fix L-2 — script generation from config**

In `crates/glass_feedback/src/lib.rs:374`:

```rust
// BEFORE:
let script_generation = true;

// AFTER:
let script_generation = state.config.script_generation.unwrap_or(true);
```

Check if `script_generation` field exists on the config struct. If not, this TODO stays as-is until the config field is added.

- [ ] **Step 4: Fix M-1 — add regex compilation tests**

Add tests that exercise each regex `OnceLock` to catch compile errors:

```rust
#[cfg(test)]
mod regex_tests {
    #[test]
    fn all_static_regexes_compile() {
        // Call each OnceLock regex to trigger compilation
        // This catches syntax errors at test time
        let _ = super::SOME_REGEX.get_or_init(|| { /* ... */ });
    }
}
```

Find each regex OnceLock in `src/main.rs` and `crates/glass_errors/` and add a test.

- [ ] **Step 5: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 6: Commit**

```bash
git add crates/glass_snapshot/src/watcher.rs crates/glass_core/src/config_watcher.rs crates/glass_core/src/event.rs crates/glass_feedback/src/lib.rs src/main.rs
git commit -m "fix: remaining medium/low bug fixes

M-1: add regex compilation tests for static OnceLock patterns
M-5: log warning on dropped snapshot watcher events
L-1: keep current config on validation error instead of resetting
L-2: read script_generation from config (or default true)"
```

---

### Task 11: Final verification and clippy

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
git commit -m "chore: clippy and fmt cleanup for bugs-error-handling branch"
```

- [ ] **Step 5: Summary — verify all items addressed**

Check off against the spec:
- [x] C-1: checkpoint path unwrap (Task 1)
- [x] C-2: session Option refactor (Task 6)
- [x] H-1/H-8: PTY spawn Result (Task 3)
- [x] H-3: SystemTime unwrap (Task 1)
- [x] H-4: config watcher thread (Task 1)
- [x] H-5: PTY send errors (Task 5)
- [x] H-6: child kill/wait (Task 1)
- [x] H-7: SessionExited event (Task 4)
- [x] H-9: GPU device-lost (Task 7)
- [x] M-1: regex tests (Task 10)
- [x] M-2: init failure dialogs (Task 8)
- [x] M-3: font thread panic (Task 8)
- [x] M-4: dirs::home_dir (Task 8)
- [x] M-5: watcher backpressure (Task 10)
- [x] M-8: SQLite corruption (Task 9)
- [x] M-13: parking_lot mutex (Task 2)
- [x] L-1: config error handling (Task 10)
- [x] L-2: script generation config (Task 10)
