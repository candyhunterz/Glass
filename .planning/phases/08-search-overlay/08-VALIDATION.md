---
phase: 8
slug: search-overlay
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-05
---

# Phase 8 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (Rust built-in) |
| **Config file** | Cargo.toml (workspace) |
| **Quick run command** | `cargo test -p glass --lib -- search_overlay` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~15 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass --lib -- search_overlay`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 8-01-01 | 01 | 0 | SRCH-01 | unit | `cargo test -p glass --lib -- search_overlay` | No — Wave 0 | pending |
| 8-01-02 | 01 | 1 | SRCH-01 | unit | `cargo test -p glass --lib -- search_overlay` | No — Wave 0 | pending |
| 8-01-03 | 01 | 1 | SRCH-02 | unit | `cargo test -p glass --lib -- search_overlay` | No — Wave 0 | pending |
| 8-01-04 | 01 | 1 | SRCH-03 | unit | `cargo test -p glass --lib -- search_overlay` | No — Wave 0 | pending |
| 8-01-05 | 01 | 1 | SRCH-04 | unit | `cargo test -p glass --lib -- search_overlay` | No — Wave 0 | pending |

*Status: pending / green / red / flaky*

---

## Wave 0 Requirements

- [ ] `src/search_overlay.rs` — SearchOverlay struct + state management unit tests
- [ ] `crates/glass_renderer/src/search_overlay_renderer.rs` — layout computation tests
- [ ] Verify RectRenderer blend state supports alpha (manual inspection)

*Wave 0 creates test stubs for all SRCH requirements.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Ctrl+Shift+F opens visual overlay | SRCH-01 | Requires GPU rendering + keyboard event | Run Glass, press Ctrl+Shift+F, verify overlay appears |
| Live results update visually | SRCH-02 | Requires visual confirmation of debounced rendering | Type in search box, verify results update after ~150ms |
| Arrow key highlight moves visually | SRCH-03 | Requires visual confirmation | Press arrow keys, verify highlight changes |
| Enter scrolls to correct block | SRCH-03 | Requires scrollback + visual verification | Select result, press Enter, verify scroll position |
| Result blocks show structured data | SRCH-04 | Requires visual inspection of layout | Search for known command, verify fields displayed |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
