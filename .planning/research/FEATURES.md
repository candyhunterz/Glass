# Feature Research

**Domain:** Pipe visualization and pipeline debugging for terminal emulator
**Researched:** 2026-03-05
**Milestone:** v1.3 Pipe Visualization
**Confidence:** MEDIUM -- no terminal emulator has pipe-stage visualization; recommendations synthesized from shell debugging patterns (tee, pv, Tee-Object), structured-data shells (Nushell), and block-based terminals (Warp). Novel territory.

---

## Feature Landscape

### Table Stakes (Users Expect These)

If Glass advertises "pipe visualization," these must work or the feature feels broken.

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Pipe detection in command text | Must identify `cmd1 | cmd2 | cmd3` automatically -- users will not annotate their commands | LOW | Parse `|` from command text already captured by OSC 133. Handle quoted/escaped pipes. Depends on: command text extraction (exists from v1.2) |
| Per-stage output capture | The entire value prop -- show what data looked like between each pipe stage | HIGH | Bash/Zsh: rewrite `a | b | c` to `a | tee /tmp/g1 | b | tee /tmp/g2 | c`. PowerShell: post-hoc via `Tee-Object` or capture after execution. Must not break command semantics. |
| Multi-row pipeline UI | Each stage rendered as a sub-block within the parent command block | MEDIUM | Extend existing Block struct with stages. Show command label per stage, expandable output. Depends on: block_manager.rs Block struct, block_renderer.rs |
| Expand/collapse stage output | Pipelines can produce huge intermediate output -- collapsed by default with click-to-expand | MEDIUM | Collapsed shows first N lines or byte count summary. Expand shows full captured output. Depends on: renderer click handling (partially exists via search overlay) |
| Correct exit code preservation | Inserting tee must not mask real exit codes from user's pipeline | MEDIUM | Bash: use `set -o pipefail` + `PIPESTATUS` array. Zsh: `pipestatus` array. PowerShell: object pipeline handles this natively. |
| Opt-out / disable flag | Some commands are TTY-sensitive (vim, htop, less) or performance-critical -- pipe rewriting must not break them | LOW | Config toggle + per-command opt-out (e.g., prefix or annotation). Auto-detect TTY-sensitive commands from a known list. |
| Pipe stage storage in history DB | Captured stage output must persist across sessions for later inspection | MEDIUM | New `pipe_stages` table linked to command_id. Retention policy consistent with existing history pruning. Depends on: glass_history schema (exists) |

### Differentiators (Competitive Advantage)

No terminal emulator visualizes pipe stages. The feature itself is the differentiator. These push further.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Visual pipeline flow diagram | Show data flow as a horizontal/vertical stage diagram with arrows, not just stacked text blocks. At-a-glance understanding of pipeline shape. | MEDIUM | Render stage boxes with `-->` arrows in the block decoration area. Label each with command name + byte count. |
| Stage output diff | Compare output between adjacent stages -- see exactly what each command filtered, transformed, or added | HIGH | Text diff between stage N and stage N+1 output. Only meaningful for text pipelines. Use similar diff engine as GlassFileDiff from v1.2. |
| GlassPipeInspect MCP tool | AI assistants can query intermediate pipeline data to debug user's pipelines. "Why did grep drop these lines?" | MEDIUM | Extend glass_mcp with tool that returns stage outputs for a command_id. Depends on: pipe_stages DB table, glass_mcp (exists) |
| Smart TTY detection | Automatically skip pipe rewriting for interactive/TTY commands (vim, less, top, ssh, docker exec -it) without user configuration | LOW | Maintain a curated list of known TTY commands. Check for `-t`, `-it`, `--interactive` flags. |
| Pipeline error highlighting | When a middle stage fails (non-zero exit), highlight that specific stage in red and show its stderr | MEDIUM | Capture per-stage exit codes via PIPESTATUS. Render failed stage with error badge like existing exit code display. |
| Streaming stage capture | Show intermediate outputs updating in real-time as the pipeline executes, not just after completion | HIGH | Would require named pipes or process substitution with async reads. Significant complexity. Defer to v1.x. |
| PowerShell object-aware capture | PowerShell pipes objects, not text. Capture with `Format-List` or `ConvertTo-Json` to show structured intermediate data | MEDIUM | PowerShell-specific: inject `Tee-Object -Variable` or post-hoc `Trace-Command`. Show object properties in a table layout. |

### Anti-Features (Commonly Requested, Often Problematic)

