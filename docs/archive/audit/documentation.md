# Glass Documentation Audit -- Prelaunch Readiness

**Auditor:** Claude Opus 4.6 (automated)
**Date:** 2026-03-18
**Scope:** All user-facing and developer-facing documentation, from the perspective of someone discovering Glass on GitHub for the first time.

---

## Executive Summary

Glass has **exceptionally strong documentation** for a prelaunch project. The README.md is comprehensive and well-structured (617 lines), an mdBook documentation site covers installation, features, configuration, MCP tools, and troubleshooting with per-platform depth. Inline code documentation is solid across all 14 crates. The main gaps are: (1) no screenshot or demo visual in the README, (2) no CHANGELOG, (3) no CONTRIBUTING.md, (4) no example configuration files or scripts, (5) CLAUDE.md has a stale crate count, and (6) a few documentation/code mismatches in font defaults. These are all fixable before launch with moderate effort.

**Overall Grade: B+** -- Documentation is above average for open-source Rust projects. The gaps identified below would elevate it to A-level before launch.

---

## Findings by Category

### 1. README.md

**Severity: Low (present and strong, minor gaps)**

**Current State:** The README is 617 lines, well-organized with a table of contents, comparison table, feature list, installation instructions, quick start guide, keyboard shortcuts, full configuration reference, CLI reference, orchestrator mode documentation, multi-agent coordination protocol, MCP tool table (all 33 tools), architecture overview, and performance metrics.

**Strengths:**
- Clear two-audience framing ("For Humans" / "For AI Agents")
- Comparison table against standard terminals is effective
- Quick Start section walks through first use with concrete examples
- Configuration reference includes a complete annotated TOML example
- All 33 MCP tools are listed with descriptions
- Architecture section lists all 16 crates with descriptions
- Performance metrics table with concrete numbers

**Gaps:**

| Issue | Severity | Detail |
|-------|----------|--------|
| No screenshot or demo | **High** | The README has zero visual content. No screenshot, no GIF, no asciinema recording. For a _terminal emulator_, a visual demo is critical -- it's the first thing a visitor evaluates. The only image asset is `assets/icon.png`. |
| No badges | **Low** | No CI status badge, license badge, or crate version badge at the top. Standard for Rust projects on GitHub. |
| CLAUDE.md lists "9 crates" | **Medium** | CLAUDE.md (line 7) says "Rust workspace with 9 crates + main binary" but there are actually 14 crates. The architecture listing below it only names 9, missing glass_errors, glass_soi, glass_agent, glass_feedback, glass_scripting. README.md correctly says 16 crates. |
| Font defaults mismatch | **Low** | README says defaults are Consolas/Menlo/Monospace. The mdBook config reference says Cascadia Code/SF Mono/DejaVu Sans Mono. Code should be checked to determine which is accurate; one document is wrong. |
| Architecture says "glass_mcp: 31 tools" | **Low** | README architecture section (line 566) says "MCP server (31 tools)" but the MCP Tools section and mdBook both say 33 tools. |

**Recommendation:**
1. Add a hero screenshot or animated GIF at the top of the README showing Glass with command blocks, pipe visualization, and SOI decorations.
2. Add CI/license/version badges.
3. Update CLAUDE.md to list all 14 crates.
4. Reconcile font default documentation.
5. Fix "31 tools" to "33 tools" in the Architecture section.

---

### 2. Installation Instructions

**Severity: Low (comprehensive)**

**Current State:** Three installation methods are documented in the README (pre-built binaries, build from source, cargo install). The mdBook has dedicated per-platform pages (`docs/src/installation/windows.md`, `macos.md`, `linux.md`) with system requirements, Gatekeeper/SmartScreen workarounds, and package manager instructions.

**Strengths:**
- Linux system dependencies listed (`libxkbcommon-dev`, etc.)
- SmartScreen and Gatekeeper workarounds documented
- System requirements per platform (OS version, GPU, architecture)
- Release workflow (`release.yml`) builds MSI, DMG, and DEB automatically

**Gaps:**

