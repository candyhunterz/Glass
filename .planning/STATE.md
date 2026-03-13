---
gsd_state_version: 1.0
milestone: v3.0
milestone_name: SOI & Agent Mode
status: planning
stopped_at: Completed 58-01-PLAN.md
last_updated: "2026-03-13T17:21:42.167Z"
last_activity: 2026-03-13 -- completed Phase 53 SOI MCP Tools
progress:
  total_phases: 13
  completed_phases: 10
  total_plans: 23
  completed_plans: 22
  percent: 100
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-13)

**Core value:** A terminal that looks and feels normal but passively watches, indexes, and snapshots everything -- surfacing intelligence only when you need it.
**Current focus:** Phase 54 -- SOI Extended Parsers

## Current Position

Phase: 54 of 60 (SOI Extended Parsers) -- Ready to plan
Plan: Not started
Status: Ready to plan
Last activity: 2026-03-13 -- completed Phase 53 SOI MCP Tools

Progress: [████████████████████] 13/13 plans (100%) (v3.0: 7/13 phases)

## Performance Metrics

**Velocity (cumulative):**
- v1.0: 12 plans in ~1.8 hours (~9 min/plan)
- v1.1: 12 plans in ~4.5 hours (~20 min/plan)
- v1.2: 13 plans in ~6 hours (~28 min/plan)
- v1.3: 11 plans in ~2 hours (~11 min/plan)
- v2.0: 12 plans in ~23 min (~4 min/plan)
- v2.1: 11 plans in ~23 min (~3 min/plan)
- v2.2: 8 plans in ~30 min (~4 min/plan)
- v2.3: 9 plans in ~35 min (~4 min/plan)
- v2.4: 7 plans in ~25 min (~4 min/plan)
- v2.5: 6 plans in ~10 min (~2 min/plan)
- Total: 101 plans across 47 phases in 8 days

## Accumulated Context

### Decisions

See PROJECT.md Key Decisions table for full history.

