# Domain Pitfalls: v1.2 Command-Level Undo with Filesystem Snapshots

**Domain:** Adding filesystem monitoring, snapshot storage, and undo capabilities to an existing Rust GPU-accelerated terminal emulator
**Researched:** 2026-03-05
**Applies to:** Glass v1.2 milestone

---

## Critical Pitfalls

Mistakes that cause rewrites, data loss, or fundamental architecture breakage.

### Pitfall 1: Race Between OSC 133;C and Pre-Exec Snapshot

**What goes wrong:** The snapshot must complete before the command starts modifying files, but OSC 133;C fires when the shell hands control to the command -- not before it. On fast commands (`rm file.txt`, `mv a b`), the file may already be deleted or moved by the time the snapshot logic receives the event, parses file targets from command text, and copies file contents.

**Why it happens:** The PTY reader thread receives OSC 133;C and sends a `ShellEvent::CommandExecuted` via the winit `EventLoopProxy`. The winit event loop processes this on the main thread asynchronously. Meanwhile, the command is already running in the PTY. The current `user_event` handler for `ShellEvent::CommandExecuted` only records `command_started_wall` -- there is no mechanism to pause command execution while snapshotting.

**Consequences:** Missing snapshots for the fastest destructive commands -- the exact commands where undo is most valuable (`rm`, `mv`, overwrite redirects). Undo silently fails or restores nothing. User trusts the undo button but it does not work when it matters most.

**Prevention:**
- Accept this as a fundamental, unsolvable limitation of the OSC 133 protocol and document it honestly. There is no way to pause the PTY between 133;C emission and command execution.
- Design the FS watcher as the primary detection mechanism, not the pre-exec snapshot. The watcher should already be running on the CWD before 133;C fires, so it captures file deletions/modifications as they happen.
- For the pre-exec snapshot path, execute it synchronously in the `ShellEvent::CommandExecuted` handler -- parse command text and read files before processing any other events. This minimizes but does not eliminate the race window.
- Track whether pre-exec snapshot succeeded per command. Display different undo confidence in the UI: "Full undo" (snapshot exists) vs "Partial undo" (watcher-only, files were already gone when snapshot attempted).

**Detection:** Test with `rm largefile.txt` -- the file should be snapshotted before deletion. Test with `rm tinyfile.txt` -- accept that this may fail. Measure the time between 133;C emission and snapshot completion.

**Phase relevance:** Must be addressed in the very first phase (FS watcher + snapshot engine design). The entire undo reliability depends on getting this architecture right.

---

### Pitfall 2: FS Watcher Buffer Overflow Silently Drops Events on Windows

**What goes wrong:** On Windows, `ReadDirectoryChangesW` uses a fixed-size kernel buffer. When a command generates rapid filesystem changes (e.g., `cargo build`, `npm install`, `git clone`), the buffer overflows. The API returns `TRUE` but sets `lpBytesReturned` to zero, meaning ALL buffered events are silently discarded -- not just the overflow, but the entire buffer contents.

**Why it happens:** The buffer cannot be resized after the directory handle is created. Network drives have a hard 64KB cap. Build tools and package managers generate thousands of file operations in milliseconds. The `notify` crate wraps this API but cannot work around the kernel-level limitation.

**Consequences:** The watcher reports zero modifications for a command that created/modified hundreds of files. Undo either does nothing (data loss for user) or produces an incomplete revert leaving the filesystem in a state worse than either before or after (inconsistent state). The failure is completely silent -- no error, no warning, just missing events.

**Prevention:**
- Use a large buffer (32KB-64KB) in the `ReadDirectoryChangesW` configuration. Verify the `notify` crate's Windows backend default buffer size and increase if needed.
- Detect zero-byte returns as buffer overflow events and flag the command as "undo coverage incomplete" in the snapshot metadata.
- After buffer overflow detection, fall back to directory enumeration: compare current directory state against the pre-exec snapshot to reconstruct what changed. This is expensive but correct.
- For commands identified as build tools by the command parser (`cargo`, `npm`, `make`, `msbuild`), proactively set undo coverage to "partial" rather than discovering it after the fact.
- Consider double-buffering: issue two overlapping `ReadDirectoryChangesW` calls so one is always pending while the other is being processed.

