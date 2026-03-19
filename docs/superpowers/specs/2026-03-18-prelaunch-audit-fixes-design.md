# Prelaunch Audit Fixes — Design Spec

**Date:** 2026-03-18
**Scope:** Fix all findings from 8 prelaunch audits (120+ findings across setup, bugs, cross-platform, UI/UX, performance, security, documentation, licensing)
**Strategy:** 8 branches, one per audit area, merged in dependency order
**Timeline:** Flexible — ship when ready

---

## Branch Structure & Merge Order

| Order | Branch | Focus | Est. Effort |
|-------|--------|-------|-------------|
| 1 | `audit/bugs-error-handling` | Panics, error propagation, failure modes | 5-6 days |
| 2 | `audit/security` | MCP auth, IPC, agent coordination, redaction | 3-4 days |
| 3 | `audit/cross-platform` | zsh/fish pipelines, macOS orphan, CI | 2-3 days |
| 4 | `audit/setup-packaging` | Shell script embedding, installers, first-run | 2-3 days |
| 5 | `audit/performance` | Render pipeline, memory, throttling | 5-6 days |
| 6 | `audit/ui-ux` | Onboarding, discoverability, polish | 6-8 days |
| 7 | `audit/documentation` | README, CHANGELOG, CONTRIBUTING, examples | 2-3 days |
| any | `audit/dependency-licensing` | License fields, attribution, cargo-deny | 1 day |

**Merge order rationale:**
- `bugs-error-handling` first: fixes panics other branches might hit while testing
- `security` second: permission model affects MCP tools referenced by cross-platform and setup
- `cross-platform` third: zsh/fish pipeline scripts must be complete before embedding
- `setup-packaging` fourth: embeds the now-complete shell integration scripts
- `performance` fifth: render pipeline refactors are independent of above
- `ui-ux` sixth: largest branch, benefits from all prior fixes being stable
- `documentation` last: needs to reflect all other changes
- `dependency-licensing` anytime: fully independent

**Process:**
- Each branch is a clean PR with CI passing (fmt + clippy + test) before merge
- After each merge, run full CI on the merged state before starting the next branch
- If a branch introduces regressions, revert before proceeding

---

## Branch 1: `audit/bugs-error-handling`

### Critical

**C-1: `cp_path.parent().unwrap()` panic** (`src/main.rs:2289,4749`)
- Replace with `if let Some(parent) = cp_path.parent() { let _ = std::fs::create_dir_all(parent); }`

**C-2: `session()`/`session_mut()` cascading panics** (`src/main.rs:234,241`)
- Change return type to `Option<&Session>` / `Option<&mut Session>`
- Introduce helper macro to reduce boilerplate at call sites:
  ```rust
  macro_rules! with_session {
      ($ctx:expr, $body:expr) => {
          if let Some(session) = $ctx.session() { $body }
      };
  }
  ```
- Apply across all call sites in the event loop

### High

**H-1/H-8: PTY spawn `expect()`** (`glass_terminal/src/pty.rs:203,221,235,258`)
- Return `Result` from `spawn_pty()`
- Show error in-window or native message box on failure
- Note: also covers H-8 (ConPTY init invisible on Windows due to `windows_subsystem`)

**H-3: `SystemTime` unwrap** (`glass_snapshot/src/pruner.rs:50`, `glass_history/src/retention.rs:16`)
- Replace with `.unwrap_or(Duration::ZERO)`

**H-4: Config watcher thread `expect()`** (`glass_core/src/config_watcher.rs:98`)
- Replace with `.ok()` + `tracing::warn!`

**H-5: 86 `let _ =` on PTY sends** (`src/main.rs` throughout)
- Check return value on PTY channel sends
- If channel closed, trigger `AppEvent::SessionDied` event
- Show "[Shell exited with code X]" message in terminal

**H-6: `child.kill()/wait()` errors** (`src/main.rs:438-439`)
- Log errors on Unix, keep silent on Windows (Job Object handles cleanup)

**H-7: No feedback when PTY child dies** (`glass_terminal/src/pty.rs:368-389`)
- Send `AppEvent::SessionExited { session_id, exit_code }`
- Display "[Shell exited with code X]" in terminal

