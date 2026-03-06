# Pitfalls Research

**Domain:** Pipe visualization for terminal emulator (adding to existing Glass v1.3)
**Researched:** 2026-03-05
**Confidence:** HIGH (most pitfalls verified through official docs, codebase analysis, and multiple sources)

## Critical Pitfalls

### Pitfall 1: TTY Detection Changes Command Output (isatty Behavioral Shift)

**What goes wrong:**
When Glass inserts `tee` (or any pipe-based capture mechanism) between pipe stages, commands detect that stdout is no longer a TTY via `isatty(1)`. This causes `ls` to switch from multi-column to single-column output, `git` to disable colors, `grep --color=auto` to emit plain text, and programs like `less`/`more` to behave as `cat`. The user sees different output than they would without pipe visualization enabled, breaking Glass's core value: "looks and feels normal."

**Why it happens:**
Most CLI tools call `isatty()` on their stdout file descriptor. When stdout points to a pipe (from tee insertion) instead of the PTY, `isatty()` returns false. This is fundamental POSIX behavior. Commands with `--color=auto` (the default for git, ls, grep on most systems) disable color. Commands like `ls` switch to one-entry-per-line format. Interactive pagers refuse to paginate.

**How to avoid:**
1. Do NOT insert tee between pipe stages for bash/zsh. Instead, capture output at the PTY reader level, where Glass already sees all bytes via `OutputBuffer`. The challenge is attributing bytes to specific pipe stages, not capturing them.
2. For per-stage capture in bash/zsh, the practical approach is post-hoc: parse the combined PTY output to reconstruct what each stage produced, using shell integration markers or heuristic splitting.
3. For PowerShell, use `Tee-Object` which preserves object types natively.
4. If tee insertion is truly unavoidable for a subset of scenarios, force `--color=always` on known commands -- but this is a maintenance nightmare and breaks unknown commands. Avoid this path.

**Warning signs:**
- `ls` output appears as one file per line when pipe visualization is active
- Colors disappear from git/grep output in visualized pipes
- Users report "my terminal looks different with pipe viz enabled"

**Phase to address:**
Phase 1 (Pipe detection and capture architecture) -- this is the foundational decision. Getting capture wrong here poisons everything downstream.

---

### Pitfall 2: Exit Code Swallowing When Inserting Tee

**What goes wrong:**
In bash, `$?` returns the exit code of the LAST command in a pipeline. If Glass rewrites `cmd1 | cmd2` to `cmd1 | tee /tmp/cap1 | cmd2 | tee /tmp/cap2`, then `$?` reflects tee's exit code (almost always 0), not cmd2's. The exit code displayed in Glass's block decoration becomes meaningless. Glass uses OSC 133;D exit codes from `glass.ps1` -- the reported exit code would be tee's, not the user's actual pipeline result.

**Why it happens:**
Bash default: `$?` = exit code of last pipeline component. Even with `set -o pipefail`, the behavior is "rightmost non-zero exit code" -- tee's success (0) masks failures in earlier stages. `PIPESTATUS` array helps but is bash-only (not available in zsh by default or PowerShell).

**How to avoid:**
1. Prefer PTY-level capture over command rewriting entirely. Glass already captures output in `OutputBuffer` -- extend this rather than rewriting commands.
2. If command rewriting is unavoidable, never let the tee process be the last command in the pipeline visible to the shell. Use process substitution: `cmd1 | tee >(cat > /tmp/cap1) | cmd2` -- tee feeds the capture via a subshell but cmd2 remains the last pipeline element.
3. Run `echo ${PIPESTATUS[@]}` or equivalent after each pipeline to collect per-stage exit codes.

**Warning signs:**
- All piped commands show exit code 0 regardless of actual failures
- `set -e` scripts break when pipe visualization is enabled
- CI/CD workflows behave differently under Glass

**Phase to address:**
Phase 1 (Capture architecture) -- must be resolved before any shell-level command rewriting is considered.

---

### Pitfall 3: Shell Quoting/Escaping Corruption in Command Rewriting

