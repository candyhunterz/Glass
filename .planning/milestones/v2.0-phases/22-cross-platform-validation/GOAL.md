# Phase 22: Cross-Platform Validation

## Goal

Validate that Glass launches and runs correctly on macOS (Metal, Cmd shortcuts, zsh, Retina) and Linux (Vulkan/GL, Wayland+X11, XDG paths). Establish cross-platform CI pipeline.

## Key Deliverables

- Glass launches on macOS with Metal backend, correct Cmd key mappings, Retina/HiDPI rendering
- Glass launches on Linux with Vulkan (GL fallback), Wayland + X11, XDG directory compliance
- wgpu surface format negotiation validated across DX12/Metal/Vulkan
- Shell integration working on zsh (macOS) and bash (Linux)
- HiDPI/scale factor plumbing through renderer and text pipeline
- Option-as-Meta configurable on macOS
- Cross-platform CI build and test matrix
- File watching (notify) validated on FSEvents (macOS) and inotify (Linux)

## Test Gate

Glass runs on all three platforms with correct rendering, keyboard, clipboard, shell integration, and file watching.

## Dependencies

Phase 21 (Session Extraction) -- needs SessionMux layer and platform cfg gates.

## Research Notes

- Wayland-specific issues (clipboard persistence, CSD vs SSD, IME) are poorly documented -- needs hands-on testing.
- macOS App Nap, NSWindow tabbingMode suppression, and fullscreen behavior need investigation.
- May need research-phase for Wayland and macOS quirks.
