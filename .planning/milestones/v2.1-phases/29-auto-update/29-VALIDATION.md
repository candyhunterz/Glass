---
phase: 29
slug: auto-update
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-07
---

# Phase 29 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test (cargo test) |
| **Config file** | Cargo.toml `[dev-dependencies]` |
| **Quick run command** | `cargo test -p glass_core -- updater` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_core -- updater`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 29-01-01 | 01 | 1 | UPDT-01 | unit | `cargo test -p glass_core -- updater::tests` | No - W0 | pending |
| 29-01-02 | 01 | 1 | UPDT-01 | unit | `cargo test -p glass_core -- updater::tests::parse` | No - W0 | pending |
| 29-01-03 | 01 | 1 | UPDT-03 | unit | `cargo test -p glass_core -- updater::tests::asset` | No - W0 | pending |
| 29-02-01 | 02 | 1 | UPDT-02 | unit | `cargo test -p glass_renderer -- status_bar::tests` | No - W0 | pending |
| 29-03-01 | 03 | 2 | UPDT-03 | manual | Manual test on Windows (MSI) | N/A | pending |
| 29-03-02 | 03 | 2 | UPDT-03 | manual | Manual test on macOS (DMG) | N/A | pending |

*Status: pending / green / red / flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_core/src/updater.rs` — unit test stubs for version comparison, JSON parsing, asset selection (UPDT-01, UPDT-03)
- [ ] `crates/glass_renderer/src/status_bar.rs` — extend existing tests for update notification rendering (UPDT-02)
- [ ] Dependencies: `cargo add ureq semver` and move `serde_json` to runtime deps

*Wave 0 creates test infrastructure before feature code.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| MSI download and msiexec upgrade on Windows | UPDT-03 | Requires running Windows installer with actual MSI file | 1. Build release MSI 2. Run Glass with old version 3. Trigger update 4. Verify msiexec launches with /passive flag |
| DMG URL open on macOS | UPDT-03 | Requires macOS with default browser | 1. Run Glass on macOS 2. Trigger update 3. Verify browser opens DMG URL |
| Linux release page notification | UPDT-03 | Requires Linux desktop with xdg-open | 1. Run Glass on Linux 2. Trigger update 3. Verify browser opens release page |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