**H-9: No GPU device-lost recovery** (`glass_renderer/src/surface.rs`)
- Detect 3 consecutive surface acquisition failures
- Trigger full GPU reinitialization (recreate instance, adapter, device, surface)

### Medium

**M-1: Regex `OnceLock` unwraps** — Add `#[cfg(test)]` unit tests exercising each regex

**M-2: Event loop/window `expect()`** (`src/main.rs:9802,2324,9893`)
- Use native `MessageBoxW` on Windows for init failures, stderr on Unix

**M-3: Font thread `join().expect()`** (`src/main.rs:2335`)
- Handle panic gracefully, fall back to default font system

**M-4: `dirs::home_dir().expect()` in CLI** (`src/main.rs:9981,10008`)
- Return user-friendly error instead of panic

**M-8: No SQLite corruption detection** (all DB crates)
- Add `PRAGMA integrity_check` on open
- On corruption: rename old file, create fresh database

**M-13: Agent stdin mutex poisoning** (`src/main.rs:1413`)
- Switch to `parking_lot::Mutex` (no poisoning)

**M-5: Filesystem watcher channel backpressure** (`glass_snapshot/src/watcher.rs:36`)
- Add `tracing::warn!` on dropped events

### Low

**L-1: Config hot-reload sends defaults on error** (`glass_core/src/config_watcher.rs:67`)
- Keep current config on validation failure, only send the error

**L-2: Script generation hardcoded** (`glass_feedback/src/lib.rs:372`)
- Read `script_generation` from config

### Cross-references

- **H-2 (GPU init `expect()`):** Covered by P-4 in Branch 4 (setup-packaging)

### Accepted risk / Deferred

- **M-6 (orchestrator silence event `let _ =` sends):** Accepted — PTY loop self-terminates on app close
- **M-7 (PTY poll error breaks loop silently):** Accepted — transient errors are rare; retry logic adds complexity without clear benefit
- **M-9 (disk-full not surfaced to user):** Accepted — errors propagate via `anyhow`; root-cause surfacing is a UX polish item
- **M-10 (font loading failure → blank terminal):** Accepted — rare on desktop systems; containers without fonts are not a launch target
- **M-11 (undo permission check UX):** Accepted — error is captured in `FileOutcome::Error`; clearer messaging is a UX polish item
- **M-12 (blob store TOCTOU):** Accepted — benign due to content-addressing, no data corruption risk
- **M-14 (agent generation counter not atomic):** Accepted — safe under current single-threaded architecture

### Testing strategy

- **C-2 (session refactor):** Unit tests verifying `session()` returns `None` when no tabs exist. Manual test: close all tabs, verify no panic
- **H-1 (PTY spawn):** Manual test on restricted environment (no PTY available). Verify error dialog appears
- **H-5 (PTY send errors):** Kill shell process, verify "[Shell exited]" message appears
- **H-9 (GPU device-lost):** Manual test: sleep/wake laptop, verify recovery or graceful degradation
- **M-8 (SQLite corruption):** Unit test: corrupt DB file, verify rename-and-recreate

---

## Branch 2: `audit/security`

### Critical

**S-1: `glass_tab_send` arbitrary command execution** (`glass_mcp/src/tools.rs:1195-1223`, `src/main.rs:8987-9017`)
- Enforce `PermissionMatrix` from config at MCP dispatch layer
- When `run_commands = Approve` (default): show confirmation dialog in Glass GUI with command text and requesting agent name. Allow/Deny/Allow-for-session options
- When `run_commands = Allow`: execute without prompt
- When `run_commands = Deny`: reject with error
- Same treatment for `glass_tab_create` and `glass_cancel_command`

### High

**S-2: IPC no authentication** (`glass_core/src/ipc.rs:91-238`)
- Unix: `chmod 0600` on socket after creation
- Windows: create named pipe with `SECURITY_ATTRIBUTES` restricted to current user SID

**S-3: MCP no access controls** (`glass_mcp/src/tools.rs`, `glass_mcp/src/lib.rs:26-46`)
- Wire `PermissionMatrix` and `allowed_tools` config into MCP tool dispatch
- Before each tool execution: check allowlist, check permission level by category
- Add audit logging: `tracing::info!` per tool invocation with tool name, agent ID, parameters

