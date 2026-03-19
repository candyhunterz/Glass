# Cross-Platform Audit Implementation Plan (Branch 3 of 8)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port pipeline capture (tee rewriting + OSC 133;S/P emission) to zsh and fish, add macOS orphan prevention, fix CI coverage gaps, and clean up cross-platform build/runtime issues.

**Architecture:** The biggest item is XP-1 (pipeline capture for zsh/fish). The bash implementation in `shell-integration/glass.bash` lines 95-309 is the reference: it intercepts Enter, detects top-level pipes, rewrites the command to insert `tee` between stages, and emits OSC sequences. Zsh and fish have different hook/interception mechanisms but the core logic (pipe detection, tee rewriting, OSC emission) is the same. After the shell scripts, the remaining tasks are small targeted fixes.

**Tech Stack:** Zsh scripting (zle widgets, add-zsh-hook), Fish scripting (commandline builtin, event handlers), Rust (#[cfg] blocks, libc), GitHub Actions YAML, wgpu, Cargo build config

**Branch:** `audit/cross-platform` off `master`

**Spec:** `docs/superpowers/specs/2026-03-18-prelaunch-audit-fixes-design.md` Branch 3

---

### Task 1: Branch setup + zsh pipeline capture (XP-1 part 1)

**Files:**
- Modify: `shell-integration/glass.zsh` (add pipeline capture after line 73)

This is the largest single task. Port all four bash pipeline functions to zsh equivalents, plus wire them into the zsh command execution lifecycle.

- [ ] **Step 1: Create branch**

```bash
git checkout -b audit/cross-platform master
```

- [ ] **Step 2: Add `__glass_has_pipes` for zsh**

Append to `shell-integration/glass.zsh` before the final hook registration. The logic is identical to bash -- walk characters tracking quote state and nesting depth, return 0 if an unquoted top-level `|` is found (not `||`).

```zsh
# -------------------------------------------------------------------
# Pipeline capture: tee rewriting + OSC 133;S/P emission
#
# Intercepts piped commands at Enter, rewrites them to insert tee
# between stages, captures intermediate output to temp files, and
# emits OSC 133;S (pipeline start) and 133;P (per-stage data) so
# the terminal can display pipe stage output.
# -------------------------------------------------------------------

# State variables for pipeline capture
__glass_capture_tmpdir=""
__glass_capture_stage_count=0

# Detect whether a command line contains unquoted top-level pipes (not ||).
__glass_has_pipes() {
    local cmd="$1"
    [[ "$cmd" == __glass_* ]] && return 1
    [[ "$cmd" == *"--no-glass"* ]] && return 1
    local in_sq=0 in_dq=0 depth=0 i=0 len=${#cmd}
    while [[ $i -lt $len ]]; do
        local c="${cmd:$i:1}"
        if [[ $in_sq -eq 1 ]]; then
            [[ "$c" == "'" ]] && in_sq=0
        elif [[ $in_dq -eq 1 ]]; then
            if [[ "$c" == '\\' ]]; then
                ((i++))
            elif [[ "$c" == '"' ]]; then
                in_dq=0
            fi
        elif [[ "$c" == '\\' ]]; then
            ((i++))
        elif [[ "$c" == "'" ]]; then
            in_sq=1
        elif [[ "$c" == '"' ]]; then
            in_dq=1
        elif [[ "$c" == '$' && "${cmd:$((i+1)):1}" == '(' ]]; then
            ((depth++))
            ((i++))
        elif [[ "$c" == '(' ]]; then
            ((depth++))
        elif [[ "$c" == ')' ]]; then
            ((depth > 0)) && ((depth--))
        elif [[ "$c" == '`' ]]; then
            ((i++))
            while [[ $i -lt $len ]]; do
                [[ "${cmd:$i:1}" == '\\' ]] && ((i++))
                [[ "${cmd:$i:1}" == '`' ]] && break
                ((i++))
            done
        elif [[ "$c" == '|' && $depth -eq 0 ]]; then
            local next="${cmd:$((i+1)):1}"
            [[ "$next" != '|' ]] && return 0
        fi
        ((i++))
    done
    return 1
}
```

Key difference from bash: none for this function -- zsh supports the same `[[ ]]` syntax and `local` keyword.

- [ ] **Step 3: Add `__glass_tee_rewrite` for zsh**

Same character-walking logic as bash. Zsh string slicing uses the same `${cmd:$i:1}` syntax. The function builds a rewritten command with `tee` inserted at each pipe boundary.

```zsh
__glass_tee_rewrite() {
    local cmd="$1"
    local tmpdir="$2"
    local result="" current="" stage_idx=0
    local in_sq=0 in_dq=0 depth=0 i=0 len=${#cmd}

    while [[ $i -lt $len ]]; do
        local c="${cmd:$i:1}"
        if [[ $in_sq -eq 1 ]]; then
            current+="$c"
            [[ "$c" == "'" ]] && in_sq=0
        elif [[ $in_dq -eq 1 ]]; then
            current+="$c"
            if [[ "$c" == '\\' ]]; then
                ((i++))
                current+="${cmd:$i:1}"
            elif [[ "$c" == '"' ]]; then
                in_dq=0
            fi
        elif [[ "$c" == '\\' ]]; then
            current+="$c"
            ((i++))
            current+="${cmd:$i:1}"
        elif [[ "$c" == "'" ]]; then
            in_sq=1
            current+="$c"
        elif [[ "$c" == '"' ]]; then
            in_dq=1
            current+="$c"
        elif [[ "$c" == '$' && "${cmd:$((i+1)):1}" == '(' ]]; then
            ((depth++))
            current+='$('
            ((i++))
        elif [[ "$c" == '(' ]]; then
            ((depth++))
            current+="$c"
        elif [[ "$c" == ')' ]]; then
            ((depth > 0)) && ((depth--))
            current+="$c"
        elif [[ "$c" == '`' ]]; then
            current+='`'
            ((i++))
            while [[ $i -lt $len ]]; do
                local bc="${cmd:$i:1}"
                current+="$bc"
                if [[ "$bc" == '\\' ]]; then
                    ((i++))
                    current+="${cmd:$i:1}"
                elif [[ "$bc" == '`' ]]; then
                    break
                fi
                ((i++))
            done
        elif [[ "$c" == '|' && $depth -eq 0 ]]; then
            local next="${cmd:$((i+1)):1}"
            if [[ "$next" == '|' ]]; then
                current+="||"
                ((i+=2))
                continue
            fi
            result+="${current} | tee '${tmpdir}/stage_${stage_idx}' |"
            current=""
            ((stage_idx++))
            ((i++))
            continue
        else
            current+="$c"
        fi
        ((i++))
    done
    result+="$current"
    __glass_capture_stage_count=$stage_idx
    printf '%s' "$result"
}
```

- [ ] **Step 4: Add `__glass_emit_stages` and `__glass_cleanup_stages` for zsh**

```zsh
__glass_emit_stages() {
    local tmpdir="$__glass_capture_tmpdir"
    [[ -z "$tmpdir" || ! -d "$tmpdir" ]] && return

    local count="$__glass_capture_stage_count"
    [[ -z "$count" || "$count" -eq 0 ]] && return

    printf '\e]133;S;%d\e\\' "$count"

    local i=0
    while [[ $i -lt $count ]]; do
        local path="${tmpdir}/stage_${i}"
        if [[ -f "$path" ]]; then
            local size
            size=$(wc -c < "$path" 2>/dev/null || echo 0)
            size=$(echo "$size" | tr -d ' ')
            printf '\e]133;P;%d;%d;%s\e\\' "$i" "$size" "$path"
        fi
        ((i++))
    done

    __glass_capture_tmpdir=""
    __glass_capture_stage_count=0
}

__glass_cleanup_stages() {
    local pattern="${TMPDIR:-/tmp}/glass_${$}_*"
    for d in $~pattern; do
        [[ -d "$d" ]] && rm -rf "$d" 2>/dev/null
    done
}
```

Key zsh difference: use `$~pattern` for glob expansion (zsh does not expand globs in unquoted variables by default unlike bash).

- [ ] **Step 5: Wire pipeline interception into zsh via zle widget**

Zsh uses `zle` widgets instead of bash's `bind -x`. Define a custom widget that intercepts Enter, rewrites pipeline commands, then calls the real `accept-line`:

```zsh
# Enter key interception via zle widget
__glass_accept_line_widget() {
    [[ "$GLASS_PIPES_DISABLED" == "1" ]] && { zle accept-line; return; }
    local cmd="$BUFFER"

    if [[ -n "$cmd" ]] && __glass_has_pipes "$cmd"; then
        local tmpdir="${TMPDIR:-/tmp}/glass_${$}_$(date +%s%N)"
        if ! mkdir -p "$tmpdir" 2>/dev/null; then
            zle accept-line
            return
        fi
        __glass_capture_tmpdir="$tmpdir"

        local rewritten
        rewritten=$(__glass_tee_rewrite "$cmd" "$tmpdir")

        # Append pipestatus capture + stage emission
        # zsh uses $pipestatus (lowercase array) instead of bash $PIPESTATUS
        BUFFER="${rewritten}; __glass_pipestatus=(\${pipestatus[@]}); __glass_emit_stages"
        CURSOR=${#BUFFER}
    fi
    zle accept-line
}
zle -N __glass_accept_line_widget
bindkey '^M' __glass_accept_line_widget
```

Key zsh differences from bash:
- `$BUFFER` / `$CURSOR` instead of `$READLINE_LINE` / `$READLINE_POINT`
- `zle accept-line` instead of `bind '"\C-j": accept-line'`
- `$pipestatus` (lowercase) instead of `$PIPESTATUS`
- `zle -N` to register the widget, `bindkey '^M'` to bind Enter

- [ ] **Step 6: Add cleanup call to `__glass_precmd`**

In the existing `__glass_precmd` function, add the cleanup call (matching how bash does it in `__glass_prompt_command`):

```zsh
__glass_precmd() {
    local exit_code=$?
    printf '\e]133;D;%d\e\\' "$exit_code"
    __glass_osc7
    printf '\e]133;A\e\\'
    # Clean up temp files from previous pipeline captures
    __glass_cleanup_stages 2>/dev/null
}
```

- [ ] **Step 7: Build and test**

Manual testing required since these are shell scripts, not Rust:

```bash
# Verify zsh syntax is valid
zsh -n shell-integration/glass.zsh

# Verify Rust still builds (no changes yet)
cargo build 2>&1
```

- [ ] **Step 8: Commit**

```bash
git add shell-integration/glass.zsh
git commit -m "feat(XP-1): port pipeline capture to zsh

Add __glass_has_pipes, __glass_tee_rewrite, __glass_emit_stages,
__glass_cleanup_stages, and __glass_accept_line_widget. Uses zle
widget for Enter interception. Emits OSC 133;S/P for pipe stages."
```

---

### Task 2: Fish pipeline capture (XP-1 part 2)

**Files:**
- Modify: `shell-integration/glass.fish` (add pipeline capture after line 72)

Fish has fundamentally different syntax from bash/zsh (no C-style for loops, no `[[ ]]`, different variable scoping). The pipe detection and tee rewriting logic must be rewritten in fish idioms.

- [ ] **Step 1: Add `__glass_has_pipes` for fish**

Fish does not have `[[ ]]` or C-style character iteration. Use `string` builtins for parsing. The approach: use `string split` and `string match` for a simplified but correct pipe detection.

```fish
# -------------------------------------------------------------------
# Pipeline capture: tee rewriting + OSC 133;S/P emission
# -------------------------------------------------------------------

set -g __glass_capture_tmpdir ""
set -g __glass_capture_stage_count 0

# Detect whether a command line contains unquoted top-level pipes (not ||).
function __glass_has_pipes
    set -l cmd $argv[1]
    # Skip internal functions
    string match -q "__glass_*" -- $cmd; and return 1
    string match -q "*--no-glass*" -- $cmd; and return 1

    # Walk characters tracking quote state and nesting depth
    set -l chars (string split "" -- $cmd)
    set -l len (count $chars)
    set -l in_sq 0
    set -l in_dq 0
    set -l depth 0
    set -l i 1  # fish arrays are 1-based
    while test $i -le $len
        set -l c $chars[$i]
        if test $in_sq -eq 1
            test "$c" = "'"; and set in_sq 0
        else if test $in_dq -eq 1
            if test "$c" = "\\"
                set i (math $i + 1)
            else if test "$c" = '"'
                set in_dq 0
            end
        else if test "$c" = "\\"
            set i (math $i + 1)
        else if test "$c" = "'"
            set in_sq 1
        else if test "$c" = '"'
            set in_dq 1
        else if test "$c" = '('
            set depth (math $depth + 1)
        else if test "$c" = ')'
            test $depth -gt 0; and set depth (math $depth - 1)
        else if test "$c" = '|' -a $depth -eq 0
            set -l next_i (math $i + 1)
            if test $next_i -le $len
                set -l next $chars[$next_i]
                test "$next" != '|'; and return 0
            else
                return 0  # trailing pipe
            end
        end
        set i (math $i + 1)
    end
    return 1
end
```

Key fish differences:
- 1-based arrays
- `set -l` instead of `local`
- `test` instead of `[[ ]]`
- `math` instead of `$(())`
- `string split ""` for character iteration
- No `$()` subshell syntax -- fish uses `()` which is handled differently in pipe detection (no need to track `$(`; just track `(` nesting)

- [ ] **Step 2: Add `__glass_tee_rewrite` for fish**

```fish
function __glass_tee_rewrite
    set -l cmd $argv[1]
    set -l tmpdir $argv[2]
    set -l result ""
    set -l current ""
    set -l stage_idx 0
    set -l in_sq 0
    set -l in_dq 0
    set -l depth 0
    set -l chars (string split "" -- $cmd)
    set -l len (count $chars)
    set -l i 1

    while test $i -le $len
        set -l c $chars[$i]
        if test $in_sq -eq 1
            set current "$current$c"
            test "$c" = "'"; and set in_sq 0
        else if test $in_dq -eq 1
            set current "$current$c"
            if test "$c" = "\\"
                set i (math $i + 1)
                test $i -le $len; and set current "$current$chars[$i]"
            else if test "$c" = '"'
                set in_dq 0
            end
        else if test "$c" = "\\"
            set current "$current$c"
            set i (math $i + 1)
            test $i -le $len; and set current "$current$chars[$i]"
        else if test "$c" = "'"
            set in_sq 1
            set current "$current$c"
        else if test "$c" = '"'
            set in_dq 1
            set current "$current$c"
        else if test "$c" = '('
            set depth (math $depth + 1)
            set current "$current$c"
        else if test "$c" = ')'
            test $depth -gt 0; and set depth (math $depth - 1)
            set current "$current$c"
        else if test "$c" = '|' -a $depth -eq 0
            set -l next_i (math $i + 1)
            if test $next_i -le $len; and test "$chars[$next_i]" = '|'
                set current "$current||"
                set i (math $i + 2)
                continue
            end
            set result "$result$current | tee '$tmpdir/stage_$stage_idx' |"
            set current ""
            set stage_idx (math $stage_idx + 1)
            set i (math $i + 1)
            continue
        else
            set current "$current$c"
        end
        set i (math $i + 1)
    end
    set result "$result$current"
    set -g __glass_capture_stage_count $stage_idx
    echo -n "$result"
end
```

- [ ] **Step 3: Add `__glass_emit_stages` and `__glass_cleanup_stages` for fish**

```fish
function __glass_emit_stages
    set -l tmpdir $__glass_capture_tmpdir
    test -z "$tmpdir" -o ! -d "$tmpdir"; and return

    set -l count $__glass_capture_stage_count
    test -z "$count" -o "$count" -eq 0; and return

    printf '\e]133;S;%d\e\\' $count

    set -l i 0
    while test $i -lt $count
        set -l path "$tmpdir/stage_$i"
        if test -f "$path"
            set -l size (wc -c < "$path" 2>/dev/null; or echo 0)
            set size (string trim -- $size)
            printf '\e]133;P;%d;%d;%s\e\\' $i $size $path
        end
        set i (math $i + 1)
    end

    set -g __glass_capture_tmpdir ""
    set -g __glass_capture_stage_count 0
end

function __glass_cleanup_stages
    set -l pattern (printf "%s/glass_%d_*" (set -q TMPDIR; and echo $TMPDIR; or echo /tmp) %self)
    for d in $pattern
        test -d "$d"; and rm -rf "$d" 2>/dev/null
    end
end
```

Key fish difference: `%self` is fish's equivalent of `$$` (current PID).

- [ ] **Step 4: Wire pipeline interception into fish via `fish_preexec` event**

Fish does not have readline `bind -x` or zle widgets. Instead, use the `fish_preexec` event which fires after Enter but before execution, and receives the command text as `$argv[1]`. Modify the existing `__glass_preexec` function:

```fish
function __glass_preexec --on-event fish_preexec
    printf '\e]133;B\e\\'
    printf '\e]133;C\e\\'

    # Pipeline capture
    if not set -q GLASS_PIPES_DISABLED; or test "$GLASS_PIPES_DISABLED" != "1"
        set -l cmd $argv[1]
        if test -n "$cmd"; and __glass_has_pipes "$cmd"
            set -l tmpdir (printf "%s/glass_%d_%s" (set -q TMPDIR; and echo $TMPDIR; or echo /tmp) %self (date +%s%N))
            if mkdir -p "$tmpdir" 2>/dev/null
                set -g __glass_capture_tmpdir "$tmpdir"
                # Fish limitation: fish_preexec cannot modify the command
                # being executed. We must use a different approach.
                # See Step 5 for the keybinding approach instead.
            end
        end
    end
end
```

**Important:** Unlike bash and zsh, fish's `fish_preexec` does NOT allow modifying the command before execution. The command text is read-only. Therefore, fish pipeline capture needs a different approach.

- [ ] **Step 5: Use fish keybinding for command rewriting**

Since `fish_preexec` cannot modify the command, use a custom keybinding for Enter that reads and rewrites the commandline buffer before execution (similar to the zsh zle approach):

```fish
function __glass_accept_line
    set -l cmd (commandline)
    if test -n "$cmd"; and not set -q GLASS_PIPES_DISABLED; and __glass_has_pipes "$cmd"
        set -l tmpdir (printf "%s/glass_%d_%s" (set -q TMPDIR; and echo $TMPDIR; or echo /tmp) %self (date +%s%N))
        if mkdir -p "$tmpdir" 2>/dev/null
            set -g __glass_capture_tmpdir "$tmpdir"
            set -l rewritten (__glass_tee_rewrite "$cmd" "$tmpdir")
            # Append stage emission after the pipeline
            # fish uses $pipestatus for per-stage exit codes
            commandline -r "$rewritten; __glass_emit_stages"
        end
    end
    commandline -f execute
end

bind \r __glass_accept_line
bind \n __glass_accept_line
```

Revert the `__glass_preexec` modification from Step 4 -- remove the pipeline capture logic from it since it is handled by the keybinding. The preexec function should remain as it was originally (just emitting OSC 133;B and 133;C).

- [ ] **Step 6: Add cleanup call to `__glass_prompt` handler**

```fish
function __glass_prompt --on-event fish_prompt
    set -l exit_code $status
    printf '\e]133;D;%d\e\\' $exit_code
    __glass_osc7
    printf '\e]133;A\e\\'
    # Clean up temp files from previous pipeline captures
    __glass_cleanup_stages 2>/dev/null
end
```

- [ ] **Step 7: Validate fish syntax**

```bash
# Check fish syntax (if fish is installed)
fish -n shell-integration/glass.fish 2>&1 || echo "fish not installed, skip syntax check"
```

- [ ] **Step 8: Commit**

```bash
git add shell-integration/glass.fish
git commit -m "feat(XP-1): port pipeline capture to fish

Add __glass_has_pipes, __glass_tee_rewrite, __glass_emit_stages,
__glass_cleanup_stages, and __glass_accept_line keybinding. Uses
commandline builtin for Enter interception and buffer rewriting.
Emits OSC 133;S/P for pipe stages."
```

---

### Task 3: macOS orphan prevention watchdog (XP-2)

**Files:**
- Modify: `src/main.rs` (add `#[cfg(target_os = "macos")]` watchdog near the Linux `PR_SET_PDEATHSIG` block at line ~1134)
- Modify: `src/ephemeral_agent.rs` (same pattern for ephemeral agent spawns)

Currently: Linux uses `prctl(PR_SET_PDEATHSIG, SIGKILL)` (line 1134-1145), Windows uses Job Objects (line 787-838), macOS has nothing.

- [ ] **Step 1: Add macOS watchdog function**

In `src/main.rs`, near the `setup_windows_job_object` function, add:

```rust
/// macOS orphan prevention: spawn a watchdog thread that periodically checks
/// if the parent process has died (reparented to launchd/PID 1). If so,
/// kill the child process.
///
/// macOS does not have prctl(PR_SET_PDEATHSIG) or Windows Job Objects,
/// so polling getppid() is the standard approach.
#[cfg(target_os = "macos")]
fn spawn_macos_orphan_watchdog(child_pid: u32) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("glass-orphan-watchdog".into())
        .spawn(move || {
            let original_ppid = unsafe { libc::getppid() };
            loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let current_ppid = unsafe { libc::getppid() };
                if current_ppid == 1 || current_ppid != original_ppid {
                    tracing::warn!(
                        "macOS orphan watchdog: parent died (ppid {} -> {}), killing child {}",
                        original_ppid, current_ppid, child_pid
                    );
                    unsafe {
                        libc::kill(child_pid as i32, libc::SIGKILL);
                    }
                    break;
                }
            }
        })
        .expect("Failed to spawn orphan watchdog thread")
}
```

- [ ] **Step 2: Wire into agent spawn path**

In the agent spawn code in `src/main.rs`, after the child process is spawned (near line 1149 after the `cmd.spawn()` call), add:

```rust
#[cfg(target_os = "macos")]
{
    let child_pid = child.id();
    let _watchdog = spawn_macos_orphan_watchdog(child_pid);
    // Store handle in AgentRuntime to keep thread alive
}
```

Add a field to the `AgentRuntime` struct (or equivalent) to hold the watchdog handle:

```rust
#[cfg(target_os = "macos")]
orphan_watchdog: Option<std::thread::JoinHandle<()>>,
```

- [ ] **Step 3: Apply same pattern to ephemeral_agent.rs**

In `src/ephemeral_agent.rs`, after spawning the claude process, add the same watchdog:

```rust
#[cfg(target_os = "macos")]
{
    let child_pid = child.id();
    let _watchdog = crate::spawn_macos_orphan_watchdog(child_pid);
}
```

Or extract `spawn_macos_orphan_watchdog` to a shared module if it needs to be called from both files. If `ephemeral_agent.rs` is in the same crate, a `pub(crate)` function in `main.rs` works, but it may be cleaner to put it in a small `src/platform.rs` module.

- [ ] **Step 4: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

Note: The `#[cfg(target_os = "macos")]` blocks won't compile on Windows. Verify no syntax errors by checking that the non-macOS build still succeeds. Full validation requires macOS CI (addressed in Task 4).

- [ ] **Step 5: Commit**

```bash
git add src/main.rs src/ephemeral_agent.rs
git commit -m "feat(XP-2): add macOS orphan prevention via getppid watchdog

Spawn a background thread that polls getppid() every 2 seconds.
If reparented to PID 1 (parent died), SIGKILL the child process.
Covers both AgentRuntime and ephemeral agent spawns."
```

---

### Task 4: CI clippy on all platforms (XP-3)

**Files:**
- Modify: `.github/workflows/ci.yml` (lines 45-54, convert clippy to matrix)

Currently clippy runs only on `windows-latest`. Platform-specific `#[cfg]` blocks (like the macOS watchdog just added) are not checked by clippy on other platforms.

- [ ] **Step 1: Convert clippy job to matrix**

In `.github/workflows/ci.yml`, replace the clippy job:

```yaml
  clippy:
    name: Clippy (${{ matrix.os }})
    strategy:
      matrix:
        os: [windows-latest, macos-latest, ubuntu-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - uses: Swatinem/rust-cache@v2
        with:
          key: clippy-${{ matrix.os }}
      - name: Install Linux dependencies
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y libxkbcommon-dev libwayland-dev libxtst-dev
      - run: cargo clippy --workspace -- -D warnings
```

- [ ] **Step 2: Verify CI YAML is valid**

```bash
# Basic YAML syntax check
python -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))" 2>&1 || echo "yaml check not available"
```

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci(XP-3): run clippy on all three platforms (Windows, macOS, Linux)

Convert single-OS clippy job to a matrix. Catches platform-specific
cfg block issues that single-platform linting misses."
```

---

### Task 5: Vulkan fallback on Windows (XP-4)

**Files:**
- Modify: `crates/glass_renderer/src/surface.rs:23-24`

Currently Windows forces DX12 only. Some Windows machines (especially VMs, older hardware, or Wine) lack DX12 but have Vulkan.

- [ ] **Step 1: Add Vulkan as fallback backend**

In `crates/glass_renderer/src/surface.rs:23-24`:

```rust
// BEFORE:
#[cfg(target_os = "windows")]
backends: wgpu::Backends::DX12,

// AFTER:
#[cfg(target_os = "windows")]
backends: wgpu::Backends::DX12 | wgpu::Backends::VULKAN,
```

wgpu tries backends in order of preference. DX12 will still be preferred when available; Vulkan is only used as fallback.

- [ ] **Step 2: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 3: Commit**

```bash
git add crates/glass_renderer/src/surface.rs
git commit -m "fix(XP-4): add Vulkan as fallback GPU backend on Windows

DX12 remains preferred. Vulkan is used as fallback for environments
where DX12 is unavailable (VMs, older hardware, Wine)."
```

---

### Task 6: winresource Windows-only build dep (XP-5)

**Files:**
- Modify: `Cargo.toml:132-133`

`winresource` is a build dependency used to embed Windows resources (icon, version info). It is currently listed unconditionally, which means it gets downloaded and compiled on macOS/Linux where it is useless.

- [ ] **Step 1: Move to target-specific build-dependencies**

In `Cargo.toml`, change:

```toml
# BEFORE:
[build-dependencies]
winresource = "0.1"

# AFTER:
[target.'cfg(windows)'.build-dependencies]
winresource = "0.1"
```

- [ ] **Step 2: Guard build.rs usage**

Check `build.rs` at the repo root. If it uses `winresource` unconditionally, wrap it in `#[cfg(windows)]` or a runtime check:

```rust
fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        // ... existing resource embedding code
        res.compile().unwrap();
    }
}
```

- [ ] **Step 3: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml build.rs
git commit -m "fix(XP-5): make winresource a Windows-only build dependency

Move from [build-dependencies] to [target.'cfg(windows)'.build-dependencies].
Avoids downloading/compiling winresource on macOS and Linux."
```

---

### Task 7: macOS Intel release binary (XP-6)

**Files:**
- Modify: `.github/workflows/release.yml:75-118`

Currently the macOS release builds only `aarch64-apple-darwin` (Apple Silicon). Intel Macs (`x86_64-apple-darwin`) are not supported.

- [ ] **Step 1: Add x86_64 target to macOS build matrix**

Convert the `build-macos` job to use a matrix:

```yaml
  build-macos:
    name: Build macOS Installer (${{ matrix.arch }})
    runs-on: macos-latest
    strategy:
      matrix:
        include:
          - arch: aarch64
            target: aarch64-apple-darwin
          - arch: x86_64
            target: x86_64-apple-darwin
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - uses: Swatinem/rust-cache@v2
        with:
          key: release-${{ matrix.target }}

      - name: Verify version matches tag
        run: |
          CARGO_VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
          TAG_VERSION="${GITHUB_REF_NAME#v}"
          if [[ "$TAG_VERSION" =~ ^[0-9]+\.[0-9]+$ ]]; then
            TAG_VERSION="${TAG_VERSION}.0"
          fi
          if [ "$CARGO_VERSION" != "$TAG_VERSION" ]; then
            echo "ERROR: Cargo.toml version ($CARGO_VERSION) does not match tag ($TAG_VERSION)"
            exit 1
          fi

      - name: Build release binary
        run: cargo build --release --target ${{ matrix.target }}

      - name: Build DMG installer
        run: bash packaging/macos/build-dmg.sh "${GITHUB_REF_NAME#v}" "${{ matrix.target }}"

      - name: Upload DMG to release
        uses: softprops/action-gh-release@v2
        with:
          files: target/macos/*.dmg
```

Note: The `build-dmg.sh` script may need to accept the target triple as a second argument to find the binary in `target/<triple>/release/` instead of `target/release/`. Check and update `packaging/macos/build-dmg.sh` accordingly.

- [ ] **Step 2: Verify build-dmg.sh handles target parameter**

Read `packaging/macos/build-dmg.sh` and update it to accept the target triple. If it currently looks for `target/release/glass`, update it to also accept `target/<triple>/release/glass`.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml packaging/macos/build-dmg.sh
git commit -m "ci(XP-6): add macOS x86_64 (Intel) release binary

Convert macOS release job to matrix with aarch64 and x86_64 targets.
Ships DMG installers for both Apple Silicon and Intel Macs."
```

---

### Task 8: IPC path constant deduplication (XP-7)

**Files:**
- Modify: `crates/glass_core/src/ipc.rs:75-86` (make functions pub)
- Modify: `crates/glass_mcp/src/ipc_client.rs:119-134` (remove duplication, import from glass_core)
- Modify: `crates/glass_mcp/Cargo.toml` (add glass_core dependency if not present)

The IPC socket path (`glass.sock`) and pipe name (`\\.\pipe\glass-terminal`) are duplicated between `glass_core::ipc` and `glass_mcp::ipc_client`, with a comment in the MCP client saying "duplicated to avoid the heavy dependency."

- [ ] **Step 1: Check if glass_mcp already depends on glass_core**

```bash
grep "glass_core" crates/glass_mcp/Cargo.toml
```

If it does, the "avoid heavy dependency" comment is outdated and we can just import. If not, evaluate whether adding the dependency is acceptable (glass_core is relatively lightweight).

- [ ] **Step 2: Option A — glass_mcp already depends on glass_core**

Remove the duplicated functions from `crates/glass_mcp/src/ipc_client.rs` and replace with imports:

```rust
// BEFORE (lines 119-134):
/// Returns the IPC socket path on Unix platforms.
/// Duplicated from `glass_core::ipc::ipc_socket_path()` to avoid the heavy dependency.
#[cfg(unix)]
fn ipc_socket_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join(".glass")
        .join("glass.sock")
}
#[cfg(windows)]
fn ipc_pipe_name() -> String {
    r"\\.\pipe\glass-terminal".to_string()
}

// AFTER:
use glass_core::ipc::{ipc_socket_path, ipc_pipe_name};
```

Ensure both functions in `glass_core::ipc` are `pub`:

```rust
pub fn ipc_socket_path() -> std::path::PathBuf { ... }
#[cfg(windows)]
pub fn ipc_pipe_name() -> String { ... }
```

- [ ] **Step 3: Option B — glass_mcp does NOT depend on glass_core**

Extract the IPC path constants to a thin shared location. Two approaches:

**Approach B1:** Add `glass_core` as a dependency of `glass_mcp` (simplest, preferred).

**Approach B2:** Extract to a const in both crates (if glass_core dep is truly too heavy):

```rust
// In both files, replace function bodies with shared constants:
const GLASS_UNIX_SOCKET_NAME: &str = "glass.sock";
const GLASS_WINDOWS_PIPE_NAME: &str = r"\\.\pipe\glass-terminal";
```

Prefer Approach B1 unless there is a concrete reason to avoid the dependency.

- [ ] **Step 4: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add crates/glass_core/src/ipc.rs crates/glass_mcp/src/ipc_client.rs crates/glass_mcp/Cargo.toml
git commit -m "refactor(XP-7): deduplicate IPC path constants between glass_core and glass_mcp

Remove duplicated ipc_socket_path() and ipc_pipe_name() from
glass_mcp::ipc_client. Import from glass_core::ipc instead.
Prevents future drift between the two definitions."
```

---

### Task 9: Job Object handle wrapper (XP-8)

**Files:**
- Modify: `src/main.rs:387-389` (replace raw isize)
- Modify: `src/main.rs:787-838` (setup_windows_job_object return type)

Currently the Windows Job Object handle is stored as a raw `isize` with `#[allow(dead_code)]` to suppress the unused warning. The handle is leaked on drop (no `CloseHandle` call), relying on process exit for cleanup.

- [ ] **Step 1: Create a newtype wrapper with Drop**

Add in `src/main.rs` (or a `src/platform.rs` module):

```rust
/// RAII wrapper for a Windows Job Object HANDLE.
/// Calls `CloseHandle` on drop, which triggers `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
#[cfg(target_os = "windows")]
struct JobObjectHandle(isize);

#[cfg(target_os = "windows")]
impl Drop for JobObjectHandle {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(
                    self.0 as windows_sys::Win32::Foundation::HANDLE,
                );
            }
        }
    }
}
```

- [ ] **Step 2: Update setup_windows_job_object return type**

```rust
// BEFORE:
fn setup_windows_job_object() -> Option<isize> {
    // ...
    Some(job as isize)
}

// AFTER:
fn setup_windows_job_object() -> Option<JobObjectHandle> {
    // ...
    Some(JobObjectHandle(job as isize))
}
```

- [ ] **Step 3: Update the field on the struct**

```rust
// BEFORE (line 387-389):
#[cfg(target_os = "windows")]
#[allow(dead_code)]
job_object_handle: Option<isize>,

// AFTER:
#[cfg(target_os = "windows")]
job_object_handle: Option<JobObjectHandle>,
```

Remove `#[allow(dead_code)]` -- the Drop impl means the field is no longer "dead" since dropping it has a side effect.

- [ ] **Step 4: Build and test**

```bash
cargo build 2>&1
cargo test --workspace 2>&1
```

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "refactor(XP-8): wrap Windows Job Object handle in RAII newtype

Replace raw isize with JobObjectHandle that calls CloseHandle on drop.
Ensures kill-on-close triggers correctly on all exit paths, not just
process termination."
```

---

### Task 10: Final verification and clippy

- [ ] **Step 1: Run clippy**

```bash
cargo clippy --workspace -- -D warnings 2>&1
```

Fix any warnings.

- [ ] **Step 2: Run fmt**

```bash
cargo fmt --all -- --check 2>&1
```

Fix any formatting issues.

- [ ] **Step 3: Run full test suite**

```bash
cargo test --workspace 2>&1
```

- [ ] **Step 4: Commit any cleanup**

```bash
git add -A
git commit -m "chore: clippy and fmt cleanup for cross-platform branch"
```

- [ ] **Step 5: Manual smoke tests**

Shell scripts cannot be unit-tested in CI easily. Manual verification checklist:

- **Zsh pipeline capture:** In a Glass terminal with zsh, run `echo hello | grep h | wc -l`. Verify OSC 133;S/P sequences appear in Glass debug log and pipe stage visualization works.
- **Fish pipeline capture:** Same test in a fish shell.
- **Pipe-free commands:** Verify that `ls`, `echo hello`, and `echo "foo|bar"` (quoted pipe) do NOT trigger pipeline capture in zsh and fish.
- **Complex pipes:** Verify `echo $(cat file | grep x) | wc` only intercepts the outer pipe, not the subshell pipe.
- **macOS orphan:** On macOS, start Glass, spawn an agent, kill Glass via `kill -9`. Verify the agent process also dies within ~4 seconds.

- [ ] **Step 6: Summary — verify all items addressed**

Check off against the spec:
- [x] XP-1: Pipeline capture for zsh (Task 1)
- [x] XP-1: Pipeline capture for fish (Task 2)
- [x] XP-2: macOS orphan prevention (Task 3)
- [x] XP-3: Clippy on all platforms (Task 4)
- [x] XP-4: Vulkan fallback on Windows (Task 5)
- [x] XP-5: winresource Windows-only (Task 6)
- [x] XP-6: macOS Intel binary (Task 7)
- [x] XP-7: IPC path deduplication (Task 8)
- [x] XP-8: Job Object handle wrapper (Task 9)