| Issue | Severity | Detail |
|-------|----------|--------|
| Build-from-source Linux deps incomplete | **Low** | README lists `libxkbcommon-dev libwayland-dev libx11-dev libxi-dev` but the Linux install page in mdBook does not list these build-time dependencies at all -- it only covers runtime GPU drivers. Someone building from source on Linux using the mdBook guide would hit missing header errors. |
| No Arch/Fedora package instructions | **Low** | Only `.deb` is documented. No `.rpm`, AUR, or Flatpak instructions despite these being mentioned in the PRD (Phase 6). Note: these may not be built yet, so this is informational. |
| Homebrew cask is placeholder | **Low** | `packaging/homebrew/glass.rb` has `<SHA256>` and `<GITHUB_USER>` placeholders. Documented as `brew install candyhunterz/glass/glass` in mdBook but the formula isn't functional yet. |
| Winget package not published | **Low** | Winget manifest exists in `packaging/winget/` but the mdBook says "Once the package is published..." -- not yet available. |

**Recommendation:**
1. Add build-from-source dependencies to the mdBook Linux page.
2. Mark Homebrew and Winget as "coming soon" more prominently, or ship them before launch.
3. Consider adding Flatpak/AppImage instructions for broader Linux coverage.

---

### 3. Configuration Reference

**Severity: Low (thorough)**

**Current State:** Configuration is documented in three places:
1. README.md -- Complete annotated `config.toml` example covering all sections (font, history, snapshot, pipes, soi, agent, agent.permissions, agent.quiet_rules, agent.orchestrator, scripting)
2. mdBook `docs/src/configuration.md` -- Full reference with tables for every key, type, default, and description. Includes hot-reload behavior, error handling, and in-app settings overlay documentation.
3. ORCHESTRATOR.md -- Additional orchestrator-specific config fields documented in a Config Reference section.

**Strengths:**
- Every config key has a documented type, default, and description
- Hot-reload behavior is explicitly explained
- Error handling (config parse errors) is documented with the overlay behavior
- In-app settings overlay documented
- Both README and mdBook have independently complete config documentation

**Gaps:**

| Issue | Severity | Detail |
|-------|----------|--------|
| README and mdBook config defaults differ | **Medium** | README `[history]` shows `max_entries = 50000` and `keep_failures = true`. mdBook shows `max_output_capture_kb = 50` only, with no mention of `max_entries` or `keep_failures`. These may be different actual config keys, but a user reading both would be confused about what the history section actually supports. |
| `soi.min_lines` default differs | **Low** | README says `min_lines = 3`, mdBook config page says default is `5`. One is wrong. |
| Missing config keys in mdBook | **Low** | The ORCHESTRATOR.md documents `fast_trigger_secs`, `orchestrator_mode`, `verify_files`, `agent_prompt_pattern`, `max_total_scripts`, `max_mcp_tools`, `script_generation`, `ablation_enabled`, `ablation_sweep_interval` -- some of these do not appear in the mdBook config reference or README. |
| No default config file shipped | **Low** | No `config/default.toml` or example config exists in the repository (the PRD Phase 0 mentions it but it was never created). Users must assemble their config from documentation. |

**Recommendation:**
1. Reconcile `soi.min_lines` default across README and mdBook.
2. Add all orchestrator/scripting config keys to the mdBook configuration reference.
3. Ship a well-commented `config.example.toml` in the repository root so users can copy and customize.

---

### 4. Feature Documentation

**Severity: Low (strong coverage)**

**Current State:** All major features are documented in both the README and mdBook:

| Feature | README | mdBook | Quality |
|---------|--------|--------|---------|
| Command blocks | Yes | `features/blocks.md` | Good |
| Undo | Yes | `features/undo.md` | Good |
| Pipe visualization | Yes | `features/pipes.md` | Good |
| History/search | Yes | `features/search.md`, `features/history.md` | Good |
| Tabs and panes | Yes | `features/tabs-panes.md` | Good |
| SOI | Yes | `features/soi.md` | Excellent -- lists all 19 parsers, compression levels |
| Agent Mode | Yes | `features/agent-mode.md` | Excellent -- modes, worktree isolation, approval UI |
| Orchestrator | Yes | `features/orchestrator.md` | Excellent -- kickoff, workflows, checkpoint, safety |
| Activity Stream | Yes | `features/activity-stream.md` | Good |
| Settings Overlay | Yes | `features/settings.md` | Good |
| Multi-agent coordination | Yes | `agent-coordination.md` | Excellent -- full protocol |
| Shell integration | Yes | `getting-started.md` | Good (automatic, no manual setup needed) |
| MCP server | Yes | `mcp-server.md` | Excellent -- all 33 tools, setup for Claude Desktop/Code |
| Scripting | Yes | Within config docs | Adequate but thin |
| Feedback loop | README mentions it | ORCHESTRATOR.md has deep coverage | Dev-only depth |

