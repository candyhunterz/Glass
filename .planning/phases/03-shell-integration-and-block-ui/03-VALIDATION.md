---
phase: 3
slug: shell-integration-and-block-ui
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-04
---

# Phase 3 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | cargo test (built-in Rust test framework) |
| **Config file** | Cargo.toml workspace test configuration |
| **Quick run command** | `cargo test -p glass_terminal --lib` |
| **Full suite command** | `cargo test --workspace` |
| **Estimated runtime** | ~10 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p glass_terminal --lib`
- **After every plan wave:** Run `cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 10 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 03-01-01 | 01 | 1 | SHEL-01 | unit | `cargo test -p glass_terminal osc_scanner` | No - W0 | pending |
| 03-01-02 | 01 | 1 | SHEL-02 | unit | `cargo test -p glass_terminal osc_scanner` | No - W0 | pending |
| 03-01-03 | 01 | 1 | BLOK-01 | unit | `cargo test -p glass_terminal block_manager` | No - W0 | pending |
| 03-01-04 | 01 | 1 | BLOK-02 | unit | `cargo test -p glass_terminal block_manager` | No - W0 | pending |
| 03-01-05 | 01 | 1 | BLOK-03 | unit | `cargo test -p glass_terminal block_manager` | No - W0 | pending |
| 03-02-01 | 02 | 1 | BLOK-01 | manual | Visual inspection: blocks have separator lines | N/A | pending |
| 03-02-02 | 02 | 1 | BLOK-02 | manual | Visual: exit code badges render correctly | N/A | pending |
| 03-02-03 | 02 | 1 | BLOK-03 | manual | Visual: duration labels display correctly | N/A | pending |
| 03-03-01 | 03 | 2 | SHEL-03 | manual | Run Glass with pwsh, execute commands, observe blocks | N/A | pending |
| 03-03-02 | 03 | 2 | SHEL-03 | manual | Verify Oh My Posh/Starship prompt preserved | N/A | pending |
| 03-04-01 | 04 | 2 | SHEL-04 | manual | Run Glass with bash, execute commands, observe blocks | N/A | pending |
| 03-04-02 | 04 | 2 | STAT-01 | unit + manual | `cargo test -p glass_terminal status` + visual | No - W0 | pending |
| 03-04-03 | 04 | 2 | STAT-02 | unit | `cargo test -p glass_terminal status` | No - W0 | pending |

*Status: pending · green · red · flaky*

---

## Wave 0 Requirements

- [ ] `crates/glass_terminal/src/osc_scanner.rs` — test stubs for SHEL-01, SHEL-02
- [ ] `crates/glass_terminal/src/block_manager.rs` — test stubs for BLOK-01, BLOK-02, BLOK-03
- [ ] `crates/glass_terminal/src/status.rs` — test stubs for STAT-01, STAT-02

*Existing cargo test infrastructure covers framework needs.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Block visual separation | BLOK-01 | Requires visual rendering inspection | Run commands in Glass, verify separator lines between blocks |
| Exit code badge colors | BLOK-02 | Color rendering requires visual check | Run successful + failing commands, verify green/red badges |
| Duration label display | BLOK-03 | Visual rendering check | Run commands of varying duration, verify labels |
| PowerShell integration | SHEL-03 | Requires live shell environment | Source glass.ps1, run commands, verify blocks appear |
| Bash integration | SHEL-04 | Requires live shell environment | Source glass.bash, run commands, verify blocks appear |
| Oh My Posh compatibility | SHEL-03 | Requires Oh My Posh installed | Install OMP, source glass.ps1, verify prompt preserved |
| Starship compatibility | SHEL-03 | Requires Starship installed | Install Starship, source glass.ps1, verify prompt preserved |
| Status bar CWD update | STAT-01 | Requires live terminal + cd | Run cd commands, verify status bar updates |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 10s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
