# Bugs & Error Handling Audit

**Project:** Glass GPU-accelerated terminal emulator
**Date:** 2026-03-18
**Scope:** Prelaunch readiness review of error handling, panic risk, and failure modes
**Methodology:** Static analysis of all crates + main binary for unwrap/expect calls, silent error swallowing, failure mode gaps, race conditions, and known TODOs.

## Summary

Glass has 1,386 `.unwrap()` calls and 173 `.expect()` calls across 65 source files. The vast majority (~85%) are in test code (`#[cfg(test)]` modules), which is acceptable. The remaining production unwraps fall into three categories:

1. **Init-only (acceptable):** PTY spawn, wgpu surface/device creation, event loop, regex compilation via `OnceLock` -- these fail early with clear messages.
2. **Guarded (safe):** Unwraps preceded by an existence check (e.g., `is_some_and()` followed by `.unwrap()`).
3. **Hot-path (risky):** Unwraps in rendering loops, JSON processing, file I/O paths that could trigger runtime panics.

The `let _ =` pattern is used extensively (148 occurrences) -- most are intentional fire-and-forget (event proxy sends, file writes for diagnostics), but some silently swallow errors that should at minimum be logged.

Overall error handling is **good for a pre-alpha/beta product** but needs hardening in several areas before a public launch.

---

## 1. Unwrap/Expect Calls

### Critical

