# Roadmap: Glass

## Milestones

- [x] **v1.0 MVP** -- Phases 1-4 (shipped 2026-03-05)
- [x] **v1.1 Structured Scrollback + MCP Server** -- Phases 5-9 (shipped 2026-03-05)
- [x] **v1.2 Command-Level Undo** -- Phases 10-14 (shipped 2026-03-06)
- [x] **v1.3 Pipe Visualization** -- Phases 15-20 (shipped 2026-03-06)
- [ ] **v2.0 Cross-Platform & Tabs** -- Phases 21-24

## Phases

<details>
<summary>v1.0 MVP (Phases 1-4) -- SHIPPED 2026-03-05</summary>

- [x] Phase 1: Scaffold (3/3 plans) -- completed 2026-03-05
- [x] Phase 2: Terminal Core (3/3 plans) -- completed 2026-03-05
- [x] Phase 3: Shell Integration and Block UI (4/4 plans) -- completed 2026-03-05
- [x] Phase 4: Configuration and Performance (2/2 plans) -- completed 2026-03-05

</details>

<details>
<summary>v1.1 Structured Scrollback + MCP Server (Phases 5-9) -- SHIPPED 2026-03-05</summary>

- [x] Phase 5: History Database Foundation (2/2 plans) -- completed 2026-03-05
- [x] Phase 6: Output Capture + Writer Integration (4/4 plans) -- completed 2026-03-05
- [x] Phase 7: CLI Query Interface (2/2 plans) -- completed 2026-03-05
- [x] Phase 8: Search Overlay (2/2 plans) -- completed 2026-03-05
- [x] Phase 9: MCP Server (2/2 plans) -- completed 2026-03-05

</details>

<details>
<summary>v1.2 Command-Level Undo (Phases 10-14) -- SHIPPED 2026-03-06</summary>

- [x] Phase 10: Content Store + DB Schema (2/2 plans) -- completed 2026-03-05
- [x] Phase 11: Command Parser (2/2 plans) -- completed 2026-03-05
- [x] Phase 12: FS Watcher Engine (2/2 plans) -- completed 2026-03-06
- [x] Phase 13: Integration + Undo Engine (4/4 plans) -- completed 2026-03-06
- [x] Phase 14: UI + CLI + MCP + Pruning (3/3 plans) -- completed 2026-03-06

</details>

<details>
<summary>v1.3 Pipe Visualization (Phases 15-20) -- SHIPPED 2026-03-06</summary>

- [x] Phase 15: Pipe Parsing Core (2/2 plans) -- completed 2026-03-06
- [x] Phase 16: Shell Capture + Terminal Transport (3/3 plans) -- completed 2026-03-06
- [x] Phase 17: Pipeline UI (2/2 plans) -- completed 2026-03-06
- [x] Phase 18: Storage + Retention (1/1 plan) -- completed 2026-03-06
- [x] Phase 19: MCP + Config + Polish (1/1 plan) -- completed 2026-03-06
- [x] Phase 20: Config Gate + Dead Code Cleanup (2/2 plans) -- completed 2026-03-06

</details>

### v2.0 Cross-Platform & Tabs (Phases 21-24)

- [x] Phase 21: Session Extraction & Platform Foundation (3 plans) (completed 2026-03-06)
  **Goal:** Extract session state from WindowContext into SessionMux, add SessionId routing, platform cfg gates, and shell integration for zsh.
  **Requirements:** [P21-01, P21-02, P21-03, P21-04, P21-05, P21-06, P21-07, P21-08, P21-09, P21-10]
  **Plans:** 3 plans
  Plans:
  - [ ] 21-01-PLAN.md -- Create glass_mux crate with Session, SessionMux, types, and platform helpers
  - [x] 21-02-PLAN.md -- Add SessionId to AppEvent/EventProxy and create zsh shell integration
  - [ ] 21-03-PLAN.md -- Refactor WindowContext to use SessionMux, verify zero regression