**What goes wrong:**
If Glass rewrites `grep "hello world" file.txt | wc -l` by parsing and reconstructing it, incorrect re-quoting produces `grep hello world file.txt | wc -l` (argument split) or double-escaping produces `grep \"hello world\" file.txt | wc -l`. Commands with nested quotes, backticks, `$()` substitutions, heredocs, or brace expansion become mangled. The existing `command_parser.rs` already flags pipes (`" | "`) as `Confidence::Low` / unparseable syntax via `contains_unparseable_syntax()` -- this is correct and must be respected.

**Why it happens:**
Shell syntax is Turing-complete (PROJECT.md explicitly notes: "Full shell command parser -- shell syntax is Turing-complete; heuristic whitelist instead"). Reconstructing a command after parsing inevitably loses quoting information. Problem cases include:
- Nested single/double quotes: `echo "it's a 'test'" | wc`
- Dollar-sign expansion: `echo "$HOME" | grep user`
- Backtick substitution: `` echo `date` | tee log ``
- Here-strings: `grep pattern <<< "$variable" | wc`
- Brace expansion: `echo {a,b,c} | tr ' ' '\n'`

**How to avoid:**
1. Never parse-and-reconstruct shell commands for rewriting. This is the single most important rule.
2. If modification is needed, use string-level insertion at pipe boundaries (`|` characters outside quotes) rather than full AST parsing. Insert `tee /tmp/capN |` at known safe positions without touching the rest of the command string.
3. Glass already uses `shlex` for POSIX tokenization -- but shlex handles ARGUMENT splitting, not full shell syntax. Do not extend it to handle command rewriting.
4. For PowerShell: use `Tee-Object` insertion which operates on the object pipeline and avoids string escaping entirely.

**Warning signs:**
- Commands with special characters produce different results with pipe visualization
- Users see "command not found" or syntax errors they didn't type
- Tests pass with simple commands but fail with real-world complex pipelines

**Phase to address:**
Phase 1 (Capture architecture) -- the decision to avoid command rewriting must be made upfront.

---

### Pitfall 4: PowerShell Object Pipeline is Not a Byte Stream

**What goes wrong:**
PowerShell passes .NET objects between pipeline stages, not text bytes. Attempting to capture PowerShell pipe stages with the same mechanism used for bash (byte-stream tee) destroys object fidelity. `Get-Process | Where-Object { $_.CPU -gt 10 }` passes Process objects, not text. If Glass serializes these to text for capture, the captured "stage output" is a Format-Table rendering that loses property types, methods, hidden properties, and the ability to re-pipe.

Additional PowerShell-specific traps:
- Format-Table truncates columns based on terminal width (evaluated from first 300ms of data)
- The FIRST object's properties determine column headers -- heterogeneous pipelines silently drop properties from later objects
- `$FormatEnumerationLimit` defaults to 4, truncating arrays silently
- PowerShell 5.1 vs 7+ handle byte-stream piping differently (`PSNativeCommandPreserveBytePipe` in 7.4+ preserves raw bytes between native executables)

**Why it happens:**
PowerShell's pipeline is fundamentally different from POSIX. POSIX pipes are kernel file descriptors carrying byte streams. PowerShell pipes are in-process .NET object transfers. There is no "wire format" to intercept -- objects exist only in memory. `Tee-Object` works because it copies object references, not bytes.

**How to avoid:**
1. Accept that PowerShell pipe stage capture is inherently lossy -- capture the text RENDERING of each stage, not the objects themselves. This is what the user sees anyway.
2. Use `Tee-Object -Variable` to capture objects into PowerShell variables, then serialize post-hoc with `ConvertTo-Json` or `Out-String -Width 9999` for storage.
3. For display, show the `Out-String -Stream` rendering at each stage.
4. Do NOT attempt to intercept the .NET object pipeline at the CLR level -- unbounded complexity.
5. Set `$FormatEnumerationLimit = -1` in capture context to avoid array truncation.
6. This must be a separate code path from bash/zsh capture, not a shared abstraction.

**Warning signs:**
- PowerShell pipe stage output shows `System.Object[]` instead of actual data
- Truncated columns with `...` ellipsis at end of values
- Properties silently missing when pipeline has mixed object types
- Different behavior between PowerShell 5.1 and 7+

**Phase to address:**
Phase 1 (Architecture decision) for fundamental approach; Phase 2 (Implementation) for PowerShell-specific capture logic.

