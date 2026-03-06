# Project Research Summary

**Project:** Glass v1.3 -- Pipe Visualization
**Domain:** Pipeline stage capture and visualization for GPU-accelerated terminal emulator
**Researched:** 2026-03-05
**Confidence:** HIGH

## Executive Summary

Glass v1.3 adds pipe visualization -- automatic capture and display of intermediate pipeline stage output. No terminal emulator currently offers this. The approach is a two-tier system: PowerShell uses `Tee-Object -Variable` insertion via PSReadLine's `Replace()` method (clean, reliable), while bash/zsh uses tee-based command rewriting in shell integration scripts with temp file capture. Both tiers share the same OSC 133;S/P protocol extension to transport captured stage data from shell to terminal.

No new heavy dependencies are needed. The feature builds entirely on existing workspace crates (shlex, rusqlite, tempfile, strip-ansi-escapes). The `glass_pipes` stub crate becomes a pure-logic crate with zero dependencies handling pipe parsing and TTY detection. Shell integration scripts (glass.ps1, glass.bash) gain pipe rewriting logic. The terminal gains OSC 133;S/P parsing, a new `pipe_stages` DB table, multi-row pipeline UI blocks, and a `GlassPipeInspect` MCP tool.

The primary risks are: (1) TTY detection changes -- inserting tee causes `isatty()` to return false, changing output format for ls/git/grep; (2) exit code swallowing -- tee's success masks real pipeline failures unless PIPESTATUS is captured; (3) shell quoting corruption during command rewriting. All three are mitigated by the chosen approach: string-level insertion at pipe boundaries (not parse-and-reconstruct), TTY command denylist, and PIPESTATUS capture. Bash tee rewriting should be carefully implemented with the understanding that it modifies command execution.

## Key Findings

### Recommended Stack

No new external dependencies. Promote `tempfile` from dev-dep to regular dep.

| Existing Crate | v1.3 Usage |
|---------------|------------|
| shlex 1.3.0 | Tokenization reference for pipe splitter |
| rusqlite 0.38.0 | New `pipe_stages` table |
| strip-ansi-escapes 0.2.1 | Strip ANSI from captured stage output |
| tempfile 3.26.0 | Temp files for bash tee capture |
| rmcp 1.1.0 | GlassPipeInspect MCP tool |

### Expected Features

**Table stakes:**
- Automatic pipe detection from command text (quote-aware `|` splitting)
- Per-stage intermediate output capture (bash: tee, PowerShell: Tee-Object)
- Multi-row pipeline UI blocks with stage labels and line/byte counts
- Expand/collapse stage output (auto-expand on failure or >2 stages)
- TTY-sensitive command detection and auto-exclusion
- Per-command opt-out flag
- Exit code preservation via PIPESTATUS/pipestatus
- Pipe stage storage in history DB with retention

**Differentiators:**
- First terminal emulator with automatic pipe stage visualization
- GlassPipeInspect MCP tool for AI pipeline debugging
- Cross-platform (PowerShell + bash/zsh)

**Defer to future:**
- Streaming stage capture (real-time), stage output diff, nested subshell pipes, pipeline performance profiling

### Architecture Approach

**Shell-side rewriting** is the only viable approach -- the terminal sees only final pipeline output via PTY.

- **PowerShell:** PSReadLine `Replace()` inserts `Tee-Object -Variable` between stages before `AcceptLine()`. Variables read post-execution, serialized via `Out-String`, emitted as OSC 133;P.
- **Bash/Zsh:** Shell integration inserts `tee /tmp/glass-pipe-$$/N` between stages. Temp files read post-execution, emitted as OSC 133;P. PIPESTATUS captured for exit codes.
- **Protocol:** OSC 133;S (pipeline start with stage count + base64 original command) and OSC 133;P (per-stage base64-encoded output).
- **glass_pipes** is a zero-dependency pure logic crate (pipe parsing, TTY detection).
- **Block struct** gains `pipeline_stages` and `pipeline_expanded` fields.
- **DB:** New `pipe_stages` table with schema migration v1->v2.

### Critical Pitfalls

1. **TTY detection (isatty)** -- tee insertion causes commands to detect non-TTY stdout. Mitigation: TTY command denylist, accept visual differences for captured stages.
2. **Exit code swallowing** -- tee's exit code masks real failures. Mitigation: PIPESTATUS/pipestatus capture in shell integration.
3. **Shell quoting corruption** -- parse-and-reconstruct breaks complex commands. Mitigation: string-level tee insertion at pipe boundaries only, never reconstruct.
4. **PowerShell object pipeline** -- fundamentally different from byte streams. Mitigation: separate code path, text-only capture via Out-String, accept lossy representation.
5. **Buffer explosion** -- large intermediate output grows memory. Mitigation: per-stage byte cap (50KB default), head/tail sampling.
6. **Pipe detection edge cases** -- `|` inside quotes, `|&`, no spaces. Mitigation: quote-aware splitter with comprehensive test suite.
7. **Security (temp files)** -- world-readable temp files containing sensitive data. Mitigation: PID-scoped temp dirs, immediate cleanup, restrictive permissions.

## Implications for Roadmap

Suggested 5-phase structure (continuing from phase 14):

**Phase 15: Pipe Parsing + glass_pipes Core** -- Zero-dependency crate with pipe splitter, TTY detector, data types. Foundation for everything.

**Phase 16: Shell Integration + OSC Protocol** -- PowerShell Tee-Object insertion, bash tee insertion, OSC 133;S/P protocol, temp file management. Highest risk phase.

**Phase 17: Terminal Detection + Event Transport** -- OscScanner extension, ShellEvent variants, Block.pipeline_stages, main.rs event wiring.

**Phase 18: Pipeline UI + Storage** -- Multi-row pipeline blocks in renderer, pipe_stages DB table, schema migration, retention integration.

**Phase 19: MCP + Config + Polish** -- GlassPipeInspect tool, [pipes] config section, GlassContext update, integration testing.

### Research Flags

- **Phase 16 (Shell Integration):** Bash DEBUG trap reliability across bash versions and prompt frameworks needs testing. PowerShell PSReadLine Replace verified reliable.
- **Phase 18 (UI):** Expanded stage output rendering for long captures may need virtual scrolling.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | No new deps, all existing crates verified |
| Features | MEDIUM | Novel territory (no precedent), but mechanisms well-understood |
| Architecture | HIGH | Direct codebase analysis, all integration points mapped |
| Pitfalls | HIGH | Verified via official docs, multiple sources, codebase analysis |

---
*Research completed: 2026-03-05*
*Ready for roadmap: yes*