Recent decisions relevant to v3.0:
- SOI summaries rendered as block decorations (NOT injected into PTY stream) to avoid OSC 133 race condition
- SOI parsing runs in spawn_blocking off main thread -- criterion input_latency benchmark must not regress
- Agent runtime is a struct in Processor (not a separate process) -- matches existing coordination poller pattern
- Approval UI is non-modal (toast + hotkeys) -- never captures keyboard focus from terminal
- max_budget_usd = 1.0 USD default is non-negotiable -- ships in Phase 56, not deferred
- git worktree registered in SQLite BEFORE creation -- crash recovery pattern from opencode PR #14649
- New crates needed: glass_soi, glass_agent; new deps: uuid 1.22, git2 0.20
- [Phase 48]: SOI Severity enum (Error/Warning/Info/Success) differs from glass_errors::Severity intentionally -- outcome-oriented scale for AI consumption
- [Phase 48]: OutputRecord is an enum not a trait object -- zero-cost dispatch and easy serde serialization
- [Phase 48]: All Phase 54 OutputType command-hint arms wired now in classifier -- future phases only add parser implementations
- [Phase 48]: cargo_build::parse delegates to glass_errors::extract_errors with Note/Help -> Info severity mapping
- [Phase 48]: cargo_test::parse chains to cargo_build::parse on compilation failure (no 'running N tests' line)
- [Phase 48]: Duration in cargo test is on summary line not separate line -- extracted from same regex match
- [Phase 48]: npm multi-match-per-line: do NOT use continue after first match since single lines can contain both added and audited counts
- [Phase 48]: jest test fixtures use concat!() macro not Rust line-continuation to preserve leading whitespace required by indented test line regex
- [Phase 49]: Severity strings use explicit match arms not Debug format in soi.rs for future rename safety
- [Phase 49]: OutputType strings use Debug format (stable single-word identifiers like "RustCompiler")
- [Phase 49]: Dynamic WHERE clause in get_output_records uses numbered positional params with Box<dyn ToSql> vec
- [Phase 49-02]: Explicit DELETE loops added BEFORE commands_fts/commands deletion to match pipe_stages pattern -- guards against orphans if CASCADE is disabled
- [Phase 50]: get_output_for_command uses Option<Option<String>> + flatten() to handle NULL output column in rusqlite
- [Phase 50]: AppEvent::SoiReady.severity is String not glass_soi::Severity to keep glass_core dep-free of glass_soi
- [Phase 50-02]: soi_spawn_data declared before session borrow block (let mut = None), populated inside -- avoids borrow conflicts while keeping data for post-borrow spawn
- [Phase 50-02]: bench_input_processing uses Some(Vec<u8>) not &[u8] to match actual process_output API signature
- [Phase 51-01]: compress() uses serde_json::Value not glass_soi::OutputRecord to avoid tight coupling and future enum churn
- [Phase 51-01]: Full budget populates record_ids (all IDs) for symmetry with greedy path even though truncated=false
- [Phase 51-01]: OneLine budget uses empty record_ids (not useful for drill-down at single-line granularity)
- [Phase 51]: FreeformChunk excluded from fingerprinting -- no stable identity; diff_compress uses None vs Some-empty for distinct messages
- [Phase 52-soi-display]: SoiSection uses Option<SoiSection> in GlassConfig (None when absent) matching PipesSection pattern
- [Phase 52-soi-display]: SOI label placed at x=cell_width*1.0 left-anchored to avoid right-side badge/duration/undo collisions
- [Phase 52-soi-display]: build_soi_hint_line is a pure module-level function (not method) so it is unit-testable without BlockManager state
- [Phase 52-soi-display]: rev().find(Complete) used instead of current_block_mut() to handle SoiReady-arrives-after-PromptStart race condition
- [Phase 53-02]: Severity stored as capitalized strings in DB (Error/Info/Warning/Success) -- assert capitalized form in MCP tests, not lowercase
- [Phase 53-02]: balanced mode in glass_compressed_context now splits budget into quarters (was thirds) to give SOI equal share alongside errors/history/files
- [Phase 53-soi-mcp-tools]: TestResult regression detection inspects JSON data column for status=Failed (severity is always None for TestResult records in DB)
- [Phase 53-soi-mcp-tools]: glass_query_drill uses inline SQL with .optional() not a HistoryDb method -- one-off lookup not worth a public API method
- [Phase 54-01]: BuildKit step lines filtered by Dockerfile instruction keywords to avoid capturing DONE/CACHED timing lines
- [Phase 54-01]: Docker/kubectl receive NO content sniffers -- hint-only classification sufficient for devops tools
- [Phase 54-02]: go_test chains to go_build::parse on compilation failure (no === RUN or ok/FAIL lines) — mirrors cargo_test -> cargo_build chain pattern
- [Phase 54-02]: JSON lines parser requires >= 2 valid JSON lines for JsonLines output type — single JSON object in output falls through to freeform to avoid false positives
- [Phase 55-01]: ActivityFilter collapses only Success/Info -- Error/Warning always pass through as actionable signals
- [Phase 55-01]: pending_collapsed retroactively updates last window event collapsed_count on fingerprint change (lazy collapse)
- [Phase 55-agent-activity-stream]: activity_stream_rx marked #[allow(dead_code)] -- Phase 56 agent runtime will .take() it; avoids spurious clippy warning
- [Phase 55-agent-activity-stream]: Activity filter call placed AFTER if-let-Some(ctx) block so owned summary/severity still available for process() which takes ownership
- [Phase 56]: AgentMode derives Default with #[default] on Off variant; CooldownTracker uses Option<Instant> (zero deps); BudgetTracker plain f64 (single-threaded Processor); extract_proposal brace-depth walker (no regex dep); windows-sys features extended in Plan 01 to avoid 2nd Cargo.toml edit
- [Phase 56-agent-runtime]: AgentSection added to GlassConfig (mode/budget/cooldown/tools defaults to Off)
- [Phase 56-agent-runtime]: try_spawn_agent checks claude binary gracefully -- returns None if not found (AGTR-04)
- [Phase 56-agent-runtime]: Writer thread inline cooldown avoids Arc<Mutex> across thread boundary
- [Phase 57-01]: WorktreeDb uses &mut self for write methods; WorktreeManager wraps in RefCell for interior mutability from &self callers
- [Phase 57-01]: create_worktree_inner creates base_dir before git worktree add (git2 requires parent to exist on Windows)
- [Phase 57-02]: agent_pending_proposals replaced by agent_proposal_worktrees pairing proposals with Option<WorktreeHandle> for Phase 58 approval UI
- [Phase 57-02]: file_changes defaults to empty Vec when files key absent -- backward compatible with Phase 56 proposals
- [Phase 58-01]: ProposalToastRenderer/ProposalOverlayRenderer are stateless pure-computation helpers following ConflictOverlay pattern -- no GPU state, unit-testable without wgpu
- [Phase 58-01]: draw_multi_pane_frame renders proposal toast/overlay window-global after all panes -- per-plan spec
- [Phase 58-01]: build_status_text gains agent_mode_text/proposal_count_text optional params -- fully backward compatible, None defaults

### Pending Todos

1 pending (Mouse drag-and-select for copy paste).

### Blockers/Concerns

- Claude CLI JSON wire protocol schema needs validation before Phase 56 (may be moving target)
- git2 0.20 Windows path handling with spaces/non-ASCII not explicitly tested (Phase 57 risk)
- MCP tool token footprint of 28 tools unmeasured (25 base + 3 SOI query tools from Phase 53)
- macOS/Windows code signing still deferred
- pruner.rs max_size_mb not enforced (minor)

## Session Continuity

Last session: 2026-03-13T17:21:35.865Z
Stopped at: Completed 58-01-PLAN.md
Resume file: None
