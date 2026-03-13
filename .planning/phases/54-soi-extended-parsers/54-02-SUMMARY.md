---
phase: 54-soi-extended-parsers
plan: "02"
subsystem: soi
tags: [rust, regex, typescript, go, json, parser, soi]

requires:
  - phase: 54-soi-extended-parsers/54-01
    provides: git/docker/kubectl parsers, git content sniffer, established parser module pattern

provides:
  - tsc.rs parser producing CompilerError records with TS error codes for TypeScript compiler output
  - go_build.rs parser producing CompilerError records (code=None) for Go build errors
  - go_test.rs parser producing TestResult/TestSummary records for verbose and non-verbose go test output
  - json_lines.rs parser producing GenericDiagnostic records per valid NDJSON line with severity mapping
  - tsc content sniffer in classifier.rs: file(line,col): error|warning TSxxxx: pattern
  - go test content sniffer in classifier.rs: === RUN/--- PASS:/--- FAIL: markers

affects: [55-soi-go-parsers, 56-agent-runtime, 53-soi-mcp-tools]

tech-stack:
  added: []
  patterns:
    - "serde_json::Value (not concrete types) for JSON parsing — avoids tight coupling per Phase 51 decision"
    - "go_test chains to go_build on compile failure (no === RUN, no ok/FAIL lines, has file:line:col: pattern)"
    - "JSON lines parser uses < 2 valid lines threshold for freeform fallback — prevents false positives on output with single JSON objects"
    - "OnceLock<Regex> for compiled regex patterns — compile once, reuse across calls"

key-files:
  created:
    - crates/glass_soi/src/tsc.rs
    - crates/glass_soi/src/go_build.rs
    - crates/glass_soi/src/go_test.rs
    - crates/glass_soi/src/json_lines.rs
  modified:
    - crates/glass_soi/src/lib.rs
    - crates/glass_soi/src/classifier.rs

key-decisions:
  - "go_test chains to go_build::parse when output has no === RUN or ok/FAIL lines but matches file:line:col: pattern — same chaining pattern as cargo_test -> cargo_build"
  - "JSON lines parser requires >= 2 valid JSON lines before returning JsonLines output type — single JSON object in output is ambiguous, falls through to freeform"
  - "tsc parser strips ANSI codes at entry (tsc may colorize output); go_build parser does not (go build never colorizes)"
  - "go test verbose failure messages collected from indented lines (4 spaces or tab prefix) after current test's RUN line until PASS/FAIL result line"

patterns-established:
  - "tsc regex: ^(.+?)\\((\\d+),(\\d+)\\): (error|warning) (TS\\d+): (.+)$ — file(line,col): level code: message format"
  - "go build regex: ^(.+?):(\\d+):(\\d+): (.+)$ — skip lines starting with # (module path comments)"
  - "JSON level mapping: error/fatal/critical/err -> Error; warn/warning -> Warning; else Info"

requirements-completed: [SOIX-04, SOIX-05, SOIX-06]

duration: 5min
completed: 2026-03-13
---

# Phase 54 Plan 02: SOI Extended Parsers Summary

**tsc, go build, go test, and NDJSON parsers producing CompilerError/TestResult/TestSummary/GenericDiagnostic records with content sniffers for TypeScript and Go test output detection**

## Performance

- **Duration:** ~5 min
- **Started:** 2026-03-13T10:00:00Z
- **Completed:** 2026-03-13T10:04:18Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments

- tsc.rs parses TypeScript compiler errors and warnings with TS error codes (TS2345, TS6133, etc.) into CompilerError records; falls through to freeform on clean output
- go_build.rs parses Go build errors (file:line:col: message) into CompilerError records with code=None; skips # module comment lines
- go_test.rs handles both verbose (=== RUN + --- PASS/FAIL/SKIP) and non-verbose (ok/FAIL package lines) output; chains to go_build::parse on compilation failure; extracts failure messages from indented output blocks
- json_lines.rs parses NDJSON structured logs into GenericDiagnostic records; maps level/severity field (error/fatal/critical -> Error, warn/warning -> Warning, else Info); requires >= 2 valid JSON lines to avoid false positives
- tsc content sniffer detects file(line,col): error|warning TSxxxx: pattern without command hint
- go test content sniffer detects === RUN/--- PASS:/--- FAIL: markers without command hint
- 182 total tests pass (up from 136), clippy and fmt clean

## Task Commits

1. **Task 1: tsc, go_build, go_test, json_lines parsers** - `072bd5f` (feat)
2. **Task 2: Wire parsers into lib.rs and add tsc/go_test content sniffers** - `c84822a` (feat)

## Files Created/Modified

- `crates/glass_soi/src/tsc.rs` - TypeScript compiler output parser: file(line,col): level TScode: message -> CompilerError records
- `crates/glass_soi/src/go_build.rs` - Go build error parser: file:line:col: message -> CompilerError records (code=None)
- `crates/glass_soi/src/go_test.rs` - Go test output parser: verbose and non-verbose paths, chains to go_build on compile failure
- `crates/glass_soi/src/json_lines.rs` - NDJSON/structured log parser: GenericDiagnostic per valid JSON line with level -> severity mapping
- `crates/glass_soi/src/lib.rs` - Added mod declarations and TypeScript/GoBuild/GoTest/JsonLines dispatch arms
- `crates/glass_soi/src/classifier.rs` - Added has_tsc_marker() and has_go_test_marker() content sniffers wired into classify_by_content()

## Decisions Made

- go_test chains to go_build::parse on compilation failure (no === RUN or ok/FAIL lines, but has file:line:col: pattern) — mirrors how cargo_test chains to cargo_build
- JSON lines parser requires >= 2 valid JSON lines to avoid returning JsonLines for output that happens to contain a single JSON object
- tsc parser calls strip_ansi() at entry because tsc colorizes output; go_build skips this since go build does not colorize
- go test failure messages collected from indented lines (spaces/tab prefix) during the current test's execution window

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## Next Phase Readiness

- All four extended parsers wired and tested; SOI now produces structured records for TypeScript, Go (build+test), and NDJSON structured logs
- MCP tools from Phase 53 will automatically serve these records via glass_query_soi
- Phase 55 can add remaining parser implementations following the same module pattern

---
*Phase: 54-soi-extended-parsers*
*Completed: 2026-03-13*
