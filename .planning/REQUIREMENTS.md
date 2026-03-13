# Requirements: Glass v3.0

**Defined:** 2026-03-12
**Core Value:** A terminal that looks and feels normal but passively watches, indexes, and snapshots everything -- surfacing intelligence only when you need it.

## v3.0 Requirements

Requirements for SOI & Agent Mode milestone. Each maps to roadmap phases.

### SOI Parsing

- [x] **SOIP-01**: SOI classifier detects output type from command text and content patterns (Rust compiler, test runner, package manager, git, docker, kubectl, structured data, freeform)
- [x] **SOIP-02**: Rust/cargo compiler error parser extracts file, line, column, severity, error code, message from cargo build/clippy output
- [x] **SOIP-03**: Rust/cargo test parser extracts test name, status (passed/failed/ignored), duration, failure message from cargo test output
- [x] **SOIP-04**: npm/Node parser extracts package events (added, removed, audited, vulnerabilities) from npm install/update output
- [x] **SOIP-05**: pytest parser extracts test name, status, duration, failure message from pytest output
- [x] **SOIP-06**: jest parser extracts test suite results, individual test status, failure diffs from jest output

### SOI Storage

- [x] **SOIS-01**: Parsed output records persist in SQLite tables (command_output_records, output_records) linked to existing commands table
- [x] **SOIS-02**: Schema migration from v2 to v3 runs automatically on startup using existing PRAGMA user_version pattern
- [x] **SOIS-03**: Individual records are queryable by command_id, severity, file path, and record type
- [x] **SOIS-04**: Retention/pruning of SOI records cascades with existing history retention policies

### SOI Pipeline

- [x] **SOIL-01**: SOI parsing runs automatically on every CommandFinished event without user intervention
- [x] **SOIL-02**: SOI parsing runs off the main thread (spawn_blocking) with no impact on terminal input latency
- [x] **SOIL-03**: SoiReady event emits after parsing completes, carrying command_id, summary, and severity
- [x] **SOIL-04**: Edge cases handled: no output, alt-screen apps, very large output (>50KB), binary output

### SOI Compression

- [x] **SOIC-01**: Compression engine produces summaries at 4 token-budget levels: OneLine (~10 tokens), Summary (~100), Detailed (~500), Full (~1000+)
- [x] **SOIC-02**: Smart truncation prioritizes errors over warnings, recent over old within budget
- [x] **SOIC-03**: Drill-down support returns record IDs for expanding specific items to full detail
- [x] **SOIC-04**: Diff-aware compression produces "compared to last run" change summaries

### SOI Display

- [x] **SOID-01**: SOI one-line summary renders as block decoration on completed command blocks
- [x] **SOID-02**: Shell summary hint line injected into PTY output stream for agent Bash tool discovery (configurable, respects min-lines threshold)
- [x] **SOID-03**: SOI display configurable via [soi] config section (enabled, shell_summary, format)

### SOI MCP Tools

- [ ] **SOIM-01**: glass_query tool returns structured output by command_id/scope/file/budget with token-budgeted response
- [ ] **SOIM-02**: glass_query_trend tool compares last N runs of same command pattern, detecting regressions
- [ ] **SOIM-03**: glass_query_drill tool expands specific record_id to full detail (context lines, stack trace)
- [x] **SOIM-04**: glass_context and glass_compressed_context updated to include SOI summaries

### SOI Extended Parsers

- [ ] **SOIX-01**: Git parser extracts action, files changed, insertions/deletions from git status/diff/log/merge/pull output
- [ ] **SOIX-02**: Docker parser extracts build progress, errors, compose events from docker build/compose output
- [ ] **SOIX-03**: kubectl parser extracts pod status, apply results, describe output from kubectl commands
- [ ] **SOIX-04**: TypeScript/tsc parser extracts file, line, column, error code, message from tsc output
- [ ] **SOIX-05**: Go compiler and test parser extracts build errors and test results from go build/test output
- [ ] **SOIX-06**: Generic JSON lines parser handles NDJSON/structured logging output

### Agent Activity Stream

