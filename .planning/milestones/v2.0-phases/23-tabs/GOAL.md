# Phase 23: Tabs

## Goal

Implement a wgpu-rendered tab bar with full tab lifecycle management. Each tab owns an independent terminal session with its own PTY, Term, BlockManager, HistoryDb, and SnapshotStore. Validates the SessionMux design with real multi-session usage.

## Key Deliverables

- wgpu-rendered tab bar (colored rectangles + text via glyphon)
- Ctrl+Shift+T / Cmd+T: new tab
- Ctrl+Shift+W / Cmd+W: close tab
- Ctrl+Tab / Ctrl+Shift+Tab: cycle tabs
- Ctrl+1-9 / Cmd+1-9: jump to tab by index
- Per-tab independent PTY, Term, BlockManager, HistoryDb, SnapshotStore
- Tab title derived from CWD (OSC 7) or process name
- New tab inherits CWD from current tab
- Middle-click or X button to close tab
- Last-tab-closed behavior (close window or keep empty)
- Session cleanup on tab close (no zombie PTY processes)

## Test Gate

Create/close/switch 50 tabs rapidly with zero zombie processes, zero resource leaks, independent history per tab.

## Dependencies

Phase 21 (Session Extraction) -- needs SessionMux with Session/Tab structs.
Phase 22 (Cross-Platform) -- tabs must work on all platforms.

## Research Notes

- Tab bar rendering is straightforward (colored rectangles + text with glyphon).
- Tab management is a Vec with an index. Standard patterns.
- Skip research-phase -- standard patterns.
