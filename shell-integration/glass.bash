#!/usr/bin/env bash
# Glass Shell Integration for Bash
#
# Emits OSC 133 (command lifecycle) and OSC 7 (CWD) escape sequences
# so that Glass can identify command boundaries and track the working directory.
#
# Usage:
#   source ~/.glass/shell-integration/glass.bash
#
# Requirements:
#   - Bash >= 4.4 for PS0 support (OSC 133;C on command execution)
#   - Works with bash >= 3.x but 133;C will not be emitted
#
# Compatible with Starship and other prompt customizers -- this script
# stashes the original PS1 before modifying it, and prepends to
# PROMPT_COMMAND rather than replacing it.

# Guard against double-sourcing
[[ -n "$__GLASS_INTEGRATION_LOADED" ]] && return
__GLASS_INTEGRATION_LOADED=1

# ---------------------------------------------------------------------------
# CWD reporting via OSC 7
# ---------------------------------------------------------------------------
__glass_osc7() {
    printf '\e]7;file://%s%s\e\\' "$HOSTNAME" "$PWD"
}

# ---------------------------------------------------------------------------
# PROMPT_COMMAND function
#
# Runs before each prompt is displayed.  Sequence:
#   [133;D;<exit_code>]   end of previous command
#   [7;file://HOST/CWD]   current working directory
#   PS1 is rebuilt as:
#     [133;A] <original PS1> [133;B]
# ---------------------------------------------------------------------------
__glass_prompt_command() {
    local exit_code=$?

    # End previous command (OSC 133;D with exit code)
    printf '\e]133;D;%d\e\\' "$exit_code"

    # Report CWD
    __glass_osc7

    # Rebuild PS1 with OSC 133;A (prompt start) and 133;B (command-input start)
    # \[...\] tells bash these are non-printing characters so it calculates
    # prompt length correctly for line editing.
    PS1='\[\e]133;A\e\\\]'"${__GLASS_ORIGINAL_PS1:-\\s-\\v\\$ }"'\[\e]133;B\e\\\]'

    # Clean up temp files from previous pipeline captures
    __glass_cleanup_stages 2>/dev/null
}

# ---------------------------------------------------------------------------
# Stash original PS1
#
# Must happen before we modify PS1.  If Starship or another customizer sets
# PS1 later (via PROMPT_COMMAND), the user should source glass.bash AFTER
# their customizer initialises.
# ---------------------------------------------------------------------------
__GLASS_ORIGINAL_PS1="$PS1"

# ---------------------------------------------------------------------------
# Chain into PROMPT_COMMAND
#
# Prepend our function, preserving any existing PROMPT_COMMAND.
# ---------------------------------------------------------------------------
PROMPT_COMMAND="__glass_prompt_command${PROMPT_COMMAND:+;$PROMPT_COMMAND}"

