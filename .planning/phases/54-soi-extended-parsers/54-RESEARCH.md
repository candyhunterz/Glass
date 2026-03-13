# Phase 54: SOI Extended Parsers - Research

**Researched:** 2026-03-13
**Domain:** Rust regex-based output parsing (glass_soi crate)
**Confidence:** HIGH

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- Follow the exact same pattern as existing parsers (cargo_build.rs, cargo_test.rs, npm.rs, pytest.rs, jest.rs)
- Each parser is a module in glass_soi/src/ with a `pub fn parse(output: &str) -> ParsedOutput` signature (or with command_hint for those that need it)
- Add `mod` declaration in lib.rs and wire the match arm in `parse()` to replace freeform_parse fallback
- Use OnceLock<Regex> for compiled regex patterns (established pattern)
- git -> `OutputRecord::GitEvent` (action, detail, files_changed, insertions, deletions)
- docker -> `OutputRecord::DockerEvent` (action, image, detail)
- kubectl -> `OutputRecord::GenericDiagnostic` (no dedicated KubectlEvent variant)
- tsc -> `OutputRecord::CompilerError` (code field holds TS error codes like "TS2345")
- go build -> `OutputRecord::CompilerError` (code field is None for Go)
- go test -> `OutputRecord::TestResult` + `OutputRecord::TestSummary`
- json lines -> individual `OutputRecord::GenericDiagnostic` per JSON line with parsed fields
- Add content sniffers to classifier.rs for git, tsc, go test output
- Docker, kubectl, JSON lines: hint-only classification is sufficient

### Claude's Discretion
- Exact regex patterns for each parser
- Error recovery when output doesn't match expected format (fall through to FreeformChunk)
- How much of git's diverse output to cover (status, diff stat, log --oneline, merge conflicts at minimum)
- Docker build step numbering extraction depth
- kubectl table vs YAML vs JSON output handling
- JSON lines field extraction strategy (level/severity/message/timestamp if present)

### Deferred Ideas (OUT OF SCOPE)
None -- discussion stayed within phase scope
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SOIX-01 | Git parser extracts action, files changed, insertions/deletions from git status/diff/log/merge/pull output | Git output formats documented below with verified regex patterns |
| SOIX-02 | Docker parser extracts build progress, errors, compose events from docker build/compose output | Docker build step and compose event formats documented with regex |
| SOIX-03 | kubectl parser extracts pod status, apply results, describe output from kubectl commands | kubectl table format and apply output documented; GenericDiagnostic variant fits |
| SOIX-04 | TypeScript/tsc parser extracts file, line, column, error code, message from tsc output | tsc error format is well-defined; CompilerError variant is exact fit |
| SOIX-05 | Go compiler and test parser extracts build errors and test results from go build/test output | Go formats documented; splits into go_build.rs and go_test.rs modules |
| SOIX-06 | Generic JSON lines parser handles NDJSON/structured logging output | serde_json already a dep; field extraction strategy documented |
</phase_requirements>

---

## Summary

Phase 54 adds six parser implementations to the `glass_soi` crate, one per tool family: git, docker, kubectl, tsc, go (build+test), and JSON lines. All infrastructure is already in place from Phase 48 — `OutputType` variants, `OutputRecord` enum arms, and classifier routing are wired. This phase only adds the `parse()` function bodies in new module files.

The existing parsers (cargo_build.rs, cargo_test.rs, npm.rs, pytest.rs, jest.rs) provide a complete, battle-tested template. Every new parser follows the same structure: OnceLock regex constants, line-by-line parsing with freeform fallback, and co-located tests using inline const string fixtures. The 103-test baseline is green and must remain green throughout.

**Primary recommendation:** Create one file per parser (git.rs, docker.rs, kubectl.rs, tsc.rs, go_build.rs, go_test.rs, json_lines.rs), wire each into lib.rs, and add three content sniffers to classifier.rs. Deliver tests first within each file using the cargo_test pattern.

---

## Standard Stack

### Core (already present — no new deps needed)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| regex | 1.x | Pattern matching for all parsers | Already in glass_soi Cargo.toml |
| serde_json | 1.0 | JSON lines parsing | Already in glass_soi Cargo.toml |
| std::sync::OnceLock | stdlib | Zero-cost regex compilation | Established project pattern |

### No New Dependencies
All required libraries are already in `crates/glass_soi/Cargo.toml`. The glass_soi crate currently depends on: `regex`, `serde`, `serde_json`, `anyhow`, `glass_errors`. No additions needed for Phase 54.

