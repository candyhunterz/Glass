---
phase: 60-agent-configuration-and-polish
verified: 2026-03-13T19:10:00Z
status: passed
score: 10/10 must-haves verified
re_verification: false
---

# Phase 60: Agent Configuration and Polish Verification Report

**Phase Goal:** All SOI and agent behavior is configurable via config.toml with hot-reload, a permission matrix, quiet rules, and graceful degradation when Claude CLI is absent
**Verified:** 2026-03-13T19:10:00Z
**Status:** passed
**Re-verification:** No -- initial verification

## Goal Achievement

### Observable Truths

| #  | Truth | Status | Evidence |
|----|-------|--------|----------|
| 1  | PermissionMatrix with edit_files/run_commands/git_operations parses from TOML [agent.permissions] | VERIFIED | config.rs:30-50, config::tests::permission_matrix_full_toml passes |
| 2  | QuietRules with ignore_exit_zero and ignore_patterns parses from TOML [agent.quiet_rules] | VERIFIED | config.rs:53-61, config::tests::quiet_rules_full_toml passes |
| 3  | AgentSection backward compatible -- omitted sub-tables yield None | VERIFIED | config.rs:106-109, config::tests::agent_section_no_sub_tables_backward_compat passes |
| 4  | classify_proposal correctly categorizes proposals by file_changes and action prefix | VERIFIED | agent_runtime.rs:332-339, 3 classify_proposal tests pass |
| 5  | should_quiet correctly suppresses events matching ignore_patterns or ignore_exit_zero | VERIFIED | agent_runtime.rs:348-357, 5 should_quiet tests pass |
| 6  | Editing config.toml [agent] section hot-reloads and restarts agent runtime without Glass restart | VERIFIED | main.rs:3890-3954: agent_config_changed detection, None drop, fresh channel, respawn |
| 7  | Quiet rules suppress matching activity events before they reach the agent | VERIFIED | main.rs:4060-4084: should_quiet gates activity_filter.process() call in SoiReady arm |
| 8  | Permission matrix gates proposals: Never drops, Approve shows overlay, Auto applies immediately | VERIFIED | main.rs:4094-4191: classify_proposal + permission_level match, three branches implemented |
| 9  | Starting Glass with agent.mode != Off but no claude binary shows a clear config hint | VERIFIED | main.rs:1239-1247 and 3939-3947: config_error set with install URL hint |
| 10 | Agent session registers with glass_coordination on start and deregisters on stop; coordination failures are soft errors | VERIFIED | main.rs:941-981 (register+lock_files), main.rs:301-316 Drop (unlock_all+deregister), all wrapped in if let Ok() |

