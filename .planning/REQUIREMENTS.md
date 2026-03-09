# Requirements: Glass v2.2

**Defined:** 2026-03-09
**Core Value:** A terminal that looks and feels normal but passively watches, indexes, and snapshots everything -- surfacing intelligence only when you need it.

## v2.2 Requirements

Requirements for multi-agent coordination milestone. Each maps to roadmap phases.

### Coordination Infrastructure

- [x] **COORD-01**: Agent can register with name, type, project root, CWD, and PID — receives UUID
- [x] **COORD-02**: Agent can deregister, releasing all locks and preserving sent messages
- [x] **COORD-03**: Agent can send heartbeat to maintain liveness (60s interval, 10min timeout)
- [x] **COORD-04**: Stale agents are auto-pruned via heartbeat timeout or PID liveness check
- [x] **COORD-05**: Agent can atomically lock multiple files (all-or-nothing, returns conflicts if any held)
- [x] **COORD-06**: File paths are canonicalized before lock storage (dunce on Windows, lowercase on NTFS)
- [x] **COORD-07**: Agent can unlock specific files or release all locks
- [ ] **COORD-08**: Agent can broadcast a typed message to all agents in the same project
- [ ] **COORD-09**: Agent can send a directed message to a specific agent
- [ ] **COORD-10**: Agent can read unread messages (marks as read, preserves messages from deregistered senders)
- [x] **COORD-11**: Agents are scoped by project root — agents on different repos don't see each other's locks

### MCP Tools

- [ ] **MCP-01**: `glass_agent_register` tool registers agent and returns ID + active agent count
- [ ] **MCP-02**: `glass_agent_deregister` tool unregisters agent and cascades cleanup
- [ ] **MCP-03**: `glass_agent_list` tool lists active agents with auto-pruning
- [ ] **MCP-04**: `glass_agent_status` tool updates agent status and task description
- [ ] **MCP-05**: `glass_agent_lock` tool atomically claims advisory file locks
- [ ] **MCP-06**: `glass_agent_unlock` tool releases file locks
- [ ] **MCP-07**: `glass_agent_locks` tool lists all active locks across agents
- [ ] **MCP-08**: `glass_agent_broadcast` tool sends typed message to all project agents
- [ ] **MCP-09**: `glass_agent_send` tool sends directed message to specific agent
- [ ] **MCP-10**: `glass_agent_messages` tool reads unread messages
- [ ] **MCP-11**: `glass_agent_heartbeat` tool refreshes liveness timestamp
- [ ] **MCP-12**: All MCP tool calls implicitly refresh the calling agent's heartbeat

### Integration

- [ ] **INTG-01**: CLAUDE.md includes coordination protocol instructions for AI agents
- [ ] **INTG-02**: Multi-server integration test validates two MCP instances coordinating via shared DB
- [ ] **INTG-03**: Integration test validates lock conflict detection across agents

### GUI

- [ ] **GUI-01**: Status bar displays active agent count from coordination DB
- [ ] **GUI-02**: Status bar displays active lock count from coordination DB
- [ ] **GUI-03**: Background polling thread reads agents.db every 5 seconds with atomic state transfer
- [ ] **GUI-04**: Tab shows visual indicator when its agent holds file locks
- [ ] **GUI-05**: Conflict warning overlay appears when two agents touch the same file

## Future Requirements

### Coordination Enhancements

- **COORD-F01**: Directory-level or glob-pattern lock acquisition
- **COORD-F02**: Lock timeout/TTL with auto-expiration independent of heartbeat
- **COORD-F03**: Agent activity log for audit trail (beyond ephemeral messages)

### GUI Enhancements

- **GUI-F01**: Agent management panel (list agents, force-deregister, view locks)
- **GUI-F02**: Lock history timeline visualization

## Out of Scope

| Feature | Reason |
|---------|--------|
| Enforced file locking | Requires intercepting file writes at PTY level, fragile and platform-specific. Advisory locks work because AI agents follow instructions. |
| A2A protocol | Enterprise-scale agent-to-agent protocol is overkill for local coordination |
| Network agent discovery | SQLite requires same-host; network adds security concerns |
| MCP over network transport | stdio sufficient for local AI; network deferred per existing decision |
| Agent authentication | Trusted local environment; no need for auth between agents on same machine |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| COORD-01 | Phase 31 | Complete |
| COORD-02 | Phase 31 | Complete |
| COORD-03 | Phase 31 | Complete |
| COORD-04 | Phase 31 | Complete |
| COORD-05 | Phase 31 | Complete |
| COORD-06 | Phase 31 | Complete |
| COORD-07 | Phase 31 | Complete |
| COORD-08 | Phase 31 | Pending |
| COORD-09 | Phase 31 | Pending |
| COORD-10 | Phase 31 | Pending |
| COORD-11 | Phase 31 | Complete |
| MCP-01 | Phase 32 | Pending |
| MCP-02 | Phase 32 | Pending |
| MCP-03 | Phase 32 | Pending |
| MCP-04 | Phase 32 | Pending |
| MCP-05 | Phase 32 | Pending |
| MCP-06 | Phase 32 | Pending |
| MCP-07 | Phase 32 | Pending |
| MCP-08 | Phase 32 | Pending |
| MCP-09 | Phase 32 | Pending |
| MCP-10 | Phase 32 | Pending |
| MCP-11 | Phase 32 | Pending |
| MCP-12 | Phase 32 | Pending |
| INTG-01 | Phase 33 | Pending |
| INTG-02 | Phase 33 | Pending |
| INTG-03 | Phase 33 | Pending |
| GUI-01 | Phase 34 | Pending |
| GUI-02 | Phase 34 | Pending |
| GUI-03 | Phase 34 | Pending |
| GUI-04 | Phase 34 | Pending |
| GUI-05 | Phase 34 | Pending |

**Coverage:**
- v2.2 requirements: 31 total
- Mapped to phases: 31
- Unmapped: 0

---
*Requirements defined: 2026-03-09*
*Last updated: 2026-03-09 after roadmap creation*
