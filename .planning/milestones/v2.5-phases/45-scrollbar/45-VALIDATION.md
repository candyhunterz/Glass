---
phase: 45
slug: scrollbar
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-10
---

# Phase 45 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (Rust built-in + criterion for benchmarks) |
| **Config file** | Cargo.toml workspace |
| **Quick run command** | `cargo test -p glass_renderer --lib` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_renderer --lib`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| SB-01 | 01 | 1 | ScrollbarRenderer produces track + thumb rects | unit | `cargo test -p glass_renderer scrollbar` | ❌ W0 | ⬜ pending |
| SB-02 | 01 | 1 | Thumb height proportional to visible/total ratio | unit | `cargo test -p glass_renderer scrollbar::tests::thumb_height` | ❌ W0 | ⬜ pending |
| SB-03 | 01 | 1 | Thumb position maps correctly from display_offset | unit | `cargo test -p glass_renderer scrollbar::tests::thumb_position` | ❌ W0 | ⬜ pending |
| SB-04 | 01 | 1 | Minimum thumb height enforced | unit | `cargo test -p glass_renderer scrollbar::tests::min_thumb` | ❌ W0 | ⬜ pending |
| SB-05 | 01 | 1 | Empty history produces full-track thumb | unit | `cargo test -p glass_renderer scrollbar::tests::empty_history` | ❌ W0 | ⬜ pending |
| SB-06 | 01 | 1 | Hit-test identifies Thumb/TrackAbove/TrackBelow | unit | `cargo test -p glass_renderer scrollbar::tests::hit_test` | ❌ W0 | ⬜ pending |
| SB-07 | 01 | 1 | Hit-test returns None outside scrollbar | unit | `cargo test -p glass_renderer scrollbar::tests::hit_test_miss` | ❌ W0 | ⬜ pending |
| SB-08 | 01 | 1 | Hover state changes thumb color | unit | `cargo test -p glass_renderer scrollbar::tests::hover_color` | ❌ W0 | ⬜ pending |
| SB-09 | 02 | 2 | Grid width subtracted in resize calculations | integration | Manual | N/A | ⬜ pending |
| SB-10 | 02 | 2 | Scrollbar drag updates display_offset | integration | Manual | N/A | ⬜ pending |
| SB-11 | 02 | 2 | Multi-pane: each pane has independent scrollbar | integration | Manual | N/A | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_renderer/src/scrollbar.rs` — new file with ScrollbarRenderer + tests (SB-01 through SB-08)
- [ ] No framework install needed — cargo test already configured

*Existing infrastructure covers framework needs.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Grid width shrinks by 8px | SB-09 | Requires running terminal with GPU rendering | Run `cargo run`, verify text doesn't overlap scrollbar |
| Scrollbar drag scrolls content | SB-10 | Requires mouse interaction with running app | Run `cargo run`, generate history, drag scrollbar thumb |
| Multi-pane scrollbars | SB-11 | Requires split pane UI with running app | Run `cargo run`, create split pane, verify each has scrollbar |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