- [ ] **AGTA-01**: Activity stream feeds compressed SOI summaries to agent runtime via bounded channel
- [ ] **AGTA-02**: Rolling budget window constrains activity context to configurable token limit (default 4096)
- [ ] **AGTA-03**: Noise filtering deduplicates and collapses repetitive success events
- [ ] **AGTA-04**: Rate limiting prevents flooding on rapid command execution

### Agent Runtime

- [ ] **AGTR-01**: Background Claude CLI process spawns with custom system prompt and MCP tool access
- [ ] **AGTR-02**: Agent receives activity stream via stdin (JSON lines protocol) and outputs proposals via stdout
- [ ] **AGTR-03**: Three autonomy modes: Watch (critical issues only), Assist (suggestions), Autonomous (proposes fixes)
- [ ] **AGTR-04**: Agent process lifecycle managed: start, restart on crash, graceful shutdown on app exit
- [ ] **AGTR-05**: Platform subprocess management: Windows Job Objects, Unix prctl for cleanup on crash
- [ ] **AGTR-06**: Cooldown timer prevents proposal spam (configurable, default 30s)
- [ ] **AGTR-07**: max_budget_usd enforced with non-unlimited default (1.0 USD) and status bar cost display

### Agent Worktree

- [ ] **AGTW-01**: WorktreeManager creates isolated git worktrees for agent code changes
- [ ] **AGTW-02**: Unified diff generated between worktree and main working tree for review
- [ ] **AGTW-03**: Apply copies changed files from worktree to working tree on user approval
- [ ] **AGTW-04**: Cleanup removes worktree after apply or dismiss
- [ ] **AGTW-05**: Crash recovery via SQLite-registered pending worktrees pruned on startup
- [ ] **AGTW-06**: Non-git projects fall back to temp directory with file copies

### Agent Approval UI

- [ ] **AGTU-01**: Status bar shows agent mode indicator and pending proposal count
- [ ] **AGTU-02**: Toast notification appears for new proposals with auto-dismiss and keyboard shortcut hint
- [ ] **AGTU-03**: Review overlay (Ctrl+Shift+A) shows scrollable proposal list with diff preview
- [ ] **AGTU-04**: Keyboard-driven approval: accept, reject, and dismiss actions on proposals
- [ ] **AGTU-05**: Approval UI is non-blocking -- terminal remains interactive while proposals are pending

### Agent Session Continuity

- [ ] **AGTS-01**: Agent produces structured handoff summary before session ends (context exhaustion, timeout)
- [ ] **AGTS-02**: Handoff stored in agent_sessions table with work completed, remaining, key decisions
- [ ] **AGTS-03**: New agent session loads most recent handoff as initial context
- [ ] **AGTS-04**: Multiple sequential sessions form a chain of handoffs with context compaction

### Agent Configuration

- [ ] **AGTC-01**: Full [agent] config section in config.toml with hot-reload support
- [ ] **AGTC-02**: Permission matrix: approve/auto/never per action type (edit_files, run_commands, git_operations)
- [ ] **AGTC-03**: Quiet rules: ignore specific commands, ignore successful commands (exit 0)
- [ ] **AGTC-04**: Graceful degradation when Claude CLI is unavailable (disable agent mode with config hint)
- [ ] **AGTC-05**: Agent integrates with glass_coordination for advisory lock management on session start/stop

## Future Requirements

Deferred to v3.x or later. Tracked but not in current roadmap.

### SOI Enhancements

- **SOIE-01**: SOI per-stage parsing for pipe stage intermediate output
- **SOIE-02**: SOI parser plugin system for user-defined parsers
- **SOIE-03**: SOI trend anomaly detection (build time regressions, flaky test detection)
- **SOIE-04**: FTS5 on structured record messages (after storage impact measured)

### Agent Enhancements

- **AGTE-01**: Agent multi-model routing (Haiku for watch, Sonnet for autonomous)
- **AGTE-02**: Agent PR/branch creation (requires GitHub integration)
- **AGTE-03**: Agent metrics dashboard (proposals made/applied/dismissed)
- **AGTE-04**: glass agent status CLI subcommand

## Out of Scope

