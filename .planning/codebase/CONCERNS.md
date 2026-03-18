# Codebase Concerns

**Analysis Date:** 2026-03-18

## Tech Debt

**Large monolithic main event loop:**
- Issue: `src/main.rs` is 10,162 lines — all window events, session multiplexing, orchestrator routing, feedback loop enforcement, script bridging in single file
- Files: `src/main.rs`
- Impact: Difficult to test in isolation, high cognitive load, slow compilation, hard to extend without touching core loop
- Fix approach: Extract orchestrator event handling into separate module, pull out feedback loop enforcement into glass_feedback, move script bridge logic into dedicated handler struct

**Orchestrator state explosion:**
- Issue: `OrchestratorState` in `src/orchestrator.rs` tracks ~20+ fields (stuck detection, checkpoint phase, metric baseline, dependency blocks, feedback counters). State transitions happen implicitly in main event loop callbacks
- Files: `src/orchestrator.rs`, `src/main.rs`
- Impact: Hard to reason about state transitions, easy to miss invariants, difficult to test full sequences
- Fix approach: Separate orchestrator module from event handlers, define explicit transition graph, add state transition tests

**Unwrap/expect calls scattered throughout codebase:**
- Issue: 80 `.unwrap()` in `src/main.rs`, 1,296 across all crates. High concentration in config loading, file I/O, PTY operations
- Files: `src/main.rs`, `crates/glass_core/src/config.rs`, `crates/glass_terminal/src/pty.rs`
- Impact: Unhandled panics in production, especially at startup if configs corrupt or PTY setup fails
- Fix approach: Replace unwrap with error propagation in critical paths, use `.unwrap_or_default()` or `.ok()?` patterns, add validation tests for corrupted state

**Concurrency primitives without clear ownership:**
- Issue: Multiple threads spawned (PTY reader, artifact watcher, usage tracker, verify commands, ephemeral agent synthesis). Shared state via Arc/Mutex but ownership model not clearly documented
- Files: `src/main.rs`, `src/ephemeral_agent.rs`, `crates/glass_terminal/src/pty.rs`, `src/usage_tracker.rs`
- Impact: Potential data races in debug builds, deadlock risk if lock ordering violated, hard to debug multi-threaded issues
- Fix approach: Document thread ownership model in module-level comments, use static analysis to verify lock ordering, add thread-safety tests

## Known Bugs

**Orchestrator agent spawn initialization race:**
- Symptoms: Agent process spawned but stdin initialization delayed, causing first input dropped. Kickoff guard doesn't properly defer TypeText when agent boots
- Files: `src/ephemeral_agent.rs` (line ~30+), `src/main.rs` (orchestrator event handler, ~line 7000-7500)
- Trigger: Ctrl+Shift+O pressed → agent spawns → first Glass Agent response types but deferred_type_text not flushed until user idle
- Workaround: User must type again or wait longer for agent response to appear; CLI 2.1.77+ has partial fix
- Context: Documented in MEMORY.md as "Orchestrator agent spawn bugs (2026-03-17)"

**Checkpoint synthesis timeout fallback incomplete:**
- Symptoms: When ephemeral claude subprocess times out (120s), fallback content written but may lack critical git state if synthesis thread dies early
- Files: `src/checkpoint_synth.rs`, `src/main.rs` (checkpoint handling, ~line 8200+)
- Trigger: Long commit history, slow git operations, or network issues during synthesis
- Workaround: Manual git state review before resume
- Fix approach: Capture git state before spawning synthesis thread, merge fallback immediately on timeout

## Security Considerations

**Arbitrary command execution via orchestrator feedback:**
- Risk: Agent can respond with TypeText that gets typed into shell, enabling arbitrary command execution if prompt injection occurs
- Files: `src/orchestrator.rs` (parse_agent_response), `src/main.rs` (TypeText routing)
- Current mitigation: None — assumes Agent responses are always benign
- Recommendations: Add response sanitization for shell-special characters (`;`, `|`, `$()`, etc.), enable shell command preview before execution in future phases, rate-limit TypeText frequency

**Snapshot file restoration without permission verification:**
- Risk: Undo engine restores any snapshotted file without verifying current permissions or ownership match pre-snapshot state
- Files: `crates/glass_snapshot/src/undo.rs`, `crates/glass_snapshot/src/blob_store.rs`
- Current mitigation: Confidence scoring (Confidence::High hardcoded), but not enforced at file level
- Recommendations: Track file permissions/ownership in snapshot metadata, verify before restore, add user confirmation for permission changes

**Configuration loading from untrusted config.toml:**
- Risk: TOML parsing may fail silently or accept malformed input; no schema validation on agent orchestrator config
- Files: `crates/glass_core/src/config.rs` (1,312 lines)
- Current mitigation: Default values and `.unwrap_or_default()` chains mask errors
- Recommendations: Add comprehensive TOML schema validation (e.g., using JSON schema + serde), fail explicitly on critical config errors, log rejected fields

