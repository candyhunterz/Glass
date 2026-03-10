# Phase 38: Structured Error Extraction - Research

**Researched:** 2026-03-10
**Domain:** Compiler error parsing, MCP tool integration
**Confidence:** HIGH

## Summary

Phase 38 introduces a new `glass_errors` pure library crate that parses raw command output into structured, machine-readable error records (file, line, column, message, severity). The crate has zero dependencies on Glass internals -- it takes a string of output (and optionally a command hint) and returns a `Vec<StructuredError>`. A thin MCP tool (`glass_extract_errors`) in `glass_mcp` wires this library to command output retrieved from the history DB or live tabs.

The implementation requires two concrete parsers (Rust JSON, generic `file:line:col`) plus an auto-detection layer. Rust's `--error-format=json` emits one JSON object per line to stderr with a well-documented schema (spans, level, message, code). The generic parser handles the ubiquitous `file:line:col: message` format used by GCC, Clang, Go, TypeScript, and most compilers. Auto-detection inspects the command text (e.g., contains "cargo" or "rustc") and falls back to output content sniffing (e.g., presence of `"$message_type":"diagnostic"`).

**Primary recommendation:** Create `crates/glass_errors/` as a pure library crate with `regex` and `serde_json` dependencies. Expose a single `extract_errors(output: &str, command_hint: Option<&str>) -> Vec<StructuredError>` entry point. Add `glass_extract_errors` MCP tool to `glass_mcp` that delegates to this library.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| ERR-01 | Agent can extract structured errors (file, line, column, message, severity) from command output via MCP | MCP tool `glass_extract_errors` calls `glass_errors::extract_errors()`, returns JSON array of `StructuredError` |
| ERR-02 | Rust parser handles both human-readable and `--error-format=json` compiler output | `RustJsonParser` handles JSON diagnostics; `RustHumanParser` handles standard `error[E0xxx]: msg` format with `-->` spans |
| ERR-03 | Generic fallback parser handles `file:line:col: message` patterns from any compiler | `GenericParser` uses regex `^([^:\s]+):(\d+):(\d+):\s*(error|warning|note):\s*(.+)` and variants |
| ERR-04 | Parser auto-detects language from command text hint and output content | `detect_parser()` checks command hint first (cargo/rustc -> Rust), then sniffs output content patterns |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| regex | 1 (workspace) | Pattern matching for generic error format | Already in workspace, zero new deps |
| serde_json | 1.0 (workspace) | Parse Rust `--error-format=json` output | Already in workspace |
| serde | 1.0 (workspace, derive) | Deserialize Rust JSON diagnostics, serialize output | Already in workspace |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| schemars | 1.0 | MCP tool parameter schema generation | Already used by glass_mcp |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Custom regex parsing | tree-sitter or LSP | Massive overkill for line-by-line error extraction |
| Manual JSON parsing | cargo_metadata crate | Only handles cargo output, not raw rustc; adds unnecessary dep |

**Installation:**
```bash
# No new dependencies -- all are already in workspace
```

## Architecture Patterns

### Recommended Project Structure
```
crates/glass_errors/
  Cargo.toml
  src/
    lib.rs             # Public API: extract_errors(), StructuredError, Severity
    rust_json.rs       # Rust --error-format=json parser
    rust_human.rs      # Rust human-readable error parser (error[E0xxx]: msg --> file:line:col)
    generic.rs         # Generic file:line:col: message parser
    detect.rs          # Auto-detection: command hint + output sniffing
```

### Pattern 1: Parser Trait with Auto-Selection
**What:** Each language parser implements a common trait. A detection function selects the right parser(s) and runs them.
**When to use:** When multiple parsers may apply to the same output (e.g., Rust human-readable contains both `error[E0xxx]` lines and generic `file:line:col` patterns).
**Example:**
```rust
/// A single structured error extracted from command output.
#[derive(Debug, Clone, Serialize)]
pub struct StructuredError {
    pub file: String,
    pub line: u32,
    pub column: Option<u32>,
    pub severity: Severity,
    pub message: String,
    /// Optional error code (e.g., "E0308" for Rust)
    pub code: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
}

/// Main entry point -- parse output into structured errors.
pub fn extract_errors(output: &str, command_hint: Option<&str>) -> Vec<StructuredError> {
    let parser = detect_parser(output, command_hint);
    parser.parse(output)
}
```

