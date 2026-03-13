---
phase: 58
slug: agent-approval-ui
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-13
---

# Phase 58 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in (`#[test]`) |
| **Config file** | none — inline `#[cfg(test)] mod tests` per crate |
| **Quick run command** | `cargo test --package glass_renderer 2>&1` |
| **Full suite command** | `cargo test --workspace 2>&1` |
| **Estimated runtime** | ~30 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --package glass_renderer 2>&1`
- **After every plan wave:** Run `cargo test --workspace 2>&1`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 58-01-01 | 01 | 1 | AGTU-01 | unit | `cargo test --package glass_renderer status_bar 2>&1` | ❌ W0 | ⬜ pending |
| 58-01-02 | 01 | 1 | AGTU-02 | unit | `cargo test --package glass_renderer proposal_toast 2>&1` | ❌ W0 | ⬜ pending |
| 58-01-03 | 01 | 1 | AGTU-03 | unit | `cargo test --package glass_renderer proposal_overlay 2>&1` | ❌ W0 | ⬜ pending |
| 58-02-01 | 02 | 2 | AGTU-04 | unit | `cargo test --workspace agent 2>&1` | ✅ (Phase 57) | ⬜ pending |
| 58-02-02 | 02 | 2 | AGTU-05 | manual | Run Glass, open overlay, type — must appear in terminal | N/A | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_renderer/src/proposal_toast_renderer.rs` — stubs + tests for AGTU-02
- [ ] `crates/glass_renderer/src/proposal_overlay_renderer.rs` — stubs + tests for AGTU-03
- [ ] Status bar unit tests extended for new `agent_mode_text` and `proposal_count_text` fields — covers AGTU-01

*Existing Phase 57 tests cover `WorktreeManager::apply()` / `dismiss()` for AGTU-04.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Terminal stays interactive while overlay is open | AGTU-05 | Requires live PTY + GPU rendering; cannot unit test key pass-through end-to-end | Open Glass, trigger proposal, open overlay (Ctrl+Shift+A), type characters — they must appear in terminal |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
