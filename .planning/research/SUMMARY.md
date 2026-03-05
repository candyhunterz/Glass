# Project Research Summary

**Project:** Glass v1.2 -- Command-Level Undo
**Domain:** Filesystem snapshot and undo system for GPU-accelerated terminal emulator
**Researched:** 2026-03-05
**Confidence:** HIGH

## Executive Summary

Glass v1.2 adds command-level undo -- a capability no terminal emulator currently offers. The approach is a dual-mechanism system: a heuristic command parser identifies file targets for pre-exec snapshots of known destructive commands (rm, mv, sed -i, etc.), and a filesystem watcher (notify crate) records what actually changed during command execution. File contents are stored in a content-addressed blob store using BLAKE3 hashing with 2-char directory sharding, while SQLite tracks snapshot metadata in a separate database from the existing history DB. The architecture follows Glass's established patterns: event-driven coordination through the winit event loop, non-fatal degradation on failure, and blocking I/O on the main thread for small operations.

The recommended approach builds on four new dependencies (notify 8.2, notify-debouncer-full 0.7, blake3 1.8.3, shlex 1.3) added to the existing glass_snapshot stub crate, which becomes the sole owner of all undo logic. The crate is intentionally decoupled from glass_history -- they share only an i64 command_id, with the root binary coordinating both. All four research streams converge on the same five-phase build order: storage foundation first, then command parsing, then FS watching, then main-thread integration with the undo engine, and finally UI/CLI/MCP polish.

The primary risks are: (1) an inherent race between OSC 133;C emission and command execution that means pre-exec snapshots may miss fast destructive commands -- the FS watcher is the safety net, not the parser; (2) silent event buffer overflow on Windows (ReadDirectoryChangesW) during build-tool commands that generate thousands of file events; and (3) unbounded snapshot storage growth if pruning is not built from day one. All three are mitigable with the documented strategies, and none require architectural changes -- they are accounted for in the proposed design.

## Key Findings

### Recommended Stack

The existing Glass stack (wgpu, winit, alacritty_terminal, rusqlite, tokio, etc.) is unchanged. Four new workspace dependencies are needed, all high-confidence choices with no viable alternatives.

**New dependencies:**
- **notify 8.2 + notify-debouncer-full 0.7:** Cross-platform FS watcher -- the only serious option in Rust (62M+ downloads, used by rust-analyzer, zed, alacritty). Debouncer is critical for tracking file renames and deduplicating editor save cycles.
- **blake3 1.8.3:** Content-addressed hashing -- 5-10x faster than SHA-256, 256-bit output eliminates collision risk, built-in hex conversion. Standard choice for local CAS in Rust.
- **shlex 1.3.0:** POSIX shell tokenization for extracting file targets from command text. Lazy iterator avoids allocating full Vec when only the command name is needed.

**Deferred:** zstd (compression, add when pruning is mature), similar (text diffing, add for GlassFileDiff MCP tool).

**Total binary size impact:** ~330KB -- negligible against the existing ~80MB binary.

### Expected Features

**Must have (table stakes):**
- Automatic pre-command file snapshots (the value proposition)
- Single-keystroke undo (Ctrl+Shift+Z) and [undo] button on command blocks
- Visual confirmation showing which files were restored
- Conflict detection -- warn if file changed since the tracked command
- Storage pruning with configurable limits (do not fill drive silently)
- CLI undo (`glass undo <id>`) matching Glass's dual UI+CLI pattern
- Scope limited to file content (not process state, env vars, network effects)

**Should have (differentiators):**
- Content-addressed deduplication (makes aggressive snapshotting practical)
- Command-aware file targeting (snapshot only affected files, not entire CWD)
- FS watcher post-exec recording (ground truth of what changed)
- MCP tools (GlassUndo, GlassFileDiff) for AI assistant integration
- Honest limitation reporting (different confidence levels per command)

**Defer to v2+:**
- Blob compression (zstd), diff view before undo, per-file partial undo, undo/redo chain, file modification timeline queries, multi-command batch undo

### Architecture Approach

The system adds four new components to the glass_snapshot crate -- SnapshotStore (CAS blob storage + SQLite metadata), CommandParser (heuristic file target extraction), FsWatcher (notify-based command-scoped monitoring), and UndoEngine (restoration with conflict detection). These integrate into the existing event-driven architecture by hooking into Shell{CommandExecuted} and Shell{CommandFinished} events on the main thread. A separate snapshots.db lives alongside history.db in the .glass/ directory, avoiding migration risk and enabling independent pruning.

**Major components:**
1. **SnapshotStore** -- BLAKE3 content-addressed blob storage with SQLite metadata (snapshots, snapshot_files, fs_changes tables)
2. **CommandParser** -- Heuristic parser for top 15 destructive commands per shell, returns file targets + confidence level + watch_cwd flag
3. **FsWatcher (CommandWatcher)** -- Wraps notify crate for command-scoped recursive FS monitoring with noise filtering and event batching
4. **UndoEngine** -- Orchestrates file restoration with conflict detection, producing RestoreReport with restored/skipped/errored file lists

