# Milestones

## v1.0 MVP (Shipped: 2026-03-05)

**Phases completed:** 4 phases, 12 plans
**Lines of code:** 4,343 Rust
**Timeline:** 2026-03-04 (1 day)
**Git range:** feat(01-01) to perf(04-02)

**Delivered:** A GPU-accelerated terminal emulator with shell integration, block-based command output, and daily-drivable performance on Windows.

**Key accomplishments:**
- 7-crate Rust workspace with wgpu DX12 GPU surface and ConPTY PTY spawn
- Full terminal rendering pipeline — instanced GPU rects, glyphon text, 24-bit color, cursor, font-metrics resize
- Complete keyboard encoding with Ctrl/Alt/arrow/function keys, clipboard, bracketed paste, scrollback
- Shell integration data layer — OscScanner, BlockManager, StatusState with 27 TDD tests
- Block UI rendering — separator lines, exit code badges, duration labels, status bar with CWD and git branch
- TOML configuration, 360ms cold start, 3-7us key latency, 86MB idle memory

**Tech debt (from audit):**
- display_offset hardcoded to 0 in frame.rs — block decorations render at wrong positions during scrollback
- ConPTY test execution not formally logged
- Nyquist validation partial for phases 2, 3, 4

---