### Pattern 2: Rust JSON Diagnostic Parsing
**What:** Parse one-JSON-object-per-line format from `rustc --error-format=json` or `cargo build --message-format=json`.
**When to use:** When output contains `{"$message_type":"diagnostic"` or `{"reason":"compiler-message"`.
**Example:**
```rust
// Cargo wraps rustc diagnostics in: {"reason":"compiler-message","message":{...diagnostic...}}
// Raw rustc emits: {"$message_type":"diagnostic","message":"...","level":"error","spans":[...]}

#[derive(Deserialize)]
struct CargoMessage {
    reason: String,
    message: Option<RustDiagnostic>,
}

#[derive(Deserialize)]
struct RustDiagnostic {
    message: String,
    level: String,       // "error", "warning", "note", "help"
    code: Option<DiagCode>,
    spans: Vec<DiagSpan>,
    children: Vec<RustDiagnostic>,
}

#[derive(Deserialize)]
struct DiagCode {
    code: String,
}

#[derive(Deserialize)]
struct DiagSpan {
    file_name: String,
    line_start: u32,
    column_start: u32,
    is_primary: bool,
}
```

### Pattern 3: Generic Regex Error Extraction
**What:** Regex-based parsing for the universal `file:line:col: severity: message` format.
**When to use:** Fallback for any compiler not explicitly supported.
**Example:**
```rust
// Matches: src/main.rs:10:5: error: unused variable
// Matches: main.c:42:1: warning: implicit declaration
// Also handles: file:line: message (no column)
lazy_static! or once_cell:
    // Primary: file:line:col: severity: message
    r"^([^:\s][^:]*):(\d+):(\d+):\s*(error|warning|note|info|hint):\s*(.+)"
    // Secondary: file:line: severity: message (no column)
    r"^([^:\s][^:]*):(\d+):\s*(error|warning|note|info|hint):\s*(.+)"
    // Tertiary: file:line:col: message (no explicit severity -- assume error)
    r"^([^:\s][^:]*):(\d+):(\d+):\s*(.+)"
```

### Pattern 4: Rust Human-Readable Error Parsing
**What:** Parse standard rustc human output format.
**When to use:** When output contains `error[E0xxx]:` patterns with `--> file:line:col` spans.
**Example:**
```
error[E0308]: mismatched types
 --> src/main.rs:10:5
  |
10 |     let x: u32 = "hello";
  |                   ^^^^^^^ expected `u32`, found `&str`
```
```rust
// Two-line pattern:
// Line 1: (error|warning)\[([A-Z]\d+)\]: (.+)
// Line 2:  --> (.+):(\d+):(\d+)
```

### Anti-Patterns to Avoid
- **Trying to parse all error formats at once:** Run the most specific parser first (Rust JSON), then fall back to generic. Do not mix JSON and regex parsing on the same line.
- **Over-engineering the trait hierarchy:** A simple enum dispatch (match on detected parser type) is better than a complex trait object graph for 2-3 parsers.
- **Deduplicating aggressively:** Rust JSON diagnostics have children (notes, help) that reference the same span. Preserve them as separate entries with different severities -- the agent can deduplicate.
- **Parsing `rendered` field instead of structured spans:** The `rendered` field in Rust JSON is for human display; always use `spans` array for structured data.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| JSON parsing | Custom JSON tokenizer | serde_json | Rust JSON diagnostics are complex nested objects |
| Regex compilation | One-off string splitting | regex crate with lazy_static/OnceLock | Patterns need anchoring, capture groups, and robustness |
| Error severity mapping | Ad-hoc string comparison | Enum with FromStr impl | Consistent mapping, exhaustive matching |

**Key insight:** The hard part is not parsing individual errors but handling the variety of formats and the auto-detection logic. The actual regex/JSON parsing is straightforward with standard crates.

## Common Pitfalls

### Pitfall 1: Cargo JSON vs Raw Rustc JSON
**What goes wrong:** Treating all JSON lines as raw rustc diagnostics when `cargo build --message-format=json` wraps them in `{"reason":"compiler-message","message":{...}}`.
**Why it happens:** Both are "Rust JSON" but have different top-level structures.
**How to avoid:** Check for `"reason"` field first. If present, unwrap the `message` field. If not, parse as raw rustc diagnostic.
**Warning signs:** Missing errors when parsing cargo output; works fine with raw rustc.

### Pitfall 2: Non-Diagnostic JSON Lines
**What goes wrong:** Attempting to parse every JSON line as a diagnostic when cargo also emits `compiler-artifact`, `build-script-executed`, and `build-finished` messages.
**Why it happens:** Not filtering by `reason` field.
**How to avoid:** Only process lines where `reason == "compiler-message"` or `$message_type == "diagnostic"`. Skip all other JSON lines silently.
**Warning signs:** Deserialization errors on valid cargo output.

