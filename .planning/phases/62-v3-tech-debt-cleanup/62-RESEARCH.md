# Phase 62: v3.0 Tech Debt Cleanup - Research

**Researched:** 2026-03-13
**Domain:** Documentation metadata cleanup (doc comments, YAML frontmatter)
**Confidence:** HIGH

## Summary

Phase 62 is a metadata-only cleanup phase with two tightly scoped tasks. No code logic changes, no new dependencies, no compilation required. All work is plain text edits to a Rust doc comment and YAML frontmatter blocks in existing SUMMARY.md files.

**Task 1** removes a stale doc comment from `crates/glass_soi/src/lib.rs` lines 41-43. The comment says "For Phase 48, all parsers are stubs that return a freeform fallback. Plans 48-02 and 48-03 will implement the full parsers." This was accurate when Plan 48-01 was written but all parsers were implemented in Plans 48-02, 48-03, and 54-01/02. The comment must be replaced with an accurate description of what `parse()` does now.

**Task 2** backfills `requirements_completed` YAML frontmatter into eight SUMMARY.md files that were written before that frontmatter convention was established. Exactly 13 REQ-IDs are missing across phases 48, 51, 56, 57, and 59.

**Primary recommendation:** One plan, two sequential tasks — fix the doc comment first, then backfill all eight SUMMARY.md files. No testing needed beyond confirming the files read back correctly; these are docs-only changes with zero runtime impact.

## Standard Stack

### Core
| Tool | Version | Purpose | Why Standard |
|------|---------|---------|--------------|
| Text editor / Write tool | N/A | Edit Rust doc comment and YAML frontmatter | Only tool needed |
| YAML (subset) | 1.2 | SUMMARY.md frontmatter format | Established convention in this repo |
| Rust doc comment (`///`) | N/A | Inline API documentation | Project convention per CLAUDE.md |

No new dependencies. No `cargo build` needed. The doc comment is not a test (the existing tests in the `#[cfg(test)]` block are unrelated to the comment text).

## Architecture Patterns

### SUMMARY.md Frontmatter Convention

SUMMARY.md files in `.planning/phases/` use YAML frontmatter delimited by `---`. The `requirements_completed` key holds a list of REQ-IDs that the plan implemented. This key is optional (some earlier plans omit it) but required by the audit.

**Established format (from 48-03-SUMMARY.md):**
```yaml
requirements-completed: [SOIP-04, SOIP-05, SOIP-06]
```

Note: the 48-03 file uses `requirements-completed` (hyphen) rather than `requirements_completed` (underscore). The 56-02 and 57-02 files use `requirements-completed` as well. **Use the hyphen form** to match the existing convention in these specific files.

**Established format (from 56-02-SUMMARY.md):**
```yaml
requirements-completed: [AGTR-01, AGTR-02, AGTR-04, AGTR-05, AGTR-06, AGTR-07]
```

**Established format (from 57-02-SUMMARY.md):**
```yaml
requirements-completed: [AGTW-01, AGTW-02, AGTW-03, AGTW-04, AGTW-05]
```

The planner should insert `requirements-completed:` as a new YAML key in the frontmatter of each file that lacks it.

### Rust Doc Comment Convention

Standard three-slash `///` doc comments for public functions. The existing `parse()` comment block to replace is at lines 40-43 of `crates/glass_soi/src/lib.rs`:

```rust
/// Dispatch parsed output to the appropriate parser based on `output_type`.
///
/// For Phase 48, all parsers are stubs that return a freeform fallback.
/// Plans 48-02 and 48-03 will implement the full parsers.
pub fn parse(...) -> ParsedOutput {
```

The replacement must keep the first line (accurate) and replace lines 42-43 with current truth: all 12 parser types are fully implemented, `FreeformText` and unrecognized variants fall back to `freeform_parse()`.

### Anti-Patterns to Avoid

- **Changing YAML key style inconsistently:** Use hyphen-form (`requirements-completed`) to match the three existing SUMMARY.md files that already have this key.
- **Touching code logic:** This phase is metadata-only. Do not change any Rust function signatures, logic, or tests.
- **Over-expanding the doc comment:** Replace only the stale lines. Keep the function signature and module-level doc block unchanged.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| YAML editing | Custom parser | Direct text edit | Frontmatter is simple key-value, direct insertion is safe |
| Doc comment update | Scripted sed | Direct Write tool | Single targeted replacement; scripting adds risk |