---

### Pitfall 5: Buffer Explosion from Large Pipe Stage Output

**What goes wrong:**
A pipeline like `find / -type f | grep pattern | head -5` produces potentially gigabytes from `find` even though the final output is 5 lines. If Glass captures each stage's output, stage 1 capture grows unbounded. Per-STAGE buffers multiply memory: a 5-stage pipeline needs 5x the memory. Worse, if capture happens at the PTY level, all bytes still flow through the PTY reader thread's `buf` (1MB `READ_BUFFER_SIZE` in `pty.rs`), and any additional processing in the hot path directly impacts throughput.

**Why it happens:**
Pipes are designed for streaming -- data flows through and is discarded. Capture turns streaming into accumulation. Unix pipes have ~64KB kernel buffers (Linux default). When capture accumulates beyond this, either memory grows unbounded or backpressure introduces latency. The PTY reader thread in Glass is a single `std::thread` doing blocking I/O -- it is the performance bottleneck for all terminal rendering.

**How to avoid:**
1. Apply per-stage byte caps identical to the existing `OutputBuffer.max_bytes` pattern. Default to the same limit as `max_output_capture_kb` per stage.
2. Track `total_seen` per stage (Glass already does this pattern) so the UI can show "captured 50KB of 2.3MB" rather than silently truncating.
3. For the PTY reader thread: do NOT add per-stage parsing in the hot path (`pty_read_with_scan`). Accumulate raw bytes and do stage attribution asynchronously or lazily.
4. Consider ring-buffer semantics: keep the LAST N bytes rather than the FIRST N bytes, since tail output is often more diagnostic.
5. Add a config option to disable per-stage capture for performance-sensitive users while still showing the pipe structure UI.

**Warning signs:**
- Memory usage spikes when running pipelines with large intermediate data
- PTY reader thread latency increases (measurable via key echo delay exceeding 5ms target)
- Glass becomes sluggish during long-running pipelines

**Phase to address:**
Phase 1 (Capture architecture) for buffer design; Phase 3 (Storage) for database schema and retention.

---

### Pitfall 6: Pipe Detection Fails for Non-Obvious Pipe Syntax

**What goes wrong:**
Glass's existing `contains_unparseable_syntax()` checks for `" | "` (pipe with spaces around it). But real-world pipe syntax includes: `cmd|cmd` (no spaces), `cmd |& cmd` (bash 4+ stderr pipe), `cmd 2>&1 | cmd` (redirect then pipe), multiline pipes with `\` continuation, and `|` characters inside string arguments (e.g., `grep "a|b" file | wc`). The pipe detector either misses pipes (no visualization offered) or incorrectly splits at `|` inside regex/string arguments.

**Why it happens:**
The `|` character appears in multiple contexts: pipe operator, regex alternation inside arguments, PowerShell `-match` patterns, heredoc content, and string literals. Naive string splitting on `|` is unreliable. The existing `command_parser.rs` correctly avoids this problem by treating pipes as unparseable.

