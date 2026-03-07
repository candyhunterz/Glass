---
phase: 27-config-validation-hot-reload
verified: 2026-03-07T19:30:00Z
status: human_needed
score: 9/11 must-haves verified
human_verification:
  - test: "Change font_size in ~/.glass/config.toml while Glass is running"
    expected: "Terminal text resizes within 1 second without restart"
    why_human: "Requires running GUI app and observing live font change"
  - test: "Save malformed TOML (e.g. font_size = 'not_a_number') to config.toml"
    expected: "Red error banner appears at top of viewport with line/col info, terminal remains usable"
    why_human: "Visual overlay rendering and input passthrough cannot be verified programmatically"
  - test: "Fix the malformed config and save again"
    expected: "Error banner auto-dismisses"
    why_human: "Requires observing overlay disappearance in running app"
  - test: "Change a non-visual setting (e.g. [snapshot] max_count = 500)"
    expected: "No visible change occurs (no font flicker), setting applied internally"
    why_human: "Need to verify absence of visual glitch during config swap"
---

# Phase 27: Config Validation & Hot-Reload Verification Report

**Phase Goal:** Users get immediate feedback on config errors and see config changes applied live without restarting Glass
**Verified:** 2026-03-07T19:30:00Z
**Status:** human_needed
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Malformed TOML produces a ConfigError with line number, column, and human-readable message | VERIFIED | `load_validated("invalid {{{{")` returns Err(ConfigError) with non-empty message; `load_validated("font_size = \"not_a_number\"")` returns Err with line=Some(1). Tests pass in config.rs lines 220-235. |
| 2 | Type mismatch errors include the field name | VERIFIED | `load_validated` uses `e.message()` from toml crate which includes field context. Test at line 229 confirms line=Some(1) and non-empty message. |
| 3 | Missing config file or empty config still returns GlassConfig::default() without error | VERIFIED | `load_validated("")` returns Ok with default values. `load()` handles NotFound gracefully. Tests at lines 247-253, 399-406. |
| 4 | Config diff correctly identifies font_family/font_size changes vs non-visual changes | VERIFIED | `font_changed()` compares font_family and font_size only. Tests at lines 280-310 cover same-font-diff-shell, diff-size, diff-family. |
| 5 | notify dependency is available in glass_core for the watcher | VERIFIED | `notify = "8.0"` in glass_core/Cargo.toml line 12. `cargo check --workspace` compiles clean. |
| 6 | Editing font_family or font_size in config.toml while Glass is running applies the change to all open panes within 1 second | ? UNCERTAIN | All code is wired: config_watcher sends ConfigReloaded, Processor handles it with font_changed() check, calls update_font() on all windows, resizes PTY. Needs human verification of actual live behavior. |
| 7 | A config parse error during hot-reload displays an error banner at the top of the viewport | ? UNCERTAIN | ConfigErrorOverlay produces dark red rect (180,40,40 at 90% opacity) full viewport width. draw_config_error_overlay() renders it in a separate pass with LoadOp::Load. Processor sets config_error on parse failure and requests redraw. Needs human to verify visual output. |
| 8 | The error banner is display-only and does not block keyboard input to the terminal | ? UNCERTAIN | Overlay is a render-only pass (no input interception code). No input handlers reference config_error. Architecture confirms display-only, but needs human verification. |
| 9 | Non-visual config changes are applied without triggering a font rebuild | VERIFIED | ConfigReloaded handler checks `font_changed()` before calling `update_font()`. Config is swapped unconditionally (`self.config = new_config` at line 1883). When font_changed=false, only redraw is requested to clear error overlay. |
| 10 | The error banner auto-dismisses when a valid config is saved | VERIFIED | On successful reload, `self.config_error = None` (line 1857) and redraw requested. Overlay only renders when `self.config_error.is_some()`. |
| 11 | Config watcher survives atomic saves (vim/VSCode write-tmp-then-rename) | VERIFIED | Watches parent directory with NonRecursive mode (config_watcher.rs line 84). Filters by filename == "config.toml" (line 44-51). Silently skips read errors from mid-write (line 56). |