### Critical Pitfalls

1. **OSC 133;C race condition** -- Pre-exec snapshot may miss fast destructive commands because the command is already running when the event reaches the main thread. Mitigation: treat FS watcher as the primary mechanism; pre-exec snapshots are a bonus for known commands. Track and display undo confidence per command.

2. **Windows ReadDirectoryChangesW buffer overflow** -- Silent event loss during high-volume commands (cargo build, npm install). The entire buffer is discarded, not just overflow events. Mitigation: use large buffers, detect zero-byte returns, flag affected commands as "partial coverage," fall back to directory enumeration.

3. **Unbounded storage growth** -- Modified files (the common case) are all unique, so dedup helps less than expected. Build commands snapshot many large files. Mitigation: implement retention from day one (max age + max size), use filesystem blob storage (not SQLite BLOBs), skip reproducible directories (target/, node_modules/).

4. **TOCTOU in snapshot read-then-store** -- File may be modified between read and hash by the concurrently running command. Mitigation: single std::fs::read() call (not streaming), verify size/mtime before and after, accept pre-exec snapshots are more reliable than watcher-triggered ones.

5. **Command parser scope creep** -- Shell syntax is Turing-complete; trying to handle all cases leads to months of work. Mitigation: hard cap of 300 lines per shell, whitelist approach for top 15 commands, accept 60-70% coverage, let FS watcher handle the rest.

## Implications for Roadmap

Based on research, suggested phase structure:

### Phase 1: Content Store + DB Schema Foundation

**Rationale:** Everything else depends on having a place to store and retrieve file snapshots. Pure library code with zero integration dependencies -- fully testable in isolation.
**Delivers:** SnapshotStore with BLAKE3 CAS, 2-char directory sharding, SQLite schema (snapshots, snapshot_files, fs_changes), blob dedup, resolve_snapshot_db_path.
**Addresses:** Content-addressed deduplication, cross-platform file operations
**Avoids:** Unbounded storage growth (Pitfall 3) by designing pruning-ready schema; schema migration risk (Pitfall 9) by using separate DB
**Tech debt:** Must also fix command text extraction (currently stored as empty string) -- this is a blocker for Phase 2.

### Phase 2: Command Parser

**Rationale:** No dependencies on other new code. Pure functions, fully testable in isolation. Must exist before main-thread integration so pre-exec snapshots know which files to target.
**Delivers:** parse_command() function handling rm, mv, cp, sed -i, chmod, chown, redirect, git checkout for Bash/Zsh. ReadOnly detection for ls, cat, grep, etc. Path resolution (relative to absolute). PowerShell matchers for Remove-Item, Move-Item, etc.
**Uses:** shlex 1.3 for POSIX tokenization
**Avoids:** Command parser scope creep (Pitfall 5) -- whitelist approach, 300-line cap per shell

### Phase 3: FS Watcher Engine

**Rationale:** Depends on SnapshotStore for recording changes. Introduces the notify dependency. Must be designed to accommodate cross-platform differences even though only Windows is implemented initially.
**Delivers:** CommandWatcher with start/stop lifecycle, noise filtering (.git, target, node_modules), event deduplication, ignore pattern configuration
**Uses:** notify 8.2 + notify-debouncer-full 0.7
**Avoids:** Event flood overwhelming winit (Pitfall 6) via batched event collection in watcher thread; Windows buffer overflow (Pitfall 2) via detection and fallback; CWD-only scope limitation (Pitfall 11) via targeted watches for out-of-CWD paths identified by parser

### Phase 4: Main Thread Integration + Undo Engine

**Rationale:** Requires Phases 1-3 complete. This is the integration phase where existing crate boundaries get extended and the snapshot lifecycle connects to the command lifecycle.
**Delivers:** CommandExecuted handler (command text extraction, parse, pre-exec snapshot, watcher start). CommandFinished handler (stop watcher, record changes, link command_id). UndoEngine with conflict detection. Ctrl+Shift+Z keybinding. SnapshotSection config. WindowContext extensions.
**Addresses:** Automatic pre-command snapshots, single-keystroke undo, conflict detection, scope limitation to file content
**Avoids:** OSC 133;C race (Pitfall 1) via synchronous snapshot + watcher as primary mechanism; TOCTOU (Pitfall 4) via single-read strategy; Ctrl+Shift+Z conflicts (Pitfall 12) via prompt-state gating; symlink following (Pitfall 10) via lstat checks

### Phase 5: UI + CLI + MCP + Pruning

**Rationale:** All infrastructure must exist first. These are presentation and lifecycle management features built on top of the core engine.
**Delivers:** [undo] label on command blocks, `glass undo <id>` CLI subcommand, GlassUndo + GlassFileDiff MCP tools, auto-pruning on startup (max_age_days, max_storage_mb), undo result feedback (toast/status bar), undoable_epochs set for renderer
**Addresses:** Undo button on blocks, visual confirmation, CLI undo, MCP tools, storage pruning, honest limitation reporting
**Avoids:** Non-atomic directory restoration (Pitfall 8) via two-phase staging; metadata loss (Pitfall 13) via permission storage

