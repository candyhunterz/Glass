---
phase: 22
slug: cross-platform-validation
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-06
---

# Phase 22 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test (`#[cfg(test)]` + `cargo test`) |
| **Config file** | None (uses Cargo.toml test config) |
| **Quick run command** | `cargo test --workspace` |
| **Full suite command** | `cargo test --workspace && cargo check --target aarch64-apple-darwin && cargo check --target x86_64-unknown-linux-gnu` |
| **Estimated runtime** | ~60 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --workspace`
- **After every plan wave:** Run `cargo test --workspace && cargo check --target aarch64-apple-darwin && cargo check --target x86_64-unknown-linux-gnu`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 22-01-01 | 01 | 1 | P22-06 | compilation | `cargo check --target x86_64-unknown-linux-gnu` | N/A (cargo) | pending |
| 22-01-02 | 01 | 1 | P22-03 | unit | `cargo test -p glass_mux -- platform` | Exists | pending |
| 22-01-03 | 01 | 1 | P22-04 | unit | `cargo test -- shell_integration` | Needs stub | pending |
| 22-01-04 | 01 | 1 | P22-05 | unit | `cargo test -p glass_core -- config` | Needs update | pending |
| 22-02-01 | 02 | 2 | P22-01 | compilation | `cargo check --target aarch64-apple-darwin` | N/A (cargo) | pending |
| 22-02-02 | 02 | 2 | P22-02 | compilation | `cargo check --target x86_64-unknown-linux-gnu` | N/A (cargo) | pending |
| 22-02-03 | 02 | 2 | P22-09 | CI | `gh workflow run ci.yml` | Wave 0 | pending |

*Status: pending / green / red / flaky*

---

## Wave 0 Requirements

- [ ] `.github/workflows/ci.yml` — CI workflow file (does not exist yet)
- [ ] Cross-compilation targets installed (`rustup target add aarch64-apple-darwin x86_64-unknown-linux-gnu`)
- [ ] `crates/glass_core/src/config.rs` test assertions updated for cfg-gated font defaults

*Existing infrastructure covers unit test execution via `cargo test --workspace`.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Glass launches on macOS with correct font | P22-01, P22-05 | Requires macOS hardware + GPU | Launch Glass on macOS, verify Menlo font renders correctly |
| Glass launches on Linux with correct font | P22-02, P22-05 | Requires Linux hardware + GPU | Launch Glass on Linux, verify Monospace font renders correctly |
| Shell integration injects on zsh | P22-04 | Requires macOS/Linux shell | Launch Glass with zsh, verify block decorations appear |
| Shell integration injects on bash | P22-04 | Requires Linux shell | Launch Glass with bash, verify block decorations appear |
| wgpu surface format correct per platform | P22-07 | Requires multi-platform GPU | Check log output for surface format on each platform |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