**Detection:** Stress test with `cargo build` on a project with 500+ source files. Count watcher events vs actual filesystem changes. If the counts diverge, buffer overflow is occurring.

**Phase relevance:** FS watcher implementation phase. The overflow detection and fallback strategy must be designed upfront, not bolted on later.

---

### Pitfall 3: Snapshot Storage Grows Unbounded and Kills Performance

**What goes wrong:** Content-addressed dedup helps with identical files, but modified files (the common case for snapshots) are all unique. A user running build commands or editing files accumulates gigabytes of snapshot data in days. SQLite performance degrades with large BLOBs, and the database file becomes unwieldy for backup/deletion.

**Why it happens:** Each command potentially snapshots multiple files. Binary files (compiled outputs, images, `node_modules` contents) are large and rarely deduplicate across snapshots. SQLite stores BLOBs inline in its B-tree pages, causing page fragmentation for BLOBs >page_size. Per SQLite documentation, BLOBs >100KB are faster to read from separate files than from SQLite.

**Consequences:** Disk space exhaustion. Database queries slow down because metadata queries must scan past BLOB pages. Terminal startup increases as the DB grows. Users lose trust in the tool and disable snapshots entirely.

**Prevention:**
- Store snapshot BLOBs in a separate location from snapshot metadata:
  - Option A: Separate SQLite database (`snapshots.db`) with just hash + content, keeping metadata in the history DB. Pruning is `DELETE` + `VACUUM`.
  - Option B: Filesystem content-addressed store (`~/.glass/blobs/ab/cd1234...`) for files >100KB, with only the hash stored in SQLite. SQLite handles <100KB blobs well; larger ones go to filesystem. This is the recommended approach based on SQLite's own benchmark data.
- Implement retention from day one: prune snapshots older than N days, cap total snapshot storage at configurable size (e.g., 500MB default). The existing history DB has `prune()` that is never auto-triggered -- do not repeat this tech debt.
- Use BLAKE3 for hashing (5x faster than SHA-256 on modern CPUs, same 32-byte output). The `blake3` crate is well-maintained and supports SIMD acceleration.
- Skip snapshotting compiled output directories (`target/`, `node_modules/`, `build/`, `.git/objects/`). These are reproducible and waste storage.
- Place BLOB columns last in table definitions, or better, in a separate table. SQLite must scan through BLOBs to access columns after them.

**Detection:** Monitor `~/.glass/` directory size growth over a week of daily-driver usage. Alert if growth exceeds 100MB/day. CI test: insert 1000 snapshots of 50KB files, verify metadata query latency stays under 50ms.

**Phase relevance:** Storage engine design phase. The blob storage strategy (SQLite vs filesystem, single vs separate DB) and retention policy must be decided before implementing the snapshot engine.

---

### Pitfall 4: TOCTOU Race in Snapshot Read-Then-Store

**What goes wrong:** The snapshot engine reads a file's content to hash and store it, but between the read and the store (or between checking "does this hash already exist?" and skipping the read), the file is modified by the concurrently running command. The stored snapshot contains partial content -- half old data, half new data -- a state the file never actually had.

**Why it happens:** File I/O is not atomic at the application level. The command runs in the PTY concurrently. There is no filesystem-level snapshot mechanism being used (no VSS, no btrfs snapshots, no APFS clonefile) -- just userspace `std::fs::read`.

**Consequences:** Corrupted snapshots. Undo restores a file to a state it never actually existed in. Worse than not having undo, because the user believes the restoration is correct and may not verify.