**OAuth token exposure in usage_tracker:**
- Risk: Token returned from usage API is stored in memory; if process crashes, unencrypted token may remain in core dump
- Files: `src/usage_tracker.rs`
- Current mitigation: Tokens are in-memory only, not persisted to disk
- Recommendations: Zero token memory after use, implement secure token storage for long-running sessions, add token rotation

## Performance Bottlenecks

**Block manager state machine transitions are O(n) linear scans:**
- Problem: BlockManager stores flat Vec<Block>. Finding current/previous blocks requires iteration through all blocks
- Files: `crates/glass_terminal/src/block_manager.rs` (1,101 lines)
- Cause: No indexing structure for current block or state transitions; every OSC event scans full history
- Improvement path: Cache current block index, maintain reverse index for O(1) lookup by epoch timestamp, profile before optimizing

**Frame composition recomputes entire grid every render frame:**
- Problem: `frame.rs` (2,619 lines) recomposes full terminal grid on every redraw, even when only scrollbar or status line changed
- Files: `crates/glass_renderer/src/frame.rs`
- Cause: No dirty-rect tracking; CPU-side grid generation before GPU upload
- Improvement path: Implement dirty-rect tracking, only recompose changed regions, consider GPU-side composition for static blocks

**History database FTS5 queries without result limiting:**
- Problem: `glass_history/src/db.rs` (1,464 lines) may return large result sets without pagination
- Files: `crates/glass_history/src/db.rs`
- Cause: UI history search displays up to 25 results, but DB query doesn't use LIMIT early, causing full scan
- Improvement path: Add LIMIT/OFFSET to all query methods, implement cursor-based pagination, benchmark with large history (1M+ commands)

**Snapshot blob deduplication requires full file read:**
- Problem: ContentAddressed store must read entire file to compute blake3 hash before checking if blob exists
- Files: `crates/glass_snapshot/src/blob_store.rs`
- Cause: No pre-computed hashes in metadata; only size-based dedup via filesystem
- Improvement path: Compute hash during file watch, cache in snapshot DB metadata, implement lazy hashing

## Fragile Areas

**Orchestrator state machine (10+ distinct states, 4+ transition types):**
- Files: `src/orchestrator.rs`, `src/main.rs`
- Why fragile: CheckpointPhase enum has 5 variants (None, Synthesizing, Synthesized, Written, Failed), each with different guards on silence detection, response_pending, and iteration counts. Adding new state type risks breaking existing transition flows
- Safe modification: Add explicit state transition tests in orchestrator tests, enumerate all valid transitions in comments, use type-level guarantees (e.g., builder pattern for state changes)
- Test coverage: Stuck detection tested, but transition graph NOT fully tested (e.g., Synthesizing→Failed→None recovery path)

**OSC event parsing in PTY reader thread:**
- Files: `crates/glass_terminal/src/osc_scanner.rs`, `crates/glass_terminal/src/pty.rs`
- Why fragile: Custom OSC sequence parsing regex-based; shell integration scripts must emit exact sequence format. Any shell update that changes OSC 133 encoding breaks block boundary detection
- Safe modification: Add comprehensive OSC parsing unit tests with malformed input, implement fallback for missing OSC sequences, version OSC extension format
- Test coverage: No malformed OSC tests; only happy-path tested

**Block lifecycle state machine (PromptActive → InputActive → Executing → Complete):**
- Files: `crates/glass_terminal/src/block_manager.rs`
- Why fragile: State transitions depend on OSC event ordering and timing. If shell integration script fails silently, blocks may remain in Executing state indefinitely
- Safe modification: Add timeout detection (if Executing > 5 min, auto-Complete), log state transition mismatches, add option to manually advance state
- Test coverage: Lifecycle tested in isolation, but not with interrupted/background commands

**Feedback rule enforcement in main event loop:**
- Files: `src/main.rs` (lines ~8095-8400)
- Why fragile: Rules trigger RuleAction::ForceCommit which runs git subprocess during event handling, potentially blocking event loop if git is slow
- Safe modification: Move rule enforcement to background task channel, add timeout for git operations, track pending rule actions in separate queue
- Test coverage: No tests for rule enforcement with slow git

## Scaling Limits

**Terminal grid rendering with large command history:**
- Current capacity: Tested up to ~100 blocks (10-50KB grid), stable at 60 FPS
- Limit: With 1000+ blocks, frame composition becomes O(n) costly. GPU memory usage grows quadratically with history length
- Scaling path: Implement block culling (only render visible blocks + overflow buffer), use sparse grid representation, add LOD (level-of-detail) rendering for collapsed blocks

