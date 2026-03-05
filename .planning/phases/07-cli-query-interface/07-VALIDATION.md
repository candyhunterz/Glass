---
phase: 7
slug: cli-query-interface
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-05
---

# Phase 7 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test (cargo test) |
| **Config file** | Cargo.toml (workspace) |
| **Quick run command** | `cargo test -p glass_history --lib query` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~15 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_history --lib query && cargo test -p glass --lib`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 07-01-01 | 01 | 0 | CLI-02 | unit | `cargo test -p glass_history --lib query` | No — Wave 0 | pending |
| 07-01-02 | 01 | 0 | CLI-03 | unit | `cargo test -p glass --lib -- history_display` | No — Wave 0 | pending |
| 07-01-03 | 01 | 0 | CLI-01 | unit | `cargo test -p glass --lib -- subcommand` | Partial | pending |
| 07-02-01 | 02 | 1 | CLI-01 | unit | `cargo test -p glass --lib -- subcommand` | No — Wave 0 | pending |
| 07-02-02 | 02 | 1 | CLI-02 | unit | `cargo test -p glass_history --lib query` | No — Wave 0 | pending |
| 07-02-03 | 02 | 1 | CLI-03 | unit | `cargo test -p glass --lib -- history_display` | No — Wave 0 | pending |

*Status: pending · green · red · flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_history/src/query.rs` — test stubs for QueryFilter + filtered_query (CLI-02)
- [ ] `src/history.rs` — test stubs for display formatting (CLI-03)
- [ ] Expand `src/tests.rs` — stubs for HistoryAction subcommand parsing (CLI-01)
- [ ] `cargo add chrono` — add chrono dependency to glass_history

*Wave 0 creates test infrastructure and stubs before implementation begins.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Terminal column alignment looks correct | CLI-03 | Visual output formatting | Run `glass history list --limit 5`, verify columns align |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
