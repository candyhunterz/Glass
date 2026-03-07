---
phase: 26
slug: performance-profiling-optimization
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-07
---

# Phase 26 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | criterion 0.5 (benchmarks) + cargo test (existing 436 tests) |
| **Config file** | benches/perf_benchmarks.rs (new — Wave 0) |
| **Quick run command** | `cargo bench -- --quick` |
| **Full suite command** | `cargo bench` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo bench -- --quick`
- **After every plan wave:** Run `cargo bench`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 26-01-01 | 01 | 1 | PERF-01 | integration | `cargo bench 2>&1 \| grep -q "cold_start\|resolve_color\|osc_scan"` | ❌ W0 | ⬜ pending |
| 26-01-02 | 01 | 1 | PERF-02 | smoke | `cargo run --release --features perf -- --help && ls glass-trace.json` | ❌ W0 | ⬜ pending |
| 26-02-01 | 02 | 2 | PERF-03 | manual | Manual: run glass, check PERF logs, compare to PERFORMANCE.md | N/A | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `benches/perf_benchmarks.rs` — criterion harness with benchmark stubs for PERF-01
- [ ] Root `Cargo.toml` — `[features] perf = [...]`, `[[bench]]` section, criterion dev-dep
- [ ] `crates/glass_terminal/Cargo.toml` — `[features] perf = []`
- [ ] `crates/glass_renderer/Cargo.toml` — `[features] perf = []`

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Cold start, input latency, idle memory meet targets | PERF-03 | Requires GPU + PTY runtime | 1. Build release: `cargo build --release` 2. Run Glass 3. Check PERF logs in terminal output 4. Compare to PERFORMANCE.md targets |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