### Pitfall 3: Windows Path Colons in Generic Parser
**What goes wrong:** `C:\Users\foo\main.rs:10:5: error` -- the regex treats `C` as the filename.
**Why it happens:** Windows paths contain colons that conflict with the `file:line:col` delimiter.
**How to avoid:** Allow an optional drive letter prefix: `^([A-Za-z]:\\[^:]+|[^:\s][^:]*):(\d+):...` or handle the `\` detection.
**Warning signs:** All Windows paths parsed incorrectly.

### Pitfall 4: Multiline Error Messages
**What goes wrong:** Only capturing the first line of a multi-line error message.
**Why it happens:** Line-by-line regex parsing naturally stops at line boundaries.
**How to avoid:** For the generic parser, this is acceptable -- capture the first line. For Rust JSON, the full message is in the `message` field. For Rust human-readable, capture the header line and the `-->` span line as a pair.
**Warning signs:** Truncated messages for complex errors.

### Pitfall 5: Empty Spans in Rust JSON
**What goes wrong:** Panicking when a Rust JSON diagnostic has an empty `spans` array.
**Why it happens:** Some diagnostics (like "aborting due to N previous errors") have no source location.
**How to avoid:** Skip diagnostics with empty spans, or emit them with empty file/line fields. Check `is_primary` to find the main span.
**Warning signs:** Crash on valid compiler output.

## Code Examples

### Rust JSON Diagnostic Parsing
```rust
// Source: https://doc.rust-lang.org/rustc/json.html
fn parse_rust_json(output: &str) -> Vec<StructuredError> {
    let mut errors = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        // Try cargo wrapper format first
        if let Ok(cargo_msg) = serde_json::from_str::<CargoMessage>(line) {
            if cargo_msg.reason == "compiler-message" {
                if let Some(diag) = cargo_msg.message {
                    collect_diagnostic(&diag, &mut errors);
                }
            }
            continue;
        }
        // Try raw rustc diagnostic
        if let Ok(diag) = serde_json::from_str::<RustDiagnostic>(line) {
            collect_diagnostic(&diag, &mut errors);
        }
    }
    errors
}

fn collect_diagnostic(diag: &RustDiagnostic, errors: &mut Vec<StructuredError>) {
    let severity = match diag.level.as_str() {
        "error" | "error: internal compiler error" => Severity::Error,
        "warning" => Severity::Warning,
        "note" => Severity::Note,
        "help" => Severity::Help,
        _ => Severity::Error,
    };
    // Find primary span
    if let Some(span) = diag.spans.iter().find(|s| s.is_primary) {
        errors.push(StructuredError {
            file: span.file_name.clone(),
            line: span.line_start,
            column: Some(span.column_start),
            severity,
            message: diag.message.clone(),
            code: diag.code.as_ref().map(|c| c.code.clone()),
        });
    }
}
```

### Generic Pattern Parsing
```rust
use regex::Regex;
use std::sync::OnceLock;

