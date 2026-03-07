---
phase: 27
slug: config-validation-hot-reload
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-07
---

# Phase 27 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test + cargo test |
| **Config file** | N/A (uses `#[cfg(test)]` modules) |
| **Quick run command** | `cargo test -p glass_core` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_core`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 27-01-01 | 01 | 1 | CONF-01 | unit | `cargo test -p glass_core config::tests::validation` | ❌ W0 | ⬜ pending |
| 27-01-02 | 01 | 1 | CONF-01 | unit | `cargo test -p glass_core config::tests::unknown_keys` | ❌ W0 | ⬜ pending |
| 27-01-03 | 01 | 1 | CONF-01 | unit | `cargo test -p glass_core config::tests::type_mismatch` | ❌ W0 | ⬜ pending |
| 27-01-04 | 01 | 1 | CONF-03 | unit | `cargo test -p glass_core config::tests::error_display` | ❌ W0 | ⬜ pending |
| 27-02-01 | 02 | 1 | CONF-02 | unit | `cargo test -p glass_core config::tests::diff_font` | ❌ W0 | ⬜ pending |
| 27-02-02 | 02 | 1 | CONF-02 | unit | `cargo test -p glass_core config::tests::diff_nonvisual` | ❌ W0 | ⬜ pending |
| 27-02-03 | 02 | 2 | CONF-02 | integration | `cargo test -p glass_core config_watcher::tests` | ❌ W0 | ⬜ pending |
| 27-02-04 | 02 | 2 | CONF-02 | unit | `cargo test -p glass_renderer grid_renderer::tests::update_font` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `glass_core/src/config.rs` — validation test stubs for CONF-01 (error messages, unknown keys, type mismatch)
- [ ] `glass_core/src/config.rs` — config diff test stubs for CONF-02 (font change, non-visual change)
- [ ] `glass_core/src/config_watcher.rs` — watcher integration test stubs for CONF-02
- [ ] `glass_renderer/src/grid_renderer.rs` — font update test stubs for CONF-02
- [ ] Add `notify` dependency to `glass_core/Cargo.toml`

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Font change applies within 1 second | CONF-02 | Requires running Glass + editing config.toml | 1. Run Glass 2. Edit font_size in config.toml 3. Verify panes update within 1s |
| Config error overlay displays on parse failure | CONF-03 | Requires visual inspection of overlay rendering | 1. Run Glass 2. Introduce syntax error in config.toml 3. Verify overlay appears with error message |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
