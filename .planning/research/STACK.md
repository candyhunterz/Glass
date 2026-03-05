# Technology Stack

**Project:** Glass v1.2 -- Command-Level Undo
**Researched:** 2026-03-05
**Confidence:** HIGH

## Scope

This document covers ONLY the new dependencies needed for v1.2 (Command-Level Undo with filesystem snapshots). The existing stack (wgpu 28.0, winit 0.30.13, alacritty_terminal 0.25.1, glyphon 0.10.0, tokio 1.50.0, rusqlite 0.38.0, rmcp 1.1.0, chrono 0.4, clap 4.5, etc.) is validated and unchanged.

---

## Existing Stack (DO NOT ADD -- Already in Workspace)

| Technology | Version | Relevant to v1.2 Because |
|------------|---------|--------------------------|
| rusqlite | 0.38.0 (bundled) | Snapshot metadata tables (snapshots, snapshot_files) live in the existing history DB |
| tokio | 1.50.0 (full) | Async integration for FS watcher event channel, async file I/O via tokio::fs |
| chrono | 0.4 | Timestamps on snapshot records |
| anyhow | 1.0.102 | Error handling in glass_snapshot |
| tracing | 0.1.44 | Logging FS events, snapshot operations |
| serde | 1.0.228 | Serialization of snapshot metadata |
| dirs | 6 | Resolving snapshot blob storage directory |
| clap | 4.5 | Adding `glass undo <command-id>` subcommand |
| windows-sys | 0.59 | Already present; notify handles Windows FS APIs internally |
| tempfile | 3 | Testing snapshot creation/restoration |

---

## New Dependencies for v1.2

### Filesystem Monitoring

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| notify | 8.2.0 | Cross-platform filesystem watcher | The only serious cross-platform FS watcher in Rust. Uses ReadDirectoryChangesW on Windows, inotify on Linux, FSEvents on macOS. Used by alacritty, rust-analyzer, deno, zed. 62M+ downloads. CC0 licensed. Provides `recommended_watcher()` that auto-selects the best backend per platform. |
| notify-debouncer-full | 0.7.0 | Event debouncing with file ID tracking | Deduplicates rapid-fire FS events (editor save-rename-write cycles) into single logical events. Critically, tracks file IDs across renames -- needed to detect `mv` operations where a file is renamed rather than created+deleted. Without this, rename events appear as separate create/delete pairs. |

**Confidence:** HIGH -- notify 8.x is the de facto standard. No viable alternative exists.

**Integration pattern:**
```rust
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use std::time::Duration;

// Create debouncer with 200ms window
let (tx, rx) = std::sync::mpsc::channel();
let mut debouncer = new_debouncer(Duration::from_millis(200), None, tx)?;

// Watch the CWD reported by OSC 7
debouncer.watcher().watch(cwd.as_ref(), RecursiveMode::Recursive)?;

// Process events on dedicated thread (matches PTY reader pattern)
std::thread::spawn(move || {
    while let Ok(result) = rx.recv() {
        match result {
            Ok(events) => { /* record file modifications */ }
            Err(errors) => { /* log via tracing */ }
        }
    }
});
```

### Content Hashing for Deduplication

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| blake3 | 1.8.3 | Content-addressed hashing for file dedup | 2-14x faster than SHA-256 on x86 via auto-detected SIMD (SSE2/AVX2/AVX-512). 256-bit output eliminates collision risk. Built-in `.to_hex()` on `Hash` type (no separate hex crate needed). Incrementally hashable -- can hash while streaming file reads. Used by IPFS, Bao, Iroh for content addressing. |

**Confidence:** HIGH -- BLAKE3 is the standard choice for content-addressed storage in Rust. SHA-256 is slower with no benefit for non-cryptographic local CAS. xxhash is only 64/128-bit (unacceptable collision risk across thousands of file snapshots).

**Integration pattern:**
```rust
use blake3::Hasher;
use std::io::Read;

fn hash_file(path: &Path) -> anyhow::Result<blake3::Hash> {
    let mut hasher = Hasher::new();
    let mut file = std::fs::File::open(path)?;
    let mut buf = [0u8; 16384]; // 16KB reads
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize())
}

// Store blob: {data_dir}/glass/snapshots/blobs/{hash[0:2]}/{hash}
// The 2-char prefix directory prevents any single directory from having millions of entries
let hex = hash.to_hex();
let blob_dir = data_dir.join("snapshots/blobs").join(&hex.as_str()[..2]);
std::fs::create_dir_all(&blob_dir)?;
let blob_path = blob_dir.join(hex.as_str());
if !blob_path.exists() {
    std::fs::copy(source_path, &blob_path)?; // Dedup: only store if new hash
}
```

