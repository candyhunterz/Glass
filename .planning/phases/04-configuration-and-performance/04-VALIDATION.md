---
phase: 4
slug: configuration-and-performance
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-04
---

# Phase 4 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test framework (cargo test) |
| **Config file** | None (tests are inline #[cfg(test)] modules) |
| **Quick run command** | `cargo test -p glass_core --release` |
| **Full suite command** | `cargo test --release` |
| **Estimated runtime** | ~10 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_core --release`
- **After every plan wave:** Run `cargo test --release`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 10 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 04-01-01 | 01 | 1 | CONF-01 | unit | `cargo test -p glass_core config -- --nocapture` | No - W0 | pending |
| 04-01-02 | 01 | 1 | CONF-02 | unit | `cargo test -p glass_core config -- --nocapture` | No - W0 | pending |
| 04-01-03 | 01 | 1 | CONF-03 | unit | `cargo test -p glass_core config -- --nocapture` | No - W0 | pending |
| 04-02-01 | 02 | 2 | PERF-01 | manual | Build `--release`, measure with tracing output | No - manual | pending |
| 04-02-02 | 02 | 2 | PERF-02 | manual | Build `--release`, measure with tracing spans | No - manual | pending |
| 04-02-03 | 02 | 2 | PERF-03 | manual | Build `--release`, check memory-stats output | No - manual | pending |

*Status: pending / green / red / flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_core/src/config.rs` — unit tests for config loading (parse valid TOML, handle missing file, handle partial config, handle malformed TOML)
- [ ] Performance tests are inherently manual (require GPU, PTY, window) — document measurement procedure instead

*Config tests cover CONF-01, CONF-02, CONF-03. Performance tests (PERF-01, PERF-02, PERF-03) require running the full application.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Cold start under 200ms | PERF-01 | Requires full GPU + PTY + window startup | Build `--release`, run Glass, check tracing output for cold start timing |
| Input latency under 5ms | PERF-02 | Requires interactive key input with PTY | Build `--release`, type in Glass, check tracing spans for key-to-screen timing |
| Idle memory under 50MB | PERF-03 | Requires full running process | Build `--release`, run Glass, check Task Manager or memory-stats log output |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 10s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
