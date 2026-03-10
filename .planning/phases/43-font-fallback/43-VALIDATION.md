---
phase: 43
slug: font-fallback
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-10
---

# Phase 43 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (built-in Rust test framework) |
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
| 43-01-01 | 01 | 0 | FONT-01 | unit | `cargo test -p glass_renderer fallback_renders_cjk_glyph -- --exact` | ❌ W0 | ⬜ pending |
| 43-01-02 | 01 | 0 | FONT-01 | unit | `cargo test -p glass_renderer fallback_renders_multi_script -- --exact` | ❌ W0 | ⬜ pending |
| 43-01-03 | 01 | 0 | FONT-02 | unit | `cargo test -p glass_renderer fallback_glyph_respects_monospace_width -- --exact` | ❌ W0 | ⬜ pending |
| 43-01-04 | 01 | 0 | FONT-02 | unit | `cargo test -p glass_renderer build_cell_buffers_handles_cjk_fallback -- --exact` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `fallback_renders_cjk_glyph` test in grid_renderer.rs — covers FONT-01
- [ ] `fallback_renders_multi_script` test in grid_renderer.rs — covers FONT-01
- [ ] `fallback_glyph_respects_monospace_width` test in grid_renderer.rs — covers FONT-02
- [ ] `build_cell_buffers_handles_cjk_fallback` test in grid_renderer.rs — covers FONT-02

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| CJK text visually aligned on same line as Latin text | FONT-02 | Requires visual inspection of rendered output | Run Glass, type mixed Latin+CJK text, verify vertical alignment |
| No visible frame stutter on first CJK glyph | FONT-01 | Performance perception | Run Glass with fresh cache, paste CJK text, observe smoothness |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
