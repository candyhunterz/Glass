---
phase: 5
slug: history-database-foundation
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-05
---

# Phase 5 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test framework (cargo test) |
| **Config file** | none — Rust's test framework works out of the box |
| **Quick run command** | `cargo test -p glass_history` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~10 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_history`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 5-01-01 | 01 | 1 | HIST-01 | unit | `cargo test -p glass_history -- insert` | No — W0 | pending |
| 5-01-02 | 01 | 1 | HIST-03 | unit | `cargo test -p glass_history -- search` | No — W0 | pending |
| 5-01-03 | 01 | 1 | HIST-04 | unit | `cargo test -p glass_history -- resolve_db_path` | No — W0 | pending |
| 5-01-04 | 01 | 1 | HIST-05 | unit | `cargo test -p glass_history -- prune` | No — W0 | pending |
| 5-02-01 | 02 | 1 | INFR-01 | integration | `cargo test -p glass -- subcommand` | No — W0 | pending |

*Status: pending · green · red · flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_history/Cargo.toml` — add rusqlite, dirs, anyhow, tracing dependencies
- [ ] `crates/glass_history/src/lib.rs` — module structure (currently stub)
- [ ] `crates/glass_history/tests/` — test stubs for HIST-01, HIST-03, HIST-04, HIST-05
- [ ] `glass/Cargo.toml` — add clap dependency
- [ ] `glass/tests/` — test stubs for INFR-01 subcommand routing

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Terminal launches when no subcommand given | INFR-01 | Requires GUI/winit event loop | Run `cargo run` with no args, verify terminal window opens |
| DB writes don't block render thread | HIST-01 | Requires visual frame timing | Run commands in terminal, check for frame drops in debug overlay |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