**Gaps:**

| Issue | Severity | Detail |
|-------|----------|--------|
| Scripting lacks dedicated mdBook page | **Medium** | Rhai scripting is a significant feature (Tier 4 of the feedback loop, 2 MCP tools, 20 hook points, GlassApi with read/action methods, profiles). It has no dedicated feature page in the mdBook. The only user-facing scripting documentation is a brief mention in the config reference. ORCHESTRATOR.md has extensive scripting documentation but that is developer-facing. |
| Feedback loop not user-documented | **Low** | The self-improvement feedback loop (rules, attribution, ablation, postmortems) is described in ORCHESTRATOR.md (developer reference) but has no user-facing documentation. Users running the orchestrator would benefit from understanding how rules work, what `rules.toml` contains, and how to pin/reject rules. |
| No "How to write a PRD" guide | **Low** | Orchestrator Mode depends on PRD.md but no guidance exists on what makes a good PRD for the orchestrator. |

**Recommendation:**
1. Create `docs/src/features/scripting.md` with user-facing scripting documentation: how to write scripts, hook points, GlassApi reference, lifecycle, and profiles.
2. Consider a brief user-facing guide to the feedback loop and how to interact with `rules.toml`.
3. Add PRD authoring tips to the orchestrator feature page.

---

### 5. Key Bindings Reference

**Severity: Low (complete)**

**Current State:** Keyboard shortcuts are documented in:
1. README.md -- Full table with Windows/Linux and macOS columns, organized by category (Core, Tabs, Panes, Navigation, Overlays)
2. mdBook `getting-started.md` -- Key shortcuts table
3. In-app: Settings overlay (Ctrl+Shift+,) has a Shortcuts tab

**Strengths:**
- All known shortcuts documented
- macOS equivalents (Cmd vs Ctrl) listed
- Mouse interactions included (middle-click tab close, drag-to-reorder, mouse selection)
- In-app cheatsheet available

**Gaps:**

| Issue | Severity | Detail |
|-------|----------|--------|
| No mention of Ctrl+Shift+U (check updates) in mdBook | **Low** | README lists it but the mdBook getting-started page omits it. |
| No mention of Cmd equivalents in mdBook | **Low** | The mdBook shortcuts table only shows `Ctrl+Shift+` variants, not macOS `Cmd+Shift+` equivalents. |

**Recommendation:**
1. Add macOS keybinding column to the mdBook shortcuts table.
2. Ensure all shortcuts from README appear in mdBook.

---

### 6. Architecture Documentation

**Severity: Medium (CLAUDE.md is stale)**

**Current State:**
- `CLAUDE.md` (108 lines) -- Developer-facing project context for Claude Code. Lists architecture, key files, tech stack, build commands, CI, platform notes, conventions.
- `ORCHESTRATOR.md` (619 lines) -- Deep developer reference for orchestrator architecture, state machine, feedback loop, scripting engine, ablation/attribution.
- `.planning/codebase/` -- 7 deep-analysis docs (STACK, ARCHITECTURE, STRUCTURE, CONVENTIONS, TESTING, INTEGRATIONS, CONCERNS).
- README.md Architecture section -- User-facing overview of all crates.
- PRD.md (738 lines) -- Full product requirements document.

**Gaps:**