**S-4: Agent coordination no auth** (`glass_coordination/src/db.rs`)
- Generate session nonce at `glass_agent_register`, return to caller
- Require nonce for all subsequent operations (lock, unlock, send, status)
- Validate PID liveness at registration

**S-5: OAuth token exposure** (`src/usage_tracker.rs:36-56`)
- Add `zeroize` dependency, wrap bearer token in `Zeroizing<String>`
- Add `#[cfg(debug_assertions)]` guard against token in tracing output

### Medium

**S-6: Command history sensitive data** (`glass_history/src/db.rs:167-189`)
- Add `[history] redact_patterns` config: list of regex patterns
- Scrub matches with `[REDACTED]` before storing
- Ship defaults: `password=\S+`, `token=\S+`, `Bearer \S+`, `--password \S+`

**S-7: Undo path validation** (`glass_snapshot/src/undo.rs:116-167`)
- Add `project_root` to `UndoEngine`
- Verify `file_path.starts_with(project_root)` before write/delete

**S-8: Blob store hash validation** (`glass_snapshot/src/blob_store.rs:40-47,86-98`)
- Add `ensure!(hash.chars().all(|c| c.is_ascii_hexdigit()))`

**S-9: Ephemeral agent permissions** (`src/ephemeral_agent.rs:115-123`)
- Remove `--dangerously-skip-permissions`
- Rely on `--allowedTools ""` alone
- Add integration test verifying no tools available
- **Testing note:** Verify ephemeral agent still works without `--dangerously-skip-permissions` across Claude CLI versions before committing. If the flag is required for non-interactive use, document the rationale instead of removing it

**S-10: Regex pattern length limit** (`glass_mcp/src/tools.rs:1264`)
- Cap `pattern` at 1000 chars
- Use `RegexBuilder::new().size_limit(1_000_000)`

**S-11: Shell temp files** (`shell-integration/glass.bash:273-298`)
- Replace manual temp path with `mktemp -d`
- Add `chmod 700` on temp directory

### Low

**S-12: Database file permissions** — Set `0600` on all `.glass/*.db` at creation (Unix)

**S-13: CWD LIKE wildcard escaping** (`glass_history/src/query.rs:153-155`) — Escape `%` and `_`

**S-14: Workspace trust for scripts** — First-load confirmation for `.glass/scripts/` from new projects. Track in `~/.glass/trusted_projects.toml`

**S-15: `cargo audit` in CI** — Add CI job

**S-16: Config path validation** — Validate `prd_path` and `checkpoint_path` within project directory

### Accepted risk / Deferred

- **Finding 12 (FTS5 query injection):** Accepted — current quoting approach is correct for phrase queries
- **Finding 14 (SQLite no encryption):** Accepted — file permissions (S-12) are the practical mitigation; encryption (SQLCipher) is a hardening item for post-launch

### Testing strategy

- **S-1 (MCP permission gate):** Unit tests for each permission level (Allow/Approve/Deny). Manual test: connect MCP client, verify Approve shows dialog
- **S-2 (IPC auth):** Verify socket permissions with `stat` on Unix. Manual test: attempt connection from different user
- **S-4 (agent nonces):** Unit test: operations without nonce are rejected
- **S-6 (redaction):** Unit tests with sensitive pattern matching
- **S-9 (ephemeral agent):** Integration test across Claude CLI versions

---

## Branch 4: `audit/setup-packaging`

### Critical

**P-1/P-2: Embed shell integration scripts in binary**
- Use `include_str!()` for all 4 scripts (glass.bash, glass.zsh, glass.fish, glass.ps1)
- At PTY spawn, write to temp dir and source from there
- Fixes: missing from installers, false README claim, silent failure

### High

**P-3: No warning when shell integration not found** (`src/main.rs:711`)
- Add `tracing::warn!` + status bar toast on fallback path

**P-4: GPU init panic → friendly error** (`glass_renderer/src/surface.rs:32,41,49`)
- Replace 3 `expect()` calls with user-friendly error message before exit
- Suggest `glass check` for diagnosis

**P-5: README Linux deps incomplete**
- Add `libxtst-dev` to the Linux dependency list

### Medium

**P-6: Homebrew/winget manifest placeholders** — Automate SHA256 in release workflow or document manual steps