fn generic_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Handle optional Windows drive letter, then file:line:col: severity: message
        Regex::new(
            r"(?m)^([A-Za-z]:\\[^:]+|[^:\s][^:]*):(\d+):(\d+):\s*(?i)(error|warning|note|info|hint):\s*(.+)$"
        ).unwrap()
    })
}
```

### Auto-Detection
```rust
fn detect_parser(output: &str, command_hint: Option<&str>) -> ParserKind {
    // 1. Command hint takes priority
    if let Some(cmd) = command_hint {
        let cmd_lower = cmd.to_lowercase();
        if cmd_lower.contains("cargo") || cmd_lower.contains("rustc") {
            // Check if output looks like JSON
            if output.lines().any(|l| l.trim_start().starts_with('{')) {
                return ParserKind::RustJson;
            }
            return ParserKind::RustHuman;
        }
    }
    // 2. Content sniffing
    if output.contains(r#""$message_type":"diagnostic""#)
        || output.contains(r#""reason":"compiler-message""#)
    {
        return ParserKind::RustJson;
    }
    if output.contains("error[E") || output.contains("warning[") {
        return ParserKind::RustHuman;
    }
    // 3. Fallback
    ParserKind::Generic
}
```

### MCP Tool Integration
```rust
// In glass_mcp/src/tools.rs -- follows existing pattern exactly
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ExtractErrorsParams {
    /// Raw command output to parse for errors.
    #[schemars(description = "Raw command output text to extract errors from")]
    pub output: String,
    /// Optional command hint for parser auto-detection (e.g. 'cargo build', 'gcc main.c').
    #[schemars(description = "Command that produced the output, for parser auto-detection")]
    pub command_hint: Option<String>,
}

#[tool(
    description = "Extract structured errors (file, line, column, message, severity) from raw command output. Auto-detects the language/compiler from the command hint or output patterns."
)]
async fn glass_extract_errors(
    &self,
    Parameters(params): Parameters<ExtractErrorsParams>,
) -> Result<CallToolResult, McpError> {
    let errors = glass_errors::extract_errors(&params.output, params.command_hint.as_deref());
    let json = serde_json::json!({
        "errors": errors,
        "count": errors.len(),
    });
    Ok(CallToolResult::success(vec![Content::text(
        serde_json::to_string_pretty(&json).unwrap_or_default(),
    )]))
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Regex-only error parsing | JSON diagnostics from `--error-format=json` | rustc 1.12+ (2016) | Machine-readable, no regex fragility |
| Parsing `rendered` field | Using `spans` array directly | Always available | Structured file/line/col data |
| Single parser | Auto-detection with fallback | Current best practice | Handles mixed output gracefully |

**Deprecated/outdated:**
- `--error-format=short`: Still available but less useful than JSON for structured extraction
- Cargo `--message-format=short`: Emits condensed human output, not machine-readable

## Open Questions

1. **Should the MCP tool accept command_id instead of raw output?**
   - What we know: The tool could look up output from history DB by command_id, similar to tab_output
   - What's unclear: Whether agents prefer passing raw text or referencing stored commands
   - Recommendation: Support BOTH -- accept `output` (raw text) OR `command_id` (lookup from history DB). This matches the dual-mode pattern in `glass_tab_output`.

2. **Should we include the Rust human-readable parser in Phase 38 or defer?**
   - What we know: ERR-02 says "both human-readable and --error-format=json"
   - What's unclear: How complex the human-readable parser needs to be
   - Recommendation: Include it -- the pattern is well-defined (error[Exxx] + --> file:line:col) and adds maybe 50 lines of code.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (built-in) |
| Config file | Cargo.toml (workspace member) |
| Quick run command | `cargo test --package glass_errors` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| ERR-01 | extract_errors returns StructuredError with all fields | unit | `cargo test --package glass_errors` | No - Wave 0 |
| ERR-01 | MCP tool returns JSON with errors array | unit | `cargo test --package glass_mcp` | No - Wave 0 |
| ERR-02 | Rust JSON parser handles rustc --error-format=json | unit | `cargo test --package glass_errors::rust_json` | No - Wave 0 |
| ERR-02 | Rust JSON parser handles cargo --message-format=json wrapper | unit | `cargo test --package glass_errors::rust_json` | No - Wave 0 |
| ERR-02 | Rust human parser handles error[E0308] with --> spans | unit | `cargo test --package glass_errors::rust_human` | No - Wave 0 |
| ERR-03 | Generic parser extracts file:line:col: error: message | unit | `cargo test --package glass_errors::generic` | No - Wave 0 |
| ERR-03 | Generic parser handles Windows paths (C:\...) | unit | `cargo test --package glass_errors::generic` | No - Wave 0 |
| ERR-04 | detect_parser selects Rust for cargo/rustc commands | unit | `cargo test --package glass_errors::detect` | No - Wave 0 |
| ERR-04 | detect_parser sniffs JSON content for Rust JSON | unit | `cargo test --package glass_errors::detect` | No - Wave 0 |
| ERR-04 | detect_parser falls back to Generic | unit | `cargo test --package glass_errors::detect` | No - Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test --package glass_errors && cargo test --package glass_mcp`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green + `cargo clippy --workspace -- -D warnings`

### Wave 0 Gaps
- [ ] `crates/glass_errors/Cargo.toml` -- new crate manifest
- [ ] `crates/glass_errors/src/lib.rs` -- public types and entry point with tests
- [ ] `crates/glass_errors/src/rust_json.rs` -- Rust JSON parser with tests
- [ ] `crates/glass_errors/src/rust_human.rs` -- Rust human parser with tests
- [ ] `crates/glass_errors/src/generic.rs` -- Generic parser with tests
- [ ] `crates/glass_errors/src/detect.rs` -- Auto-detection with tests

## Sources

### Primary (HIGH confidence)
- [rustc JSON output docs](https://doc.rust-lang.org/rustc/json.html) -- Complete diagnostic JSON schema with field types, span structure, severity levels
- [cargo external tools docs](https://doc.rust-lang.org/cargo/reference/external-tools.html) -- Cargo JSON message wrapping format with `reason` field
- [GNU coding standards - errors](https://www.gnu.org/prep/standards/html_node/Errors.html) -- Standard `file:line:col: message` format definition

### Secondary (MEDIUM confidence)
- Existing `glass_mcp/src/tools.rs` code -- Verified MCP tool patterns, parameter structs, schemars usage
- Existing workspace `Cargo.toml` -- Verified available dependencies (regex, serde_json, serde)

### Tertiary (LOW confidence)
- None -- all findings verified against primary sources

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all dependencies already in workspace, no new crates needed
- Architecture: HIGH -- follows established Glass crate patterns, well-understood parsing domain
- Pitfalls: HIGH -- Rust JSON format is well-documented, Windows path issue is a known classic

**Research date:** 2026-03-10
**Valid until:** 2026-04-10 (stable domain, Rust JSON format rarely changes)