| Issue | Severity | Detail |
|-------|----------|--------|
| CLAUDE.md lists 9 crates, actual count is 14 | **Medium** | The architecture listing is significantly outdated. Missing: glass_errors, glass_soi, glass_agent, glass_feedback, glass_scripting. Also missing glass_config and glass_protocol mentioned in README (if they exist as separate crates). The key files list is also incomplete. |
| CLAUDE.md config section lists only 5 sections | **Low** | Says "Sections: font, shell, history, snapshot, pipes" but config now includes soi, agent, agent.permissions, agent.quiet_rules, agent.orchestrator, scripting. |
| No user-facing architecture doc in mdBook | **Low** | The mdBook has no architecture or "How it works" page. For users who want to understand Glass's design without reading CLAUDE.md, there is no overview. README has one, but the mdBook is the documentation site. |

**Recommendation:**
1. Update CLAUDE.md to list all 14 crates, all config sections, and all key files for newer features.
2. Consider adding a brief architecture page to the mdBook for curious users.

---

### 7. API/MCP Documentation

**Severity: Low (excellent)**

**Current State:** MCP tools are documented in:
1. README.md -- Full 33-tool table with categories and descriptions
2. mdBook `mcp-server.md` -- Detailed per-tool descriptions organized by category, setup instructions for Claude Desktop/Claude Code/generic MCP clients, token efficiency features, privacy notice
3. CLAUDE.md -- Multi-agent coordination protocol

**Strengths:**
- All 33 tools listed and described
- Setup instructions for multiple AI clients
- Token efficiency patterns documented
- Privacy statement included
- Coordination protocol is step-by-step

**Gaps:**

| Issue | Severity | Detail |
|-------|----------|--------|
| No parameter documentation for MCP tools | **Medium** | Tool descriptions explain what each tool does, but none document the parameters they accept, return types, or example request/response payloads. An AI agent developer integrating with Glass would need to rely on the MCP `tools/list` schema discovery. |
| No MCP error handling docs | **Low** | No documentation on error responses, rate limiting, or what happens when the Glass GUI is not running while MCP is invoked. |

**Recommendation:**
1. Add parameter schemas and example payloads for the most commonly used tools (glass_history, glass_context, glass_compressed_context, glass_agent_register, glass_tab_send).
2. Document error behavior (e.g., "glass_undo returns an error if no snapshot exists for the given command ID").

---

### 8. Shell Integration Documentation

**Severity: Low (adequate)**

**Current State:** Shell integration is documented as automatic and invisible to the user:
- README mentions it briefly
- mdBook `getting-started.md` explains the OSC 133 sequences and lists supported shells
- CLAUDE.md references the `shell-integration/` directory
- Shell scripts themselves are well-commented (e.g., `glass.bash` has 17 lines of header comments explaining usage, requirements, and compatibility)

**Strengths:**
- Auto-injection means users never need to manually configure shell integration
- All four shells documented as supported (bash, zsh, fish, PowerShell)
- Scripts have inline documentation
- Troubleshooting page covers "Shell not detected" scenario

**Gaps:**

| Issue | Severity | Detail |
|-------|----------|--------|
| No manual installation fallback docs | **Low** | If auto-injection fails (unusual shell configs, restricted environments), there are no instructions for manually sourcing the integration scripts. The scripts exist at `shell-integration/glass.{bash,zsh,fish,ps1}` but this path is not documented for users. |
| No compatibility notes | **Low** | The bash script comments note it needs bash >= 4.4 for full functionality, but this is not in the user-facing docs. |

**Recommendation:**
1. Add a "Manual Shell Integration" section to the troubleshooting page for fallback scenarios.
2. Note bash version requirement in the getting-started page.

---

### 9. Inline Code Documentation

**Severity: Low (good coverage)**

**Current State:**
- **Module-level docs (`//!`):** 470 occurrences across 103 source files. All 14 crate `lib.rs` files have module-level documentation. Most individual source files have module-level doc comments.
- **Item-level docs (`///`):** 1,391 occurrences across 128 source files. Public types, functions, and key methods are generally documented.
- **Quality highlights:**
  - `glass_soi/src/lib.rs` has a usage example in its doc comment
  - `glass_mcp/src/lib.rs` documents all four original tools with descriptions
  - `glass_terminal/src/lib.rs` documents re-exports
  - `glass_errors/src/lib.rs` documents the extract_errors API
  - `glass_coordination/src/lib.rs` documents resolve_db_path and canonicalize_path
  - `glass_core/src/config.rs` has doc comments on all public types and fields