---

## Architecture Patterns

### Established File Structure Pattern
Every parser follows this layout:
```
crates/glass_soi/src/
├── git.rs           # NEW: GitEvent records
├── docker.rs        # NEW: DockerEvent records
├── kubectl.rs       # NEW: GenericDiagnostic records
├── tsc.rs           # NEW: CompilerError records (TS2345-style codes)
├── go_build.rs      # NEW: CompilerError records (no code)
├── go_test.rs       # NEW: TestResult + TestSummary records
└── json_lines.rs    # NEW: GenericDiagnostic per JSON line
```

### Pattern 1: Parser Module Template
Every parser module follows this exact structure:
```rust
// Source: crates/glass_soi/src/npm.rs and cargo_test.rs (verified directly)
use std::sync::OnceLock;
use regex::Regex;
use crate::types::{OutputRecord, OutputSummary, OutputType, ParsedOutput, Severity};

fn re_something() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"pattern").expect("description"))
}

pub fn parse(output: &str) -> ParsedOutput {
    let raw_line_count = output.lines().count();
    let raw_byte_count = output.len();
    let mut records: Vec<OutputRecord> = Vec::new();

    for line in output.lines() {
        if line.len() > 4096 { continue; }
        // matching logic...
    }

    if records.is_empty() {
        return crate::freeform_parse(output, Some(OutputType::Git), None);
    }
    // build summary and return ParsedOutput
}
```

### Pattern 2: ANSI Stripping (for colorized tools)
git, docker, kubectl, and tsc output often contains ANSI color codes. Use strip_ansi before parsing:
```rust
// Source: crates/glass_soi/src/jest.rs (verified directly)
pub fn parse(output: &str) -> ParsedOutput {
    let clean = crate::ansi::strip_ansi(output);
    parse_clean(&clean, output.len())
}

fn parse_clean(clean: &str, raw_byte_count: usize) -> ParsedOutput {
    // use clean for all matching, raw_byte_count for metrics
}
```

### Pattern 3: Freeform Fallback
When no patterns match, fall back to freeform:
```rust
// Source: crates/glass_soi/src/pytest.rs (verified directly)
if records.is_empty() && summary_record.is_none() {
    return crate::freeform_parse(output, Some(OutputType::Pytest), None);
}
```

### Pattern 4: lib.rs Wiring
```rust
// Source: crates/glass_soi/src/lib.rs (verified directly)
// Add mod declarations at top with existing parsers:
mod git;
mod docker;
mod kubectl;
mod tsc;
mod go_build;
mod go_test;
mod json_lines;

// Replace `other =>` fallback with specific arms:
OutputType::Git => git::parse(output),
OutputType::Docker => docker::parse(output),
OutputType::Kubectl => kubectl::parse(output),
OutputType::TypeScript => tsc::parse(output),
OutputType::GoBuild => go_build::parse(output),
OutputType::GoTest => go_test::parse(output),
OutputType::JsonLines => json_lines::parse(output),
other => freeform_parse(output, Some(other), command_hint),
```

### Pattern 5: Content Sniffer Registration
```rust
// Source: crates/glass_soi/src/classifier.rs (verified directly)
// Add after existing jest sniffer in classify_by_content():
if has_git_marker(output) {
    return OutputType::Git;
}
if has_tsc_marker(output) {
    return OutputType::TypeScript;
}
if has_go_test_marker(output) {
    return OutputType::GoTest;
}

// Each sniffer uses OnceLock<Regex>:
fn has_git_marker(output: &str) -> bool {
    // git status distinctive: "On branch" or "Changes not staged"
    output.contains("On branch ") || output.contains("Changes not staged for commit")
        || output.contains("nothing to commit")
}
```

### Anti-Patterns to Avoid
- **Using `continue` after partial match on same line:** npm.rs demonstrates you must NOT use `continue` when a single line can match multiple patterns (e.g., "added 142 packages, and audited 143 packages in 3s"). This applies to any tool where one line can carry multiple data points.
- **Blocking regex on long lines:** Always guard with `if line.len() > 4096 { continue; }` before regex matching.
- **Panicking regex init:** Use `.expect("descriptive name")` not `.unwrap()` so failures are diagnosable.
- **Tight coupling in summary:** Use `serde_json::Value` not concrete types when deserializing JSON lines (avoids coupling to schema).