### Shell Command Text Parsing

| Technology | Version | Purpose | Why |
|------------|---------|---------|-----|
| shlex | 1.3.0 | Parse command text into shell argument tokens | POSIX shell tokenization handling quoting (`"foo bar"`, `'hello'`), escaping (`foo\ bar`), and multi-word arguments. Needed to extract file path arguments from commands like `rm -rf "my folder"`. Lightweight, zero dependencies. Provides `Shlex` iterator for lazy tokenization (inspect first token before parsing rest). |

**Confidence:** HIGH for bash/zsh. MEDIUM for PowerShell.

**PowerShell note:** shlex implements POSIX shell quoting. PowerShell uses backtick escapes and different string interpolation. For PowerShell, hand-roll a simple tokenizer (split on whitespace, handle `"` and `'` quoting). PowerShell's quoting is simple enough that a 30-line function handles it -- no crate needed.

**Integration pattern:**
```rust
use shlex::Shlex;

fn extract_file_targets(command_text: &str, cwd: &Path) -> Vec<PathBuf> {
    let mut lexer = Shlex::new(command_text);
    let cmd = match lexer.next() {
        Some(cmd) => cmd,
        None => return vec![],
    };
    let args: Vec<String> = lexer.collect();

    match cmd.as_str() {
        "rm" | "del" => extract_rm_targets(&args, cwd),
        "mv" | "move" => extract_mv_targets(&args, cwd),
        "cp" | "copy" => extract_cp_targets(&args, cwd),
        "sed" if args.contains(&"-i".to_string()) => extract_sed_targets(&args, cwd),
        "git" if args.first().map_or(false, |a| a == "checkout") => extract_git_targets(&args, cwd),
        "chmod" | "chown" => extract_trailing_paths(&args, cwd),
        _ => vec![], // Unknown command -- rely on FS watcher for recording
    }
}
```

---

## Deferred Dependencies (NOT for v1.2 MVP)

### Compression

| Technology | Version | Purpose | Why Defer |
|------------|---------|---------|-----------|
| zstd | 0.13.3 | Compress snapshot blobs | 3-5x compression on source code. But: raw blob storage is simpler for MVP. Content-addressed design means compression can be added non-breakingly later (compress before store, decompress on read; hash stays the same since it hashes original content). Add when storage pruning is implemented. |

### Diff Display

| Technology | Version | Purpose | Why Defer |
|------------|---------|---------|-----------|
| similar | 2.x | Text diffing for GlassFileDiff MCP tool | Undo = restore full file from snapshot. Diff display is a nice-to-have for the MCP tool, not required for the core undo flow. Add when GlassFileDiff is implemented. |

---

## Alternatives Considered

| Category | Recommended | Alternative | Why Not |
|----------|-------------|-------------|---------|
| FS watcher | notify 8.2 | Raw platform APIs (inotify/FSEvents/RDCW) | notify abstracts all three platforms. Rolling your own is 500+ lines of unsafe platform code per platform for no benefit. |
| FS watcher | notify 8.2 | watchexec-events | watchexec is a CLI tool wrapper, not a library. Its event types are heavier than needed. |
| FS debouncer | notify-debouncer-full | notify-debouncer-mini | Mini does not track file IDs across renames. Miss rename detection = broken `mv` tracking. |
| FS debouncer | notify-debouncer-full | No debouncer (raw notify) | Raw notify fires multiple events per editor save (temp write, rename, chmod). Without debouncing, snapshot logic must handle duplicate events manually. |
| Hashing | blake3 | sha2 (SHA-256) | 2-14x slower. No benefit for local (non-cryptographic, non-interop) CAS. |
| Hashing | blake3 | xxhash (xxh3) | Only 64/128-bit. Collision probability unacceptable for dedup across thousands of files over time. Birthday paradox at ~4B files for 64-bit. |
| Hashing | blake3 | seahash | Only 64-bit. Same collision concern as xxhash. |
| Shell parsing | shlex | shell-words | Both work. shlex has `Shlex` iterator for lazy tokenization -- can inspect the command name without allocating a full Vec. shell-words forces upfront full allocation. |
| Shell parsing | shlex | tree-sitter-bash | Massive dependency for AST parsing. We need argument tokenization, not syntax trees. |
| Shell parsing | shlex | regex-based | Breaks on quoted strings, escaped characters, nested quotes. Regex cannot correctly parse shell quoting rules. |
| CAS storage | Custom (~50 LOC) | casq / bdstorage crates | Over-engineered. Our CAS is: hash file, check blob exists, copy if not. SQLite tracks metadata. A full CAS library adds abstraction layers we don't need. |
| Snapshot DB | Extend glass_history DB | Separate SQLite file | Snapshots are metadata about commands. Foreign keys to the commands table require same DB. Single DB = atomic transactions (record command + snapshot together). |
| Snapshot DB | PRAGMA user_version migrations | refinery / sqlx | Already using user_version pattern in glass_history. Consistency > novelty. |