# ---------------------------------------------------------------------------
# PS0 for OSC 133;C (command executed / output start)
#
# PS0 is evaluated after the user presses Enter but before the command runs.
# Available in bash >= 4.4.
# ---------------------------------------------------------------------------
if [[ "${BASH_VERSINFO[0]}" -ge 5 ]] || \
   [[ "${BASH_VERSINFO[0]}" -eq 4 && "${BASH_VERSINFO[1]}" -ge 4 ]]; then
    PS0='\[\e]133;C\e\\\]'

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

    # Detect whether a command line contains unquoted pipes (not ||)
    __glass_has_pipes() {
        local cmd="$1"
        # Skip internal functions
        [[ "$cmd" == __glass_* ]] && return 1
        # Skip --no-glass commands
        [[ "$cmd" == *"--no-glass"* ]] && return 1
        # Walk characters, tracking quote state
        local in_sq=0 in_dq=0 i=0 len=${#cmd}
        while [[ $i -lt $len ]]; do
            local c="${cmd:$i:1}"
            if [[ $in_sq -eq 1 ]]; then
                [[ "$c" == "'" ]] && in_sq=0
            elif [[ $in_dq -eq 1 ]]; then
                [[ "$c" == '"' ]] && in_dq=0
            elif [[ "$c" == "'" ]]; then
                in_sq=1
            elif [[ "$c" == '"' ]]; then
                in_dq=1
            elif [[ "$c" == '|' ]]; then
                local next="${cmd:$((i+1)):1}"
                [[ "$next" != '|' ]] && return 0  # found unquoted pipe
            fi
            ((i++))
        done
        return 1
    }

    # Rewrite a pipeline command to insert tee between stages.
    # Sets __glass_capture_stage_count as a side effect.
    __glass_tee_rewrite() {
        local cmd="$1"
        local tmpdir="$2"
        local result="" current="" stage_idx=0
        local in_sq=0 in_dq=0 i=0 len=${#cmd}

        while [[ $i -lt $len ]]; do
            local c="${cmd:$i:1}"
            if [[ $in_sq -eq 1 ]]; then
                current+="$c"
                [[ "$c" == "'" ]] && in_sq=0
            elif [[ $in_dq -eq 1 ]]; then
                current+="$c"
                [[ "$c" == '"' ]] && in_dq=0
            elif [[ "$c" == "'" ]]; then
                in_sq=1
                current+="$c"
            elif [[ "$c" == '"' ]]; then
                in_dq=1
                current+="$c"
            elif [[ "$c" == '|' ]]; then
                local next="${cmd:$((i+1)):1}"
                if [[ "$next" == '|' ]]; then
                    # Logical OR -- pass through
                    current+="||"
                    ((i+=2))
                    continue
                fi
                # Pipe boundary: append current stage with tee, then pipe
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

    # Emit OSC 133;S (pipeline start) and 133;P (per-stage) sequences
    __glass_emit_stages() {
        local tmpdir="$__glass_capture_tmpdir"
        [[ -z "$tmpdir" || ! -d "$tmpdir" ]] && return

        local count="$__glass_capture_stage_count"
        [[ -z "$count" || "$count" -eq 0 ]] && return

        # Emit pipeline start marker
        printf '\e]133;S;%d\e\\' "$count"

        # Emit each stage with temp file path
        local i=0
        while [[ $i -lt $count ]]; do
            local path="${tmpdir}/stage_${i}"
            if [[ -f "$path" ]]; then
                local size
                size=$(wc -c < "$path" 2>/dev/null || echo 0)
                size=$(echo "$size" | tr -d ' ')  # macOS wc adds leading spaces
                printf '\e]133;P;%d;%d;%s\e\\' "$i" "$size" "$path"
            fi
            ((i++))
        done

        # Clear state (temp files cleaned up on next prompt cycle)
        __glass_capture_tmpdir=""
        __glass_capture_stage_count=0
    }

    # Clean up temp dirs from previous pipeline executions.
    # Called in __glass_prompt_command -- by the time we get back to the
    # prompt the terminal has already read the temp files.
    __glass_cleanup_stages() {
        local pattern="${TMPDIR:-/tmp}/glass_${$}_*"
        for d in $pattern; do
            [[ -d "$d" ]] && rm -rf "$d" 2>/dev/null
        done
    }

    # Enter key interception: rewrite pipeline commands before execution
    __glass_accept_line() {
        local cmd="$READLINE_LINE"

        if [[ -n "$cmd" ]] && __glass_has_pipes "$cmd"; then
            local tmpdir="${TMPDIR:-/tmp}/glass_${$}_$(date +%s%N)"
            mkdir -p "$tmpdir" 2>/dev/null
            __glass_capture_tmpdir="$tmpdir"

            local rewritten
            rewritten=$(__glass_tee_rewrite "$cmd" "$tmpdir")

            # Append PIPESTATUS capture + stage emission after the pipeline.
            # Use ; not && so emission happens even if pipeline fails.
            # Capture PIPESTATUS immediately (before any other command runs).
            READLINE_LINE="${rewritten}; __glass_pipestatus=(\"\${PIPESTATUS[@]}\"); __glass_emit_stages"
            READLINE_POINT=${#READLINE_LINE}
        fi
    }

    # Bind Enter to intercept pipeline commands.
    # Two-step bind: custom key sequence triggers __glass_accept_line,
    # then \C-j (accept-line) actually executes the command.
    bind -x '"\e[glass-pre": __glass_accept_line'
    bind '"\C-m": "\e[glass-pre\C-j"'
    bind '"\C-j": accept-line'
fi