**Gaps:**

| Issue | Severity | Detail |
|-------|----------|--------|
| Some crate lib.rs missing module docs | **Low** | `glass_scripting/src/lib.rs` and `glass_core/src/lib.rs` have no `//!` module doc comment (just bare `pub mod` declarations). |
| Sparse docs on some key modules | **Low** | `glass_snapshot/src/undo.rs` has only 1 doc comment. `glass_history/src/db.rs` has only 5. These are core APIs that benefit from thorough documentation. |
| No `cargo doc` integration in CI | **Low** | The CI workflow does not run `cargo doc --no-deps` to verify doc builds. A `docs.yml` workflow exists for mdBook but not for Rust API docs. |

**Recommendation:**
1. Add `//!` module docs to `glass_scripting/src/lib.rs` and `glass_core/src/lib.rs`.
2. Improve doc coverage on `glass_snapshot/src/undo.rs` (the core undo API).
3. Consider adding `cargo doc` to CI and publishing API docs alongside mdBook.

---

### 10. CHANGELOG

**Severity: High (missing entirely)**

**Current State:** No CHANGELOG, CHANGES, or RELEASES file exists anywhere in the repository. The Cargo.toml version is `2.5.0`, indicating many releases have occurred. There is a release workflow that generates release notes from git history, but no persistent changelog.