---

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| JSON parsing | Custom JSON field scanner | `serde_json::from_str::<serde_json::Value>` | Already a dep; handles all JSON edge cases |
| ANSI stripping | Char-level state machine | `crate::ansi::strip_ansi()` | Already implemented in ansi.rs |
| Freeform fallback | Custom "unrecognized" path | `crate::freeform_parse()` | Consistent behavior for all unmatched output |
| Regex compilation | `Regex::new()` in parse() | `OnceLock<Regex>` pattern | Compile once; zero cost on subsequent calls |

---

## Common Pitfalls

### Pitfall 1: git output is highly diverse
**What goes wrong:** git has dozens of subcommand formats. Trying to handle all of them produces a sprawling parser that handles none well.
**Why it happens:** "git status" output differs from "git diff --stat" differs from "git log --oneline" differs from "git pull".
**How to avoid:** Cover the 4 most useful subsets: (1) `git status` branch/clean state, (2) `git diff --stat` / `git show` stat block for files_changed/insertions/deletions, (3) `git log --oneline` for commit summaries, (4) merge conflict markers. Fall through to freeform for anything else.
**Warning signs:** Parser growing beyond ~150 lines means scope creep.

### Pitfall 2: Docker build output changed with BuildKit
**What goes wrong:** Modern docker (BuildKit enabled) emits different progress output than legacy builder. The classic "Step 1/5" format is legacy; BuildKit uses `#N [stage]` format.
**Why it happens:** BuildKit became default in Docker 23.0+. Many docs show legacy format.
**How to avoid:** Handle both: legacy `Step N/M` AND BuildKit `#N` step markers. Emit DockerEvent records for each recognized step. Fall through to freeform for unrecognized progress lines.
**Warning signs:** Tests pass on legacy fixtures but fail on BuildKit output.

### Pitfall 3: tsc outputs to stderr by default
**What goes wrong:** TypeScript errors go to stderr; the OSC 133 command output capture includes stderr in the recorded output. This is fine — just be aware the fixture in tests must represent stderr-merged output.
**Why it happens:** tsc writes diagnostics to stderr.
**How to avoid:** Treat the merged output normally. tsc error format is deterministic: `file(line,col): error TScode: message`. Parse this one format.

### Pitfall 4: go test -v vs go test (no -v)
**What goes wrong:** `go test` without -v only prints per-test lines on failure. `go test -v` prints `--- PASS: TestName (0.00s)` for every test. Parser must handle both.
**Why it happens:** Users run both forms depending on CI vs local.
**How to avoid:** Detect the presence of `--- PASS:` / `--- FAIL:` lines. If absent, derive pass/fail from the summary `ok/FAIL` lines only.

### Pitfall 5: JSON lines false positives
**What goes wrong:** Many commands emit a single JSON object (not NDJSON). The JsonLines parser fires on single-JSON-line output classified by hint (e.g., `kubectl get pod -o json` would be hint-classified as Kubectl, not JsonLines — so this is actually safe for hint-classified flows).
**Why it happens:** Hint-only classification is used for JsonLines, meaning the user explicitly redirected a NDJSON-producing command.
**How to avoid:** In json_lines.rs, skip blank lines, skip lines that don't start with `{`, fall through to freeform if < 2 valid JSON lines parsed. Use `serde_json::from_str::<serde_json::Value>` per line with `?` equivalent (`.ok()` to skip malformed lines).

### Pitfall 6: kubectl table output has variable column count
**What goes wrong:** `kubectl get pods` outputs a text table with variable column widths based on data. Regex anchored to fixed columns breaks on different data.
**Why it happens:** kubectl right-pads or truncates columns based on terminal width.
**How to avoid:** Use whitespace-split approach for table rows rather than fixed-column regex. For `kubectl apply`, match on "configured", "created", "unchanged" keywords which are stable.

---

## Code Examples

Verified patterns from existing codebase:

### Git diff stat format (SOIX-01)
The `git diff --stat` and `git show` stat block format:
```
 src/main.rs | 42 ++++++++--
 src/lib.rs  |  3 ---
 2 files changed, 42 insertions(+), 3 deletions(-)
```
Regex for the summary line:
```rust
// Verified against actual git output format
fn re_git_stat_summary() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(\d+) files? changed(?:, (\d+) insertions?\(\+\))?(?:, (\d+) deletions?\(-\))?")
            .expect("git stat summary regex")
    })
}
```

