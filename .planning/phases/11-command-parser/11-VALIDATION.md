---
phase: 11
slug: command-parser
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-05
---

# Phase 11 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[cfg(test)]` + tempfile 3 |
| **Config file** | None — Rust's built-in test harness |
| **Quick run command** | `cargo test -p glass_snapshot -- command_parser` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~5 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_snapshot -- command_parser`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 5 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 11-01-01 | 01 | 1 | SNAP-03 | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_rm` | W0 | pending |
| 11-01-02 | 01 | 1 | SNAP-03 | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_mv` | W0 | pending |
| 11-01-03 | 01 | 1 | SNAP-03 | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_cp` | W0 | pending |
| 11-01-04 | 01 | 1 | SNAP-03 | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_sed_inplace` | W0 | pending |
| 11-01-05 | 01 | 1 | SNAP-03 | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_chmod` | W0 | pending |
| 11-01-06 | 01 | 1 | SNAP-03 | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_git_checkout` | W0 | pending |
| 11-01-07 | 01 | 1 | SNAP-03 | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_readonly` | W0 | pending |
| 11-01-08 | 01 | 1 | SNAP-03 | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_path_resolution` | W0 | pending |
| 11-01-09 | 01 | 1 | SNAP-03 | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_redirect` | W0 | pending |
| 11-01-10 | 01 | 1 | SNAP-03 | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_quoted_args` | W0 | pending |
| 11-01-11 | 01 | 1 | SNAP-03 | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_powershell` | W0 | pending |
| 11-01-12 | 01 | 1 | SNAP-03 | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_unknown_command` | W0 | pending |
| 11-01-13 | 01 | 1 | SNAP-03 | unit | `cargo test -p glass_snapshot -- command_parser::tests::test_unparseable` | W0 | pending |

*Status: pending · green · red · flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_snapshot/src/command_parser.rs` — new module with parser logic + inline tests
- [ ] Update `crates/glass_snapshot/src/types.rs` — add `ParseResult`, `Confidence` types
- [ ] Update `crates/glass_snapshot/src/lib.rs` — add `pub mod command_parser;` and re-exports
- [ ] Update `crates/glass_snapshot/Cargo.toml` — add `shlex = { workspace = true }`
- [ ] Update root `Cargo.toml` — add `shlex = "1.3.0"` to `[workspace.dependencies]`

---

## Manual-Only Verifications

*All phase behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 5s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
