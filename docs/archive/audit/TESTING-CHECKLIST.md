# Prelaunch Audit — Manual Testing Checklist

**Date:** 2026-03-18
**Binary:** `target/release/glass.exe` (built from master with all 8 audit PRs merged)
**Tests passing:** 1,647 automated tests, 0 failures

---

## Tier 1: Basic Functionality (do these first)

- [x ] Launch `glass.exe` — window opens, terminal renders
- [x ] Type `echo hello` — output appears, command completes
- [ x] Shell integration loaded — command blocks visible with green "OK" badge
- [x ] Exit badge shows "E:1" for `false` command (not just "X")
- [ ] Status bar shows CWD and git branch
- [ ] Ctrl+Shift+, opens settings overlay
- [ ] Escape closes settings overlay
- [ ] Close window — process exits cleanly (no orphan processes)

## Tier 2: Session/Tab/Pane Stability (C-2 refactor)

- [ ] Ctrl+Shift+T — new tab opens
- [ ] Ctrl+Shift+W — close tab
- [ ] Open 3 tabs, close middle one — no crash
- [ ] Close all tabs — app exits (or shows empty state, no panic)
- [ ] Ctrl+Shift+D — horizontal split
- [ ] Ctrl+Shift+E — vertical split
- [ ] Alt+Arrow — focus moves between panes (only when >1 pane)
- [ ] Close one pane in a split — remaining pane fills space
- [ ] Open 10+ tabs — scroll arrows appear in tab bar
- [ ] Click scroll arrows — tabs scroll correctly
- [ ] "+" button always visible at right edge

## Tier 3: Performance (render pipeline changes)

- [ ] `cat` a large file (1000+ lines) — renders smoothly, no obvious lag
- [ ] Rapid output (`yes | head -1000`) — no freeze, CPU doesn't spike to 100%
- [ ] Idle terminal — near-zero CPU usage (dirty flag working)
- [ ] Long-running command (`sleep 5`) — blue "running" badge with elapsed timer
- [ ] Timer updates every second while command executes
- [ ] After completion — timer stops, badge changes to green OK or red E:N

## Tier 4: New Features

### First-run onboarding
- [ ] Delete `~/.glass/state.toml`
- [ ] Launch Glass — "Tip: Ctrl+Shift+, = settings & shortcuts" appears in status bar
- [ ] Launch 5 more times — hint disappears after 5th session

### Glass check subcommand
- [ ] Run `glass check` from an existing terminal
- [ ] Shows: version, config path, GPU adapter, detected shell, shell integration status

### Undo
- [ ] Run a file-modifying command (e.g., `echo test > /tmp/glass_test.txt`)
- [ ] Ctrl+Shift+Z — status bar shows undo result ("1 file restored" or "Nothing to undo")
- [ ] Ctrl+Shift+Z with nothing to undo — "Nothing to undo" message

### Theme
- [ ] Add `[theme]` section to `~/.glass/config.toml`: `preset = "light"`
- [ ] Config hot-reloads — terminal chrome switches to light colors
- [ ] Change back to `preset = "dark"` — reverts

### Config
- [ ] Delete `~/.glass/config.toml`
- [ ] Launch Glass — default config file created with comments
- [ ] Edit config with invalid TOML — error banner shows `~/.glass/config.toml` path
- [ ] Fix the error — banner disappears (hot-reload)

### Search
- [ ] Ctrl+Shift+F — search overlay opens
- [ ] Header says "Search History:" (not just "Search:")
- [ ] Escape closes search overlay

### Pipeline visualization
- [ ] Run `ls | grep .rs` — command executes normally
- [ ] Ctrl+Shift+P — pipeline panel opens with stage data
- [ ] Escape — pipeline panel closes
- [ ] "[Esc] close" hint visible in panel header

### Scrollback
- [ ] Generate output (`seq 1 500`)
- [ ] Shift+PageUp — scrolls up
- [ ] Shift+Up — scrolls up one line
- [ ] Shift+Down — scrolls down one line
- [ ] Shift+PageDown — scrolls back to bottom

### Copy/paste
- [ ] Select text with mouse, Ctrl+C — copies to clipboard (no SIGINT)
- [ ] No selection, Ctrl+C — sends SIGINT to running process

## Tier 5: Security (MCP + agent coordination)

- [ ] Start Glass with `[agent.permissions] run_commands = "Never"` in config
- [ ] Connect MCP client — `glass_tab_send` calls should be rejected
- [ ] Remove the config line — `glass_tab_send` works again
- [ ] Run `glass check` — verify no warnings about missing shell integration

## Tier 6: Cross-Platform (if you have access)

### macOS
- [ ] Pipeline capture works in zsh (`ls | grep test`)
- [ ] Force-kill Glass — verify no orphan `claude` process

### Linux
- [ ] Build with all system deps installed
- [ ] Pipeline capture works in bash and fish

## Tier 7: Documentation Spot-Check

- [ ] README.md has CI/license badges at top
- [ ] README.md has screenshot placeholder section
- [ ] `config.example.toml` exists at repo root, is well-commented
- [ ] `CHANGELOG.md` exists
- [ ] `CONTRIBUTING.md` exists with build instructions
- [ ] `examples/scripts/` has 3 Rhai examples
- [ ] `THIRD-PARTY-LICENSES` file exists

---

## Results

**Tester:** _______________
**Date:** _______________
**OS:** _______________
**Issues found:**

1.
2.
3.