**Prevention:**
- Read the entire file into memory in one `std::fs::read()` call before hashing. Do not stream-hash then re-read for storage. One read, one hash, one store.
- For large files, accept that the read itself is not atomic. The OS may give you data from mid-write if another process is writing. Mitigate by recording the file size and mtime before and after the read -- if they differ, the snapshot is suspect.
- The pre-exec snapshot (taken at OSC 133;C before the command modifies files) is inherently more reliable than watcher-triggered snapshots (taken while the command is running). This is why both mechanisms are needed.
- On undo, verify the file's current content hash matches the "after" state recorded by the watcher before reverting. If it does not match (file modified since the tracked command ran), warn the user and require confirmation.

**Detection:** Unit test: spawn a thread writing 10MB to a file in a loop while another thread snapshots it repeatedly. Verify each snapshot is internally consistent (e.g., if writing sequential numbers, verify no gaps or overlaps).

**Phase relevance:** Snapshot engine implementation phase.

---

### Pitfall 5: Command Text Parsing Scope Creep

**What goes wrong:** Attempting to parse arbitrary shell command text to extract file targets leads to an ever-growing parser that never handles all cases. Shell syntax is Turing-complete. Variable expansion (`$var`), globs (`*.txt`), command substitution (`$(find ...)`), heredocs, aliases, functions, pipelines, and shell-specific syntax (PowerShell `-Path` vs Bash positional args) make reliable general-purpose parsing impossible.

**Why it happens:** The temptation: `rm foo.txt` -> snapshot `foo.txt`, `cp a b` -> snapshot `b`, `mv x y` -> snapshot both. Then reality: `rm $(find . -name "*.tmp")`, `cat file | tee output`, `cmd1 && cmd2`, `for f in *.log; do rm "$f"; done`, PowerShell's `Remove-Item -Path $items -Recurse -Force`.

**Consequences:** Months spent on an increasingly complex parser that still misses cases. False sense of security: parser finds some targets, misses others, undo partially reverts. Different shell languages (PowerShell, Bash, Zsh, Fish) each need entirely different parsers.

**Prevention:**
- Keep the command parser deliberately simple and limited. Handle only the top 10-15 most common destructive commands with literal arguments:
  - Bash/Zsh: `rm`, `mv`, `cp`, `chmod`, `chown`, `sed -i`, `truncate`, `> file` (redirect)
  - PowerShell: `Remove-Item`, `Move-Item`, `Copy-Item`, `Set-Content`, `Clear-Content`
- The parser's job is to provide "bonus" pre-exec snapshots for obvious cases. The FS watcher is the primary detection mechanism.
- Use a whitelist approach (known commands with known arg patterns) rather than general shell parsing. If you cannot identify file targets with high confidence, skip pre-exec snapshot and rely on the watcher.
- Never claim 100% coverage. The UI should clearly indicate undo confidence level per command.
- PowerShell and Bash need separate matchers. Do not try to unify parsing -- the syntaxes are fundamentally different (named parameters vs positional arguments).
- Set a hard rule: the parser code must not exceed ~300 lines per shell. If it does, you are over-engineering it.

**Detection:** Maintain a test suite of 50+ command strings across both shells. Track hit rate (% of commands where file targets are correctly identified). Accept diminishing returns past ~60-70% coverage of common destructive patterns.

**Phase relevance:** Command parsing phase. Scope the parser tightly at design time. Resist scope creep during implementation.

---

## Moderate Pitfalls

### Pitfall 6: FS Watcher Event Flood Overwhelming the Winit Event Loop

**What goes wrong:** The existing architecture routes all cross-thread communication through `EventLoopProxy<AppEvent>` (PTY events, shell events, git status, command output). Adding per-file FS watcher events to this same channel during a `cargo build` (thousands of file events per second) starves the main thread -- it spends all time processing watcher events instead of rendering or handling keyboard input.

