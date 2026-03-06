# Roadmap: Glass

## Milestones

- [x] **v1.0 MVP** -- Phases 1-4 (shipped 2026-03-05)
- [x] **v1.1 Structured Scrollback + MCP Server** -- Phases 5-9 (shipped 2026-03-05)
- [ ] **v1.2 Command-Level Undo** -- Phases 10-14 (in progress)

## Phases

<details>
<summary>v1.0 MVP (Phases 1-4) -- SHIPPED 2026-03-05</summary>

- [x] Phase 1: Scaffold (3/3 plans) -- completed 2026-03-05
- [x] Phase 2: Terminal Core (3/3 plans) -- completed 2026-03-05
- [x] Phase 3: Shell Integration and Block UI (4/4 plans) -- completed 2026-03-05
- [x] Phase 4: Configuration and Performance (2/2 plans) -- completed 2026-03-05

</details>

<details>
<summary>v1.1 Structured Scrollback + MCP Server (Phases 5-9) -- SHIPPED 2026-03-05</summary>

- [x] Phase 5: History Database Foundation (2/2 plans) -- completed 2026-03-05
- [x] Phase 6: Output Capture + Writer Integration (4/4 plans) -- completed 2026-03-05
- [x] Phase 7: CLI Query Interface (2/2 plans) -- completed 2026-03-05
- [x] Phase 8: Search Overlay (2/2 plans) -- completed 2026-03-05
- [x] Phase 9: MCP Server (2/2 plans) -- completed 2026-03-05

</details>

### v1.2 Command-Level Undo (In Progress)

**Milestone Goal:** Automatic filesystem snapshots per command with one-keystroke revert via Ctrl+Shift+Z.

- [x] **Phase 10: Content Store + DB Schema** - Content-addressed blob storage and snapshot database foundation (completed 2026-03-05)
- [x] **Phase 11: Command Parser** - Heuristic command text parsing to identify file targets for pre-exec snapshot (completed 2026-03-05)
- [x] **Phase 12: FS Watcher Engine** - Filesystem monitoring during command execution with ignore patterns (completed 2026-03-06)
- [x] **Phase 13: Integration + Undo Engine** - Main-thread snapshot lifecycle and file restoration with conflict detection (completed 2026-03-06)
- [ ] **Phase 14: UI + CLI + MCP + Pruning** - Undo button, CLI command, MCP tools, and storage lifecycle management

## Phase Details

### Phase 10: Content Store + DB Schema
**Goal**: Files can be stored and retrieved by content hash, with snapshot metadata tracked in a dedicated database
**Depends on**: Phase 9 (v1.1 complete)
**Requirements**: SNAP-02, SNAP-05, SNAP-06
**Success Criteria** (what must be TRUE):
  1. File contents written to the blob store are deduplicated -- storing the same file twice produces one blob on disk
  2. Snapshot metadata (command_id, timestamp, file paths, hashes) persists in snapshots.db and survives process restart
  3. Command text is extracted from the terminal grid at command start time, replacing the empty-string tech debt
**Plans**: 2 plans

Plans:
- [ ] 10-01-PLAN.md — Build glass_snapshot crate: BlobStore (BLAKE3 CAS) + SnapshotDb (SQLite metadata) + SnapshotStore coordinator
- [ ] 10-02-PLAN.md — Move command text extraction to CommandExecuted time + wire SnapshotStore into main binary

### Phase 11: Command Parser
**Goal**: Glass can identify which files a command will modify before it runs
**Depends on**: Phase 10
**Requirements**: SNAP-03
**Success Criteria** (what must be TRUE):
  1. Given a command like `rm foo.txt bar.txt`, the parser returns the correct file paths as targets
  2. Known destructive commands (rm, mv, sed -i, cp, chmod, git checkout) are recognized with their file arguments extracted
  3. Read-only commands (ls, cat, grep) are classified as non-modifying and produce no snapshot targets
  4. Relative paths in command arguments are resolved to absolute paths against the working directory
**Plans**: 2 plans

Plans:
- [ ] 11-01-PLAN.md — POSIX command parser: shlex tokenization, whitelist dispatch, per-command extractors, redirect detection, path resolution + TDD tests
- [ ] 11-02-PLAN.md — PowerShell command parser: cmdlet detection, named parameter extraction, aliases + TDD tests

### Phase 12: FS Watcher Engine
**Goal**: Glass records all file modifications that occur during a command's execution as ground truth
**Depends on**: Phase 10
**Requirements**: SNAP-04, STOR-02
**Success Criteria** (what must be TRUE):
  1. Starting a command triggers filesystem monitoring on the working directory, and stopping the command stops monitoring
  2. File create, modify, rename, and delete events during command execution are captured and recorded
  3. Noise directories (.git, node_modules, target) and user-defined .glassignore patterns are excluded from monitoring