**Recommendation:**
1. Create `CHANGELOG.md` following the [Keep a Changelog](https://keepachangelog.com/) format.
2. At minimum, document changes for v1.0, v2.0, v2.5, and v3.0 milestones referenced in the README feature list.
3. Consider using `git cliff` or similar tooling to auto-generate from conventional commits.

---

### 11. LICENSE

**Severity: Low (present and correct)**

**Current State:** `LICENSE` file exists at the repository root with a standard MIT License text. Copyright is "2026 Glass Contributors". The README references it at the bottom. `Cargo.toml` declares `license = "MIT"`. The `.deb` package metadata references it.

No issues found.

---

### 12. CONTRIBUTING.md

**Severity: High (missing entirely)**

**Current State:** No CONTRIBUTING.md, CONTRIBUTING, or contributor guide exists. The README has no "Contributing" section. There are no issue templates or PR templates in `.github/`.

For a project seeking external contributors, this is a significant gap. The project does have strong internal conventions (documented in CLAUDE.md and `.planning/codebase/CONVENTIONS.md`) but none of these are user-facing.

**Recommendation:**
1. Create `CONTRIBUTING.md` covering:
   - How to report bugs (issue template)
   - How to propose features
   - Development setup instructions (prerequisites, build, test, lint)
   - Code style (cargo fmt, clippy -D warnings)
   - Test conventions (inline tests, platform gating)
   - PR process (master branch for dev, main for PRs)
   - Architecture overview (pointer to CLAUDE.md)
2. Add `.github/ISSUE_TEMPLATE/` with bug report and feature request templates.
3. Add `.github/PULL_REQUEST_TEMPLATE.md`.

---

### 13. Examples

**Severity: Medium (no examples exist)**

**Current State:** No `examples/` directory, no `config.example.toml`, no example Rhai scripts, no example PRD.md for the orchestrator. The only "examples" are inline code snippets in the README and mdBook.

**Gaps:**

| Issue | Severity | Detail |
|-------|----------|--------|
| No example config file | **Medium** | Users must assemble their config from scattered documentation. A well-commented `config.example.toml` would save significant time. |
| No example Rhai scripts | **Medium** | The scripting feature has no example scripts showing how to write a hook script, an MCP tool script, or use the GlassApi. This makes adoption of the scripting feature difficult. |
| No example PRD for orchestrator | **Low** | Users are told to "write a PRD.md" but have no template or example to work from. |
| No example MCP client integration | **Low** | Beyond the Claude Desktop/Code JSON snippets, no example of programmatic MCP client usage exists. |

**Recommendation:**
1. Create `config.example.toml` with all sections and annotated comments.
2. Create `examples/scripts/` with 2-3 example Rhai scripts (e.g., auto-commit on test pass, force snapshot on specific commands, custom MCP tool).
3. Create `examples/prd-template.md` as a starting point for orchestrator users.

---

## Additional Findings

### 14. mdBook Documentation Site

**Severity: Low (well-structured)**

The mdBook site at `docs/` is comprehensive with 14 user-facing pages organized into Getting Started, Features (11 pages), and Reference (4 pages). A GitHub Actions workflow (`docs.yml`) deploys it to GitHub Pages. The site covers all major features with consistent style and good cross-linking between pages.

The `docs/superpowers/` directory contains internal planning/spec documents (17 files) that are not linked from the mdBook SUMMARY.md -- this is correct; they are development artifacts, not user docs.

### 15. Stale Internal References

| Issue | Severity | Detail |
|-------|----------|--------|
| CLAUDE.md "glass_mcp" lists 4 tools | **Low** | The `//!` doc comment in `glass_mcp/src/lib.rs` (line 3-8) says "Provides four tools" but there are now 33. This is the original description from before tools were added. |
| CLAUDE.md key files list is incomplete | **Low** | Missing entries for orchestrator.rs, script_bridge.rs, and other files added in later milestones. However, CLAUDE.md does list orchestrator.rs in the Key Files section -- the issue is the architecture section listing only 9 crates. |
| PRD Phase 0 references `config/default.toml` | **Low** | This file was never created. |

---

## Priority Fix List

Ordered by impact on a first-time GitHub visitor:

| Priority | Item | Severity | Effort |
|----------|------|----------|--------|
| **P0** | Add screenshot/demo GIF to README | High | 1-2 hours |
| **P1** | Create CHANGELOG.md | High | 2-4 hours |
| **P2** | Create CONTRIBUTING.md | High | 1-2 hours |
| **P3** | Create config.example.toml | Medium | 30 min |
| **P4** | Create example Rhai scripts | Medium | 1-2 hours |
| **P5** | Create scripting feature page in mdBook | Medium | 1-2 hours |
| **P6** | Update CLAUDE.md crate count and architecture listing | Medium | 30 min |
| **P7** | Add MCP tool parameter documentation | Medium | 2-3 hours |
| **P8** | Reconcile config default mismatches (soi.min_lines, fonts, history keys) | Medium | 30 min |
| **P9** | Add CI/license badges to README | Low | 15 min |
| **P10** | Add macOS keybinding column to mdBook shortcuts | Low | 15 min |
| **P11** | Fix "31 tools" to "33 tools" in README Architecture | Low | 5 min |
| **P12** | Add build deps to mdBook Linux install page | Low | 15 min |
| **P13** | Add module docs to glass_scripting and glass_core lib.rs | Low | 15 min |
| **P14** | Update glass_mcp lib.rs doc comment (4 tools -> 33) | Low | 5 min |
| **P15** | Add .github/ issue and PR templates | Low | 30 min |
| **P16** | Add cargo doc to CI | Low | 15 min |
| **P17** | Create PRD template example | Low | 30 min |

---

## Summary Scorecard

| Category | Score | Notes |
|----------|-------|-------|
| README.md | 9/10 | Exceptionally thorough; just needs a screenshot |
| Install instructions | 8/10 | Per-platform with system reqs; minor gaps in build-from-source |
| Configuration reference | 8/10 | Complete in mdBook; minor default mismatches across sources |
| Feature documentation | 8/10 | All features covered; scripting needs its own page |
| Key bindings | 9/10 | Complete with macOS equivalents in README |
| Architecture docs | 7/10 | CLAUDE.md is stale; ORCHESTRATOR.md is excellent |
| API/MCP docs | 8/10 | All tools listed; needs parameter schemas |
| Shell integration | 8/10 | Auto-injection well-documented; manual fallback missing |
| Inline code docs | 7/10 | Good coverage overall; a few thin spots |
| CHANGELOG | 0/10 | Missing entirely |
| LICENSE | 10/10 | Present and correct |
| CONTRIBUTING.md | 0/10 | Missing entirely |
| Examples | 2/10 | Only inline snippets; no standalone examples |

**Overall: B+** -- Strong foundation with a few critical gaps (CHANGELOG, CONTRIBUTING, visual demo) that are straightforward to address before launch.