**How to avoid:**
1. Use quote-aware pipe splitting: track single/double quote state (as `strip_redirects()` already does in `command_parser.rs`) and only recognize `|` as a pipe operator when outside quotes.
2. For regex-heavy commands (grep, sed, awk), the `|` inside a regex argument is protected by quotes in well-formed commands. Handle the unquoted case as best-effort.
3. Handle `|&` (bash stderr pipe) explicitly as a pipe variant.
4. For multiline commands (continuation with `\`): accumulate the full command text from OSC 133;B to 133;C before pipe detection.
5. Do NOT try to handle `$(cmd | cmd)` nesting -- mark as unparseable, consistent with existing approach.
6. PowerShell: watch for `|` inside `-match` patterns and scriptblocks `{ }`.

**Warning signs:**
- Pipes not detected in commands with no spaces around `|`
- False pipe detection inside grep/sed regex patterns
- Multiline piped commands show as separate non-piped commands

**Phase to address:**
Phase 1 (Pipe detection) -- this is the first thing that must work before any visualization.

---

### Pitfall 7: Command Rewriting Creates Security Vulnerabilities

**What goes wrong:**
If Glass rewrites shell commands (inserting tee, process substitution, etc.), a crafted filename like `file$(rm -rf /)` or a git branch name containing shell metacharacters could be injected into the rewritten command if Glass doesn't properly escape inserted paths. Even without malicious intent, `tee /tmp/glass_capture_XXXX` creates world-readable temp files containing potentially sensitive pipe data (passwords, API keys, database query outputs).

**Why it happens:**
Shell command construction via string concatenation is the root cause of command injection vulnerabilities. If Glass constructs `cmd1 | tee /tmp/glass_capture | cmd2` by string interpolation, any part Glass didn't sanitize can break out. Temp files in `/tmp` are accessible to all users. Named pipes (FIFOs) have similar permission issues.

**How to avoid:**
1. Strongly prefer PTY-level capture (no command rewriting = no injection surface). This eliminates the entire vulnerability class.
2. If temp files are needed, use `mktemp` with restrictive permissions (0600), place them in a Glass-owned directory under the user's Glass data directory, and clean them up immediately after capture.
3. Never concatenate user-visible command text into a rewritten command string.
4. For PowerShell `Tee-Object`: use `-Variable` (in-memory) instead of `-FilePath` to avoid temp file creation.

**Warning signs:**
- Temp files left behind after pipe visualization
- Sensitive data visible in capture files
- Command rewriting logic uses string formatting with user input

**Phase to address:**
Phase 1 (Architecture) -- choosing PTY-level capture eliminates this class. If command rewriting is used, mandatory security review before Phase 2.

---

## Technical Debt Patterns

| Shortcut | Immediate Benefit | Long-term Cost | When Acceptable |
|----------|-------------------|----------------|-----------------|
| Capture only whole-pipeline output (no per-stage) | Reuses existing `OutputBuffer`, no new capture mechanism | Users can't see intermediate stage data -- the core value prop of pipe viz | MVP only -- must add per-stage in follow-up |
| Text-only PowerShell capture | Avoids .NET object complexity entirely | Loses object properties, truncated columns | Acceptable permanently -- object capture impractical outside PS runtime |
| Fixed per-stage buffer cap (no config) | Simpler implementation | Power users can't tune for their workflows | Never -- Glass already makes `OutputBuffer` cap configurable |
| Synchronous stage attribution in PTY reader thread | Simpler data flow, single-threaded reasoning | Degrades PTY throughput for ALL commands, not just piped ones | Never -- must be zero-cost when pipes are not detected |
| Storing pipe stages as separate DB rows with FK | Clean relational schema | JOIN latency on every history lookup, even non-piped commands | Acceptable with LEFT JOIN or lazy-load pattern |
| Detecting pipes only with spaces (` | `) | Trivial regex, matches most typed commands | Misses programmatic/script pipes like `cmd|cmd` | MVP only -- must handle no-space pipes quickly |

## Integration Gotchas

Mistakes when integrating pipe visualization with existing Glass systems.

| Integration | Common Mistake | Correct Approach |
|-------------|----------------|------------------|
| OutputBuffer (capture) | Running two capture systems (OutputBuffer for whole command + new per-stage capture) that conflict or double-count bytes | Extend `OutputBuffer` with stage awareness OR replace it for piped commands. One system, not two. |
| BlockManager (lifecycle) | Treating a pipeline as multiple blocks, or trying to add sub-blocks to Block struct -- confusing rendering logic | A pipeline IS one block in `BlockManager`. Pipe stages are a NESTED concept within the block, managed by a new `PipeStageManager` or similar in `glass_pipes`. |
| OSC 133 shell integration | Trying to emit per-stage OSC markers from `glass.ps1` -- adds complexity to shell scripts and relies on shell cooperation | Pipe detection and stage attribution should happen in Glass (Rust side), not in shell integration scripts. The shell emits existing 133;B/C/D; Glass does the rest. |
| Command parser (glass_snapshot) | Extending `command_parser.rs` to handle pipes -- it already explicitly marks them as unparseable for file target extraction | Create a separate `pipe_parser` module in `glass_pipes` crate. Do not modify `command_parser.rs`'s pipe-rejection logic -- it is correct for its purpose. |
| History DB schema | Adding pipe columns to the existing `commands` table | Add a new `pipe_stages` table with FK to `commands.id`. Don't modify existing schema -- migration risk. |
| MCP server | Returning pipe stage data in the existing `GlassHistory` tool response | Add a new `GlassPipeInspect` tool (as planned in PROJECT.md). Keep `GlassHistory` unchanged. |
| PTY reader thread (`pty_read_with_scan`) | Adding pipe-stage parsing logic to the hot read loop | Pipe stage attribution must happen OUTSIDE the PTY reader thread, or be gated by a fast "is this a piped command?" check before any processing. |

## Performance Traps

| Trap | Symptoms | Prevention | When It Breaks |
|------|----------|------------|----------------|
| Per-byte pipe-char scanning in PTY reader | Key echo latency >5ms, visible lag during fast output | Scan for `|` only in command text (at `CommandStart`), not during output streaming | Any pipeline producing >100KB/sec output |
| Storing full stage output in SQLite BLOBs | History DB grows 5-10x faster, query latency increases | Use blob store (like `glass_snapshot`'s filesystem store) for stage output above threshold | After ~1000 piped commands with medium output |
| Re-rendering pipe stage UI on every PTY read | GPU frame rate drops, wgpu pipeline stalls | Only update pipe stage UI on buffer threshold changes or explicit expand/collapse | Pipelines with continuous output (tail -f \| grep) |
| Parsing pipe structure on every command | CPU spike on `CommandExecuted` even for non-piped commands | Fast-path check: scan for unquoted `|` first, full parsing only if found | High command frequency (scripted loops) |
| Keeping all stage buffers live in memory | RAM grows proportional to (stages x cap x active_commands) | Drop stage buffers to disk/DB when command completes; keep only summary in memory | Sessions with hundreds of completed piped commands |

## Security Mistakes

| Mistake | Risk | Prevention |
|---------|------|------------|
| Temp files for tee capture in world-readable /tmp | Other users/processes read sensitive piped data (DB queries, API keys, credentials) | Use in-memory capture via PTY-level interception. If files needed, user-private dir with 0600 permissions. |
| Command rewriting with string concatenation | Shell injection via crafted filenames, branch names, or prompt content | Avoid command rewriting entirely. If unavoidable, use only Glass-controlled constants. |
| Capturing pipe output containing secrets | Secrets persisted in Glass history DB or blob store indefinitely | Apply existing retention policies to pipe stage data. Add per-command opt-out. Document this risk. |
| MCP GlassPipeInspect exposing internal pipeline data | AI assistants could access sensitive intermediate data from pipelines | Apply same access controls as existing MCP tools. Respect per-command opt-out flags. |
| Shell integration script modifications for pipe capture | Modified glass.ps1 could be targeted for prompt injection | Keep pipe logic in Rust side. Shell scripts should remain minimal (current 123 lines is good). |

## UX Pitfalls

| Pitfall | User Impact | Better Approach |
|---------|-------------|-----------------|
| Showing pipe visualization for EVERY piped command | Visual noise -- most pipes are simple (`ls \| head`). Clutters the block view. | Default to collapsed/minimal view. Only auto-expand for failed pipelines (non-zero exit) or user click. |
| Pipe stage output shown with raw ANSI escape codes | `\x1b[31m` text in stage preview is unreadable | Strip ANSI codes for stage preview (Glass already does this for output capture). Preserve in full/expanded view. |
| No way to disable pipe visualization | Power users who don't want overhead or visual clutter are stuck | Config option: `[pipes] enabled = true/false`. Respect it globally. |
| Pipe stages shown inline, expanding block height dramatically | A 5-stage pipeline with expanded output dominates scrollback, pushing other blocks offscreen | Use overlay/popup for expanded stage output, or horizontal tab-style layout within the block. |
| Different pipe visualization for bash vs PowerShell without indication | User confusion when switching shells | Clear shell indicator on pipe blocks. Document behavioral differences (object vs byte capture). |
| TTY-sensitive commands showing degraded output in visualization | User sees plain `ls` output in pipe stage preview while the actual terminal showed colored columns | Detect TTY-sensitive commands (ls, git, grep) and add UI note: "output may differ from terminal display due to pipe detection" |

## "Looks Done But Isn't" Checklist

- [ ] **Pipe detection:** Works with `cmd|cmd` (no spaces) not just `cmd | cmd` -- verify spacing variants
- [ ] **Pipe detection:** Works with `cmd |& cmd` (bash stderr pipe) -- verify bash 4+ syntax
- [ ] **Pipe detection:** Does NOT false-trigger on `grep "a|b" file` (pipe inside quotes) -- verify quote awareness
- [ ] **Pipe detection:** Handles multiline commands with `\` continuation -- verify accumulation works
- [ ] **Stage capture:** Handles binary output (images, compressed data) without corruption -- verify with `cat image.png | head -c 100`
- [ ] **Stage capture:** Works with empty stages (e.g., `grep nomatch file | wc -l` where grep produces nothing) -- verify zero-byte stage display
- [ ] **Exit codes:** Pipeline exit code in block decoration matches behavior WITHOUT pipe visualization -- verify with `false | true` and `true | false`
- [ ] **PowerShell:** `Get-Process | Select-Object Name` shows process names, not `System.Diagnostics.Process` -- verify object rendering
- [ ] **Performance:** Key echo latency unchanged (<5ms) when pipe visualization is enabled -- benchmark before/after
- [ ] **Performance:** Memory stays bounded during `find / -type f 2>/dev/null | head -1` -- verify per-stage cap
- [ ] **Storage:** Pipe stage data respects retention policies and gets pruned -- verify after configured retention period
- [ ] **Collapse/expand:** Collapsed pipe blocks show correct summary (stage count, overall exit code) -- verify against actual data
- [ ] **Opt-out:** `[pipes] enabled = false` completely disables all pipe-related processing including detection -- verify zero overhead when disabled
- [ ] **Integration:** Existing `OutputBuffer` whole-command capture still works correctly for piped commands -- verify no regression

## Recovery Strategies

| Pitfall | Recovery Cost | Recovery Steps |
|---------|---------------|----------------|
| TTY detection breaking command output | LOW | Add config flag to disable pipe-level capture, falling back to whole-command capture. No data migration needed. |
| Exit code corruption | MEDIUM | Fix the capture mechanism. Old commands with wrong exit codes in history DB cannot be retroactively corrected. |
| Shell quoting corruption | HIGH | If commands were executed with corrupted arguments, data loss or unintended effects already occurred. This is why command rewriting should be avoided. |
| Buffer explosion (OOM) | LOW | Kill Glass, reduce per-stage cap in config, restart. No persistent damage. |
| PowerShell object loss | LOW | Accept text-only capture. No recovery possible for lost object data -- design limitation, not bug. |
| Security (temp file exposure) | HIGH | Delete exposed temp files, rotate any credentials visible in captured output, switch to in-memory capture. |
| Pipe detection false positive | LOW | Disable pipe viz for affected command pattern. Add pattern to exclusion list. No data corruption. |

## Pitfall-to-Phase Mapping

| Pitfall | Prevention Phase | Verification |
|---------|------------------|--------------|
| TTY detection changes output (P1) | Phase 1: Capture Architecture | Run `ls`, `git status`, `grep --color=auto` through pipe with viz enabled; compare to non-visualized |
| Exit code swallowing (P2) | Phase 1: Capture Architecture | `false \| true` and `true \| false` -- exit codes must match expected values |
| Shell quoting corruption (P3) | Phase 1: Architecture (by avoiding rewriting) | Commands with nested quotes, `$()`, backticks through pipe viz; verify identical execution |
| PowerShell object pipeline (P4) | Phase 1: Architecture + Phase 2: PS capture | `Get-Process \| Where-Object CPU -gt 0 \| Select-Object Name,CPU` -- stage output readable |
| Buffer explosion (P5) | Phase 1: Buffer design | `find / -type f 2>/dev/null \| head -5` -- monitor memory; verify cap respected |
| Pipe detection edge cases (P6) | Phase 1: Pipe parser | Test suite: no spaces, `\|&`, quoted pipes, multiline, nested `$()` |
| Security from rewriting (P7) | Phase 1: Architecture (by choosing PTY-level capture) | Audit: no string concatenation with user input in any shell command construction |
| Per-stage capture performance | Phase 2: Implementation | Benchmark PTY reader throughput with/without pipe viz; verify <5% overhead |
| Storage growth | Phase 3: DB schema + retention | Run 1000 piped commands, measure DB growth, verify pruning |
| UI clutter | Phase 4: Pipeline UI | User testing with mixed piped/non-piped sessions; verify collapsed default works |

## Sources

- [Linux Tools Pipe Behavior Differences](https://www.howtogeek.com/these-linux-tools-behave-very-differently-when-you-pipe-them/) -- TTY vs pipe behavioral changes
- [The TTY Demystified](https://www.linusakesson.net/programming/tty/) -- foundational PTY/TTY architecture
- [A Terminal Case of Linux](https://fasterthanli.me/articles/a-terminal-case-of-linux) -- deep dive on terminal I/O and isatty
- [How to Force git status Color Output](https://www.codestudy.net/blog/force-git-status-to-output-color-on-the-terminal-inside-a-script/) -- git isatty behavior
- [Color and TTYs](https://eklitzke.org/ansi-color-codes) -- why commands disable color in pipes
- [Process Substitution and Race Conditions](https://www.natewoodward.org/blog/2019/11/25/process-substitution-and-race-conditions) -- tee/process substitution races in bash vs ksh vs zsh
- [Greg's Wiki: ProcessSubstitution](https://mywiki.wooledge.org/ProcessSubstitution) -- bash process substitution edge cases
- [Exit Status of Piped Processes (Baeldung)](https://www.baeldung.com/linux/exit-status-piped-processes) -- PIPESTATUS and pipefail behavior
- [Capture Exit Status When Piping Output](https://www.codestudy.net/blog/pipe-output-and-capture-exit-status-in-bash/) -- tee exit code masking problem
- [PIPESTATUS and pipefail](https://www.signorini.ch/content/bash-pipestatus-and-pipefail) -- detailed analysis of exit code propagation
- [PowerShell Pipeline Objects](https://renenyffenegger.ch/notes/Windows/PowerShell/pipeline/index) -- PS object vs text pipeline fundamentals
- [PowerShell Issue #1908: Keep bytes as-is](https://github.com/PowerShell/PowerShell/issues/1908) -- byte stream corruption in PS pipelines
- [PowerShell Issue #4552: Mixed object types](https://github.com/PowerShell/PowerShell/issues/4552) -- heterogeneous pipeline formatting loss
- [Viewing Truncated PowerShell Output](https://greiginsydney.com/viewing-truncated-powershell-output/) -- Format-Table truncation gotchas
- [ConPTY Introduction (Microsoft)](https://devblogs.microsoft.com/commandline/windows-command-line-introducing-the-windows-pseudo-console-conpty/) -- ConPTY architecture and limitations
- [ConPTY Buffer Sync Issues #15976](https://github.com/microsoft/terminal/issues/15976) -- ConPTY buffer desynchronization
- [VSCode ConPTY Performance #214529](https://github.com/microsoft/vscode/issues/214529) -- ConPTY overhead concerns
- [OWASP Command Injection](https://owasp.org/www-community/attacks/Command_Injection) -- shell injection prevention fundamentals
- [Python Buffering and tee (Baeldung)](https://www.baeldung.com/linux/python-buffering-and-tee) -- pipe buffering behavior
- [PowerShell Piped Data Buffering #19036](https://github.com/PowerShell/PowerShell/issues/19036) -- PS pipeline buffering semantics
- Glass source: `crates/glass_terminal/src/pty.rs` -- PTY reader architecture, `READ_BUFFER_SIZE`, `OutputBuffer` integration
- Glass source: `crates/glass_terminal/src/output_capture.rs` -- existing `OutputBuffer` pattern (max_bytes, total_seen, alt-screen detection)
- Glass source: `crates/glass_snapshot/src/command_parser.rs` -- existing pipe-as-unparseable decision, `contains_unparseable_syntax()`
- Glass source: `crates/glass_terminal/src/block_manager.rs` -- block lifecycle, single-block-per-command model
- Glass source: `shell-integration/glass.ps1` -- current shell integration (123 lines, OSC 133 only)

---
*Pitfalls research for: Glass v1.3 -- Pipe Visualization*
*Researched: 2026-03-05*
