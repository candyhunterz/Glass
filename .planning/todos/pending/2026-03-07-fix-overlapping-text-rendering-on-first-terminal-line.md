---
created: 2026-03-07T18:14:41.662Z
title: Fix overlapping text rendering on first terminal line
area: renderer
files:
  - crates/glass_renderer/src/frame.rs
---

## Problem

The first line of PowerShell output ("Windows PowerShell") displays with overlapping/garbled characters. The "W" appears to overlap with adjacent characters, suggesting a grid offset or cell positioning issue in the text renderer. This is visible immediately on launch when PowerShell prints its banner text.

Screenshot reference: User reported overlapping text on line 1 of terminal output in PowerShell session.

## Solution

Investigate the grid cell positioning logic in the renderer — likely an off-by-one in the first row's glyph placement or an incorrect initial cursor position. Check if the PTY's initial cursor state aligns with the renderer's grid origin.