- [x] Phase 22: Cross-Platform Validation (2 plans) (completed 2026-03-07)
  **Goal:** Fix cross-platform compilation blockers, add platform-aware defaults, surface format logging, ScaleFactorChanged handling, and establish CI pipeline.
  **Requirements:** [P22-01, P22-02, P22-03, P22-04, P22-05, P22-06, P22-07, P22-09, P22-10]
  **Plans:** 2 plans
  Plans:
  - [ ] 22-01-PLAN.md -- Platform compilation fixes (windows-sys gating, spawn_pty shell detection, shell integration injection, font defaults)
  - [ ] 22-02-PLAN.md -- Surface format logging, ScaleFactorChanged handler, CI workflow, cross-compilation validation
- [x] Phase 23: Tabs (3 plans) (completed 2026-03-07)
  **Goal:** Implement a wgpu-rendered tab bar with full tab lifecycle management. Each tab owns an independent terminal session with its own PTY, Term, BlockManager, HistoryDb, and SnapshotStore.
  **Requirements:** [TAB-01, TAB-02, TAB-03, TAB-04, TAB-05]
  **Plans:** 3 plans
  Plans:
  - [ ] 23-01-PLAN.md -- SessionMux tab CRUD methods and Tab title field
  - [ ] 23-02-PLAN.md -- TabBarRenderer and spawn_pty working_directory parameter
  - [ ] 23-03-PLAN.md -- Full main.rs integration: keyboard shortcuts, rendering, session lifecycle
- [ ] Phase 24: Split Panes -- not started

## Progress

| Phase | Milestone | Plans Complete | Status | Completed |
|-------|-----------|----------------|--------|-----------|
| 1. Scaffold | v1.0 | 3/3 | Complete | 2026-03-05 |
| 2. Terminal Core | v1.0 | 3/3 | Complete | 2026-03-05 |
| 3. Shell Integration and Block UI | v1.0 | 4/4 | Complete | 2026-03-05 |
| 4. Configuration and Performance | v1.0 | 2/2 | Complete | 2026-03-05 |
| 5. History Database Foundation | v1.1 | 2/2 | Complete | 2026-03-05 |
| 6. Output Capture + Writer Integration | v1.1 | 4/4 | Complete | 2026-03-05 |
| 7. CLI Query Interface | v1.1 | 2/2 | Complete | 2026-03-05 |
| 8. Search Overlay | v1.1 | 2/2 | Complete | 2026-03-05 |
| 9. MCP Server | v1.1 | 2/2 | Complete | 2026-03-05 |
| 10. Content Store + DB Schema | v1.2 | 2/2 | Complete | 2026-03-05 |
| 11. Command Parser | v1.2 | 2/2 | Complete | 2026-03-05 |
| 12. FS Watcher Engine | v1.2 | 2/2 | Complete | 2026-03-06 |
| 13. Integration + Undo Engine | v1.2 | 4/4 | Complete | 2026-03-06 |
| 14. UI + CLI + MCP + Pruning | v1.2 | 3/3 | Complete | 2026-03-06 |
| 15. Pipe Parsing Core | v1.3 | 2/2 | Complete | 2026-03-06 |
| 16. Shell Capture + Terminal Transport | v1.3 | 3/3 | Complete | 2026-03-06 |
| 17. Pipeline UI | v1.3 | 2/2 | Complete | 2026-03-06 |
| 18. Storage + Retention | v1.3 | 1/1 | Complete | 2026-03-06 |
| 19. MCP + Config + Polish | v1.3 | 1/1 | Complete | 2026-03-06 |
| 20. Config Gate + Dead Code Cleanup | v1.3 | 2/2 | Complete | 2026-03-06 |
| 21. Session Extraction & Platform Foundation | v2.0 | 3/3 | Complete | 2026-03-06 |
| 22. Cross-Platform Validation | 2/2 | Complete    | 2026-03-07 | -- |
| 23. Tabs | 3/3 | Complete   | 2026-03-07 | -- |
| 24. Split Panes | v2.0 | 0/? | Not Started | -- |
