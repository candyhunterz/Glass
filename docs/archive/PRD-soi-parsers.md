# Glass Self-Improvement: SOI Parser Expansion

## Goal

Implement the 8 missing SOI parsers that are already defined as `OutputType` variants but have no parser implementation. Each parser follows the same pattern as the existing 12 parsers in `crates/glass_soi/src/parsers/`.

## Context

Glass's Structured Output Intelligence (SOI) auto-parses command output using format-specific parsers. The `OutputType` enum in `crates/glass_soi/src/classifier.rs` defines all recognized output types. 12 have working parsers. 8 do not. The classifier already routes these types — they just fall through to the generic handler.

## Requirements

For each parser below, implement the parser module and add comprehensive tests. Follow the exact pattern of existing parsers (e.g., `cargo_test.rs`, `jest.rs`, `npm.rs`).

Each parser must:
1. Live in its own file under `crates/glass_soi/src/parsers/`
2. Implement the `parse()` function matching the existing parser signature
3. Extract structured records (passed/failed/errors/warnings as appropriate)
4. Be registered in the parser dispatch in `crates/glass_soi/src/lib.rs` (or wherever parsers are dispatched)
5. Include at least 3 unit tests with realistic output samples
6. Pass `cargo test --package glass_soi`
7. Pass `cargo clippy --package glass_soi -- -D warnings`

## Parsers to Implement (in priority order)

### 1. Pip Parser (`pip.rs`)
Parse output from `pip install`, `pip install -r requirements.txt`.
- Extract: package names, versions installed, warnings, errors
- Handle: "Successfully installed X-1.0 Y-2.0", "Requirement already satisfied", "ERROR: ..."
- Sample: `pip install requests flask` output

### 2. C/C++ Compiler Parser (`cpp_compiler.rs`)
Parse output from `gcc`, `g++`, `clang`, `clang++`.
- Extract: errors, warnings, notes with file:line:col locations
- Handle: "file.c:10:5: error: ...", "file.c:10:5: warning: ...", "N errors generated"
- Sample: typical gcc compilation with errors and warnings

### 3. Terraform Parser (`terraform.rs`)
Parse output from `terraform plan`, `terraform apply`.
- Extract: resources to add/change/destroy, plan summary
- Handle: "Plan: 3 to add, 1 to change, 0 to destroy", resource blocks, errors
- Sample: `terraform plan` output with mixed add/change/destroy

### 4. Generic TAP Parser (`tap.rs`)
Parse Test Anything Protocol output (used by many test frameworks).
- Extract: test count, passed, failed, skipped, with test names
- Handle: "1..N", "ok 1 - test name", "not ok 2 - test name", "# skip reason"
- Sample: TAP output from any TAP-producing test runner

### 5. CSV Parser (`csv.rs`)
Parse CSV/TSV tabular output.
- Extract: column headers, row count, detect delimiter (comma vs tab)
- Handle: header row detection, quoted fields, basic structure summary
- Note: This is structural parsing, not full CSV semantics — just enough for SOI summary

### 6. JSON Object Parser (`json_object.rs`)
Parse single JSON object/array output (not JSON Lines — that parser exists already).
- Extract: top-level keys, array length, nested structure summary
- Handle: pretty-printed and compact JSON, arrays of objects
- Differentiate from json_lines (which handles newline-delimited JSON)

### 7. Generic Compiler Parser (`generic_compiler.rs`)
Parse output matching the common `file:line:col: severity: message` pattern.
- Extract: errors, warnings, file locations
- Handle: GCC-style, Clang-style, rustc-style patterns as a fallback
- This is the catch-all for compilers not covered by specific parsers

### 8. Cargo Subcommand Parser (`cargo_misc.rs`)
Parse output from `cargo add`, `cargo update`, `cargo fetch`, `cargo install`.
- Extract: package names, versions, actions taken
- Handle: "Updating crates.io index", "Adding X v1.0", "Installed package X"
- Distinct from cargo_build and cargo_test which have their own parsers

## Implementation Order

Work through parsers 1-8 in order. After each parser:
- Run `cargo test --package glass_soi` to verify no regressions
- Run `cargo clippy --package glass_soi -- -D warnings`
- Commit with message: `feat(soi): add {name} parser`
- Then move to the next parser

## What NOT to Do

- Do NOT modify existing parsers
- Do NOT change the OutputType enum (types are already defined)
- Do NOT modify the classifier logic
- Do NOT add external dependencies — use std and regex (already available)
- Do NOT refactor the parser dispatch system
- Do NOT touch any crate other than glass_soi

## Success Criteria

- All 8 parsers implemented with tests
- `cargo test --package glass_soi` passes (existing + new tests)
- `cargo clippy --package glass_soi -- -D warnings` clean
- Each parser committed separately
