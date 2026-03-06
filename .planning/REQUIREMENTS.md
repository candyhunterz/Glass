# Requirements: Glass

**Defined:** 2026-03-05
**Core Value:** A terminal that looks and feels normal but passively watches, indexes, and snapshots everything -- surfacing intelligence only when you need it.

## v1.2 Requirements

Requirements for command-level undo milestone. Each maps to roadmap phases.

### Snapshot Engine

- [ ] **SNAP-01**: Glass automatically snapshots target files before a command executes, triggered by OSC 133;C
- [x] **SNAP-02**: File contents are stored in a content-addressed blob store using BLAKE3 hashing with deduplication
- [x] **SNAP-03**: Command text is parsed to identify file targets for pre-exec snapshot (rm, mv, sed -i, cp, chmod, git checkout, etc.)
- [x] **SNAP-04**: FS watcher monitors CWD during command execution and records all file modifications as ground truth
- [x] **SNAP-05**: Command text is extracted from the terminal grid at command start (fixes empty-string tech debt)
- [x] **SNAP-06**: Snapshot metadata is stored in a separate snapshots.db with command_id linking to history.db

### Undo Mechanics

- [x] **UNDO-01**: User can undo the most recent file-modifying command via Ctrl+Shift+Z
- [x] **UNDO-02**: Undo restores snapshotted file contents to their pre-command state
- [x] **UNDO-03**: Conflict detection warns if a file has been modified since the tracked command ran
- [x] **UNDO-04**: Each command displays its undo confidence level (pre-exec snapshot vs watcher-only)

### User Interface

- [ ] **UI-01**: File-modifying command blocks display an [undo] label
- [ ] **UI-02**: After undo, visual confirmation shows which files were restored, skipped, or errored
- [ ] **UI-03**: User can undo a specific command via `glass undo <command-id>` CLI

### Storage & Lifecycle

- [ ] **STOR-01**: Storage pruning enforces configurable max age and max size limits with automatic cleanup
- [x] **STOR-02**: `.glassignore` patterns exclude directories from snapshot tracking (node_modules, target, .git)
- [x] **STOR-03**: Snapshot configuration section in config.toml (enabled, max_count, max_size_mb, retention_days)

### AI Integration

- [ ] **MCP-01**: GlassUndo MCP tool allows AI assistants to trigger undo programmatically
- [ ] **MCP-02**: GlassFileDiff MCP tool allows AI assistants to inspect file diffs from commands

## Future Requirements

### Snapshot Enhancements

- **SNAP-F01**: Blob compression with zstd for storage efficiency
- **SNAP-F02**: Diff view before undo (preview what will change)
- **SNAP-F03**: Per-file partial undo from multi-file commands
- **SNAP-F04**: Undo/redo chain navigation
- **SNAP-F05**: File modification timeline queries ("what changed config.ts?")
- **SNAP-F06**: Multi-command batch undo

## Out of Scope

| Feature | Reason |
|---------|--------|
| Full directory tree snapshots | Storage explosion (node_modules = 500MB+), prohibitively slow |
| Process state undo | Impossible -- killed processes, env changes, network effects are irreversible |
| Undo for sudo/elevated commands | Security implications of silently writing to system paths |
| Continuous real-time backup | Glass is a terminal, not a backup tool; battery drain and disk I/O noise |
| VSS/APFS snapshot integration | Volume-level only, requires admin privileges, not portable |
| Binary file diff display | Binary diffs meaningless to humans; show metadata instead |
| Full shell command parser | Shell syntax is Turing-complete; heuristic whitelist approach instead |
| Automatic undo of failed commands | Failed commands may have partial effects the user wants to keep |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| SNAP-01 | Phase 13 | Pending |
| SNAP-02 | Phase 10 | Complete |
| SNAP-03 | Phase 11 | Complete |
| SNAP-04 | Phase 12 | Complete |
| SNAP-05 | Phase 10 | Complete |
| SNAP-06 | Phase 10 | Complete |
| UNDO-01 | Phase 13 | Complete |
| UNDO-02 | Phase 13 | Complete |
| UNDO-03 | Phase 13 | Complete |
| UNDO-04 | Phase 13 | Complete |
| UI-01 | Phase 14 | Pending |
| UI-02 | Phase 14 | Pending |
| UI-03 | Phase 14 | Pending |
| STOR-01 | Phase 14 | Pending |
| STOR-02 | Phase 12 | Complete |
| STOR-03 | Phase 13 | Complete |
| MCP-01 | Phase 14 | Pending |
| MCP-02 | Phase 14 | Pending |

**Coverage:**
- v1.2 requirements: 18 total
- Mapped to phases: 18
- Unmapped: 0

---
*Requirements defined: 2026-03-05*
*Last updated: 2026-03-05 after roadmap creation*