### Git status format (SOIX-01)
```
On branch main
Changes not staged for commit:
  modified:   src/lib.rs
Untracked files:
  new_file.txt
nothing to commit, working tree clean
```
Key markers: `On branch `, `Changes not staged`, `nothing to commit`, `Changes to be committed`.

### Git log --oneline format (SOIX-01)
```
a1b2c3d feat: add new feature
e4f5g6h fix: resolve crash
```
Regex: `r"^([0-9a-f]{7,12}) (.+)$"` per line.

### Docker legacy build format (SOIX-02)
```
Step 1/5 : FROM ubuntu:22.04
 ---> 3b418d7b466a
Step 2/5 : RUN apt-get update
 ---> Running in 4a8b2c1d3e5f
Successfully built abc123def456
Successfully tagged myapp:latest
```

### Docker BuildKit format (SOIX-02)
```
#1 [internal] load build definition from Dockerfile
#1 DONE 0.0s
#2 [1/3] FROM ubuntu:22.04
#2 DONE 1.2s
#4 ERROR: failed to solve
```

### Docker compose format (SOIX-02)
```
[+] Running 3/3
 ✔ Container myapp-db-1  Started  0.5s
 ✔ Container myapp-web-1 Started  1.2s
 ✘ Container myapp-cache Error
```

### tsc error format (SOIX-04)
```
src/main.ts(10,5): error TS2345: Argument of type 'string' is not assignable to parameter of type 'number'.
src/utils.ts(3,1): warning TS6133: 'x' is declared but its value is never read.
```
Regex:
```rust
fn re_tsc_error() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(.+?)\((\d+),(\d+)\): (error|warning) (TS\d+): (.+)$")
            .expect("tsc error regex")
    })
}
```

### Go build error format (SOIX-05)
```
./main.go:10:5: undefined: fmt.Println2
./utils.go:3:1: imported and not used: "os"
```
Regex:
```rust
fn re_go_build_error() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(.+?):(\d+):(\d+): (.+)$")
            .expect("go build error regex")
    })
}
```

### Go test format (SOIX-05)
With `-v`:
```
=== RUN   TestAdd
--- PASS: TestAdd (0.00s)
=== RUN   TestSubtract
--- FAIL: TestSubtract (0.01s)
    main_test.go:15: expected 3, got 2
ok  	example.com/myapp	0.013s
FAIL	example.com/myapp	0.013s
```

Without `-v`:
```
FAIL	example.com/myapp [build failed]
ok  	example.com/myapp	0.013s
FAIL	example.com/myapp	0.005s
```

### JSON lines format (SOIX-06)
```json
{"level":"info","ts":1710000000,"msg":"server started","port":8080}
{"level":"error","ts":1710000001,"msg":"connection failed","error":"timeout"}
```
Field extraction: check for `level`/`severity`, `msg`/`message`, `ts`/`timestamp`, `error`/`err` keys. Map level strings to `Severity`: "error"/"fatal"/"critical" -> Error, "warn"/"warning" -> Warning, "info"/"debug"/"trace" -> Info.

### Content sniffer patterns (classifier.rs additions)
```rust
// Git: distinctive phrases in git status/log output
fn has_git_marker(output: &str) -> bool {
    output.contains("On branch ")
        || output.contains("Changes not staged for commit")
        || output.contains("nothing to commit")
        || output.contains("Untracked files:")
}

// tsc: error format "file(line,col): error TS"
fn has_tsc_marker(output: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"\(\d+,\d+\): (?:error|warning) TS\d+:").expect("tsc marker regex")
    });
    re.is_match(output)
}

// go test: "--- PASS:" or "--- FAIL:" lines
fn has_go_test_marker(output: &str) -> bool {
    output.contains("--- PASS:") || output.contains("--- FAIL:")
        || output.contains("=== RUN   ")
}
```

---

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| docker build Step N/M | BuildKit #N format | Docker 23.0 (2023) | Must handle both formats |
| go test per-package summary only | `go test -v` per-test lines | Go 1.x always had -v | Must handle both verbose and non-verbose |
| tsc error on stderr | tsc error merged in terminal output capture | Phase 50 | OSC 133 captures merged stdout+stderr |

**Active in this codebase:**
- Phase 48 wired all OutputType match arms — this phase fills in parse function bodies
- OnceLock<Regex> is mandatory (established in cargo_build.rs, npm.rs, classifier.rs)
- jest.rs demonstrates ANSI-first approach: strip ANSI before all regex matching

