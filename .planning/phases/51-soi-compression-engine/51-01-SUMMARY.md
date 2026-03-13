---
phase: 51-soi-compression-engine
plan: 01
subsystem: glass_history
tags: [soi, compression, token-budget, rust]
dependency_graph:
  requires: [49-soi-storage, 50-soi-pipeline]
  provides: [compress-module, token-budget-api]
  affects: [phase-52-display, phase-53-mcp, phase-55-activity-stream]
tech_stack:
  added: []
  patterns: [greedy-token-budget, severity-ranked-truncation, json-extraction]
key_files:
  created:
    - crates/glass_history/src/compress.rs
  modified:
    - crates/glass_history/src/lib.rs
    - crates/glass_history/src/db.rs
decisions:
  - "compress() does not use glass_soi::OutputRecord for deserialization -- uses serde_json::Value to avoid tight coupling and future enum churn"
  - "Full budget populates record_ids (all IDs) for symmetry with greedy path even though truncated=false"
  - "OneLine budget uses empty record_ids (not useful for drill-down at that level)"
metrics:
  duration: ~8 min
  completed: 2026-03-13
  tasks_completed: 2
  files_changed: 3
---

# Phase 51 Plan 01: SOI Compression Engine Summary

**One-liner:** Token-budgeted SOI compression engine with four levels (OneLine/Summary/Detailed/Full), severity-ranked greedy truncation, and drill-down record IDs stored in glass_history::compress.

## What Was Built

The compression engine transforms raw `output_records` DB rows into `CompressedOutput` at the caller's desired token granularity:

- **`TokenBudget` enum** with `token_limit()` method: OneLine (10), Summary (100), Detailed (500), Full (usize::MAX)
- **`CompressedOutput` struct**: `budget`, `text`, `record_ids`, `token_count`, `truncated`
- **`compress()` function**: routes to budget-specific implementation
  - OneLine: counts Error records, formats "{N} error(s) in {first-error-file}", falls back to `summary.one_line`
  - Full: includes all records, never truncates
  - Summary/Detailed: sorts by severity_rank ASC then id ASC, greedily includes records until token ceiling hit
- **`HistoryDb::compress_output()`**: convenience delegation that fetches summary + records from DB then calls `compress()`
- Both `TokenBudget` and `CompressedOutput` re-exported from `glass_history` crate root

## Verification Results

- `cargo test -p glass_history -- compress`: 8/8 pass
- `cargo test -p glass_history`: 84/84 pass (no regressions)
- `cargo clippy -p glass_history -- -D warnings`: clean
- `cargo test --workspace`: all suites pass (no regressions)

## Deviations from Plan

None - plan executed exactly as written.

## Self-Check

- [x] `crates/glass_history/src/compress.rs` created with all required types and logic
- [x] `crates/glass_history/src/lib.rs` updated with `pub mod compress` and re-exports
- [x] `crates/glass_history/src/db.rs` updated with `compress_output` delegation
- [x] Commit dd58bd2: feat(51-01): add SOI compression engine with TokenBudget and compress()
- [x] Commit b4646c7: feat(51-01): add HistoryDb::compress_output delegation method

## Self-Check: PASSED
