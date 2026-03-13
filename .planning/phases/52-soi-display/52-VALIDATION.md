---
phase: 52
slug: soi-display
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-13
---

# Phase 52 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in (`#[test]`) |
| **Config file** | None — tests inline per project convention |
| **Quick run command** | `cargo test -p glass_renderer -- block_renderer && cargo test -p glass_core -- config` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_renderer -- block_renderer && cargo test -p glass_core -- config`
- **After every plan wave:** Run `cargo test --workspace && cargo clippy --workspace -- -D warnings`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 52-01-01 | 01 | 1 | SOID-03 | unit | `cargo test -p glass_core -- config::test_soi_section_defaults` | ❌ W0 | ⬜ pending |
| 52-01-02 | 01 | 1 | SOID-03 | unit | `cargo test -p glass_core -- config::test_soi_section_roundtrip` | ❌ W0 | ⬜ pending |
| 52-01-03 | 01 | 1 | SOID-03 | unit | `cargo test -p glass_core -- config::test_soi_section_absent_uses_defaults` | ❌ W0 | ⬜ pending |
| 52-01-04 | 01 | 1 | SOID-01 | unit | `cargo test -p glass_renderer -- block_renderer::test_soi_label_emitted_for_complete_block` | ❌ W0 | ⬜ pending |
| 52-01-05 | 01 | 1 | SOID-01 | unit | `cargo test -p glass_renderer -- block_renderer::test_soi_label_absent_when_no_summary` | ❌ W0 | ⬜ pending |
| 52-01-06 | 01 | 1 | SOID-01 | unit | `cargo test -p glass_renderer -- block_renderer::test_soi_label_color_error` | ❌ W0 | ⬜ pending |
| 52-01-07 | 01 | 1 | SOID-01 | unit | `cargo test -p glass_renderer -- block_renderer::test_soi_label_left_anchored` | ❌ W0 | ⬜ pending |
| 52-02-01 | 02 | 2 | SOID-02 | unit | `cargo test -p glass_terminal -- hint_line_format` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_core/src/config.rs` — add `SoiSection` struct, defaults, field on `GlassConfig`, 3 unit tests
- [ ] `crates/glass_terminal/src/block_manager.rs` — add `soi_summary: Option<String>`, `soi_severity: Option<String>` to `Block`
- [ ] `crates/glass_renderer/src/block_renderer.rs` — add `soi_color_for_severity()` helper and SOI label emission, 4 unit tests
- [ ] `crates/glass_core/src/event.rs` — add `raw_line_count: i64` to `AppEvent::SoiReady`

*Existing test infrastructure covers all phase requirements — no new framework needed.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Visual SOI decoration appears on command blocks | SOID-01 | GPU rendering output not testable in headless CI | Build release, run `cargo build`, verify muted line below output |
| Hint line visible to Claude Code Bash tool | SOID-02 | Requires real shell + agent integration | Enable `shell_summary`, run command, check Bash tool output |
| Hot-reload of `soi.enabled = false` suppresses decorations | SOID-03 | Requires running terminal + config edit | Toggle config while terminal is open, verify suppression |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
