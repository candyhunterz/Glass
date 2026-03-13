---
phase: 55
slug: agent-activity-stream
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-13
---

# Phase 55 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test (`cargo test`) |
| **Config file** | None (inline `#[cfg(test)]`) |
| **Quick run command** | `cargo test -p glass_core -- activity_stream` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~5 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_core -- activity_stream`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 10 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 55-01-01 | 01 | 1 | AGTA-01 | unit | `cargo test -p glass_core -- activity_stream::tests::test_channel_receives_event` | ❌ W0 | ⬜ pending |
| 55-01-02 | 01 | 1 | AGTA-02 | unit | `cargo test -p glass_core -- activity_stream::tests::test_budget_window_evicts_oldest` | ❌ W0 | ⬜ pending |
| 55-01-03 | 01 | 1 | AGTA-03 | unit | `cargo test -p glass_core -- activity_stream::tests::test_noise_filter_collapses_identical` | ❌ W0 | ⬜ pending |
| 55-01-04 | 01 | 1 | AGTA-04 | unit | `cargo test -p glass_core -- activity_stream::tests::test_rate_limiter_burst` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_core/src/activity_stream.rs` — new file with ActivityEvent, ActivityFilter, ActivityStreamConfig types + unit tests for all four AGTA-* requirements

*All tests live in the same module file per project convention.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| SoiReady handler feeds channel in same event loop tick | AGTA-01 | Integration with winit event loop | Run a command in Glass terminal, verify activity event appears in channel via debug logging |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 10s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