---

## What NOT to Add

| Temptation | Why Not |
|------------|---------|
| A CAS library (casq, bdstorage) | Our CAS is ~50 lines of code. Libraries add abstraction without value for this use case. |
| A migration framework | Already using `PRAGMA user_version` in glass_history. Extend that pattern. |
| similar (diff library) | Not needed for v1.2 core undo. Undo restores full files, no diff required. Defer to GlassFileDiff phase. |
| walkdir | `std::fs::read_dir` with manual recursion is sufficient for scanning known directories. walkdir adds a dependency for marginal convenience. |
| globset / glob | File target extraction works from parsed command arguments, not glob expansion. The shell expands globs before Glass sees the command text. |
| inotify / fsevent / windows-sys (direct) | notify wraps all three. Direct platform API usage duplicates notify's work. |
| tokio-rusqlite | Hides threading model. Dedicated thread + channel matches existing PTY reader architecture and gives explicit control. |

---

## Workspace Integration Plan

### Root Cargo.toml additions

```toml
[workspace.dependencies]
# v1.2: Command-Level Undo (NEW)
notify                = "8.2.0"
notify-debouncer-full = "0.7.0"
blake3                = "1.8.3"
shlex                 = "1.3.0"
```

### glass_snapshot/Cargo.toml (fill in stub crate)

```toml
[package]
name = "glass_snapshot"
version = "0.1.0"
edition = "2021"

[dependencies]
notify = { workspace = true }
notify-debouncer-full = { workspace = true }
blake3 = { workspace = true }
shlex = { workspace = true }
rusqlite = { workspace = true }
tokio = { workspace = true }
anyhow = { workspace = true }
tracing = { workspace = true }
chrono = { workspace = true }
dirs = { workspace = true }
serde = { workspace = true }

[dev-dependencies]
tempfile = "3"
```

### Root binary additions

```toml
[dependencies]
# Add to existing:
glass_snapshot = { path = "crates/glass_snapshot" }
```

### Crate Dependency Flow

```
glass_terminal (OSC 133 pre-exec + command-finished events)
    |
    v  command text + CWD via AppEvent
glass_snapshot (ALL undo logic)
    |-- notify + debouncer (FS monitoring)
    |-- blake3 (content hashing)
    |-- shlex (command text parsing)
    |-- rusqlite (snapshot metadata in shared history DB)
    |
    v  snapshot/restore results via AppEvent
glass_core (orchestrates undo: Ctrl+Shift+Z keybinding, undo button clicks)
    |
    v
glass_renderer ([undo] button on command blocks, restore feedback toast)
```

### DB Integration

**Extend the existing glass_history SQLite database.** Rationale:
- Snapshots reference commands (foreign key to `commands` table)
- Single DB = atomic operations (snapshot + command record in one transaction)
- glass_history already handles DB path resolution, migrations (user_version), WAL mode
- Add new tables via migration (increment user_version):