#### C-1: `cp_path.parent().unwrap()` in checkpoint paths (2 locations)
- **File:** `src/main.rs`, lines 2289 and 4749
- **Code:** `let _ = std::fs::create_dir_all(cp_path.parent().unwrap());`
- **Current behavior:** Panics if `cp_path` is a root path (e.g., `/` or `C:\`) since `parent()` returns `None` for root paths.
- **Risk:** If the orchestrator's `cwd` is somehow empty or root, the entire application crashes. The `cp_path` is constructed from user-configured `checkpoint_path` joined to `cwd`.
- **Severity:** Critical -- user-controlled input path leads to potential panic in the hot event loop.
- **Recommendation:** Replace with `if let Some(parent) = cp_path.parent() { let _ = std::fs::create_dir_all(parent); }`.

#### C-2: `WindowContext::session()` and `session_mut()` expect calls
- **File:** `src/main.rs`, lines 234, 241
- **Code:** `.expect("no focused session")`
- **Current behavior:** Panics if no session is focused. These methods are called throughout the event loop.
- **Risk:** If tab closing or session management has a bug that leaves no focused session, the entire app crashes. This is a single-point-of-failure called from hundreds of sites.
- **Severity:** Critical -- any session management bug cascades into an app crash.
- **Recommendation:** Return `Option<&Session>` / `Option<&mut Session>` and handle at call sites, or add a runtime invariant check that guarantees a session always exists.

### High

#### H-1: PTY spawn uses `.expect()` (3 locations)
- **File:** `crates/glass_terminal/src/pty.rs`, lines 203, 221, 235, 258
- **Code:** `tty::new(...).expect("Failed to spawn PTY")`, `Poller::new().expect(...)`, `pty.register(...).expect(...)`, thread spawn `.expect(...)`
- **Current behavior:** Panics on PTY spawn failure.
- **Risk:** On systems with restricted resources (no available PTYs, ConPTY disabled, sandboxed environments), the app crashes instead of showing an error dialog. This is init-only but affects the user experience significantly.
- **Severity:** High -- PTY failure is a realistic scenario (corporate lockdown, WSL misconfiguration).
- **Recommendation:** Return `Result` from `spawn_pty()` and show a user-facing error message in the window.

#### H-2: wgpu surface/adapter/device `.expect()` (3 locations)
- **File:** `crates/glass_renderer/src/surface.rs`, lines 32, 41, 49
- **Code:** `.expect("Failed to create wgpu surface")`, `.expect("No compatible GPU adapter found")`, `.expect("Failed to create wgpu device")`
- **Current behavior:** Panics if GPU initialization fails.
- **Risk:** Machines without DX12/Vulkan/Metal support, remote desktop sessions, or headless environments will crash with no user-facing error. GPU driver crashes during the app lifecycle are not handled (device lost after init).
- **Severity:** High -- GPU availability is not guaranteed, especially over RDP/VNC.
- **Recommendation:** Return `Result` from `GlassRenderer::new()` and show a fallback error window or console message. Consider a software rasterizer fallback.

#### H-3: `SystemTime::now().duration_since(UNIX_EPOCH).unwrap()` in production paths
- **File:** `crates/glass_snapshot/src/pruner.rs`, line 50; `crates/glass_history/src/retention.rs`, line 16
- **Current behavior:** Panics if the system clock is before Unix epoch.
- **Risk:** While rare, clock skew on certain VMs or misconfigured systems can cause this. These are called during background pruning operations.
- **Severity:** High -- background pruning crash leaves app running but snapshots/history grow unbounded.
- **Recommendation:** Use `.unwrap_or(Duration::ZERO)` or handle the error gracefully.

#### H-4: Config watcher thread `.expect()`
- **File:** `crates/glass_core/src/config_watcher.rs`, line 98
- **Code:** `.expect("Failed to spawn config watcher thread")`
- **Current behavior:** Panics if the OS refuses to create a new thread.
- **Risk:** Under extreme resource pressure or OS-level thread limits, the app crashes on startup.
- **Severity:** High -- thread spawn failure should be logged, not fatal.
- **Recommendation:** Use `.ok()` and log a warning; config hot-reload is a nice-to-have, not essential.

### Medium

#### M-1: Regex `.unwrap()` in `OnceLock` initializers
- **Files:** `src/main.rs` lines 1661-1665; `crates/glass_errors/src/generic.rs` lines 19, 30, 37; `crates/glass_errors/src/rust_human.rs`
- **Current behavior:** Panics if regex compilation fails.
- **Risk:** These are compile-time-constant regex patterns, so the unwrap is practically safe. However, if a regex is ever changed to include a syntax error, it panics at first use.
- **Severity:** Medium -- practically safe but violates defense-in-depth.
- **Recommendation:** Add `#[cfg(test)]` unit tests that exercise each regex to catch compilation errors at test time. Consider using `regex!` macro or asserting at build time.

#### M-2: Event loop and window creation `.expect()`
- **File:** `src/main.rs`, lines 9802, 2324, 9893
- **Current behavior:** Panics on event loop or window creation failure.
- **Risk:** These are init-only and extremely unlikely to fail, but the error message goes to stderr which is hidden on Windows (due to `#![windows_subsystem = "windows"]`).
- **Severity:** Medium -- user sees no error on failure, just a silent crash.
- **Recommendation:** Use a native message box (e.g., `MessageBoxW` on Windows) for init failures before the window exists.

#### M-3: Font system thread `.expect()`
- **File:** `src/main.rs`, line 2335
- **Code:** `font_handle.join().expect("Font system thread panicked")`
- **Current behavior:** Panics if the font enumeration thread panicked.
- **Risk:** Font system failures (corrupted font files, out of memory) cascade into app crash.
- **Severity:** Medium -- could happen on systems with corrupted font caches.

#### M-4: `dirs::home_dir().expect()` in CLI subcommands
- **File:** `src/main.rs`, lines 9981, 10008
- **Current behavior:** Panics if home directory cannot be determined during `profile export/import`.
- **Risk:** Sandboxed environments or misconfigured `$HOME` cause CLI crash.
- **Severity:** Medium -- CLI-only path, not the main GUI.
- **Recommendation:** Return a user-friendly error message instead of panicking.

---

## 2. Error Propagation / Silent Swallowing

### High

#### H-5: 86 `let _ =` patterns in `src/main.rs` event loop
- **File:** `src/main.rs` (throughout event handling)
- **Pattern:** `let _ = session.pty_sender.send(PtyMsg::Input(...))`, `let _ = std::fs::write(...)`, `let _ = glass_core::config::update_config_field(...)`
- **Current behavior:** Errors from PTY communication, file writes, and config updates are silently discarded.
- **Risk breakdown:**
  - **PTY sends (25+ occurrences):** If the PTY channel is closed (child process died), these silently fail. The user sees a frozen terminal with no indication of what happened.
  - **Config writes (5+ occurrences):** Failed config persistence means user changes to settings are lost without any feedback.
  - **File system writes (10+ occurrences):** Checkpoint, diagnostic, and script file writes fail silently.
- **Severity:** High -- PTY send failures should trigger session cleanup or user notification.
- **Recommendation:**
  - For PTY sends: Check return value; if the channel is dead, trigger an `AppEvent::SessionDied` event.
  - For config writes: Show a status bar notification on failure.
  - For diagnostic/checkpoint writes: Log at `warn` level (some already do, but not all).

#### H-6: `let _ = child.kill(); let _ = child.wait();`
- **File:** `src/main.rs`, lines 438-439
- **Current behavior:** Kill and wait errors on child process are silently ignored.
- **Risk:** On Windows, if the process is already dead, `kill()` returns an error that is safely ignored. But if `wait()` fails, zombie processes could accumulate.
- **Severity:** Medium -- mostly safe on Windows due to Job Object cleanup, but problematic on Unix.

### Medium

#### M-5: `tx.send(res).ok()` in filesystem watcher
- **File:** `crates/glass_snapshot/src/watcher.rs`, line 36
- **Current behavior:** If the watcher's event channel is full or disconnected, filesystem events are silently dropped.
- **Risk:** During heavy I/O operations, snapshot events could be lost, leading to incomplete pre-command snapshots.
- **Severity:** Medium -- bounded channel backpressure is acceptable, but lost events reduce undo fidelity.

#### M-6: Orchestrator silence event `let _ =` sends
- **File:** `crates/glass_terminal/src/pty.rs`, lines 356, 428; `src/main.rs`, line 1793
- **Current behavior:** If the event loop proxy is closed, orchestrator silence events are silently dropped.
- **Risk:** The orchestrator could miss silence triggers, stalling the automation loop.
- **Severity:** Medium -- only affects orchestrator mode, and the PTY loop will break on its own when the app closes.

---

## 3. PTY Failure Modes

### High

#### H-7: No user-visible feedback when PTY child process dies
- **File:** `crates/glass_terminal/src/pty.rs`, lines 368-389
- **Current behavior:** When the child process exits, the PTY loop drains remaining bytes, calls `terminal.lock().exit()`, sends `Event::Wakeup`, and breaks out of the loop. The PTY reader thread then terminates silently.
- **Risk:** The user sees a frozen terminal with no indication that the shell has exited. There is no "[Process exited]" message, no visual indicator, and no way to know that typing will have no effect.
- **Severity:** High -- common user confusion scenario (shell crash, `exit` command).
- **Recommendation:** Send an `AppEvent::SessionExited { session_id, exit_code }` event and display a "[Shell exited with code X]" message in the terminal.

#### H-8: ConPTY initialization failure is fatal
- **File:** `crates/glass_terminal/src/pty.rs`, line 203
- **Current behavior:** `tty::new(...).expect("Failed to spawn PTY")` panics.
- **Risk:** If ConPTY is unavailable (e.g., old Windows Server, containerized environments), the app crashes immediately with no user-facing error. The `#![windows_subsystem = "windows"]` attribute means stderr is invisible.
- **Severity:** High -- see H-1 above.

### Medium

#### M-7: PTY poll error breaks the loop silently
- **File:** `crates/glass_terminal/src/pty.rs`, lines 319-327
- **Current behavior:** Non-interrupt poll errors log via `tracing::error!` and break the event loop, which silently terminates the PTY reader thread.
- **Risk:** The user sees a frozen terminal after a transient OS-level polling error.
- **Severity:** Medium -- transient errors could be retried.

---

## 4. SQLite Error Handling

### Good Practices Found
- All three database crates (`glass_history`, `glass_snapshot`, `glass_coordination`) use `anyhow::Result` and `?` propagation consistently.
- WAL mode and `busy_timeout = 5000` are set on all connections, which handles most concurrent access scenarios.
- `PRAGMA foreign_keys = ON` is correctly enabled.
- Transactions use `BEGIN IMMEDIATE` in the coordination DB to prevent `SQLITE_BUSY` during writes.
- Migration paths check schema version and are additive (no destructive migrations).

### Medium

#### M-8: No corruption detection or recovery
- **Files:** All three DB open functions (`glass_history/src/db.rs:52`, `glass_snapshot/src/db.rs:19`, `glass_coordination/src/db.rs:26`)
- **Current behavior:** If the SQLite database file is corrupted, `Connection::open()` may succeed but subsequent queries will fail with `SQLITE_CORRUPT`. The error propagates up and is typically logged but not acted upon.
- **Risk:** A corrupted history.db means all MCP tools, context building, and query features fail. There is no automatic recovery (e.g., delete and recreate).
- **Severity:** Medium -- corruption is rare with WAL mode, but disk failures happen.
- **Recommendation:** Add an integrity check (`PRAGMA integrity_check`) on open (or periodically), and if corrupt, rename the old file and create a fresh database.

#### M-9: Disk-full scenario not explicitly handled
- **Files:** All DB insert/write operations
- **Current behavior:** Disk-full errors surface as `rusqlite::Error` and propagate via `?`. In the main event loop, these are typically caught at the call site and logged.
- **Risk:** If the disk is full, snapshot creation, history recording, and coordination all fail. The user gets no clear indication of "disk full" specifically.
- **Severity:** Medium -- errors propagate but the root cause is not surfaced to the user.

---

## 5. Config Errors

### Good Practices Found
- `GlassConfig::load()` gracefully falls back to defaults on missing/unreadable/malformed files.
- `GlassConfig::load_validated()` returns a structured `ConfigError` with line/column information.
- Config hot-reload validates before applying; malformed saves send the error through `AppEvent::ConfigReloaded { error: Some(err) }`.
- All config fields use `#[serde(default = "...")]` with sensible fallback values.

### Low

#### L-1: Config hot-reload sends default config on validation error
- **File:** `crates/glass_core/src/config_watcher.rs`, line 67
- **Current behavior:** When validation fails, the watcher sends `config: Box::new(GlassConfig::default())` along with the error. This means a momentary config reset could occur.
- **Risk:** A brief flash of default settings (font size, colors) before the user fixes the typo.
- **Severity:** Low -- the error is displayed and the user can fix the config quickly.
- **Recommendation:** On validation failure, keep the current config and only send the error.

---

## 6. Rendering Errors

### Good Practices Found
- `GlassRenderer::draw()` and `get_current_texture()` handle `SurfaceError::Lost` and `SurfaceError::Outdated` gracefully by reconfiguring the surface and skipping the frame.
- All other `SurfaceError` variants log via `tracing::error!` and return without panicking.

### High

#### H-9: No device-lost recovery
- **File:** `crates/glass_renderer/src/surface.rs`
- **Current behavior:** If the GPU device is lost (driver crash, GPU hang, sleep/wake cycle), the surface reconfiguration will fail because the device itself is invalid. The renderer enters a permanent "skip frame" loop.
- **Risk:** The app appears frozen after a GPU driver crash or laptop sleep/wake. The only recovery is to restart Glass.
- **Severity:** High -- sleep/wake GPU loss is common on laptops.
- **Recommendation:** Detect persistent surface acquisition failures (e.g., 3 consecutive failures) and trigger full GPU reinitialization (recreate instance, adapter, device, surface).

### Medium

#### M-10: Font loading failure
- **File:** `src/main.rs`, line 2335; `crates/glass_renderer/src/frame.rs`
- **Current behavior:** `FontSystem::new()` can fail if no fonts are found on the system. This would likely cause glyphon to return empty glyph runs, resulting in a blank terminal.
- **Risk:** Misconfigured systems or containers without fonts installed would show a blank window.
- **Severity:** Medium -- rare on desktop systems but possible in containers.

---

## 7. File System Errors

### Good Practices Found
- Blob store handles non-existent files in `store_file()` by recording NULL hash.
- Symlinks are explicitly skipped during snapshot creation.
- Undo engine checks for conflicts (post-modification detection) before restoring.
- `BlobStore::read_blob()` validates hash length with `anyhow::ensure!`.

### Medium

#### M-11: No permission checks before file restoration in undo
- **File:** `crates/glass_snapshot/src/undo.rs`, lines 145-148
- **Current behavior:** `std::fs::write(&path, &content)` will fail with a permission error if the file is read-only or locked. The error is captured in `FileOutcome::Error`.
- **Risk:** The undo operation reports a partial failure but the error message may not clearly explain the permission issue to the user.
- **Severity:** Medium -- error is handled but UX could be better.

#### M-12: Blob store race condition on concurrent writes
- **File:** `crates/glass_snapshot/src/blob_store.rs`, lines 31-33
- **Code:** `if !blob_path.exists() { std::fs::create_dir_all(&shard_dir)?; std::fs::write(&blob_path, &content)?; }`
- **Current behavior:** TOCTOU race -- two threads could both see `!blob_path.exists()` as true and both write. Since the content is content-addressed (same hash = same content), the race is benign (identical writes).
- **Risk:** No actual data corruption risk due to content-addressing. Minor risk of a partial write being read by the other thread.
- **Severity:** Low -- content-addressing makes this effectively safe.

---

## 8. Race Conditions

### Good Practices Found
- Terminal state is protected by `FairMutex` (from `alacritty_terminal::sync`), preventing reader/writer conflicts between the PTY thread and the rendering thread.
- PTY communication uses `mpsc::channel` for thread-safe message passing.
- Usage tracker state uses `Arc<Mutex<UsageState>>` for cross-thread access.
- Coordination DB is WAL-mode SQLite, supporting concurrent readers.

### Medium

#### M-13: Shared stdin writer for agent process
- **File:** `src/main.rs`, line 1413
- **Code:** `let shared_writer = std::sync::Arc::new(std::sync::Mutex::new(BufWriter::new(stdin)));`
- **Current behavior:** The agent's stdin is shared between the activity writer thread and the orchestrator. Access is serialized via `Mutex`.
- **Risk:** If one thread holds the lock and panics, the other thread deadlocks. A poisoned mutex would prevent all further agent communication.
- **Severity:** Medium -- mutex poisoning is rare but would silently kill the agent.
- **Recommendation:** Use `.lock().unwrap_or_else(|e| e.into_inner())` to recover from poisoned mutexes, or use `parking_lot::Mutex` which doesn't poison.

#### M-14: Agent generation counter without atomic operations
- **File:** `src/main.rs`
- **Current behavior:** The `agent_generation` counter is a plain `u64` on the `Processor` struct, which is only accessed from the main thread (winit event loop).
- **Risk:** None currently -- single-threaded access. But if event handling is ever parallelized, this becomes a data race.
- **Severity:** Low -- safe under current architecture.

---

## 9. Known TODOs/FIXMEs

Only 3 TODO/FIXME/HACK comments found in production Rust code:

### Low

#### L-2: `glass_feedback` script generation config TODO
- **File:** `crates/glass_feedback/src/lib.rs`, line 372
- **Code:** `// TODO: Read script_generation from FeedbackConfig/GlassConfig when it becomes available on FeedbackState. For now, default to enabled.`
- **Risk:** Script generation is always enabled, which may not be the user's intent.
- **Severity:** Low -- the feature is opt-in via orchestrator activation.

#### L-3: `glass_core/agent_runtime.rs` cost format comment
- **File:** `crates/glass_core/src/agent_runtime.rs`, line 137
- **Comment:** `/// Returns the accumulated cost formatted as "$X.XXXX".`
- **Risk:** None -- this is documentation, not a TODO.

---

## Priority Fix List

### Must Fix Before Launch (Critical + High)

| ID | Severity | File | Issue | Effort |
|----|----------|------|-------|--------|
| C-1 | Critical | `src/main.rs:2289,4749` | `cp_path.parent().unwrap()` can panic on root paths | 5 min |
| C-2 | Critical | `src/main.rs:234,241` | `session()`/`session_mut()` expect panics on empty session list | 2 hours |
| H-1 | High | `crates/glass_terminal/src/pty.rs:203` | PTY spawn expect -- show error dialog instead | 1 hour |
| H-2 | High | `crates/glass_renderer/src/surface.rs:32,41,49` | GPU init expects -- show error dialog instead | 1 hour |
| H-5 | High | `src/main.rs` (86 sites) | `let _ =` on PTY sends hides dead sessions | 4 hours |
| H-7 | High | `crates/glass_terminal/src/pty.rs:368` | No user feedback when shell process dies | 2 hours |
| H-9 | High | `crates/glass_renderer/src/surface.rs` | No GPU device-lost recovery after sleep/wake | 4 hours |

### Should Fix Before Launch (Medium)

| ID | Severity | File | Issue | Effort |
|----|----------|------|-------|--------|
| H-3 | High | `pruner.rs:50`, `retention.rs:16` | `SystemTime` unwrap on clock skew | 5 min |
| H-4 | High | `config_watcher.rs:98` | Thread spawn expect | 5 min |
| M-2 | Medium | `src/main.rs:9802,2324` | Silent crash on init failure (no message box) | 2 hours |
| M-8 | Medium | All DB crates | No SQLite corruption detection/recovery | 4 hours |
| M-13 | Medium | `src/main.rs:1413` | Mutex poisoning on agent stdin writer | 15 min |

### Nice To Have (Low)

| ID | Severity | File | Issue | Effort |
|----|----------|------|-------|--------|
| L-1 | Low | `config_watcher.rs:67` | Config reset on validation error | 30 min |
| L-2 | Low | `glass_feedback/src/lib.rs:372` | Script generation hardcoded to enabled | 15 min |
| M-12 | Low | `blob_store.rs:31` | TOCTOU in blob writes (benign) | N/A |

### Total Estimated Effort: ~21 hours for Critical + High + Medium fixes