**P-7: macOS Gatekeeper docs** — Prominently document workaround in README install section

**P-8: MSRV declaration** — Add `rust-version` field to root Cargo.toml

**P-9: No Fedora/Arch build deps** — Add `dnf install` and `pacman -S` equivalents

**P-10: No `cargo install` metadata** — Add `repository`, `homepage`, `categories`, `keywords` to Cargo.toml

**P-11: Config not created on first run** — Create commented-out default `~/.glass/config.toml` on first launch

**P-12: No Scoop manifest** — Create `packaging/scoop/glass.json`

### Low

**P-13: `glass check` subcommand** — Report GPU adapter, detected shell, shell integration path, config path

**P-14: Unsupported shell warning** — Log warning when shell doesn't match known integrations

**P-15: `windows_subsystem` CLI limitation** — Document in README

---

## Branch 5: `audit/performance`

### Critical

**PERF-R01: Per-cell glyphon Buffer allocation** (`glass_renderer/src/grid_renderer.rs:340-409`)
- Add generation counter to `GridSnapshot`
- Cache `glyphon::Buffer` objects in `HashMap<(row, col), Buffer>`
- Row-level dirty tracking: only reshape rows whose content changed
- Skip `build_cell_buffers` entirely when generation matches

### High

**PERF-R02: No dirty-flag redraw tracking** (`src/main.rs:2539-2635`)
- Add `dirty: AtomicBool` set by PTY thread, cleared after render
- When not dirty, skip entire render pipeline

**PERF-L01: No frame rate throttling** (`glass_terminal/src/pty.rs`, `src/main.rs`)
- Add `last_redraw: Instant` field
- Skip redraws less than 1ms since last frame
- Caps effective rate at ~500fps during floods

**PERF-M01: Unbounded block memory** (`glass_terminal/src/block_manager.rs`)
- Evict `pipeline_stages` from blocks >1000 lines from viewport
- Keep metadata, lazy-reload from history DB on scroll-back

### Medium

**PERF-G02: Glyph atlas never trimmed** (`glass_renderer/src/glyph_cache.rs:81-83`)
- Add `ctx.frame_renderer.trim()` call after `frame.present()` — one line

**PERF-M02: Vec\<char\> zerowidth allocation** (`glass_terminal/src/grid_snapshot.rs:310`)
- Replace with `SmallVec<[char; 0]>`

**PERF-A01: Deep clone of visible blocks** (`src/main.rs:2628-2633`)
- Restructure borrow to avoid cloning block data during `draw_frame`

**PERF-R03: Linear cursor wide-char scan** (`glass_renderer/src/grid_renderer.rs:137-140`)
- Direct index lookup instead of `.iter().any()`

**PERF-R04: Overlay buffers rebuilt every frame** (`glass_renderer/src/frame.rs:373+`)
- Cache with text comparison, only reshape on change

**PERF-S02: Individual DELETE in pruning** (`glass_history/src/retention.rs:29-56,81-107`)
- Batch `WHERE command_id IN (...)` — 5N statements → 5

**PERF-B01: Missing hot-path benchmarks** (`benches/perf_benchmarks.rs`)
- Add Criterion benchmarks for `build_cell_buffers`, `snapshot_term`, `build_rects`

**PERF-F01: Recursive directory watch** (`glass_snapshot/src/watcher.rs:39`)
- Lazy watching or 50ms debounce coalesce for large trees

### Low

**PERF-M03: Non-configurable scrollback** — Add `[terminal] scrollback` config field

**Implementation order within branch:**
1. Dirty flag + frame throttling (biggest impact, least effort)
2. Glyph atlas trim (one-liner)
3. Buffer caching + row-level dirty tracking (the hard one)
4. Quick wins: cursor lookup, batch DELETE, clone removal, SmallVec
5. Benchmarks last (measure improvements)

### Accepted risk / Deferred

- **PERF-TH02 (FairMutex contention):** Mitigated by PERF-R02 (dirty flags reduce lock frequency)
- **PERF-R05 (duplicate draw_frame/draw_multi_pane_frame):** No runtime impact, refactor deferred
- **PERF-L02 (output capture buffer):** Current bounded design is sound
- **PERF-G01 (instance buffer growth without shrink):** 256KB max, not practical concern
- **PERF-G03, S01, S03, T01, T02, T03, P01, P02:** Already optimized / correct design
- **PERF-A02 (tab title clone):** Low impact (1-5 short strings)

