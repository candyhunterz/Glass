# Requirements: Glass

**Defined:** 2026-03-05
**Core Value:** A terminal that looks and feels normal but passively watches, indexes, and snapshots everything -- surfacing intelligence only when you need it.

## v1.3 Requirements

Requirements for Pipe Visualization milestone. Each maps to roadmap phases.

### Pipe Parsing

- [x] **PIPE-01**: User's piped commands are detected and parsed into individual stages
- [x] **PIPE-02**: User can opt out of pipe capture per-command with `--no-glass` flag
- [x] **PIPE-03**: TTY-sensitive commands (less, vim, fzf, git log) are auto-excluded from interception

### Capture Engine

- [x] **CAPT-01**: Byte-stream capture points inserted between bash/zsh pipe stages via tee-based rewriting
- [x] **CAPT-02**: PowerShell pipe stages captured via post-hoc string representation after pipeline completes
- [x] **CAPT-03**: Per-stage buffer capped at 10MB with head/tail sampling for overflow
- [x] **CAPT-04**: Binary data in pipe stages detected and shown as `[binary: <size>]`

### Pipeline UI

- [x] **UI-01**: Piped commands render as multi-row pipeline blocks showing each stage with line/byte count
- [x] **UI-02**: Pipeline blocks auto-expand on failure or >2 stages, collapse for simple success
- [x] **UI-03**: User can expand any stage to view its full intermediate output
- [x] **UI-04**: User can collapse/expand pipeline blocks with click or keyboard

### Storage & History

- [x] **STOR-01**: Pipe stage data stored in `pipe_stages` table linked to command_id in history.db
- [x] **STOR-02**: Stage data included in retention/pruning policies

### MCP Integration

- [x] **MCP-01**: `GlassPipeInspect(command_id, stage)` MCP tool returns intermediate output for a pipeline stage
- [x] **MCP-02**: `GlassContext` tool updated to include pipeline stats in activity summaries

### Configuration

- [x] **CONF-01**: `[pipes]` section in config.toml with enabled, max_capture_mb, and auto_expand settings

## Future Requirements

### Pipe Visualization Enhancements

- **PIPE-04**: Process substitution (`<()`, `>()`) and subshell pipes decomposed
- **PIPE-05**: Stderr captured separately and shown alongside stdout per stage
- **PIPE-06**: Stage-level timing (duration per pipe stage)
- **PIPE-07**: Export pipeline stage output to file

### PowerShell Deep Integration

- **PS-01**: PowerShell object type information displayed per stage
- **PS-02**: Object-to-table rendering for PowerShell pipeline stages

## Out of Scope

| Feature | Reason |
|---------|--------|
| Live streaming stage output during execution | Complexity of real-time tee rendering; show after completion |
| Pipeline stage editing/re-running | IDE-level feature, not terminal |
| Cross-shell pipe translation | Each shell's pipe semantics are fundamentally different |
| Pipeline performance profiling | Separate concern from visualization |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| PIPE-01 | Phase 15 | Complete |
| PIPE-02 | Phase 15 | Complete |
| PIPE-03 | Phase 15 | Complete |
| CAPT-01 | Phase 16 | Complete |
| CAPT-02 | Phase 16 | Complete |
| CAPT-03 | Phase 15 | Complete |
| CAPT-04 | Phase 15 | Complete |
| UI-01 | Phase 17 | Complete |
| UI-02 | Phase 17 | Complete |
| UI-03 | Phase 17 | Complete |
| UI-04 | Phase 17 | Complete |
| STOR-01 | Phase 18 | Complete |
| STOR-02 | Phase 18 | Complete |
| MCP-01 | Phase 19 | Complete |
| MCP-02 | Phase 19 | Complete |
| CONF-01 | Phase 19 | Complete |

**Coverage:**
- v1.3 requirements: 16 total
- Mapped to phases: 16
- Unmapped: 0

---
*Requirements defined: 2026-03-05*
*Last updated: 2026-03-05 after roadmap creation*