## Common Pitfalls

### Pitfall 1: Wrong YAML key name
**What goes wrong:** Writing `requirements_completed` (underscore) instead of `requirements-completed` (hyphen), creating a key mismatch with the convention in the three existing files.
**Why it happens:** YAML supports both forms; underscore is the "obvious" choice.
**How to avoid:** Check the three reference files (48-03, 56-02, 57-02) before writing — they all use the hyphen form.
**Warning signs:** If a new key doesn't match the others visually, it's wrong.

### Pitfall 2: Missing or wrong REQ-IDs
**What goes wrong:** Assigning the wrong REQ-IDs to a plan (e.g., putting all six SOIP-0x IDs in 48-01 instead of splitting them correctly).
**Why it happens:** The REQUIREMENTS.md traceability table maps requirements to phases, not to individual plans within a phase.
**How to avoid:** Use the per-plan SUMMARY.md content to determine which requirements each plan actually implemented. See the Requirement-to-Plan Map below.

### Pitfall 3: Touching test comments in lib.rs
**What goes wrong:** Updating the stale comment on `parse()` but also accidentally changing the test comment on line 98 (`// Stub: returns freeform with RustCompiler type`).
**Why it happens:** The test comment is also stale (the parse now delegates to a real parser, not a stub) and may tempt a cleanup.
**How to avoid:** Success criterion 1 only requires the function-level doc comment change. Changing the test comment is out of scope for this phase.

## Code Examples

### Current stale doc comment (lines 40-43 of crates/glass_soi/src/lib.rs)
```rust
/// Dispatch parsed output to the appropriate parser based on `output_type`.
///
/// For Phase 48, all parsers are stubs that return a freeform fallback.
/// Plans 48-02 and 48-03 will implement the full parsers.
```

### Replacement doc comment
```rust
/// Dispatch parsed output to the appropriate parser based on `output_type`.
///
/// Fully implemented parsers: `RustCompiler`, `RustTest`, `Npm`, `Pytest`, `Jest`,
/// `Git`, `Docker`, `Kubectl`, `TypeScript`, `GoBuild`, `GoTest`, `JsonLines`.
/// Unrecognized variants fall back to [`freeform_parse`].
```

### YAML frontmatter insertion example (48-01-SUMMARY.md)
The file currently ends its frontmatter without `requirements-completed`. Insert before the closing `---`:
```yaml
requirements-completed: [SOIP-01]
```

## Requirement-to-Plan Map

This is the authoritative mapping the planner must use for backfilling. Derived from reading each SUMMARY.md's "What Was Built" section against REQUIREMENTS.md.

| SUMMARY.md File | REQ-IDs to Add | Rationale |
|----------------|----------------|-----------|
| 48-01-SUMMARY.md | SOIP-01 | Plan 01 built the classifier (`classify()`) and OutputType taxonomy — that IS the "SOI classifier detects output type" requirement |
| 48-02-SUMMARY.md | SOIP-02, SOIP-03 | Plan 02 built cargo_build (SOIP-02) and cargo_test (SOIP-03) parsers |
| 51-01-SUMMARY.md | SOIC-01, SOIC-02, SOIC-03 | Plan 01 built 4-level `compress()` (SOIC-01), severity-ranked truncation (SOIC-02), drill-down record_ids (SOIC-03) |
| 51-02-SUMMARY.md | SOIC-04 | Plan 02 built `diff_compress()` — the diff-aware compression requirement |
| 56-01-SUMMARY.md | AGTR-03 | Plan 01 built `AgentMode` enum (Off/Watch/Assist/Autonomous) and `should_send_in_mode()` — the three autonomy modes requirement |
| 57-01-SUMMARY.md | AGTW-06 | Plan 01 built the temp-directory fallback path for non-git projects (the non-git fallback requirement) |
| 59-01-SUMMARY.md | AGTS-01, AGTS-02, AGTS-03 | Plan 01 built `extract_handoff` (AGTS-01), `AgentSessionDb` (AGTS-02), `format_handoff_as_user_message` (AGTS-03) |
| 59-02-SUMMARY.md | AGTS-04 | Plan 02 wired prior handoff injection in the writer thread, enabling chained sessions (AGTS-04) |

