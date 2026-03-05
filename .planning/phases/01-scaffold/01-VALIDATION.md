---
phase: 1
slug: scaffold
status: draft
nyquist_compliant: true
wave_0_complete: true
created: 2026-03-04
updated: 2026-03-04
---

# Phase 1 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | `cargo test` (built-in) |
| **Config file** | none — standard `cargo test` sufficient |
| **Quick run command** | `cargo test --workspace` |
| **Full suite command** | `cargo test --workspace --all-targets` |
| **Estimated runtime** | ~15 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo build --workspace` (~10s compile check)
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 01-01-01 | 01 | 1 | CORE-01 | compile | `cargo build --workspace` | N/A | pending |
| 01-02-01 | 02 | 2 | RNDR-01 | compile | `cargo build --workspace` | N/A | pending |
| 01-02-02 | 02 | 2 | RNDR-01 | manual | Run Glass.exe, verify DX12 in log | N/A | pending |
| 01-03-00 | 03 | 3 | CORE-01 | compile | `cargo check --workspace --tests` | Yes — Wave 0 | pending |
| 01-03-01 | 03 | 3 | CORE-01 | integration | `cargo test -p glass_terminal -- escape_seq` | Yes — created by Task 0 | pending |
| 01-03-02 | 03 | 3 | CORE-01 | integration | `cargo test -p glass_terminal -- pty` | Yes — created by Task 0 | pending |
| 01-03-03 | 03 | 3 | CORE-01 | unit | `cargo test -p glass -- codepage` | Yes — created by Task 0 | pending |

*Status: pending / green / red / flaky*

---

## Wave 0 Requirements

- [x] `crates/glass_terminal/src/tests.rs` — ConPTY escape sequence fixture tests (CORE-01) -- created by Plan 01-03 Task 0
- [x] `crates/glass_terminal/src/tests.rs` — PTY keyboard round-trip tests (CORE-01) -- created by Plan 01-03 Task 0
- [x] `src/tests.rs` — UTF-8 codepage assertion test (CORE-01) -- created by Plan 01-03 Task 0

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| wgpu selects DX12 backend on Windows | RNDR-01 | Requires GPU hardware | Run Glass.exe, check tracing log for "GPU backend: Dx12" |
| Window resize does not crash or freeze | RNDR-01 | Requires visual inspection | Drag-resize window for 5 seconds, observe no flicker/crash |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 15s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved (revision pass — Wave 0 gaps closed by Plan 01-03 Task 0)