### Testing strategy

- **PERF-R01/R02 (buffer caching + dirty flags):** Benchmark before/after with `build_cell_buffers` Criterion bench. Manual test: `cat` large file, verify no frame drops
- **PERF-L01 (frame throttling):** Manual test: rapid output flood, monitor CPU usage
- **PERF-M01 (block eviction):** Unit test: create 1000+ blocks, verify old pipeline data evicted, verify scroll-back rehydrates

---

## Branch 3: `audit/cross-platform`

### Medium

**XP-1: Pipeline capture for zsh/fish** (`shell-integration/glass.zsh`, `glass.fish`)
- Port `tee` rewriting and OSC 133;S/P emission from `glass.bash`
- zsh: use `preexec`/`precmd` hooks and `zle` widgets
- fish: use `fish_preexec`/`fish_postexec` and `commandline`

**XP-2: macOS orphan prevention** (`src/main.rs`, `src/ephemeral_agent.rs`)
- Add watchdog thread under `#[cfg(target_os = "macos")]`
- Periodically check `getppid() == 1`, kill children if reparented to init

### Low

**XP-3: Clippy on all platforms** (`.github/workflows/ci.yml`)
- Convert clippy job to matrix across Windows, macOS, Linux

**XP-4: Vulkan fallback on Windows** (`glass_renderer/src/surface.rs`)
- `Backends::DX12` → `Backends::DX12 | Backends::VULKAN`

**XP-5: winresource Windows-only** (`Cargo.toml:133`)
- Move to `[target.'cfg(windows)'.build-dependencies]`

**XP-6: macOS Intel binary** (`.github/workflows/release.yml`)
- Add `x86_64-apple-darwin` target to release CI matrix

**XP-7: IPC path constant deduplication**
- Extract shared constant to avoid drift between `glass_core::ipc` and `glass_mcp::ipc_client`

**XP-8: Job Object handle wrapper** (`src/main.rs:387-389`)
- Replace raw `isize` with newtype calling `CloseHandle` on drop

---

## Branch 6: `audit/ui-ux`

### High

**UX-1: First-run onboarding**
- Detect first launch via absence of `~/.glass/config.toml`
- Show overlay: "Welcome to Glass. Press Ctrl+Shift+, for settings & shortcuts."
- Show "Ctrl+Shift+, = settings" in status bar for first 5 sessions
- Track session count in `~/.glass/state.toml`

**UX-2: Undo feedback**
- Inject visible message into terminal via PTY: `[Glass] Undo: N files restored, M conflicts`
- Status bar flash for "Nothing to undo"

**UX-3: Running command indicator**
- Add live elapsed timer in block decoration area for `Executing` state
- Update per-second, reuse existing duration rendering

**UX-4: Pipeline panel dismiss**
- Add Escape key handler to close pipeline panel
- Add "[Esc] close" hint in panel header

**UX-5: CWD truncation**
- Replace hardcoded 60-char limit with dynamic calculation based on viewport width minus right-side elements

### Medium

**UX-6: Search overlay label** — "Search:" → "Search History:"

**UX-7: Active tab indicator** — 2px cornflower blue accent underline on active tab

**UX-8: Exit code in badge** — "X" → "E:1", "E:127". Widen badge from 3 to 5 cells

**UX-9: Shift+Up/Down scrolling** — Single-line scrollback via Shift+Arrow

**UX-10: Tab bar overflow** — Left/right scroll arrows when tabs exceed viewport. `+` button always visible at right edge

**UX-11: Alt+Arrow conflict** — Only intercept for pane focus when pane count > 1 AND not in alternate screen mode

**UX-12: Config error path** — Include `~/.glass/config.toml` path in error banner

**UX-13: Theme support** — Extract hardcoded chrome colors into `[theme]` config section. Ship `dark` and `light` presets

### Low

**UX-14: Ctrl+C copy with selection** — Copy when selection active, SIGINT when not

**UX-15: Document D/E split mnemonic** — Tooltip in Shortcuts overlay