---

## Open Questions

1. **kubectl describe output**
   - What we know: kubectl describe produces multi-section text (Name, Namespace, Labels, Status, etc.)
   - What's unclear: The field format is not machine-parseable without a schema
   - Recommendation: For kubectl, focus on `kubectl apply` ("created"/"configured"/"unchanged") and `kubectl get` table rows (pod name + status column). Use GenericDiagnostic for each meaningful line. Fall through to freeform for kubectl describe.

2. **Docker compose v1 vs v2 format**
   - What we know: `docker-compose` (v1 Python) and `docker compose` (v2 Go plugin) have similar but not identical output
   - What's unclear: Exact differences in compose up output format
   - Recommendation: classifier.rs already handles both `docker ` and `docker-compose ` prefixes. Parser should handle v2 format (checkmark/cross format) primarily and fall through for v1.

3. **go test with -count, -run flags**
   - What we know: These change which tests run but not the output format
   - What's unclear: Whether `-json` flag output (JSON test events) should be a separate concern
   - Recommendation: Treat `go test -json` output as JsonLines (hint would still be GoTest, but the output would be NDJSON). The go_test.rs parser should check for JSON format and fall through to json_lines-style parsing if detected. Or simply fall through to freeform — `-json` is an advanced usage.

---

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in (`#[test]`) — no external test framework |
| Config file | none (workspace Cargo.toml controls) |
| Quick run command | `cargo test -p glass_soi` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SOIX-01 | git status/diff/log produces GitEvent records | unit | `cargo test -p glass_soi git::` | ❌ Wave 0 (git.rs not yet created) |
| SOIX-02 | docker build/compose produces DockerEvent records | unit | `cargo test -p glass_soi docker::` | ❌ Wave 0 |
| SOIX-03 | kubectl apply/get produces GenericDiagnostic records | unit | `cargo test -p glass_soi kubectl::` | ❌ Wave 0 |
| SOIX-04 | tsc errors produce CompilerError records with TS codes | unit | `cargo test -p glass_soi tsc::` | ❌ Wave 0 |
| SOIX-05 | go build errors and go test results produce correct records | unit | `cargo test -p glass_soi go_build:: go_test::` | ❌ Wave 0 |
| SOIX-06 | JSON lines produce GenericDiagnostic per line | unit | `cargo test -p glass_soi json_lines::` | ❌ Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_soi`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_soi/src/git.rs` — covers SOIX-01 (created as part of implementation)
- [ ] `crates/glass_soi/src/docker.rs` — covers SOIX-02
- [ ] `crates/glass_soi/src/kubectl.rs` — covers SOIX-03
- [ ] `crates/glass_soi/src/tsc.rs` — covers SOIX-04
- [ ] `crates/glass_soi/src/go_build.rs` — covers SOIX-05 (build half)
- [ ] `crates/glass_soi/src/go_test.rs` — covers SOIX-05 (test half)
- [ ] `crates/glass_soi/src/json_lines.rs` — covers SOIX-06

Note: No external test infrastructure gaps. Tests live in the same file as code per CLAUDE.md convention. Baseline is 103 passing tests; each new parser adds ~8-12 tests.

---

## Sources

### Primary (HIGH confidence)
- `crates/glass_soi/src/*.rs` — all 9 existing files read directly; patterns extracted from live code
- `crates/glass_soi/Cargo.toml` — dependency list verified directly
- `.planning/phases/54-soi-extended-parsers/54-CONTEXT.md` — locked decisions from user discussion

### Secondary (MEDIUM confidence)
- tsc error format: well-known `file(line,col): error TScode: message` — stable since TypeScript 1.x
- go build error format: `file:line:col: message` — stable, matches POSIX compiler convention
- git diff --stat summary line format — stable for decades

### Tertiary (LOW confidence)
- Docker BuildKit `#N` format — observed from general knowledge; exact format should be validated against actual BuildKit output during implementation

---

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — no new deps, everything verified in Cargo.toml
- Architecture: HIGH — patterns extracted directly from existing codebase
- Pitfalls: HIGH for tsc/go (stable formats), MEDIUM for docker (BuildKit variation), MEDIUM for kubectl (table column variability)
- Content sniffers: HIGH for git/tsc/go test, MEDIUM for docker (no reliable content marker)

**Research date:** 2026-03-13
**Valid until:** 2026-04-13 (stable tool formats; Docker BuildKit format changes are LOW risk in 30 days)