**Why it happens:** The winit event loop is single-threaded and processes `user_event()` calls sequentially. The current event types (TerminalDirty, Shell, CommandOutput, GitInfo) are low-frequency. FS watcher events during a build are high-frequency.

**Prevention:**
- Do NOT route individual FS events through the winit event loop. The watcher thread should accumulate events in its own thread-local storage (`Arc<Mutex<Vec<FsChange>>>` or a lock-free queue).
- The main thread queries the accumulated changes only at two points: (1) when a command finishes (OSC 133;D), to build the complete modification record, and (2) when the user presses Ctrl+Shift+Z, to show what would be undone.
- If a summary notification is needed (e.g., to update a "files changed" counter in the UI), send at most one `AppEvent::FsActivity` per 200ms, not per file.
- Consider a dedicated snapshot coordinator thread that receives watcher events and manages the snapshot store, communicating with the main thread only via high-level summaries.

**Phase relevance:** FS watcher integration phase. Architecture decision about event flow between watcher, snapshot engine, and UI.

---

### Pitfall 7: Cross-Platform FS Watcher Behavioral Differences

**What goes wrong:** The three platform FS notification APIs have fundamentally different semantics that the `notify` crate abstracts but cannot fully hide:

| Behavior | Windows (ReadDirectoryChangesW) | Linux (inotify) | macOS (FSEvents) |
|----------|------|-------|-------|
| Granularity | Per-file events | Per-file events | Per-directory events (coalesced) |
| Recursive watching | Native, single handle | Must add watch per subdirectory manually | Native, single stream |
| Event latency | Near-realtime (~ms) | Near-realtime (~ms) | Configurable, default ~1s coalescing |
| Rename tracking | Paired events with cookie | Paired events with cookie | No rename tracking (reports "modified") |
| New subdirectory race | None (recursive is atomic) | Race between mkdir and adding watch -- events in new dir can be missed | None (recursive is atomic) |
| Watch limits | Per-handle, generous | System-wide limit (default ~8K-128K watches) | Per-process, generous |
| Buffer overflow | Silent (zero-byte return) | Queue overflow (IN_Q_OVERFLOW event) | No overflow (kernel coalesces) |

**Consequences:** Code working perfectly on Windows fails on macOS (events arrive 1s late, rename info lost) or Linux (watch limit exhaustion on large monorepos, missed events from race in new directories). Tests pass on CI (one platform), fail in production (another platform).