```sql
-- v2 migration
CREATE TABLE snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    command_id INTEGER NOT NULL REFERENCES commands(id),
    created_at TEXT NOT NULL,
    restored_at TEXT,  -- NULL until undo is triggered
    cwd TEXT NOT NULL
);

CREATE TABLE snapshot_files (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    snapshot_id INTEGER NOT NULL REFERENCES snapshots(id),
    file_path TEXT NOT NULL,
    blob_hash TEXT NOT NULL,  -- BLAKE3 hex hash
    file_size INTEGER NOT NULL,
    file_mode INTEGER,  -- Unix permissions (NULL on Windows)
    snapshot_type TEXT NOT NULL CHECK(snapshot_type IN ('pre_exec', 'watcher'))
);

CREATE INDEX idx_snapshot_files_hash ON snapshot_files(blob_hash);
CREATE INDEX idx_snapshots_command ON snapshots(command_id);
```

**Blob storage** lives on the filesystem, not in SQLite:
```
{data_dir}/glass/snapshots/blobs/{hash[0:2]}/{hash}
```

The 2-char prefix directory prevents any single directory from having millions of entries. SQLite stores only hash references. This keeps the DB small and blob I/O fast.

### glass_snapshot and glass_history Relationship

Two options:

**Option A (RECOMMENDED): glass_snapshot depends on glass_history**
- glass_snapshot imports glass_history's `HistoryDb` to get the `Connection` or a handle
- Snapshot tables are migrated by glass_history (single migration path)
- Pro: single point of DB management
- Con: couples the crates

**Option B: glass_snapshot gets its own Connection to the same file**
- Both crates open the same SQLite file independently
- WAL mode allows concurrent access
- Pro: crate independence
- Con: migration coordination is error-prone

Recommend Option A. The coupling is justified because snapshots are inherently linked to command records.

---

## Version Compatibility

| New Package | Compatible With | Notes |
|-------------|-----------------|-------|
| notify 8.2 | tokio 1.x | notify itself is sync (uses std::sync::mpsc). Integration with tokio is via channel bridging. No version conflict. |
| notify-debouncer-full 0.7 | notify 8.2 | Part of the notify-rs workspace. Version-locked to notify 8.x. |
| blake3 1.8.3 | MSRV 1.85 | Matches workspace MSRV expectations. No async runtime dependency. Pure computation. |
| shlex 1.3.0 | Any Rust edition | Zero dependencies. No compatibility concerns. |

---

## Compile & Binary Size Impact

| Dependency | Compile Impact | Binary Size | Notes |
|------------|---------------|-------------|-------|
| notify 8.2 | LOW -- mostly platform API bindings | ~100 KB | Thin wrappers around OS APIs. Windows: uses windows-sys (already in workspace). |
| notify-debouncer-full 0.7 | MINIMAL | ~20 KB | Small event processing logic on top of notify. |
| blake3 1.8.3 | LOW-MODERATE | ~200 KB | SIMD implementations compiled for multiple targets. Auto-detected at runtime. |
| shlex 1.3.0 | MINIMAL | ~10 KB | Tiny crate, zero dependencies. |
| **Total v1.2 addition** | **~330 KB** | Negligible vs existing ~80MB binary with GPU drivers. |

---

## Sources

- [notify crate (crates.io)](https://crates.io/crates/notify) -- v8.2.0, 62M+ downloads, CC0 license
- [notify docs.rs](https://docs.rs/notify/latest/notify/) -- API: recommended_watcher(), RecursiveMode, Event/EventKind types
- [notify-rs GitHub](https://github.com/notify-rs/notify) -- backends: ReadDirectoryChangesW (Windows), inotify (Linux), FSEvents (macOS)
- [notify-debouncer-full (lib.rs)](https://lib.rs/crates/notify-debouncer-full) -- v0.7.0, file ID tracking across renames
- [blake3 crate (crates.io)](https://crates.io/crates/blake3) -- v1.8.3
- [blake3 docs.rs](https://docs.rs/blake3/latest/blake3/) -- Hash::to_hex(), Hasher streaming API
- [BLAKE3 GitHub releases](https://github.com/BLAKE3-team/BLAKE3/releases/tag/1.8.3) -- v1.8.3: Hash::as_slice, MSRV 1.85
- [shlex docs.rs](https://docs.rs/shlex/latest/shlex/) -- v1.3.0, Shlex iterator, POSIX shell tokenization
- [shell-words docs.rs](https://docs.rs/shell-words/latest/shell_words/) -- alternative considered, split() returns Vec<String>
- [zstd crate (crates.io)](https://crates.io/crates/zstd) -- v0.13.3, deferred recommendation

---
*Stack research for: Glass v1.2 Command-Level Undo*
*Researched: 2026-03-05*
