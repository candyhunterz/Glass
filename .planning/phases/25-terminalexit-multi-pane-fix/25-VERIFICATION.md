---
phase: 25-terminalexit-multi-pane-fix
verified: 2026-03-07T05:10:00Z
status: passed
score: 4/4 must-haves verified
re_verification: false
must_haves:
  truths:
    - "Shell exit in one pane of a multi-pane tab closes only that pane"
    - "Shell exit in a single-pane tab closes the entire tab"
    - "Last tab closing exits the application"
    - "Remaining panes resize after a pane exit"
  artifacts:
    - path: "src/main.rs"
      provides: "TerminalExit handler with pane-aware close logic"
      contains: "close_pane"
  key_links:
    - from: "src/main.rs (TerminalExit handler)"
      to: "SessionMux::close_pane"
      via: "pane_count check before dispatch"
      pattern: "pane_count.*close_pane"
---

# Phase 25: TerminalExit Multi-Pane Fix Verification Report

**Phase Goal:** Fix the TerminalExit handler to use close_pane() for multi-pane tabs instead of close_tab(), completing SPLIT-11 satisfaction.
**Verified:** 2026-03-07T05:10:00Z
**Status:** passed
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Shell exit in one pane of a multi-pane tab closes only that pane | VERIFIED | TerminalExit handler at line 1386 checks `pane_count > 1` and calls `close_pane(session_id)` at line 1389 |
| 2 | Shell exit in a single-pane tab closes the entire tab | VERIFIED | Else branch at line 1404 calls `close_tab(idx)` for single-pane case |
| 3 | Last tab closing exits the application | VERIFIED | Lines 1410-1413 check `tab_count() == 0` and call `event_loop.exit()` |
| 4 | Remaining panes resize after a pane exit | VERIFIED | Line 1402 calls `resize_all_panes()` after close_pane in the multi-pane branch |

**Score:** 4/4 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/main.rs` | TerminalExit handler with pane-aware close logic containing `close_pane` | VERIFIED | Lines 1380-1418: full pane-count dispatch logic with close_pane for multi-pane, close_tab for single-pane, resize_all_panes after closure |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| src/main.rs (TerminalExit handler, line 1385) | SessionMux::close_pane (crates/glass_mux/src/session_mux.rs line 172) | pane_count check at line 1386 before close_pane dispatch at line 1389 | WIRED | Handler checks `tabs()[idx].pane_count() > 1` then calls `ctx.session_mux.close_pane(session_id)` -- mirrors Ctrl+Shift+W pattern at line 933 |
| src/main.rs (TerminalExit handler, line 1402) | resize_all_panes (src/main.rs line 256) | Direct call after pane closure | WIRED | `resize_all_panes(&mut ctx.session_mux, &ctx.frame_renderer, size.width, size.height)` called in multi-pane branch |
| src/main.rs (TerminalExit handler, line 1383) | Tab::session_ids + Tab::pane_count (crates/glass_mux/src/tab.rs lines 24, 29) | Tab lookup via session_ids().contains(), pane_count() for dispatch | WIRED | Uses `session_id` from event directly (not focused_session_id) for correct tab identification |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|------------|-------------|--------|----------|
| SPLIT-11 | 25-01-PLAN | Pane close on last pane closes tab; shell exit closes only pane in multi-pane tab | SATISFIED | TerminalExit handler now uses close_pane for multi-pane tabs and close_tab for single-pane tabs. 2 unit tests for close_pane exist in session_mux.rs (close_pane_last_pane_closes_tab, close_pane_two_pane_split_leaves_single_pane). All 66 glass_mux tests pass. |

No REQUIREMENTS.md file exists in this project. SPLIT-11 is defined in the ROADMAP.md and phase 24 documents. No orphaned requirements found.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (none) | - | - | - | No anti-patterns detected in the TerminalExit handler |

### Compilation and Test Verification

- `cargo check` passes with no errors (0.20s, cached)
- `cargo test -p glass_mux` passes all 66 tests with 0 failures

### Human Verification Required

### 1. Multi-pane shell exit behavior

**Test:** Open a tab, split it into 2 panes (Ctrl+Shift+D or similar), type `exit` in one pane
**Expected:** Only the exited pane closes; the remaining pane expands to fill the tab
**Why human:** Requires running the application and observing visual pane behavior after a shell process exits

### 2. Single-pane shell exit behavior

**Test:** Open a single-pane tab, type `exit`
**Expected:** The entire tab closes. If it was the last tab, the application exits.
**Why human:** Requires running the application to confirm no regression in single-pane exit

### 3. Ctrl+Shift+W parity

**Test:** In a multi-pane tab, press Ctrl+Shift+W on one pane, then in another multi-pane tab type `exit`
**Expected:** Both close only the targeted pane with identical behavior (resize, focus shift)
**Why human:** Behavioral parity between two code paths requires visual confirmation

### Gaps Summary

No gaps found. All four observable truths are verified against the actual codebase. The TerminalExit handler at src/main.rs lines 1380-1418 correctly mirrors the Ctrl+Shift+W handler pattern: it checks pane_count on the specific tab containing the exited session, dispatches close_pane for multi-pane tabs and close_tab for single-pane tabs, and calls resize_all_panes after pane closure. The commit `7ba2674` is present on the master branch and the code is confirmed in the working tree. All 66 glass_mux unit tests pass including the two close_pane tests that validate SPLIT-11 core behavior.

---

_Verified: 2026-03-07T05:10:00Z_
_Verifier: Claude (gsd-verifier)_