| Feature | Reason |
|---------|--------|
| Built-in AI chat | Glass exposes data TO AI assistants via MCP, not an AI itself |
| Auto-apply without approval by default | Trust must be earned; approval-gated default is industry consensus |
| FTS5 on raw output content | Storage explosion risk; SOI structured records are the searchable layer |
| Networked/cloud MCP transport | Security surface expansion; stdio sufficient for local AI |
| Agent running 24/7 continuously | API cost spirals; event-driven with cooldown is correct pattern |
| Real-time streaming SOI parsing | Full output needed for accurate summaries; parse on CommandFinished |
| Parser for every CLI tool | Diminishing returns; FreeformChunk fallback for unrecognized output |
| Agent permission to push/open PRs | Cross-platform repo hosting assumptions; user pushes manually |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| SOIP-01 | Phase 48 | Complete |
| SOIP-02 | Phase 48 | Complete |
| SOIP-03 | Phase 48 | Complete |
| SOIP-04 | Phase 48 | Complete |
| SOIP-05 | Phase 48 | Complete |
| SOIP-06 | Phase 48 | Complete |
| SOIS-01 | Phase 49 | Complete |
| SOIS-02 | Phase 49 | Complete |
| SOIS-03 | Phase 49 | Complete |
| SOIS-04 | Phase 49 | Complete |
| SOIL-01 | Phase 50 | Complete |
| SOIL-02 | Phase 50 | Complete |
| SOIL-03 | Phase 50 | Complete |
| SOIL-04 | Phase 50 | Complete |
| SOIC-01 | Phase 51 | Complete |
| SOIC-02 | Phase 51 | Complete |
| SOIC-03 | Phase 51 | Complete |
| SOIC-04 | Phase 51 | Complete |
| SOID-01 | Phase 52 | Complete |
| SOID-02 | Phase 52 | Complete |
| SOID-03 | Phase 52 | Complete |
| SOIM-01 | Phase 53 | Pending |
| SOIM-02 | Phase 53 | Pending |
| SOIM-03 | Phase 53 | Pending |
| SOIM-04 | Phase 53 | Complete |
| SOIX-01 | Phase 54 | Pending |
| SOIX-02 | Phase 54 | Pending |
| SOIX-03 | Phase 54 | Pending |
| SOIX-04 | Phase 54 | Pending |
| SOIX-05 | Phase 54 | Pending |
| SOIX-06 | Phase 54 | Pending |
| AGTA-01 | Phase 55 | Pending |
| AGTA-02 | Phase 55 | Pending |
| AGTA-03 | Phase 55 | Pending |
| AGTA-04 | Phase 55 | Pending |
| AGTR-01 | Phase 56 | Pending |
| AGTR-02 | Phase 56 | Pending |
| AGTR-03 | Phase 56 | Pending |
| AGTR-04 | Phase 56 | Pending |
| AGTR-05 | Phase 56 | Pending |
| AGTR-06 | Phase 56 | Pending |
| AGTR-07 | Phase 56 | Pending |
| AGTW-01 | Phase 57 | Pending |
| AGTW-02 | Phase 57 | Pending |
| AGTW-03 | Phase 57 | Pending |
| AGTW-04 | Phase 57 | Pending |
| AGTW-05 | Phase 57 | Pending |
| AGTW-06 | Phase 57 | Pending |
| AGTU-01 | Phase 58 | Pending |
| AGTU-02 | Phase 58 | Pending |
| AGTU-03 | Phase 58 | Pending |
| AGTU-04 | Phase 58 | Pending |
| AGTU-05 | Phase 58 | Pending |
| AGTS-01 | Phase 59 | Pending |
| AGTS-02 | Phase 59 | Pending |
| AGTS-03 | Phase 59 | Pending |
| AGTS-04 | Phase 59 | Pending |
| AGTC-01 | Phase 60 | Pending |
| AGTC-02 | Phase 60 | Pending |
| AGTC-03 | Phase 60 | Pending |
| AGTC-04 | Phase 60 | Pending |
| AGTC-05 | Phase 60 | Pending |

**Coverage:**
- v3.0 requirements: 62 total (note: REQUIREMENTS.md header said 52 -- actual count from requirement definitions is 62)
- Mapped to phases: 62
- Unmapped: 0

---
*Requirements defined: 2026-03-12*
*Last updated: 2026-03-12 after roadmap creation (phases 48-60)*