**Score:** 10/10 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `crates/glass_core/src/config.rs` | PermissionMatrix, QuietRules, PermissionLevel types with serde + Default | VERIFIED | Lines 4-61: all four types present, serde(rename_all="snake_case") on PermissionLevel, #[derive(Default)] on QuietRules, #[default] on Approve variant |
| `crates/glass_core/src/config.rs` | AgentSection extended with permissions and quiet_rules optional fields | VERIFIED | Lines 106-109: `pub permissions: Option<PermissionMatrix>` and `pub quiet_rules: Option<QuietRules>` with #[serde(default)] |
| `crates/glass_core/src/agent_runtime.rs` | classify_proposal and should_quiet helper functions | VERIFIED | Lines 332 and 348: both are `pub fn`, pure, no side effects, full test coverage |
| `src/main.rs` | Agent restart in ConfigReloaded, quiet filter in SoiReady, permission check in AgentProposal, coordination in try_spawn_agent/Drop, degradation hint | VERIFIED | All five wiring points present and substantive |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| config.rs | agent_runtime.rs | `use crate::config::{PermissionKind, QuietRules}` | WIRED | agent_runtime.rs line 1 imports both types; classify_proposal returns PermissionKind, should_quiet takes QuietRules |
| main.rs ConfigReloaded arm | try_spawn_agent | agent_config_changed comparison triggers drop + respawn | WIRED | main.rs:3900-3953: old_agent != self.config.agent -> self.agent_runtime = None -> try_spawn_agent |
| main.rs SoiReady arm | agent_runtime::should_quiet | quiet bool gates activity_filter.process() | WIRED | main.rs:4062-4084: should_quiet called, `if !quiet` guards activity stream send |
| main.rs AgentProposal arm | agent_runtime::classify_proposal | permission_level derived from PermissionKind match | WIRED | main.rs:4095-4106: classify_proposal -> match kind -> p.edit_files/run_commands/git_operations |
| main.rs try_spawn_agent | glass_coordination::CoordinationDb | register + lock_files on successful spawn | WIRED | main.rs:946-980: open_default() -> register() -> lock_files(), all soft-error wrapped |
| main.rs AgentRuntime Drop | glass_coordination::CoordinationDb | unlock_all + deregister on drop | WIRED | main.rs:303-316: if let Some(agent_id) -> open_default() -> unlock_all -> deregister |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| AGTC-01 | 60-01, 60-02 | Full [agent] config section in config.toml with hot-reload support | SATISFIED | AgentSection with new fields (60-01), agent_config_changed detection + restart (60-02 main.rs:3900) |
| AGTC-02 | 60-01, 60-02 | Permission matrix: approve/auto/never per action type | SATISFIED | PermissionMatrix type (60-01), AgentProposal arm enforcement (60-02 main.rs:4094-4191) |
| AGTC-03 | 60-01, 60-02 | Quiet rules: ignore specific commands, ignore successful commands | SATISFIED | QuietRules type + should_quiet (60-01), SoiReady filter (60-02 main.rs:4060-4084) |
| AGTC-04 | 60-02 | Graceful degradation when Claude CLI is unavailable | SATISFIED | main.rs:1239-1247 (initial start) and 3939-3947 (hot-reload restart) show config hint with install URL |
| AGTC-05 | 60-02 | Agent integrates with glass_coordination for advisory lock management | SATISFIED | try_spawn_agent registration (main.rs:941-981), Drop deregistration (main.rs:301-316), all failures soft |

No orphaned requirements. Plans 60-01 and 60-02 collectively claim AGTC-01 through AGTC-05, matching all five requirements assigned to Phase 60.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| agent_runtime.rs | 132 | "Returns..." in doc comment -- false positive grep match | Info | Not an anti-pattern; doc comment for BudgetTracker method |

No actual TODOs, FIXMEs, placeholders, or stub implementations found in modified files.

### Human Verification Required

#### 1. Hot-reload agent restart in live session

**Test:** Modify `~/.glass/config.toml` to change `[agent]` section (e.g., toggle mode) while Glass is running.
**Expected:** Agent runtime stops (old claude process killed) and restarts with new config within the file-watcher poll interval (~1s). No Glass window restart needed.
**Why human:** Hot-reload behavior requires a running Glass process and live file system event.

#### 2. Graceful degradation hint visibility

**Test:** Set `agent.mode = "watch"` in config.toml with claude CLI not on PATH. Launch Glass.
**Expected:** A visible config error overlay appears with message containing "claude CLI not found on PATH" and the install URL.
**Why human:** UI rendering of config_error overlay cannot be verified by grep -- requires visual inspection.

#### 3. Permission matrix Auto mode end-to-end

**Test:** Set `[agent.permissions] edit_files = "auto"` and trigger an agent file-edit proposal.
**Expected:** Files are applied immediately without user confirmation overlay. A brief "Auto-applied: ..." toast appears.
**Why human:** Requires running agent that produces a real AgentProposal event with file_changes.

#### 4. Quiet rules suppression in live session

**Test:** Set `[agent.quiet_rules] ignore_exit_zero = true` and run a command that exits 0.
**Expected:** Agent activity stream does NOT receive an event for that command; agent is not triggered.
**Why human:** Requires live shell interaction and verification that no agent context window update occurs.

### Gaps Summary

No gaps found. All 10 observable truths are verified, all 5 AGTC requirements are satisfied, all key links are wired, and the workspace builds cleanly (build + clippy -D warnings + fmt + 103 tests passing).

---

_Verified: 2026-03-13T19:10:00Z_
_Verifier: Claude (gsd-verifier)_
