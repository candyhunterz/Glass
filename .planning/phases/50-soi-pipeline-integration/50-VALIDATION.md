---
phase: 50
slug: soi-pipeline-integration
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-12
---

# Phase 50 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in (`#[test]`) + criterion for benchmarks |
| **Config file** | None — tests inline per project convention |
| **Quick run command** | `cargo test --workspace` |
| **Full suite command** | `cargo test --workspace && cargo bench` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --workspace`
- **After every plan wave:** Run `cargo test --workspace && cargo clippy --workspace -- -D warnings`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 50-01-01 | 01 | 0 | SOIL-03 | unit | `cargo test -p glass_core event_soi_ready` | ❌ W0 | ⬜ pending |
| 50-01-02 | 01 | 0 | SOIL-02 | benchmark | `cargo bench -- bench_input_processing` | ❌ W0 | ⬜ pending |
| 50-01-03 | 01 | 0 | SOIL-01 | integration | `cargo test -p glass_history soi_pipeline` | ❌ W0 | ⬜ pending |
| 50-01-04 | 01 | 0 | SOIL-04 | unit | `cargo test -p glass_history soi_worker_no_output` | ❌ W0 | ⬜ pending |
| 50-01-05 | 01 | 0 | SOIL-04 | unit | `cargo test -p glass_history soi_worker_binary` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `benches/perf_benchmarks.rs` — add `bench_input_processing` benchmark covering `glass_history::output::process_output` on a 50 KB payload (SOIL-02 proxy)
- [ ] Test for `AppEvent::SoiReady` variant in `glass_core` — add to `crates/glass_core/src/event.rs` `#[cfg(test)]` block
- [ ] Integration test for SOI worker flow in `glass_history` — requires a temp DB with a command row; call worker logic as a function

*If none: "Existing infrastructure covers all phase requirements."*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Terminal input latency unchanged during SOI parse | SOIL-02 | Perceptual latency requires interactive use | Type rapidly while a large command output is being parsed; verify no noticeable delay |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
