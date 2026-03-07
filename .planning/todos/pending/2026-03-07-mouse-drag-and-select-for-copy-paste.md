---
created: 2026-03-07T18:14:41.662Z
title: Mouse drag-and-select for copy paste
area: ui
files:
  - src/main.rs
  - crates/glass_renderer/src/frame.rs
---

## Problem

Users cannot select terminal text with mouse drag to copy commands and output. This is a fundamental usability feature expected in any terminal emulator — without it, users have no way to copy/paste text from the terminal.

## Solution

Implement mouse-based text selection:
1. Track mouse down + drag events to define selection range (start cell to end cell)
2. Highlight selected cells with an inverted/blue selection overlay
3. On Ctrl+C (or right-click), copy selected text to system clipboard
4. Clear selection on next keypress or click elsewhere
5. Handle multi-line selection with proper line-break insertion