**Total: 13 REQ-IDs across 8 files.**

Files that already have `requirements-completed` and must NOT be touched:
- 48-03-SUMMARY.md (has SOIP-04, SOIP-05, SOIP-06)
- 56-02-SUMMARY.md (has AGTR-01, AGTR-02, AGTR-04, AGTR-05, AGTR-06, AGTR-07)
- 57-02-SUMMARY.md (has AGTW-01, AGTW-02, AGTW-03, AGTW-04, AGTW-05)

## Validation Architecture

> nyquist_validation is enabled (true in .planning/config.json).

### Test Framework
| Property | Value |
|----------|-------|
| Framework | cargo test (Rust built-in) |
| Config file | none (workspace-level) |
| Quick run command | `cargo test -p glass_soi -- --test-output immediate 2>&1 \| head -5` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements → Test Map

This phase has no code requirements (metadata-only). No automated tests exist or are needed for doc comment text or YAML frontmatter content.

| Task | Behavior | Test Type | Automated Command | Notes |
|------|----------|-----------|-------------------|-------|
| Fix parse() doc comment | Doc text no longer references Phase 48 stubs | manual-only | verify by reading lib.rs lines 40-43 | No runtime behavior change; no test can assert doc text |
| Backfill SUMMARY.md frontmatter | requirements_completed fields present | manual-only | grep -l "requirements-completed" across 8 files | YAML frontmatter has no test harness |

**Manual verification commands:**
```bash
# Verify doc comment fixed
grep -n "Phase 48\|stubs\|Plans 48" crates/glass_soi/src/lib.rs
# Expected: no output

# Verify frontmatter backfilled (all 8 files)
grep -l "requirements-completed" .planning/phases/48-soi-classifier-and-parser-crate/*.md
grep -l "requirements-completed" .planning/phases/51-soi-compression-engine/*.md
grep -l "requirements-completed" .planning/phases/56-agent-runtime/*.md
grep -l "requirements-completed" .planning/phases/57-agent-worktree/*.md
grep -l "requirements-completed" .planning/phases/59-agent-session-continuity/*.md
```

### Sampling Rate
- **Per task commit:** `cargo test -p glass_soi` (confirms no accidental breakage)
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
None — no test infrastructure needed for metadata changes.

## Open Questions

1. **Test comment in lib.rs (line 98)**
   - What we know: `// Stub: returns freeform with RustCompiler type` is also stale (the parser is now real)
   - What's unclear: Whether success criterion 1 implies fixing all stale comments or only the function-level doc
   - Recommendation: Treat as out of scope. Success criterion 1 says "doc comment no longer references Phase 48 stubs" — the function-level `///` comment is the one that says this. The `//` test comment is a separate, lower-priority cleanup. Do not fix it in this phase.

## Sources

### Primary (HIGH confidence)
- Direct file inspection: `crates/glass_soi/src/lib.rs` — confirmed stale comment at lines 40-43
- Direct file inspection: `.planning/phases/48-*/`, `51-*/`, `56-*/`, `57-*/`, `59-*/` SUMMARY.md files — confirmed missing `requirements-completed` keys
- `.planning/REQUIREMENTS.md` — authoritative REQ-ID definitions and traceability table
- `.planning/ROADMAP.md` Phase 62 entry — success criteria text

### Secondary (MEDIUM confidence)
- `.planning/phases/48-03-SUMMARY.md`, `56-02-SUMMARY.md`, `57-02-SUMMARY.md` — three reference files showing established `requirements-completed` hyphen-key convention

## Metadata

**Confidence breakdown:**
- Task scope (what to change): HIGH — directly verified by reading source files
- REQ-ID mapping: HIGH — derived from reading each plan's documented accomplishments against REQUIREMENTS.md
- YAML key naming convention: HIGH — verified from three existing files using the hyphen form
- No code risk: HIGH — metadata-only, no function signatures or logic touched

**Research date:** 2026-03-13
**Valid until:** 2026-04-13 (stable; these files won't change before the phase runs)
