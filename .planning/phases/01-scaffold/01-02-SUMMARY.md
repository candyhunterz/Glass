---
phase: 01-scaffold
plan: 02
subsystem: rendering
tags: [wgpu, winit, dx12, gpu, surface, event-loop, windows-sys]

# Dependency graph
requires:
  - phase: 01-scaffold plan 01
    provides: Cargo workspace with 7 crates compiled; glass_core AppEvent types; glass_renderer stub
provides:
  - GlassRenderer struct with wgpu Device/Queue/Surface, clear-to-color draw, and resize handling
  - winit ApplicationHandler Processor wiring window creation, resize, close, and AppEvent dispatch
  - Windows UTF-8 console code page setup (SetConsoleCP/SetConsoleOutputCP 65001) in main()
affects: [02-pty, 03-text-rendering, all phases using glass_renderer]

# Tech tracking
tech-stack:
  added: [windows-sys 0.59 (Win32_System_Console)]
  patterns: [wgpu surface + device init via pollster::block_on in winit resumed(), SurfaceError::Lost|Outdated reconfigure-and-skip pattern]

key-files:
  created:
    - crates/glass_renderer/src/surface.rs
  modified:
    - crates/glass_renderer/src/lib.rs
    - src/main.rs
    - Cargo.toml
    - Cargo.lock

key-decisions:
  - "winit 0.30.13 uses resumed() not can_create_surfaces() — research was incorrect; confirmed against installed source"
  - "wgpu 28.0.0 request_device() takes 1 arg (no trace path); RenderPassColorAttachment needs depth_slice; RenderPassDescriptor needs multiview_mask"
  - "Guard zero-size resize (width==0 || height==0) to handle minimization without surface reconfigure"

patterns-established:
  - "Pattern: GlassRenderer::new() is async; call via pollster::block_on() from winit sync callbacks"
  - "Pattern: On SurfaceError::Lost|Outdated — reconfigure surface and return early, never panic"
  - "Pattern: Window creation in resumed() guarded by windows.is_empty() check for re-resume safety"

requirements-completed: [RNDR-01]

# Metrics
duration: 2min
completed: 2026-03-05
---

# Phase 1 Plan 02: wgpu DX12 Surface Summary

**wgpu DX12 GPU surface with winit ApplicationHandler: dark-gray clear-to-color render loop with crash-free resize and Windows UTF-8 console setup**

## Performance

- **Duration:** ~2 min
- **Started:** 2026-03-05T01:31:42Z
- **Completed:** 2026-03-05T01:34:04Z
- **Tasks:** 1 of 2 (Task 2 is human-verify checkpoint)
- **Files modified:** 5

## Accomplishments
- GlassRenderer struct with wgpu DX12 auto-selection, async init, clear-to-dark-gray draw, and zero-size-guarded resize
- winit 0.30.13 ApplicationHandler event loop: window creation in `resumed()`, RedrawRequested, Resized, CloseRequested, and AppEvent dispatch
- Windows UTF-8 console code page (65001) set at startup via windows-sys before event loop
- `cargo build --workspace` green with all 7 crates

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement GlassRenderer with wgpu DX12 surface and winit ApplicationHandler** - `8ac66c3` (feat)

**Plan metadata:** (added after human-verify checkpoint completes)

## Files Created/Modified
- `crates/glass_renderer/src/surface.rs` - GlassRenderer with async new(), draw(), resize(); wgpu DX12 surface management
- `crates/glass_renderer/src/lib.rs` - pub mod surface; pub use surface::GlassRenderer
- `src/main.rs` - Full winit ApplicationHandler with Processor struct, window HashMap, AppEvent handling
- `Cargo.toml` - Added windows-sys = { version = "0.59", features = ["Win32_System_Console"] }
- `Cargo.lock` - Updated with new dependency resolutions

## Decisions Made
- Used `resumed()` instead of `can_create_surfaces()`: the research documented that 0.30.13 introduced `can_create_surfaces` as a required method, but the installed crate does not have this method. Confirmed against installed source at `/c/Users/nkngu/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/winit-0.30.13/src/application.rs` — only `resumed` exists as required. Window creation placed in `resumed()` with `windows.is_empty()` guard.
- Guarded resize with `width == 0 || height == 0` check to safely handle window minimization.
- Used `size.width.max(1)` and `size.height.max(1)` in initial surface config to prevent zero-dimension surface configure at startup if window reports zero size before layout.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed wgpu 28.0.0 API: request_device() takes 1 argument, not 2**
- **Found during:** Task 1 (build verification)
- **Issue:** Research pattern used `adapter.request_device(&desc, None)` but wgpu 28.0.0 `request_device` signature takes only 1 argument (trace path removed from public API)
- **Fix:** Changed to `adapter.request_device(&wgpu::DeviceDescriptor::default())`
- **Files modified:** crates/glass_renderer/src/surface.rs
- **Verification:** `cargo build --workspace` passes
- **Committed in:** 8ac66c3 (Task 1 commit)

**2. [Rule 1 - Bug] Fixed wgpu 28.0.0 RenderPassColorAttachment missing depth_slice field**
- **Found during:** Task 1 (build verification)
- **Issue:** wgpu 28.0.0 `RenderPassColorAttachment` struct requires `depth_slice: None` field not present in research pattern
- **Fix:** Added `depth_slice: None` to RenderPassColorAttachment initializer
- **Files modified:** crates/glass_renderer/src/surface.rs
- **Verification:** `cargo build --workspace` passes
- **Committed in:** 8ac66c3 (Task 1 commit)

**3. [Rule 1 - Bug] Fixed wgpu 28.0.0 RenderPassDescriptor missing multiview_mask field**
- **Found during:** Task 1 (build verification)
- **Issue:** wgpu 28.0.0 `RenderPassDescriptor` struct requires `multiview_mask: None` field not present in research pattern
- **Fix:** Added `multiview_mask: None` to RenderPassDescriptor initializer
- **Files modified:** crates/glass_renderer/src/surface.rs
- **Verification:** `cargo build --workspace` passes
- **Committed in:** 8ac66c3 (Task 1 commit)

**4. [Rule 1 - Bug] Used resumed() instead of can_create_surfaces() (winit 0.30.13 API mismatch)**
- **Found during:** Task 1 (build verification)
- **Issue:** Research stated winit 0.30.13 introduced `can_create_surfaces` as required method, but the installed crate does not expose this method — it does not exist in the trait definition
- **Fix:** Implemented `resumed()` as required by the actual installed trait, with `windows.is_empty()` guard to prevent duplicate window creation on re-resume
- **Files modified:** src/main.rs
- **Verification:** `cargo build --workspace` passes; `ApplicationHandler` impl accepted by compiler
- **Committed in:** 8ac66c3 (Task 1 commit)

---

**Total deviations:** 4 auto-fixed (all Rule 1 — API mismatches between research patterns and actual installed library versions)
**Impact on plan:** All auto-fixes necessary for compilation. No scope creep. Core architecture (wgpu surface, winit handler, resize safety) is exactly as planned.

## Issues Encountered
- winit 0.30.13 `can_create_surfaces` does not exist in installed crate despite research documentation claiming it was added in this version. Using `resumed()` is correct for this version. This should be noted for future plan updates if winit is upgraded.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- GPU surface and event loop ready for Plan 03 (ConPTY PTY integration)
- GlassRenderer::draw() and ::resize() API stable and ready for glyphon text rendering (Phase 2)
- Awaiting human verification (Task 2 checkpoint) to confirm DX12 backend selection and resize stability

---
*Phase: 01-scaffold*
*Completed: 2026-03-05*
