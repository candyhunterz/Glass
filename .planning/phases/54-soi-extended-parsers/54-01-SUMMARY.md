---
phase: 54-soi-extended-parsers
plan: "01"
subsystem: soi
tags: [rust, regex, git, docker, kubectl, parser, soi]

requires:
  - phase: 48-soi-foundation
    provides: glass_soi crate with OutputType, OutputRecord enums, classifier, freeform_parse
  - phase: 53-soi-mcp-tools
    provides: SOI MCP tools wired for git/docker/kubectl OutputTypes

provides:
  - git.rs parser producing GitEvent records for status/diff-stat/log/conflict output
  - docker.rs parser producing DockerEvent records for legacy build, BuildKit, and compose output
  - kubectl.rs parser producing GenericDiagnostic records for apply results and pod table rows
  - Git content sniffer in classifier.rs for hint-less git output detection

affects: [55-soi-go-parsers, 56-agent-runtime, 53-soi-mcp-tools]

tech-stack:
  added: []
  patterns:
    - "OnceLock<Regex> for compiled regex patterns — compile once, reuse across calls"
    - "freeform_parse fallback when records vec is empty — same pattern as npm.rs/jest.rs"
    - "strip_ansi() called at parser entry before line-by-line processing"
    - "line.len() > 4096 guard at top of each line loop"

key-files:
  created:
    - crates/glass_soi/src/git.rs
    - crates/glass_soi/src/docker.rs
    - crates/glass_soi/src/kubectl.rs
  modified:
    - crates/glass_soi/src/lib.rs
    - crates/glass_soi/src/classifier.rs

key-decisions:
  - "git log --oneline regex anchored to 7-12 hex char hash prefix to avoid matching other hash-like patterns"
  - "BuildKit step lines filtered by instruction keyword (FROM/RUN/COPY/etc.) to avoid capturing DONE/CACHED timing lines as build-step records"
  - "Pod status severity uses prefix matching for CrashLoop/ImagePullBackOff variants to cover all observed k8s status strings"
  - "Docker and kubectl receive NO content sniffers per plan spec — hint-only classification is sufficient for devops tools"

patterns-established:
  - "Parser module structure: strip_ansi at entry, OnceLock regex, line loop with 4096 guard, empty records -> freeform fallback"
  - "Severity determination: scan records vec after parsing, not during"
  - "One-line summary: surface the most important structured data (diff numbers, error counts) not a generic record count"

requirements-completed: [SOIX-01, SOIX-02, SOIX-03]

duration: 5min
completed: 2026-03-13
---

# Phase 54 Plan 01: SOI Extended Parsers Summary

**Git, Docker, and kubectl SOI parsers producing machine-readable GitEvent/DockerEvent/GenericDiagnostic records from devops tool output, with git content sniffer for hint-less detection**

## Performance

- **Duration:** ~5 min
- **Started:** 2026-03-13T09:53:02Z
- **Completed:** 2026-03-13T09:58:00Z
- **Tasks:** 2
- **Files modified:** 5

## Accomplishments

- git.rs parser handles status (branch, clean, modified files), diff --stat (with file/insertion/deletion counts), log --oneline (hash + message), and conflict output
- docker.rs parser handles legacy build Steps, BuildKit `#N` steps (filtered to instruction lines), BuildKit errors, compose `[+] Running` summaries, and container status lines
- kubectl.rs parser handles apply results (configured/created/unchanged/deleted) and get pods table rows with severity-mapped pod statuses
- Git content sniffer in classifier.rs detects "On branch", "nothing to commit", "Untracked files:", and "Changes not staged" without a command hint
- 136 total tests pass (103 original + 33 new)

## Task Commits

1. **Task 1: Git, Docker, kubectl parsers** - `86b990a` (feat)
2. **Task 2: Wire parsers into lib.rs and add git content sniffer** - `5aceab3` (feat)

## Files Created/Modified

- `crates/glass_soi/src/git.rs` - Git output parser: status/diff-stat/log/conflict -> GitEvent records
- `crates/glass_soi/src/docker.rs` - Docker output parser: legacy build/BuildKit/compose -> DockerEvent records
- `crates/glass_soi/src/kubectl.rs` - kubectl output parser: apply results + pod table -> GenericDiagnostic records
- `crates/glass_soi/src/lib.rs` - Added mod declarations and Git/Docker/Kubectl dispatch arms; updated parse_git_fallback test
- `crates/glass_soi/src/classifier.rs` - Added has_git_marker() content sniffer and git sniff tests

## Decisions Made

- BuildKit step lines filtered by Dockerfile instruction keywords (FROM/RUN/COPY/etc.) because `#N DONE 0.0s` and `#N CACHED` lines match the `#N` pattern but are not build steps
- Pod status severity uses prefix-match branches (`starts_with("CrashLoop")`, `starts_with("ImagePullBackOff")`) to cover variant forms without exhaustively listing all
- Git log --oneline regex uses `[0-9a-f]{7,12}` to match standard short hashes without matching longer text accidentally

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## Next Phase Readiness

- All three parsers wired and tested; SOI now produces structured records for git, docker, and kubectl commands
- Phase 55 can add Go build/test parsers following the same module pattern
- MCP tools from Phase 53 will automatically serve these records via glass_query_soi

---
*Phase: 54-soi-extended-parsers*
*Completed: 2026-03-13*