| Feature | Why Requested | Why Problematic | Alternative |
|---------|---------------|-----------------|-------------|
| Always-on pipe rewriting | "Just capture everything" | Breaks TTY programs, adds latency to every command, temp file pollution, changes command semantics for edge cases | Opt-in by default with smart auto-detection. Capture only when safe. |
| Real-time streaming visualization | "Show data flowing through pipes live" | Extreme complexity: async named pipe orchestration, real-time rendering updates, buffering semantics change pipeline behavior | Capture after completion. Show final stage outputs. Stream visualization is a v2+ feature. |
| Pipe stage editing/replay | "Let me edit stage 2 and re-run from there" | Requires reproducing exact shell state, env vars, CWD. Side effects of stage 1 may be non-reproducible. | Show captured outputs for inspection only. User re-runs manually. |
| Automatic pipeline optimization | "Suggest faster pipe chains" | Shell command semantics are subtle. `sort | uniq` vs `sort -u` have different behavior with `-c`. Wrong suggestions erode trust. | Show byte counts per stage so users can spot bottlenecks themselves. |
| Binary pipe visualization | "Show hex dump of binary pipe data" | Binary intermediate data is unreadable. Hex dumps of megabytes are useless. | Show byte count and content-type heuristic (text/binary). Only render text stages. |
| Nested subshell pipe capture | "Capture pipes inside $() and backticks" | Requires full shell AST parsing. Subshells run in child processes with no hook access. Turing-complete problem. | Capture top-level pipe stages only. Document limitation. |
| Cross-command pipe chains | "Track data from `cmd > file` then `cat file | ...`" | Requires understanding file-based data flow across commands. Impossible to do reliably. | Each pipeline is independent. Use history search to find related commands. |

## Feature Dependencies

```
[Command Text Extraction] (EXISTING v1.2)
    |
    v
[Pipe Detection & Parsing]
    |
    +---> [TTY-Sensitive Command Detection]
    |         |
    |         v
    |     [Opt-Out Decision]
    |
    v
[Command Rewriting Engine]
    |
    +---> [Bash/Zsh: tee injection]
    |         |
    |         v
    |     [Temp file management]
    |         |
    |         v
    |     [PIPESTATUS capture]
    |
    +---> [PowerShell: post-hoc capture]
              |
              v
          [Tee-Object / output variable capture]
    |
    v
[Stage Output Collection]
    |
    v
[pipe_stages DB Table] ---------> [Retention Policy]
    |
    +---> [Pipeline UI Rendering]
    |         |
    |         +---> [Multi-row stage blocks]
    |         |
    |         +---> [Expand/collapse]
    |         |
    |         +---> [Error highlighting]
    |
    +---> [GlassPipeInspect MCP Tool]
```

### Dependency Notes

- **Pipe Detection requires Command Text Extraction:** Already built in v1.2 for the undo command parser. Reuse the same text capture path.
- **Command Rewriting requires Pipe Detection:** Must parse the pipe structure before injecting tee.
- **Stage Output Collection requires Command Rewriting (Bash/Zsh):** Tee files must exist before we can read them.
- **Pipeline UI requires Stage Output Collection:** Nothing to render without captured data.
- **DB storage requires Stage Output Collection:** Must have data before persisting.
- **MCP tool requires DB storage:** Queries the pipe_stages table.
- **TTY Detection conflicts with Command Rewriting:** If command is TTY-sensitive, skip rewriting entirely.

## MVP Definition

### Launch With (v1.3)

Minimum viable pipe visualization -- capture and display works for common cases.

- [ ] Pipe detection from command text (split on unquoted `|`) -- foundation for everything
- [ ] Bash/Zsh tee-based command rewriting with temp file capture -- covers the primary use case
- [ ] PIPESTATUS/pipestatus exit code preservation -- correctness is non-negotiable
- [ ] TTY-sensitive command detection with opt-out -- prevents breakage
- [ ] Multi-row pipeline UI with stage labels and collapsed output -- the visual payoff
- [ ] Click-to-expand stage output -- handles large intermediate data
- [ ] pipe_stages table in history DB with retention -- persistence across sessions
- [ ] Config section for pipe visualization on/off and settings -- user control

### Add After Validation (v1.x)

Features to add once core pipe visualization is proven reliable.

- [ ] PowerShell Tee-Object integration -- different pipe semantics, needs separate implementation path
- [ ] GlassPipeInspect MCP tool -- valuable but not blocking core UX
- [ ] Stage output diff view -- powerful but complex, depends on diff engine
- [ ] Pipeline error highlighting per stage -- nice UX polish
- [ ] Visual flow diagram with arrows -- cosmetic enhancement over stacked blocks

