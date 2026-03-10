# Requirements: Glass

**Defined:** 2026-03-09
**Core Value:** A terminal that looks and feels normal but passively watches, indexes, and snapshots everything -- surfacing intelligence only when you need it.

## v2.3 Requirements

Requirements for milestone v2.3 Agent MCP Features. Each maps to roadmap phases.

### MCP Infrastructure

- [x] **INFRA-01**: MCP server can send requests to the main event loop and receive responses via async channel
- [x] **INFRA-02**: Main event loop processes MCP requests without blocking rendering or keyboard input

### Tab Orchestration

- [x] **TAB-01**: Agent can create a new tab with optional shell and working directory via MCP
- [x] **TAB-02**: Agent can list all open tabs with their state (name, cwd, running command) via MCP
- [x] **TAB-03**: Agent can send a command to a specific tab's PTY via MCP
- [x] **TAB-04**: Agent can read the last N lines of output from a specific tab, with optional regex filtering, via MCP
- [x] **TAB-05**: Agent can close a tab via MCP (refuses to close the last tab)
- [x] **TAB-06**: Tab tools accept both numeric tab_id and stable session_id as identifiers

### Token Saving

- [x] **TOKEN-01**: Agent can retrieve filtered command output (by pattern, line count, head/tail) via MCP
- [x] **TOKEN-02**: Agent can check if a previous command's result is still valid (cached result with file-change staleness detection) via MCP
- [x] **TOKEN-03**: Agent can see which files a command modified with unified diffs via MCP
- [x] **TOKEN-04**: Agent can request compressed context with a token budget and focus mode via MCP

### Error Extraction

- [ ] **ERR-01**: Agent can extract structured errors (file, line, column, message, severity) from command output via MCP
- [ ] **ERR-02**: Rust parser handles both human-readable and `--error-format=json` compiler output
- [ ] **ERR-03**: Generic fallback parser handles `file:line:col: message` patterns from any compiler
- [ ] **ERR-04**: Parser auto-detects language from command text hint and output content

### Live Awareness

- [ ] **LIVE-01**: Agent can check whether a command is currently running in a tab via MCP
- [ ] **LIVE-02**: Agent can cancel a running command (send SIGINT) in a tab via MCP

## Future Requirements

### Additional Error Parsers

- **ERR-05**: Python traceback parser with stack frame extraction
- **ERR-06**: Node.js/TypeScript error parser
- **ERR-07**: Go compiler error parser
- **ERR-08**: GCC/Clang error parser

### Advanced Token Saving

- **TOKEN-05**: Output delta tracking between polls (per-caller state)
- **TOKEN-06**: Exact token counting via tokenizer integration

## Out of Scope

| Feature | Reason |
|---------|--------|
| Built-in AI command suggestion | Glass exposes data TO agents, not an agent itself |
| Streaming output via MCP | MCP stdio transport doesn't support server-initiated streaming; polling with has_running_command flag sufficient |
| Automatic error correction | Crosses into agent territory; return structured data, let agent decide |
| Persistent named sessions across restarts | Session persistence adds complexity; ephemeral tabs match agent workflow |
| Remote MCP transport (HTTP/SSE) | Security attack surface for command execution; stdio is a security feature |
| Full shell AST parsing | Shell syntax is Turing-complete; regex heuristics are practical |
| Exact token counting in budget mode | Requires tokenizer dep; char-based approximation (1 token ~ 4 chars) sufficient |
| Tab output diffing (delta between polls) | Per-caller state management complexity; agents can diff locally |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| INFRA-01 | Phase 35 | Complete |
| INFRA-02 | Phase 35 | Complete |
| TAB-01 | Phase 36 | Complete |
| TAB-02 | Phase 36 | Complete |
| TAB-03 | Phase 36 | Complete |
| TAB-04 | Phase 36 | Complete |
| TAB-05 | Phase 36 | Complete |
| TAB-06 | Phase 36 | Complete |
| TOKEN-01 | Phase 37 | Complete |
| TOKEN-02 | Phase 37 | Complete |
| TOKEN-03 | Phase 37 | Complete |
| TOKEN-04 | Phase 37 | Complete |
| ERR-01 | Phase 38 | Pending |
| ERR-02 | Phase 38 | Pending |
| ERR-03 | Phase 38 | Pending |
| ERR-04 | Phase 38 | Pending |
| LIVE-01 | Phase 39 | Pending |
| LIVE-02 | Phase 39 | Pending |

**Coverage:**
- v2.3 requirements: 16 total
- Mapped to phases: 16
- Unmapped: 0

---
*Requirements defined: 2026-03-09*
*Last updated: 2026-03-09 after initial definition*