**UX-16: Tab title from CWD** — Auto-update from session's current working directory

**UX-17: Pipeline stage click-to-expand** — Mouse handlers for [+]/[-] indicators

**UX-18: Block separator visibility** — Configurable thickness/color, default 2px at (80,80,80)

**UX-19: Terminal content padding** — `padding.x`/`padding.y` config fields, default 4px

**UX-20: Max split depth feedback** — Status bar toast when limit reached

**UX-21: Tab context menu** — Right-click: Rename, Duplicate, Close Others

### Deferred

- Configurable keybindings (large feature)
- Search overlay advanced filters (large feature)
- Status bar density management (dependent on theme)
- MEDIUM-24 (undo label overlaps terminal content) — cosmetic, acceptable tradeoff
- LOW-05 (Ctrl+1-9 tab jumping overlap) — minimal real-world conflict
- LOW-17 (no cursor blinking in search input) — polish item
- LOW-27 (no loading state for agent operations) — most operations are fast
- LOW-28 (background tabs resized with full-window dimensions) — self-corrects on next redraw

---

## Branch 7: `audit/documentation`

### High

**DOC-1: README screenshot/demo**
- Capture hero screenshot or GIF showing command blocks, pipe viz, exit badges, splits
- Place at top of README after title
- **Requires user** to provide the actual image — spec prepares markup and placeholder

**DOC-2: CHANGELOG.md**
- Create following Keep a Changelog format
- Backfill from git history for v1.0 through v2.5 milestones

**DOC-3: CONTRIBUTING.md**
- Build instructions, test/lint commands, code style, PR process
- Add `.github/ISSUE_TEMPLATE/bug_report.md` and `feature_request.md`
- Add `.github/PULL_REQUEST_TEMPLATE.md`

### Medium

**DOC-4: Update CLAUDE.md** — 9→14 crates, add missing crates, update config sections, fix tool count

**DOC-5: config.example.toml** — All sections, commented, at repo root

**DOC-6: Example Rhai scripts** — `examples/scripts/` with 2-3 examples

**DOC-7: Scripting feature page** — `docs/src/features/scripting.md` for mdBook

**DOC-8: MCP tool parameter docs** — Schemas and example payloads for top 5 tools

**DOC-9: Reconcile config mismatches** — soi.min_lines, fonts, history keys, tool counts

### Low

**DOC-10:** CI/license badges in README
**DOC-11:** macOS keybinding column in mdBook
**DOC-12:** Build deps on mdBook Linux page
**DOC-13:** Module docs for glass_scripting and glass_core lib.rs
**DOC-14:** Update glass_mcp lib.rs doc comment (4→33 tools)
**DOC-15:** PRD template at `examples/prd-template.md`
**DOC-16:** Add `cargo doc` to CI
**DOC-17:** Manual shell integration fallback in troubleshooting

---

## Branch 8: `audit/dependency-licensing`

**LIC-1:** Add `license = "MIT"` to all 14 internal crate Cargo.toml files
**LIC-2:** Generate `THIRD-PARTY-LICENSES` file via `cargo-about`
**LIC-3:** Document `self_cell` Apache-2.0 license choice
**LIC-4:** Verify OpenSSL linkage per platform (`ldd`/`otool -L`)
**LIC-5:** Add `cargo-deny` to CI with `deny.toml` rejecting GPL/AGPL/SSPL
**LIC-6:** Verify Windows builds use SChannel (not OpenSSL)

---

## Cross-Branch Coordination Notes

- **GPU error messages:** Setup branch (Branch 4) handles the user-friendly errors for GPU init. Bugs branch (Branch 1) handles device-lost recovery. No overlap.
- **Shell integration:** Cross-platform branch (Branch 3) adds pipeline capture to zsh/fish FIRST. Then setup branch (Branch 4) embeds the now-complete scripts via `include_str!()`. This dependency is reflected in the merge order.
- **Permission model:** Security branch (Branch 2) builds the MCP permission gate. Other branches reference it but don't modify it.
- **Documentation:** Must merge last (Branch 7) to reflect all changes from other branches.
- **Setup branch P-4 covers bugs audit H-2:** GPU init `expect()` user-friendly errors live in the setup branch, not the bugs branch. The bugs branch cross-references this.
