---
phase: 2
slug: terminal-core
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-04
---

# Phase 2 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` (built-in) |
| **Config file** | None |
| **Quick run command** | `cargo test --workspace` |
| **Full suite command** | `cargo test --workspace --all-targets` |
| **Estimated runtime** | ~15 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --workspace`
- **After every plan wave:** Run `cargo test --workspace --all-targets`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 02-01-xx | 01 | 1 | CORE-02 | Integration | `cargo test -p glass_terminal -- color_resolution` | No -- Wave 0 | ⬜ pending |
| 02-02-xx | 02 | 1 | RNDR-02 | Integration | `cargo test -p glass_terminal -- truecolor` | No -- Wave 0 | ⬜ pending |
| 02-02-xx | 02 | 1 | RNDR-03 | Smoke (manual) | Open neovim in Glass, verify cursor changes | N/A | ⬜ pending |
| 02-02-xx | 02 | 1 | RNDR-04 | Smoke (manual) | Change font_size, verify re-render | N/A | ⬜ pending |
| 02-03-xx | 03 | 2 | CORE-03 | Unit | `cargo test -p glass_terminal -- input_encoding` | No -- Wave 0 | ⬜ pending |
| 02-03-xx | 03 | 2 | CORE-04 | Unit | `cargo test -p glass_terminal -- bracketed_paste` | No -- Wave 0 | ⬜ pending |
| 02-03-xx | 03 | 2 | CORE-05 | Integration | `cargo test -p glass_terminal -- scrollback` | No -- Wave 0 | ⬜ pending |
| 02-03-xx | 03 | 2 | CORE-06 | Manual | Select text, Ctrl+Shift+C, Ctrl+Shift+V | N/A | ⬜ pending |
| 02-03-xx | 03 | 2 | CORE-07 | Integration | `cargo test -p glass_terminal -- resize_reflow` | No -- Wave 0 | ⬜ pending |
| 02-03-xx | 03 | 2 | CORE-08 | Smoke (manual) | Run `echo "cafe\u0301 🦀"` in Glass | N/A | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_terminal/src/input.rs` + tests — keyboard escape sequence encoding unit tests (CORE-03)
- [ ] `crates/glass_terminal/src/grid_snapshot.rs` + tests — color resolution tests (CORE-02)
- [ ] Bracketed paste unit test in `glass_terminal` (CORE-04)
- [ ] Scrollback configuration test verifying `Config { scrolling_history: 10000 }` (CORE-05)
- [ ] Add `glyphon = "0.10.0"` and `arboard = "3"` to workspace dependencies
- [ ] Add `glyphon.workspace = true` to `glass_renderer/Cargo.toml`

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Copy/paste via Ctrl+Shift+C/V | CORE-06 | Requires clipboard and user interaction | Run Glass, select text, Ctrl+Shift+C, open new app, Ctrl+V |
| UTF-8 renders without mojibake | CORE-08 | Visual verification of glyph rendering | Run `echo "cafe\u0301 🦀 漢字"` in Glass |
| Truecolor from bat/delta/neovim | RNDR-02 | Requires external tools and visual check | Run `bat --color=always Cargo.toml` in Glass |
| Cursor shapes | RNDR-03 | Visual verification of cursor rendering | Open neovim, verify cursor changes between modes |
| Font config | RNDR-04 | Visual verification | Change `GlassConfig.font_size`, verify text re-renders |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
