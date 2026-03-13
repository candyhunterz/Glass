# Phase 54: SOI Extended Parsers - Context

**Gathered:** 2026-03-13
**Status:** Ready for planning

<domain>
## Phase Boundary

Add parser implementations for git, docker, kubectl, tsc, Go, and generic JSON lines to the glass_soi crate. All types, record variants, and classifier routing are already wired from Phase 48 -- this phase only adds the `parse()` function bodies and their tests.

</domain>

<decisions>
## Implementation Decisions

### Parser pattern
- Follow the exact same pattern as existing parsers (cargo_build.rs, cargo_test.rs, npm.rs, pytest.rs, jest.rs)
- Each parser is a module in glass_soi/src/ with a `pub fn parse(output: &str) -> ParsedOutput` signature (or with command_hint for those that need it)
- Add `mod` declaration in lib.rs and wire the match arm in `parse()` to replace freeform_parse fallback
- Use OnceLock<Regex> for compiled regex patterns (established pattern)

### Record variant mapping
- git -> `OutputRecord::GitEvent` (already defined: action, detail, files_changed, insertions, deletions)
- docker -> `OutputRecord::DockerEvent` (already defined: action, image, detail)
- kubectl -> `OutputRecord::GenericDiagnostic` (no dedicated KubectlEvent variant -- use generic)
- tsc -> `OutputRecord::CompilerError` (same variant as Rust, code field holds TS error codes like "TS2345")
- go build -> `OutputRecord::CompilerError` (same variant, code field is None for Go)
- go test -> `OutputRecord::TestResult` + `OutputRecord::TestSummary` (same as Rust test)
- json lines -> individual `OutputRecord::GenericDiagnostic` per JSON line with parsed fields

### Content sniffing
- Add content sniffers to classifier.rs for git, tsc, go test output (these have distinctive markers)
- Docker, kubectl, JSON lines: hint-only classification is sufficient (no reliable content markers)

### Claude's Discretion
- Exact regex patterns for each parser
- Error recovery when output doesn't match expected format (fall through to FreeformChunk)
- How much of git's diverse output to cover (status, diff stat, log --oneline, merge conflicts at minimum)
- Docker build step numbering extraction depth
- kubectl table vs YAML vs JSON output handling
- JSON lines field extraction strategy (level/severity/message/timestamp if present)

</decisions>

<specifics>
## Specific Ideas

No specific requirements -- open to standard approaches. The existing 5 parsers set a clear quality bar.

</specifics>

<code_context>
## Existing Code Insights

### Reusable Assets
- `glass_soi::ansi::strip_ansi()`: All parsers should strip ANSI before parsing (existing pattern)
- `glass_soi::freeform_parse()`: Fallback for unrecognized sections within parser output
- `OutputRecord` variants: GitEvent, DockerEvent, CompilerError, TestResult, TestSummary, GenericDiagnostic all already defined in types.rs
- Classifier already routes Git/Docker/Kubectl/TypeScript/GoBuild/GoTest/JsonLines via command hints

### Established Patterns
- OnceLock<Regex> for zero-cost regex compilation (classifier.rs, cargo_build.rs, etc.)
- cargo_test::parse chains to cargo_build::parse on compilation failure (composition pattern)
- npm multi-match-per-line pattern: don't use `continue` after first match (relevant for parsers with multi-field lines)
- jest concat!() macro for test fixtures preserving whitespace

### Integration Points
- lib.rs `parse()` match arms: currently `other => freeform_parse(output, Some(other), command_hint)` catches all Phase 54 types
- Each new parser module needs: `mod` in lib.rs, match arm in `parse()`, tests in same file
- No changes needed to classifier.rs beyond adding content sniffers
- No changes needed to types.rs -- all variants already exist

</code_context>

<deferred>
## Deferred Ideas

None -- discussion stayed within phase scope

</deferred>

---

*Phase: 54-soi-extended-parsers*
*Context gathered: 2026-03-13*