**Prevention:**
- Use the `notify` crate (v7.x) which abstracts these differences. But understand the abstraction is leaky -- different platforms will produce different event sequences for the same filesystem operations.
- On macOS, set FSEvents latency to 0 or 10ms (not the default ~1s). Accept higher CPU usage for responsiveness. Use `kqueue` as an alternative for single-file watches.
- On Linux, handle inotify watch limit exhaustion gracefully: catch `ENOSPC`, fall back to polling for that subtree, and warn the user. Consider `fanotify` (Linux 5.1+) for true recursive watching without per-directory watches.
- Since macOS and Linux are "Future" scope in the project, design the watcher abstraction trait now but only implement Windows first. Do not design data structures or event types that encode Windows-specific assumptions (e.g., don't assume rename events always come in pairs).
- Test on all target platforms. The behavioral differences are not edge cases.

**Phase relevance:** FS watcher design phase. The trait/interface must accommodate platform differences even if only Windows is implemented initially.

---

### Pitfall 8: Non-Atomic Undo of Directory Operations

**What goes wrong:** Undoing `rm -rf directory/` requires recreating the directory structure and restoring all files within it. If this fails partway through (disk full, permission denied on one file, path too long, file locked by another process), the filesystem is left in a partially restored state -- worse than either the "before" or "after" state.

**Why it happens:** There is no transactional filesystem API on any mainstream OS. Each file restoration is an independent write. Errors accumulate and interact. A partially restored directory may confuse tools that expected either the full directory or no directory.

**Prevention:**
- Implement undo as a two-phase operation where possible: (1) create all restored files in a temporary staging directory (`~/.glass/tmp/undo-{id}/`), (2) move them into place. If step 1 fails, clean up the staging dir and report failure without touching the target.
- For scattered file modifications (not a single directory), restore files one at a time but track progress. On any failure, offer the user three options: continue with remaining files, undo the partial restoration, or leave as-is.
- Verify available disk space before starting undo. Snapshotted content requires at least as much space as the original files.
- Report partial undo results clearly: "Restored 47/50 files. 3 files failed: [list with paths and error reasons]."
- Never silently succeed on partial undo. The user must know if restoration was incomplete.

**Phase relevance:** Undo execution phase. Error handling and partial-failure UX design.

---

### Pitfall 9: Schema Migration Breaking Existing History Database

**What goes wrong:** v1.2 snapshot features require new tables and columns in the SQLite database. The existing database is at schema version 1 (added in v1.1 for the output column). A botched migration corrupts command history, fails on databases created by v1.1, or creates forward-compatibility issues where v1.1 binaries cannot open v1.2 databases.

**Why it happens:** The existing migration system uses `PRAGMA user_version` with a linear version check. Adding snapshot tables (file_snapshots, snapshot_blobs, command_file_changes) and foreign keys to an existing database with real user data requires careful transaction handling. Particularly, FTS5 virtual tables cannot participate in normal transaction rollback.

**Prevention:**
- **Recommended: Use a separate SQLite database file for snapshot data.** Store snapshot metadata and blobs in `~/.glass/{project}/snapshots.db`, keeping command history in the existing `history.db`. Benefits:
  - Isolation: snapshot DB corruption does not affect history.
  - Independent retention: delete the entire snapshot DB to reclaim space without touching history.
  - No migration risk to existing data.
  - Simpler schema: no foreign keys spanning databases.
- If using the same database: wrap all schema changes in a single transaction. Test migration from v1 -> v2 with databases containing 10K+ records. Never drop or rename existing columns/tables. Verify that v1.1 binaries opening a v2 database gracefully ignore unknown tables (SQLite does this naturally).
- Link command records to snapshots via `command_id` (the integer primary key), not by timestamp or command text.

**Phase relevance:** Storage engine design phase. Database architecture decision must be made before any snapshot code is written.

---

### Pitfall 10: Symlink and Junction Point Following During Snapshot/Restore

**What goes wrong:** On Windows, directory junctions and symlinks can point outside the CWD to system directories, other drives, or network shares. If the snapshot engine follows symlinks:
1. Snapshots system files (wasting space, reading sensitive data).
2. Follows circular symlinks and loops indefinitely.
3. Triggers network I/O for remote targets, causing multi-second hangs.
4. On restore, recreating symlinks vs recreating target content is ambiguous.

**Why it happens:** `std::fs::read()` and `std::fs::read_dir()` follow symlinks by default. Rust's own stdlib had a TOCTOU vulnerability in `remove_dir_all` (CVE-2022-21658) from this exact class of bug -- checking if something is a symlink then operating on it, with a race window where an attacker swaps a directory for a symlink.

**Prevention:**
- Use `std::fs::symlink_metadata()` (lstat equivalent) instead of `std::fs::metadata()` to detect symlinks before reading content.
- Never follow symlinks during snapshot operations. Store the symlink target path as metadata, not the pointed-to content.
- Set a maximum directory depth for recursive operations (e.g., 10 levels).
- Set a maximum total snapshot size per command (e.g., 50MB) to bound resource usage regardless of symlink resolution.
- On restore, recreate the symlink itself (pointing to the same target), not the content of the target.
- Exclude well-known junction points on Windows: `AppData\Local\Application Data` (circular junction in user profiles).

**Phase relevance:** Snapshot engine implementation phase.

---

### Pitfall 11: Watcher Scope -- CWD-Only Misses Cross-Directory Modifications

**What goes wrong:** Watching only the CWD misses commands that modify files outside it: `rm /tmp/scratch.txt`, `cp file.txt ~/backup/`, `mv report.pdf ../archive/`. The watcher sees nothing, the undo system has no record, the user believes the operation is covered.

**Why it happens:** Recursive directory watching is scoped to a single root. Watching `/` or `C:\` generates millions of irrelevant events, exceeds watch limits, and burns CPU. There is no practical "watch everything" solution.

**Prevention:**
- Watch the CWD as the primary scope. This covers 80-90% of normal terminal usage.
- When the command parser identifies file targets outside the CWD (absolute paths, `../` relative paths), create temporary targeted watches or perform direct pre-exec snapshots of those specific files. Single-file watches are cheap on all platforms.
- Do not attempt whole-filesystem watching. It is impractical and generates overwhelming noise.
- Clearly indicate in the UI when undo coverage is CWD-scoped: "Changes outside [cwd] are not tracked."
- When CWD changes (OSC 7 event), stop the old watcher and start a new one on the new CWD. Handle the transition atomically to avoid missing events during the switch.

**Phase relevance:** FS watcher design phase.

---

## Minor Pitfalls

### Pitfall 12: Ctrl+Shift+Z Keybinding Conflicts with Terminal Applications

**What goes wrong:** `Ctrl+Shift+Z` is the standard "Redo" keybinding in many applications running inside the terminal (text editors, IDEs, etc.). If Glass intercepts this globally, users cannot use Redo in their terminal apps.

**Prevention:**
- Only intercept `Ctrl+Shift+Z` when the shell is at a prompt -- between OSC 133;A (prompt start) and OSC 133;C (command executed). When a command is running or the terminal is in alt-screen mode (vim, less, nano, tmux), pass the keypress through to the PTY.
- The existing codebase already has the pattern for this: `TermMode` is checked for bracketed paste, and `BlockManager` tracks command lifecycle state. Use the same pattern.
- Also check `display_offset`: if the user is scrolled into history, they may be reviewing output rather than at a prompt. Still allow undo in this case (they might be looking at the command they want to undo).
- Consider making the keybinding configurable in `config.toml` for users who prefer a different binding.

**Phase relevance:** Keybinding/UI integration phase.

---

### Pitfall 13: File Permission and Metadata Loss on Restore

**What goes wrong:** Snapshotting only file content loses metadata: permissions (chmod bits, Windows ACLs), timestamps (mtime/atime), read-only flags, hidden attribute (Windows). Restoring a file with default permissions can break executables (lost +x bit), expose sensitive files (lost restrictive permissions), or confuse build tools (wrong mtime triggers unnecessary rebuilds).

**Prevention:**
- Store essential file metadata alongside content in the snapshot: file mode bits (Unix), read-only attribute (Windows), file size, mtime.
- On restore, set permissions after writing content. Use `std::fs::set_permissions()` for basic permission restoration.
- Do not attempt to restore ownership (uid/gid on Unix, owner SID on Windows) -- requires elevated privileges and is rarely needed for undo.
- Do not restore timestamps by default -- the restored file should have a new mtime to signal that it was modified. But store original timestamps in snapshot metadata for user inspection.
- Document what is and is not restored in the undo operation.

**Phase relevance:** Snapshot engine implementation phase.

---

### Pitfall 14: Hash Algorithm Choice Affects Performance Budget

**What goes wrong:** Using SHA-256 for content-addressed hashing is safe but slow. Hashing a 10MB file with SHA-256 takes ~30ms on modern hardware. If a command touches 50 files averaging 5MB each, hashing alone takes 750ms -- noticeable latency before the command even starts.

**Prevention:**
- Use BLAKE3 instead of SHA-256. BLAKE3 is ~5x faster on x86-64 (SIMD-accelerated), produces the same 32-byte hash, and is purpose-built for content addressing. The `blake3` Rust crate is well-maintained and widely used.
- Store hashes as 32-byte BLOBs in SQLite, not 64-character hex strings. Saves 50% storage per hash and makes index lookups faster.
- For large files (>1MB), consider hashing only the first and last 64KB plus the file size and mtime as a fast "change detection" hash, with full content hash computed lazily when dedup is actually needed.

**Phase relevance:** Storage engine design phase.

---

### Pitfall 15: Background Process Modifications After Command Completion

**What goes wrong:** Commands like `npm start`, `cargo watch`, `docker compose up`, or anything that forks daemons continue modifying files after OSC 133;D (command finished) fires. The watcher stops associating changes with that command, but the spawned processes keep writing. Undoing the "finished" command reverts its initial changes while the background process continues creating new files, resulting in a confused filesystem state.

**Prevention:**
- Undo reverts to the pre-command snapshot state. It cannot and should not try to account for ongoing background processes.
- Warn the user if the undo target is a recent command (within last 30 seconds) and the watcher is still detecting filesystem activity in the CWD. Display: "Warning: files are still being modified. Undo may be incomplete."
- Do not attempt to kill background processes spawned by the undone command. That is out of scope and dangerous.
- This is fundamentally a documentation/UX limitation, not a solvable technical problem. Be honest about it in the UI and docs.

**Phase relevance:** Undo execution phase (UX design).

---

### Pitfall 16: SQLite WAL Checkpoint Interaction with Snapshot DB

**What goes wrong:** If snapshots are stored in the same SQLite database as history, the WAL file grows large during bulk snapshot writes (e.g., snapshotting 100 files before a build command). The MCP server or CLI, holding read transactions open, prevents WAL checkpointing. The WAL file grows to hundreds of MB, effectively doubling database disk usage.

**Prevention:**
- Use a separate database file for snapshots (see Pitfall 9), which isolates WAL growth.
- If using a single DB, run `PRAGMA wal_checkpoint(PASSIVE)` periodically in the write connection. PASSIVE checkpointing never blocks readers but will skip pages that are locked by readers.
- Keep read transactions short in the CLI and MCP server -- query, collect results to memory, close transaction. Never hold a read transaction open while piping output to a pager.
- The existing history DB already sets `PRAGMA synchronous = NORMAL` and `PRAGMA busy_timeout = 5000` -- maintain these settings for the snapshot DB as well.

**Phase relevance:** Storage engine design phase.

---

## Phase-Specific Warnings

| Phase Topic | Likely Pitfall | Mitigation |
|-------------|---------------|------------|
| FS Watcher Engine | Buffer overflow silently drops events on Windows (Pitfall 2) | Detect zero-byte returns, implement fallback directory enumeration |
| FS Watcher Engine | Event flood overwhelming winit main thread (Pitfall 6) | Batch events in watcher thread, query only at command completion |
| FS Watcher Engine | Cross-platform behavioral differences (Pitfall 7) | Design abstract trait; implement Windows first; do not encode platform assumptions |
| FS Watcher Engine | CWD-only scope misses cross-directory ops (Pitfall 11) | Targeted watches for parser-identified out-of-CWD targets |
| Pre-exec Snapshot | Race with command execution timing (Pitfall 1) | Accept best-effort; watcher is primary mechanism; synchronous snapshot in event handler |
| Pre-exec Snapshot | TOCTOU during file read (Pitfall 4) | Single read() call; verify size/mtime before and after |
| Pre-exec Snapshot | Symlink following danger (Pitfall 10) | Use lstat; never follow symlinks; depth and size limits |
| Command Parser | Scope creep from trying to parse all shell syntax (Pitfall 5) | Whitelist top 15 commands per shell; cap parser at 300 lines |
| Snapshot Storage | Unbounded growth (Pitfall 3) | Retention from day one; BLAKE3 hashing; separate blob storage for >100KB |
| Snapshot Storage | Schema migration risk to existing DB (Pitfall 9) | Separate SQLite database for snapshots |
| Snapshot Storage | BLOB performance degradation (Pitfall 3) | Filesystem storage for >100KB, BLOBs in separate table for <100KB |
| Snapshot Storage | WAL growth during bulk writes (Pitfall 16) | Separate DB; periodic PASSIVE checkpoint |
| Snapshot Storage | Hash algorithm performance (Pitfall 14) | BLAKE3, not SHA-256; BLOB storage not hex |
| Undo Execution | Non-atomic directory restoration (Pitfall 8) | Two-phase staging; report partial results; verify disk space |
| Undo Execution | Metadata loss on restore (Pitfall 13) | Store permissions + size + mtime in snapshot metadata |
| Undo Execution | Background processes still modifying files (Pitfall 15) | Warn user; do not attempt to kill processes |
| Keybinding/UI | Ctrl+Shift+Z conflicts with terminal apps (Pitfall 12) | Only intercept at shell prompt; check alt-screen mode |

---

## Sources

- [ReadDirectoryChangesW documentation (Microsoft)](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-readdirectorychangesw) -- buffer overflow behavior, network drive limitations
- [Understanding ReadDirectoryChangesW Part 2 (Jim Beveridge)](https://qualapps.blogspot.com/2010/05/understanding-readdirectorychangesw_19.html) -- race conditions between calls, FILE_SHARE_DELETE pitfall
- [Using ReadDirectoryChangesW (Tresorit Engineering)](https://medium.com/tresorit-engineering/how-to-get-notifications-about-file-system-changes-on-windows-519dd8c4fb01) -- practical implementation guidance
- [ReadDirectoryChangesW stops working on large file counts (Microsoft Q&A)](https://learn.microsoft.com/en-us/answers/questions/1428660/readdirectorychangesw-stops-working-on-large-amoun) -- real-world buffer overflow reports
- [inotify(7) Linux manual page](https://man7.org/linux/man-pages/man7/inotify.7.html) -- recursive watch limitations, rename event pairing, queue overflow
- [Correct or inotify: pick one (wingolog)](https://wingolog.org/archives/2018/05/21/correct-or-inotify-pick-one) -- fundamental correctness limitations of inotify
- [inotify limitations (Boutnaru)](https://medium.com/@boutnaru/the-linux-concept-journey-inofity-inode-notification-limitations-c05de30d14fb) -- watch limits, pseudo-filesystem exclusions
- [FSEvents Programming Guide (Apple)](https://developer.apple.com/library/archive/documentation/Darwin/Conceptual/FSEvents_ProgGuide/UsingtheFSEventsFramework/UsingtheFSEventsFramework.html) -- latency parameter, coalescing behavior
- [Mac FSEvents limitations (Watchexec docs)](https://watchexec.github.io/docs/macos-fsevents.html) -- per-directory granularity, coalescing pitfalls
- [SQLite Internal vs External BLOBs](https://sqlite.org/intern-v-extern-blob.html) -- 100KB threshold for inline vs filesystem BLOBs
- [SQLite faster than filesystem (for small blobs)](https://sqlite.org/fasterthanfs.html) -- 35% faster for thumbnails
- [notify-rs/notify GitHub](https://github.com/notify-rs/notify) -- cross-platform FS watcher crate for Rust
- [notify-rs panic with Rust 1.81.0 (issue #636)](https://github.com/notify-rs/notify/issues/636) -- debouncer sorting bug
- [Race condition in std::fs::remove_dir_all (CVE-2022-21658)](https://github.com/rust-lang/rust/security/advisories/GHSA-r9cc-f5pr-p3j2) -- TOCTOU symlink following vulnerability
- [TOCTOU race conditions (Wikipedia)](https://en.wikipedia.org/wiki/Time-of-check_to_time-of-use) -- general background on the vulnerability class

---
*Pitfalls research for: Glass v1.2 -- Command-Level Undo with Filesystem Snapshots*
*Researched: 2026-03-05*
