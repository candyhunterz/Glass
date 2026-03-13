---
phase: 51-soi-compression-engine
plan: "02"
subsystem: glass_history
tags: [soi, compression, diff, history, fingerprint]
dependency_graph:
  requires: [51-01]
  provides: [DiffSummary, RecordFingerprint, diff_compress, get_previous_run_records]
  affects: [glass_history, glass_mcp (Phase 53), activity stream (Phase 55)]
tech_stack:
  added: []
  patterns:
    - HashSet-based fingerprint diffing for new/resolved record identification
    - OptionalExtension SQL query for prior run lookup
    - Record identity via (record_type, severity, file_path, message_prefix) tuple
key_files:
  created: []
  modified:
    - crates/glass_history/src/compress.rs
    - crates/glass_history/src/soi.rs
    - crates/glass_history/src/db.rs
    - crates/glass_history/src/lib.rs
decisions:
  - FreeformChunk records excluded from fingerprinting -- no stable identity field
  - message_prefix is first 80 chars of identity field to bound HashSet element size
  - get_previous_run_records uses ORDER BY started_at DESC LIMIT 1 to always find the most recent prior run
  - diff_compress None vs Some(&[]) produce distinct messages ("first run" vs "no structured data")
metrics:
  duration_minutes: 3
  completed_date: "2026-03-13"
  tasks_completed: 2
  files_modified: 4
requirements-completed: [SOIC-04]
---

# Phase 51 Plan 02: SOI Diff-Aware Compression Summary

**One-liner:** Diff-aware compression comparing current vs prior run using HashSet fingerprinting, producing new/resolved issue counts for AI agent consumption.

## What Was Built

Added `diff_compress()` function and supporting types to the SOI compression engine. AI agents and MCP tools can now see "compared to last run: 2 new, 1 resolved" rather than a static snapshot, enabling faster regression triage.

### New Types (compress.rs)

**RecordFingerprint** — stable identity for an output record:
- `record_type: String` (e.g. "CompilerError", "TestResult")
- `severity: Option<String>`
- `file_path: Option<String>`
- `message_prefix: String` — first 80 chars of identity field (message/name/package)
- Derives `Hash + Eq` for `HashSet` membership testing

**DiffSummary** — delta between two runs:
- `new_records: Vec<RecordFingerprint>` — appeared in current, not previous
- `resolved_records: Vec<RecordFingerprint>` — appeared in previous, not current
- `new_count / resolved_count: usize`
- `change_line: String` — human-readable delta ("compared to last run: N new, M resolved")

### New Functions

**`diff_compress(current, previous) -> DiffSummary`** (compress.rs):
- `previous = None` → "first run -- no comparison available"
- `previous = Some(&[])` → "no structured data for previous run"
- Otherwise → HashSet diff producing new/resolved record counts
- FreeformChunk records excluded (no stable identity)

**`get_previous_run_records(conn, command_text, current_id) -> Result<Option<Vec<OutputRecordRow>>>`** (soi.rs):
- SQL: `SELECT id FROM commands WHERE command = ?1 AND id != ?2 ORDER BY started_at DESC LIMIT 1`
- Returns `Ok(None)` for first run, `Ok(Some(records))` otherwise

**`HistoryDb::get_previous_run_records()`** (db.rs):
- Delegation method following existing `compress_output` / `get_output_records` pattern

### Re-exports (lib.rs)

`DiffSummary`, `RecordFingerprint`, `diff_compress` added to crate root re-exports.

## Tests Written

6 new tests covering all behavior cases:
- `diff_compress_second_run` — new_count=1, resolved_count=1 on changed run
- `diff_compress_first_run_no_prior` — change_line contains "first run"
- `diff_compress_empty_previous` — change_line contains "no structured data"
- `diff_compress_identical_runs` — new_count=0, resolved_count=0
- `get_previous_run_records_finds_prior` — DB test with two commands, same text
- `get_previous_run_records_no_prior` — DB test, single command returns None

## Verification Results

- `cargo test -p glass_history -- compress` — 12 passed
- `cargo test -p glass_history -- get_previous_run` — 2 passed
- `cargo test --workspace` — all 807 tests passed (0 failed)
- `cargo clippy --workspace -- -D warnings` — clean

## Deviations from Plan

None - plan executed exactly as written.

## Self-Check: PASSED

- `crates/glass_history/src/compress.rs` — modified with RecordFingerprint, DiffSummary, diff_compress, fingerprint helper
- `crates/glass_history/src/soi.rs` — modified with get_previous_run_records
- `crates/glass_history/src/db.rs` — modified with delegation method
- `crates/glass_history/src/lib.rs` — modified with re-exports
- Commit 2fb6e0f — feat(51-02): add diff_compress(), RecordFingerprint, DiffSummary, and get_previous_run_records()