### Phase Ordering Rationale

- **Dependency chain is strict:** Store -> Parser -> Watcher -> Integration -> UI. Each phase produces a component consumed by the next.
- **Isolation enables testing:** Phases 1-3 are pure library code with zero integration dependencies. Unit tests validate each component before the complex integration in Phase 4.
- **Risk front-loading:** The hardest architectural decisions (blob storage layout, DB schema, watcher event flow) are resolved in Phases 1 and 3. Phase 4 is integration of known-good components.
- **Tech debt first:** Command text extraction fix in Phase 1 unblocks everything downstream. Without it, the command parser has no input.

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 3 (FS Watcher):** Cross-platform behavioral differences are well-documented but the interaction between notify's debouncer, Windows buffer overflow detection, and the batched-event architecture needs careful design. The notify crate's internal buffer size configuration and overflow signaling behavior should be verified against source code.
- **Phase 4 (Integration):** The exact timing of command text extraction at CommandExecuted time (vs the current CommandFinished time) needs validation -- the terminal grid state at 133;C may differ from assumptions.

Phases with standard patterns (skip research-phase):
- **Phase 1 (Content Store):** Content-addressed storage is a well-established pattern (git, IPFS). BLAKE3 API is straightforward. SQLite schema is simple.
- **Phase 2 (Command Parser):** Pure string parsing with well-defined scope. shlex API is minimal.
- **Phase 5 (UI/CLI/MCP):** Extends existing patterns (block labels, clap subcommands, rmcp tools). No new architectural decisions.

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | All four dependencies are de facto standards with no alternatives. Version-locked, compatibility verified. |
| Features | MEDIUM | No direct precedent for command-level undo in terminals. Feature set synthesized from adjacent domains (VCS, backup tools, editor undo). Table stakes are clear; differentiator value is assumed. |
| Architecture | HIGH | Based on direct source code analysis of Glass v1.1 (8,473 LOC across 9 crates). All proposed patterns match existing conventions. |
| Pitfalls | HIGH | Windows FS watcher pitfalls backed by Microsoft docs and real-world reports. Race conditions are fundamental to the domain. Storage concerns validated by SQLite benchmarks. |

**Overall confidence:** HIGH -- the stack and architecture are well-grounded, the pitfalls are well-documented with clear mitigations, and the feature set is appropriately scoped for a first iteration.

### Gaps to Address

- **Command text extraction timing:** Currently happens at CommandFinished. Moving to CommandExecuted needs validation that the terminal grid reliably contains the command text at 133;C time. Test with multi-line commands and commands entered after scrollback review.
- **notify buffer size on Windows:** The default buffer size in notify's Windows backend needs verification. If it is too small (e.g., 4KB), it must be configurable or patched.
- **PowerShell command parsing:** shlex handles POSIX shells. PowerShell uses fundamentally different quoting (backtick escapes, different string interpolation). A separate 30-line tokenizer is recommended but not yet designed.
- **Large file handling threshold:** Architecture mentions skipping files >10MB and commands with >20 targets, but the exact thresholds need tuning based on real-world usage patterns.
- **Undo of commands with no pre-exec snapshot:** When only watcher data exists (no pre-state captured), undo cannot restore original content -- only report what changed. The UX for this "informational only" undo case needs design.

## Sources

### Primary (HIGH confidence)
- Glass v1.1 source code -- direct analysis of all 9 crates, event flow, DB patterns, keybinding conventions
- [notify crate v8.2 (docs.rs)](https://docs.rs/notify/8.2.0/notify/) -- cross-platform FS notification API
- [notify-rs GitHub](https://github.com/notify-rs/notify) -- platform backends, debouncer behavior
- [BLAKE3 crate (crates.io)](https://crates.io/crates/blake3) -- hashing API, SIMD acceleration, v1.8.3
- [ReadDirectoryChangesW (Microsoft)](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-readdirectorychangesw) -- buffer overflow behavior
- [SQLite Internal vs External BLOBs](https://sqlite.org/intern-v-extern-blob.html) -- 100KB threshold guidance

### Secondary (MEDIUM confidence)
- [Understanding ReadDirectoryChangesW (Jim Beveridge)](https://qualapps.blogspot.com/2010/05/understanding-readdirectorychangesw_19.html) -- race conditions, practical implementation
- [inotify(7) man page](https://man7.org/linux/man-pages/man7/inotify.7.html) -- Linux FS watching limitations
- [FSEvents Programming Guide (Apple)](https://developer.apple.com/library/archive/documentation/Darwin/Conceptual/FSEvents_ProgGuide/) -- macOS latency and coalescing
- Content-addressed storage patterns from git, IPFS, Bao
- [openSUSE/snapper](https://github.com/openSUSE/snapper) -- closest prior art for command-level system undo

### Tertiary (LOW confidence)
- shlex 1.3 / shell-words comparison -- both work, shlex chosen for lazy iteration
- PowerShell tokenization strategy -- hand-rolled, not yet validated

---
*Research completed: 2026-03-05*
*Ready for roadmap: yes*
