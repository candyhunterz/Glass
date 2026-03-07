---
phase: 30
slug: documentation-distribution
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-07
---

# Phase 30 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Manual verification (content authoring phase — no Rust code) |
| **Config file** | none — Wave 0 installs mdBook |
| **Quick run command** | `mdbook build docs && echo "Build OK"` |
| **Full suite command** | `mdbook build docs && mdbook test docs` |
| **Estimated runtime** | ~5 seconds |

---

## Sampling Rate

- **After every task commit:** Run `mdbook build docs && echo "Build OK"`
- **After every plan wave:** Run `mdbook build docs && mdbook test docs`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 5 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 30-01-01 | 01 | 1 | DOCS-01 | smoke | `mdbook build docs` | No — W0 | pending |
| 30-01-02 | 01 | 1 | DOCS-01 | smoke | `mdbook build docs` | No — W0 | pending |
| 30-02-01 | 02 | 1 | DOCS-02 | manual | Check URL after CI runs | No — W0 | pending |
| 30-03-01 | 03 | 2 | DOCS-03 | manual | Visual inspection of rendered README | No — W0 | pending |
| 30-04-01 | 04 | 2 | PKG-05 | smoke | `winget validate --manifest packaging/winget/` | No — W0 | pending |
| 30-04-02 | 04 | 2 | PKG-06 | smoke | `brew audit --cask glass` (requires macOS) | No — W0 | pending |

*Status: pending · green · red · flaky*

---

## Wave 0 Requirements

- [ ] `docs/book.toml` — mdBook configuration
- [ ] `docs/src/SUMMARY.md` — Table of contents
- [ ] `README.md` — Does not currently exist
- [ ] `packaging/winget/` — Winget manifest directory
- [ ] `.github/workflows/docs.yml` — GitHub Pages deployment workflow
- [ ] mdBook installation: `cargo install mdbook` or use CI action

*Wave 0 creates all foundational files needed for validation.*

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Docs site accessible on GitHub Pages | DOCS-02 | Requires live deployment and DNS | Push to main, wait for Actions, visit URL |
| README renders correctly on GitHub | DOCS-03 | Requires GitHub rendering engine | View repo page after push |
| Screenshots display properly | DOCS-03 | Visual content verification | Check README images load and are clear |
| winget install works end-to-end | PKG-05 | Requires winget-pkgs PR merged | Submit PR, wait for merge, run `winget install` |
| brew install works end-to-end | PKG-06 | Requires tap repo and macOS | Create tap, push formula, run `brew install` |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 5s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
