---
title: Glass Self-Improvement Pass
mode: build
verify: cargo test --workspace
---

# Glass Self-Improvement PRD

## Goal
Systematically harden the Glass codebase by fixing error handling, removing dead code, and addressing correctness issues. Each fix must be atomic (one commit per batch), pass all tests, and not change any public behavior.

## Constraints
- Do NOT refactor large files into separate modules
- Do NOT add new features or change observable behavior
- Do NOT modify test files (src/tests.rs, #[cfg(test)] modules)
- Changing function signatures (e.g. returning Result instead of panicking) is allowed, including updating callers in src/main.rs — but do NOT restructure the event loop or move code between modules
- Every commit must pass `cargo test --workspace` and `cargo clippy --workspace -- -D warnings`
- Commit each batch independently with a descriptive message

## Deliverables (in execution order — easy wins first)

### 1. Dead code cleanup
- Find all `#[allow(dead_code)]` annotations in non-test code
- If the code is actually used elsewhere, remove only the annotation
- If the code is truly dead, delete it and the annotation
- Commit once when done
- Check: `cargo clippy --workspace -- -D warnings`

### 2. Silent error audit
All .rs files except test modules:
- Search for `let _ =` patterns
- For file I/O operations (writes, directory creation, file removal): replace with `if let Err(e) = ... { tracing::warn!(...); }`
- Keep `let _ =` for: channel sends (receiver may be dropped), display/rendering ops, and cases where failure is truly inconsequential
- Only fix the ones where silent failure masks a real problem
- Commit in one batch

### 3. Agent backend error handling
Files: `crates/glass_agent_backend/src/*.rs`
- Replace `.unwrap()` in SSE line parsing with `.unwrap_or_default()` or match/if-let
- JSON parse failures in streaming should log a warning and skip the line, not crash
- serde_json::to_string calls can use `.unwrap_or_default()`
- Commit once when done
- Test: `cargo test -p glass_agent_backend`

### 4. Orchestrator error handling
File: `src/orchestrator.rs`
- Replace `.unwrap()` calls with proper error handling
- For functions that return `()`: use if-let or match with tracing::warn on failure
- For functions that can return Result: convert signature and update callers
- Focus areas: checkpoint file I/O, git command output parsing, iterations.tsv operations
- Commit once when done
- Test: `cargo test --workspace`

### 5. MCP tools.rs error handling (batched)
File: `crates/glass_mcp/src/tools.rs` (~86 unwraps across ~3400 lines)

**Do this in batches of 10-15 unwraps. Commit after each batch.**

Work top-to-bottom through the file:
- For `serde_json::from_str(...).unwrap()`: replace with match that returns an MCP error response on parse failure
- For `serde_json::to_string(...).unwrap()`: replace with `.unwrap_or_default()` or `unwrap_or_else(|_| "null".into())`
- For `.unwrap()` on Option types: use `.unwrap_or_default()` or return an error
- Each batch: fix 10-15 unwraps, verify `cargo test -p glass_mcp` passes, commit
- Expected: 6-8 commits to cover all ~86 unwraps

## Done Condition
All 5 deliverables committed. `cargo test --workspace` passes. `cargo clippy --workspace -- -D warnings` clean.
