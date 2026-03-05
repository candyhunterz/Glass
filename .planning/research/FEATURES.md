# Feature Landscape

**Domain:** Command-level undo with filesystem snapshots for terminal emulator
**Researched:** 2026-03-05
**Milestone:** v1.2 Command-Level Undo
**Confidence:** MEDIUM -- no direct precedent for command-level undo in terminal emulators; recommendations synthesized from adjacent domains (version control, filesystem snapshots, editor undo, backup tools)

---

## Table Stakes

If Glass advertises "command-level undo," users will expect these features to work reliably or will consider the feature broken.

| Feature | Why Expected | Complexity | Depends On |
|---------|--------------|------------|------------|
| Automatic pre-command file snapshots | The whole value proposition -- if users must opt in, they will not | HIGH | OSC 133;C pre-exec timing (exists), command text parsing (new) |
| Single-keystroke undo (Ctrl+Shift+Z) | Every undo UX uses a single gesture | LOW | winit input handling (exists) |
| Undo button on command blocks | Block UI already has exit code/duration badges -- [undo] is the natural extension | LOW | Block decoration rendering (exists) |
| Visual confirmation of undo result | Users must see what was reverted -- "undo succeeded" with no details is anxiety-inducing | MEDIUM | Block UI / toast rendering |
| Safe restore with conflict detection | Must not silently destroy post-command changes -- warn if file changed since command | MEDIUM | Content-addressed blob store (new) |
| Storage pruning and limits | Snapshots consume disk -- must not fill drive silently | MEDIUM | Existing retention policy pattern in glass_history |
| CLI undo (`glass undo <id>`) | Glass pattern: features accessible via both UI and CLI | LOW | Clap routing (exists) |
| Scope limited to file content | Users expect file content to revert, NOT process state, env vars, or network effects | LOW | Design constraint, communicated in UI |
| Cross-platform file operations | Windows + Linux + macOS file copy/restore | MEDIUM | std::fs + notify (cross-platform) |

---

## Differentiators

No terminal emulator offers command-level undo. The feature itself is a differentiator.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Content-addressed deduplication | BLAKE3 hash file contents; store blob once, reference by hash. Makes aggressive snapshotting practical. | MEDIUM | ~50 LOC for CAS, massive storage savings |
| Command-aware file targeting | Parse command text to identify affected files. Snapshot only those -- far more efficient than watching entire CWD. | HIGH | Heuristic for rm, mv, cp, sed, git checkout, etc. |
| FS watcher post-exec recording | Record what files actually changed during command execution. Ground truth. | HIGH | notify crate, CWD-scoped watching |
| MCP tools (GlassUndo, GlassFileDiff) | AI assistants can inspect changes and trigger undo programmatically. Unique to Glass. | MEDIUM | Extend existing glass_mcp |
| Per-file partial undo | Undo specific files from multi-file command, not all-or-nothing. Architecturally natural with per-file blobs. | MEDIUM | CAS blob store |
| Honest limitation reporting | Tell user when undo is incomplete (script ran, targets unknown). Differentiate pre_exec vs watcher-only. | LOW | snapshot_type field |
| File modification timeline | History DB tracks which files each command touched. Valuable for "what changed my config?" | MEDIUM | glass_history DB schema extension |

---

## Anti-Features

Features to explicitly NOT build.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| Full directory tree snapshots | Storage explosion (node_modules = 500MB+). Prohibitively slow. | Target specific files via command parsing + FS watcher. Add `.glassignore` for exclusions. |
| Process state undo | Impossible. Killed processes, env changes, network effects are irreversible. | Scope to filesystem changes only. Document honestly. |
| Undo for sudo/elevated commands | Security implications of silently writing to system paths. | Record change but require explicit `glass undo --sudo` with elevation prompt. |
| Continuous real-time backup | Glass is a terminal, not a backup tool. Battery drain, disk I/O noise. | Snapshot only at command boundaries (pre-exec to post-exec). |
| VSS/APFS snapshot integration | Volume-level only (not per-file), requires admin privileges, not portable. | User-space file copying with content-addressed dedup. |
| Binary file diff display | Binary diffs meaningless to humans. | Show metadata (size change, hash change) for binary files. Diff only text. |
| Full shell command parser | Shell syntax is Turing-complete. Variable expansion, subshells, aliases make perfect parsing impossible. | Heuristic parser for common destructive patterns. Accept fallback to FS watcher. |
| Undo across sessions without warning | File state may have diverged significantly over hours/days. | Allow via CLI with mandatory diff display and staleness warning. |
| Automatic undo of failed commands | "Failed" commands may have partial effects the user wants to keep. | Offer undo button, never auto-trigger. |
| Multi-command batch undo | Complex state machine, confusing UX. | Undo one command at a time. Sequential undo for multiple. |

---

## Feature Dependencies

```
[OSC 133;C Pre-Exec] (EXISTING) ──────> [Command Text Extraction]
                                              │
                                              v
                                    [shlex Argument Parsing]
                                              │
                                              v
[OSC 7 CWD Tracking] (EXISTING) ──> [File Target Identification]
        │                                     │
        v                                     v
[notify FS Watcher Setup] ──────> [Pre-Exec File Snapshot]
        │                                     │
        v                                     v
[Post-Exec Change Recording] ──> [Content-Addressed Blob Store (BLAKE3)]
                                              │
                                              v
                                    [Snapshot Metadata in SQLite]
                                              │
                   ┌──────────────────────────┼──────────────────────┐
                   v                          v                      v
            [Ctrl+Shift+Z]           [Block [undo] Button]   [`glass undo`]
                   │                          │                      │
                   └──────────────────────────┼──────────────────────┘
                                              v
                                    [File Restoration Engine]
                                              │
                                              v
                                    [UI Feedback (toast/overlay)]
                                              │
                                              v
                                    [Storage Pruning (ref-counted blobs)]
```

### Critical Tech Debt Blocker

**Command text is currently stored as empty string.** The v1.1 history DB records commands but the actual command text extraction from the terminal grid was deferred. For v1.2 undo, command text is essential for the file target parser. Must be fixed first.

---

## MVP Recommendation

### Phase 1: Foundation (Content Store + DB Schema)
1. Content-addressed blob store (BLAKE3 hashing, dedup, file CAS)
2. Snapshot metadata tables in history DB (migration v1 -> v2)
3. Command text extraction fix (tech debt resolution)

### Phase 2: Snapshot Engine
1. shlex command text parsing + file target identification
2. Pre-exec snapshot engine (on OSC 133;C, parse command, snapshot targets)
3. FS watcher integration (notify crate watching CWD, record changes)

### Phase 3: Undo + UI
1. File restoration engine with conflict detection
2. Ctrl+Shift+Z keystroke handler
3. [undo] button on command blocks
4. Visual feedback (which files restored)
5. `glass undo <id>` CLI

### Phase 4: Integration + Polish
1. Storage pruning (age + size limits, ref-counted blob cleanup)
2. GlassUndo + GlassFileDiff MCP tools
3. `.glassignore` exclusions

**Defer:** Compression (zstd), diff view before undo, per-file partial undo, undo/redo chain, file modification timeline queries.

---

## Sources

- Glass PROJECT.md -- existing OSC 133/7 hooks, command lifecycle, block UI
- [notify-rs/notify](https://github.com/notify-rs/notify) -- cross-platform FS watcher, 62M+ downloads
- [openSUSE/snapper](https://github.com/openSUSE/snapper) -- closest prior art for "undo system modifications"
- Content-addressed storage patterns from git, IPFS, Bao

---
*Feature research for: Glass v1.2 Command-Level Undo*
*Researched: 2026-03-05*