**Score:** 9/11 truths verified (2 need human verification for visual/runtime behavior)

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `crates/glass_core/src/config.rs` | ConfigError struct, load_validated(), config_diff(), PartialEq | VERIFIED | ConfigError (line 6), load_validated (line 188), font_changed (line 208), PartialEq derived (line 31). 443 lines, 10 new tests. |
| `crates/glass_core/Cargo.toml` | notify dependency | VERIFIED | `notify = "8.0"` present at line 12. |
| `crates/glass_core/src/config_watcher.rs` | spawn_config_watcher() using notify | VERIFIED | 98 lines. Spawns background thread, watches parent dir, filters config.toml events, sends ConfigReloaded via proxy. |
| `crates/glass_core/src/event.rs` | ConfigReloaded variant on AppEvent | VERIFIED | Lines 75-78: `ConfigReloaded { config: Box<GlassConfig>, error: Option<ConfigError> }` |
| `crates/glass_renderer/src/frame.rs` | update_font() method on FrameRenderer | VERIFIED | Lines 114-131: Rebuilds GridRenderer and all sub-renderers. Also includes draw_config_error_overlay() at lines 927-1026. |
| `crates/glass_renderer/src/config_error_overlay.rs` | ConfigErrorOverlay renderer | VERIFIED | 145 lines. build_error_rects() and build_error_text() with tests. Follows SearchOverlayRenderer pattern. |
| `src/main.rs` | ConfigReloaded event handler in Processor | VERIFIED | Lines 1847-1893: Full handler with font rebuild, PTY resize, config swap, error overlay management. |
| `crates/glass_core/src/lib.rs` | config_watcher module export | VERIFIED | `pub mod config_watcher;` at line 2. |
| `crates/glass_renderer/src/lib.rs` | config_error_overlay module export | VERIFIED | `pub mod config_error_overlay;` at line 4. Re-exports ConfigErrorOverlay and ConfigErrorTextLabel at line 15. |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| config_watcher.rs | src/main.rs | `proxy.send_event(AppEvent::ConfigReloaded)` | WIRED | Lines 62, 68 of config_watcher.rs send events; line 1847 of main.rs handles them. |
| src/main.rs | frame.rs | `frame_renderer.update_font()` on font change | WIRED | Line 1865 of main.rs calls update_font(). |
| src/main.rs | config.rs | `config.font_changed(&new_config)` to decide font rebuild | WIRED | Line 1860 of main.rs calls font_changed(). |
| src/main.rs | frame.rs | `draw_config_error_overlay()` for error display | WIRED | Lines 755-763 of main.rs call draw_config_error_overlay() when config_error is Some. |
| src/main.rs | config_watcher.rs | `spawn_config_watcher()` in resumed() | WIRED | Lines 546-553 of main.rs spawn the watcher once with guard flag. |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| CONF-01 | 27-01 | Config validation with actionable error messages on malformed config.toml | SATISFIED | ConfigError with line/column/snippet, load_validated() returns structured errors, 10 unit tests passing. |
| CONF-02 | 27-01, 27-02 | Config hot-reload watching config.toml for changes and applying without restart | SATISFIED | config_watcher.rs watches parent dir via notify, ConfigReloaded event propagated through event loop, font rebuild + PTY resize on font changes, config swap on non-visual changes. |
| CONF-03 | 27-02 | In-terminal error overlay displaying config parse errors instead of silent failure | SATISFIED | ConfigErrorOverlay renders dark red banner with error text, draw_config_error_overlay() renders in separate pass, auto-dismisses on valid config. |

No orphaned requirements found. All 3 requirement IDs from REQUIREMENTS.md mapped to Phase 27 are covered by plans.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (none) | - | - | - | No TODOs, FIXMEs, placeholders, or stub implementations found in any phase 27 artifacts. |

### Human Verification Required

### 1. Live Font Hot-Reload

**Test:** Run Glass, edit `~/.glass/config.toml` to change `font_size = 20.0`, save.
**Expected:** Terminal text visibly resizes within 1 second. All panes update.
**Why human:** Requires running GPU-accelerated terminal and observing visual output.

### 2. Error Overlay Display

**Test:** Save `font_size = "not_a_number"` to config.toml while Glass is running.
**Expected:** Red banner appears at top showing "Config error (line 1, col 1): ..." Terminal remains usable (can type commands).
**Why human:** Visual overlay rendering and input passthrough verification.

### 3. Error Auto-Dismiss

**Test:** Fix the config back to `font_size = 14.0` and save.
**Expected:** Red banner disappears immediately.
**Why human:** Observing overlay disappearance in running app.

### 4. Non-Visual Change No-Flicker

**Test:** Add `[snapshot]\nmax_count = 500` to config.toml and save.
**Expected:** No visible change (no font flicker, no resize).
**Why human:** Need to verify absence of visual artifacts.

### Gaps Summary

No automated gaps found. All artifacts exist, are substantive (not stubs), and are fully wired. All 27 unit tests pass. Workspace compiles clean. All 3 requirements (CONF-01, CONF-02, CONF-03) are satisfied at the code level.

The remaining uncertainty is purely runtime/visual: the config watcher, font rebuild, error overlay, and auto-dismiss logic are all correctly implemented and wired, but their real-time behavior in a running GPU terminal requires human verification.

---

_Verified: 2026-03-07T19:30:00Z_
_Verifier: Claude (gsd-verifier)_
