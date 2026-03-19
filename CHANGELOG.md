# Changelog

All notable changes to Glass are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [3.0] - 2026-03-18

### Added
- Ablation testing for confirmed feedback rules
- Attribution tracking for orchestrator feedback rules
- Ablation sweep interval configuration field
- LLM-based qualitative feedback analysis after orchestrator runs
- Feedback loop data model with Tier 1/2/3 rule storage
- Prompt hint injection into orchestrator checkpoint synthesis
- Self-improving feedback loop: 15 detectors, auto config tuning, regression guard, rule lifecycle
- Tier 4 scripting: Rhai-based automation scripts generated from feedback findings
- Low-priority UX polish batch (UX-14/15/18/20)
- Extractable chrome colors into ThemeConfig with dark/light presets
- First-run onboarding via ~/.glass/state.toml
- Tab bar overflow with scroll arrows
- Undo feedback, Shift+Arrow scroll, Alt+Arrow guard, pipeline Escape
- Running command indicator, dynamic CWD truncation, pipeline Escape hint

### Fixed
- Search label, exit code badge, tab underline, config path fixes

## [2.5] - 2026-03-17

### Added
- Performance audit fixes: skip buffer rebuild when terminal unchanged, dirty-flag redraw tracking, frame throttling

## [2.0] - 2026-03-16

### Added
- Orchestrator Mode: silence-triggered autonomous build/audit loop
- Metric guard with floor verification (test counts, clippy, build)
- Checkpoint synthesis and stuck detection
- Settings overlay with live config editing
- Activity stream with severity-colored event feed
- Orchestrator overlay showing iteration log, PRD progress, metric history
- Commented-out default config on first launch
- Scoop package manifest for Windows
- Expanded README with Linux deps, Gatekeeper notes, CLI reference

### Fixed
- GPU init shows friendly error instead of panicking
- Warn when shell integration is unavailable or shell unsupported
- Repository, homepage, keywords, rust-version in Cargo.toml

## [1.5] - 2026-03-14

### Added
- Self-improvement scripting layer (Rhai engine, hook system, sandbox, profiles)
- Script lifecycle management (provisional, confirmed, stale, archived)
- MCP tool exposure from Rhai scripts
- Script generation from feedback loop
- Embedded shell integration scripts in binary
- macOS Intel (x86_64) release binary in CI
- Pipeline capture for zsh and fish shells

### Fixed
- Deduplicated IPC path constants
- winresource made Windows-only build dependency
- Vulkan fallback on Windows for older GPUs
- macOS orphan prevention watchdog
- Clippy on all three CI platforms

## [1.0] - 2026-03-08

### Added
- GPU-accelerated terminal emulator with wgpu rendering
- Command blocks with exit codes, durations, CWD badges
- Command-level undo via pre-exec filesystem snapshots
- Visual pipeline debugging with per-stage capture
- Full-text history search (SQLite FTS5)
- Tabs and split panes (binary split tree)
- 33 MCP tools for AI agent integration
- Structured Output Intelligence (SOI) with 19 format-specific parsers
- Multi-agent coordination (advisory locks, messaging, registry)
- Shell integration for bash, zsh, fish, and PowerShell (OSC 133)
- Cross-platform: Windows (ConPTY), macOS (forkpty), Linux (forkpty)
- MCP permission gate and audit logging
- Session nonce authentication for agent coordination
- Content-addressed snapshot blob store (blake3 hashing)
- Config hot-reload via filesystem watcher

### Fixed
- Hardened IPC socket permissions to current user only
- Zeroize OAuth bearer token after usage poll
- Redact sensitive patterns from command history
- Validate undo paths within project root, enforce hex blob hashes
- Regex size limit, documented ephemeral agent permissions
- mktemp for unpredictable shell temp directories
- Set 0600 on DB files, escape LIKE wildcards in CWD filter
- Validate config paths, workspace trust
- Detect corrupt SQLite databases and auto-recover
- Native error dialog on init failure
- Track consecutive GPU surface failures for device-lost recovery
- session()/session_mut() return Option instead of panicking
