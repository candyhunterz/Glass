---
phase: 15
slug: pipe-parsing-core
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-05
---

# Phase 15 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[cfg(test)]` + cargo test |
| **Config file** | None needed — Cargo.toml handles it |
| **Quick run command** | `cargo test -p glass_pipes` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~5 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_pipes`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 10 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 15-01-01 | 01 | 0 | PIPE-01 | unit | `cargo test -p glass_pipes -- parser` | No — W0 | pending |
| 15-01-02 | 01 | 0 | PIPE-02 | unit | `cargo test -p glass_pipes -- classify::opt_out` | No — W0 | pending |
| 15-01-03 | 01 | 0 | PIPE-03 | unit | `cargo test -p glass_pipes -- classify::tty` | No — W0 | pending |
| 15-01-04 | 01 | 0 | CAPT-03 | unit | `cargo test -p glass_pipes -- buffer` | No — W0 | pending |
| 15-01-05 | 01 | 0 | CAPT-04 | unit | `cargo test -p glass_pipes -- buffer::binary` | No — W0 | pending |

*Status: pending · green · red · flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_pipes/src/types.rs` — Pipeline, PipeStage, StageBuffer, FinalizedBuffer types
- [ ] `crates/glass_pipes/src/parser.rs` — parse_pipeline() with #[cfg(test)] module
- [ ] `crates/glass_pipes/src/classify.rs` — classify_pipeline() with #[cfg(test)] module
- [ ] `crates/glass_pipes/Cargo.toml` — needs shlex dependency added

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|

*All phase behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 10s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