**SQLite history database on projects with 100k+ commands:**
- Current capacity: FTS5 queries on ~50k commands responsive (< 1s)
- Limit: Full-text index size grows; WAL checkpoint overhead increases; concurrent access (history DB + agent coordination DB + snapshot DB) may saturate I/O
- Scaling path: Partition history by date ranges, implement async DB access, consider moving to separate database process or SQLite remote protocol

**PTY read buffer with high-throughput commands (cargo build, large file transfers):**
- Current capacity: 1MB read buffer (READ_BUFFER_SIZE = 0x10_0000) before forced terminal sync
- Limit: Very fast commands (>10MB/s output) may overflow buffer if PTY thread blocked
- Scaling path: Use adaptive buffer sizing based on terminal size, implement backpressure to PTY, benchmark with real workloads (large git clones, docker build output)

**Artifact watcher with large project (>100k files):**
- Current capacity: Fine for typical projects (< 10k files)
- Limit: notify crate may become slow with massive source trees; file watching has per-project overhead
- Scaling path: Implement gitignore-aware watching, add file count limits with warnings, consider explicit file list instead of recursive watching

## Dependencies at Risk

**alacritty_terminal = "=0.25.1" (pinned exact version):**
- Risk: Pinned to specific version to ensure deterministic behavior. Updates blocked indefinitely; if alacritty discovers security bugs, we can't easily update
- Impact: Long-term maintenance burden, potential future incompatibility with system VTE updates
- Migration plan: Monitor alacritty releases quarterly, document any blockers for upgrade, plan major version migration with compatibility testing

**wgpu 28.0 with metal/vulkan/dx12 backends:**
- Risk: GPU rendering may have platform-specific bugs; large dependency tree with native bindings
- Impact: Build failures on new OS versions (e.g., macOS Metal API changes), potential memory leaks in GPU resource management
- Migration plan: Test on each new OS release beta, monitor wgpu changelog, establish GPU memory profiling baseline

**Rhai scripting engine (newly added, single maintainer):**
- Risk: New feature relies on embedded scripting language; if Rhai becomes unmaintained, custom scripts become unsupported
- Impact: Users may write scripts that cannot be updated, feature becomes maintenance burden
- Migration plan: Keep Rhai integration minimal, implement script versioning, plan fallback to non-scripted execution

## Missing Critical Features

**No distributed multi-machine orchestration:**
- Problem: Orchestrator assumes single-machine setup. If agent spawns on different host (future federation), checkpoint.md and iterations.tsv live on different machines
- Blocks: Multi-host Glass clusters, distributed build orchestration

**No agent proposal review UI:**
- Problem: Agent can propose file edits via MCP, but no visual diff review before acceptance. Requires manual git review
- Blocks: Hands-off automation; users must verify every edit

**No performance regression detection across checkpoints:**
- Problem: Metric baseline only covers test pass/fail, not performance metrics (runtime, memory). Slow optimizations may pass verification but degrade system perf
- Blocks: Performance-sensitive projects (compilers, databases)

## Test Coverage Gaps

**Orchestrator state transitions with stuck detection + checkpoint + verify:**
- What's not tested: Scenario where agent gets stuck (3 identical responses) while synthesis in progress, then metric verify reverts (should it interrupt synthesis?)
- Files: `src/orchestrator.rs`, `src/main.rs`
- Risk: Undefined behavior if revert+checkpoint overlap
- Priority: High

**PTY read/write concurrency with high-frequency input + output:**
- What's not tested: Rapid keystrokes + streaming command output simultaneously; PTY buffer handling under contention
- Files: `crates/glass_terminal/src/pty.rs`
- Risk: Input dropped or output reordered
- Priority: High

**Snapshot undo with concurrent file modifications:**
- What's not tested: User modifies file, undo engine restores from snapshot, but file modified again before restore completes
- Files: `crates/glass_snapshot/src/undo.rs`
- Risk: Conflict detection incomplete; final state undefined
- Priority: Medium

**OSC 133 parsing with malformed or partial sequences:**
- What's not tested: Shell integration script emits broken OSC sequence, or network lag causes partial receive
- Files: `crates/glass_terminal/src/osc_scanner.rs`
- Risk: Block state machine hangs or misaligns
- Priority: Medium

**Feedback rule enforcement with git operations timing out:**
- What's not tested: Rule triggers RuleAction::ForceCommit, but git reset takes > 30s (slow disk)
- Files: `src/main.rs`
- Risk: Event loop freezes during git operation
- Priority: Medium

**Multi-agent coordination under lock contention:**
- What's not tested: Two agents both try glass_agent_lock on same files simultaneously; resolution order and message delivery
- Files: `crates/glass_coordination/src/db.rs`
- Risk: Deadlock or lost notifications
- Priority: Medium

---

*Concerns audit: 2026-03-18*