### Future Consideration (v2+)

- [ ] Real-time streaming stage capture -- extreme complexity, marginal benefit over post-completion
- [ ] PowerShell object-aware visualization -- requires deep PS integration
- [ ] Nested subshell pipe capture -- Turing-complete parsing problem

## Feature Prioritization Matrix

| Feature | User Value | Implementation Cost | Priority |
|---------|------------|---------------------|----------|
| Pipe detection & parsing | HIGH | LOW | P1 |
| Bash/Zsh tee injection capture | HIGH | HIGH | P1 |
| Exit code preservation (PIPESTATUS) | HIGH | MEDIUM | P1 |
| TTY command detection + opt-out | HIGH | LOW | P1 |
| Multi-row pipeline UI blocks | HIGH | MEDIUM | P1 |
| Expand/collapse stage output | MEDIUM | MEDIUM | P1 |
| pipe_stages DB + retention | MEDIUM | MEDIUM | P1 |
| Config section | MEDIUM | LOW | P1 |
| PowerShell post-hoc capture | MEDIUM | MEDIUM | P2 |
| GlassPipeInspect MCP tool | MEDIUM | LOW | P2 |
| Pipeline error highlighting | MEDIUM | LOW | P2 |
| Visual flow diagram | LOW | MEDIUM | P2 |
| Stage output diff | MEDIUM | HIGH | P3 |
| Streaming capture | LOW | HIGH | P3 |

**Priority key:**
- P1: Must have for v1.3 launch
- P2: Should have, add in follow-up
- P3: Nice to have, future consideration

## Competitor Feature Analysis

| Feature | Warp Terminal | Nushell | Traditional (tee/pv) | Glass v1.3 Plan |
|---------|--------------|---------|----------------------|-----------------|
| Command blocks | Yes -- groups cmd+output | N/A (shell, not emulator) | No | Yes (existing) |
| Pipe detection | No | Built-in (structured data) | No | Yes (new) |
| Intermediate output capture | No | Manual (`| inspect`) | Manual (`| tee file`) | Automatic |
| Per-stage visualization | No | No (shows final only) | No | Yes (new -- unique) |
| TTY-safe pipe handling | N/A | N/A | User responsibility | Auto-detection |
| Pipeline progress | No | No | pv (byte throughput) | Stage completion status |
| Exit code per stage | No | Yes (structured errors) | PIPESTATUS (manual) | Automatic per-stage display |
| Pipeline history/storage | No | No | No | Yes (DB persistence) |
| AI integration for pipes | No | No | No | GlassPipeInspect MCP |

**Key insight:** No existing tool combines automatic capture with visual per-stage display. Nushell has the richest pipeline model but is a shell, not an emulator. Warp has blocks but no pipe awareness. Traditional tools (tee, pv) require manual setup per use. Glass would be the first to automate capture and visualize stages within the terminal emulator itself.

## Sources

- [Pipe Viewer (pv)](https://www.ivarch.com/programs/pv.shtml) -- throughput monitoring for pipe stages
- [pipeview (Rust)](https://github.com/mihaigalos/pipeview) -- Rust pipe inspection utility
- [Nushell Pipelines](https://www.nushell.sh/book/pipelines.html) -- structured data pipeline model
- [Nushell Pipeline Processing](https://deepwiki.com/nushell/nushell/5.3-pipeline-processing) -- pipeline data flow internals
- [Warp Blocks](https://docs.warp.dev/terminal/blocks) -- command+output grouping in block-based terminal
- [PowerShell Tee-Object](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.utility/tee-object?view=powershell-7.5) -- pipeline inspection without disrupting flow
- [PowerShell Out-GridView](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.utility/out-gridview?view=powershell-7.5) -- visual pipeline data inspection
- [PowerShell Trace-Command](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.utility/trace-command?view=powershell-7.5) -- pipeline execution tracing
- [Pipe Debugging (softpanorama)](https://softpanorama.org/Scripting/Piporama/debugging.shtml) -- traditional pipe debugging techniques
- [strace across pipes](https://github.com/nh2/strace-pipes-presentation) -- system-level pipe debugging
- [Debugging with pipes (gllghr.com)](https://gllghr.com/blog/debugging-with-pipes) -- incremental pipeline building pattern
- Glass PROJECT.md -- existing Block struct, OSC 133 lifecycle, command text extraction, history DB

---
*Feature research for: Glass v1.3 Pipe Visualization*
*Researched: 2026-03-05*
