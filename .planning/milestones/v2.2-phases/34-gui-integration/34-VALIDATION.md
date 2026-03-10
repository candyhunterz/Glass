---
phase: 34
slug: gui-integration
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-09
---

# Phase 34 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test (`#[cfg(test)] mod tests` pattern) |
| **Config file** | None needed |
| **Quick run command** | `cargo test --workspace -q` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --workspace -q`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green + `cargo clippy --workspace -- -D warnings`
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 34-01-01 | 01 | 1 | GUI-03 | unit | `cargo test -p glass_core coordination_poller -q` | ❌ W0 | ⬜ pending |
| 34-01-02 | 01 | 1 | GUI-01, GUI-02 | unit | `cargo test -p glass_renderer status_bar -q` | ✅ partial | ⬜ pending |
| 34-02-01 | 02 | 1 | GUI-04 | unit | `cargo test -p glass_renderer tab_bar -q` | ✅ partial | ⬜ pending |
| 34-02-02 | 02 | 1 | GUI-05 | unit | `cargo test -p glass_renderer conflict_overlay -q` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_core/src/coordination_poller.rs` — new module with unit tests for poll_once logic
- [ ] `crates/glass_renderer/src/conflict_overlay.rs` — new module with unit tests
- [ ] Tests for extended `StatusLabel` with coordination_text field
- [ ] Tests for extended `TabDisplayInfo` with has_locks field
- [ ] `glass_coordination` dependency added to `glass_core/Cargo.toml`

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Status bar visually shows agent/lock counts | GUI-01, GUI-02 | Visual rendering | Run Glass, register agents via MCP, verify counts appear in status bar |
| Tab lock indicator renders correctly | GUI-04 | Visual rendering | Have an agent lock a file, verify tab shows lock icon |
| Conflict overlay appears on conflict | GUI-05 | Visual rendering + multi-agent scenario | Have two agents lock overlapping files, verify overlay appears |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