**Plans**: 2 plans

Plans:
- [ ] 12-01-PLAN.md — IgnoreRules (.glassignore pattern matching) + FsWatcher (notify-based filesystem monitoring) with TDD tests
- [ ] 12-02-PLAN.md — Wire FsWatcher into main.rs CommandExecuted/CommandFinished lifecycle

### Phase 13: Integration + Undo Engine
**Goal**: Users can undo the most recent file-modifying command and have their files restored to pre-command state
**Depends on**: Phase 11, Phase 12
**Requirements**: SNAP-01, UNDO-01, UNDO-02, UNDO-03, UNDO-04, STOR-03
**Success Criteria** (what must be TRUE):
  1. When a command runs, Glass automatically snapshots target files before execution (triggered by OSC 133;C)
  2. Pressing Ctrl+Shift+Z restores files to their pre-command state for the most recent file-modifying command
  3. If a file has been modified since the undone command ran, Glass warns about the conflict before overwriting
  4. Each file-modifying command displays its undo confidence level (full pre-exec snapshot vs watcher-only recording)
  5. Snapshot behavior is configurable via config.toml (enabled, max_count, max_size_mb, retention_days)
**Plans**: 4 plans

Plans:
- [ ] 13-01-PLAN.md — Config extension (SnapshotSection), undo types (FileOutcome, UndoResult), DB query methods
- [ ] 13-02-PLAN.md — UndoEngine with TDD: file restoration, conflict detection, per-file outcomes
- [ ] 13-03-PLAN.md — Wire pre-exec snapshots into CommandExecuted + Ctrl+Shift+Z keybinding for undo
- [ ] 13-04-PLAN.md — Gap closure: confidence display in undo output + config gating of pre-exec snapshots

### Phase 14: UI + CLI + MCP + Pruning
**Goal**: Undo is discoverable through the UI, accessible via CLI and MCP, and storage is managed automatically
**Depends on**: Phase 13
**Requirements**: UI-01, UI-02, UI-03, STOR-01, MCP-01, MCP-02
**Success Criteria** (what must be TRUE):
  1. File-modifying command blocks display an [undo] label that the user can see
  2. After undo completes, visual feedback shows which files were restored, skipped, or errored
  3. User can undo a specific command by running `glass undo <command-id>` from the CLI
  4. AI assistants can trigger undo and inspect file diffs through GlassUndo and GlassFileDiff MCP tools
  5. Snapshot storage is automatically pruned on startup based on configured max age and max size limits
**Plans**: 3 plans

Plans:
- [ ] 14-01-PLAN.md — Storage pruning module + UndoEngine undo_command refactor (shared foundation)
- [ ] 14-02-PLAN.md — [undo] block label, CLI undo subcommand, visual feedback, startup pruning wiring
- [ ] 14-03-PLAN.md — GlassUndo and GlassFileDiff MCP tools for AI assistant integration

## Progress

**Execution Order:**
Phases execute in numeric order: 10 -> 11 -> 12 -> 13 -> 14
(Note: Phases 11 and 12 can execute in parallel -- both depend only on Phase 10)

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1. Scaffold | v1.0 | 3/3 | Complete | 2026-03-05 |
| 2. Terminal Core | v1.0 | 3/3 | Complete | 2026-03-05 |
| 3. Shell Integration and Block UI | v1.0 | 4/4 | Complete | 2026-03-05 |
| 4. Configuration and Performance | v1.0 | 2/2 | Complete | 2026-03-05 |
| 5. History Database Foundation | v1.1 | 2/2 | Complete | 2026-03-05 |
| 6. Output Capture + Writer Integration | v1.1 | 4/4 | Complete | 2026-03-05 |
| 7. CLI Query Interface | v1.1 | 2/2 | Complete | 2026-03-05 |
| 8. Search Overlay | v1.1 | 2/2 | Complete | 2026-03-05 |
| 9. MCP Server | v1.1 | 2/2 | Complete | 2026-03-05 |
| 10. Content Store + DB Schema | 2/2 | Complete    | 2026-03-05 | - |
| 11. Command Parser | 2/2 | Complete    | 2026-03-05 | - |
| 12. FS Watcher Engine | 2/2 | Complete    | 2026-03-06 | - |
| 13. Integration + Undo Engine | 4/4 | Complete    | 2026-03-06 | - |
| 14. UI + CLI + MCP + Pruning | v1.2 | 0/3 | Not started | - |
