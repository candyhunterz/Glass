# Glass Shell Integration for Zsh
#
# Emits OSC 133 (command lifecycle) and OSC 7 (CWD) escape sequences
# so that Glass can identify command boundaries and track the working directory.
#
# Usage:
#   source /path/to/glass/shell-integration/glass.zsh
#
# Compatible with Starship, Oh My Posh, and Powerlevel10k -- this script
# uses add-zsh-hook which cooperates with other precmd/preexec hooks.

# Guard against double-sourcing
[[ -n "$__GLASS_INTEGRATION_LOADED" ]] && return
__GLASS_INTEGRATION_LOADED=1

# ---------------------------------------------------------------------------
# CWD reporting via OSC 7
# ---------------------------------------------------------------------------
__glass_urlencode() {
    local string="$1" i c encoded=""
    for (( i = 0; i < ${#string}; i++ )); do
        c="${string:$i:1}"
        case "$c" in
            [a-zA-Z0-9/_.~:-]) encoded+="$c" ;;
            *) encoded+="$(printf '%%%02X' "'$c")" ;;
        esac
    done
    printf '%s' "$encoded"
}

__glass_osc7() {
    printf '\e]7;file://%s%s\e\\' "${HOST}" "$(__glass_urlencode "${PWD}")"
}

# ---------------------------------------------------------------------------
# precmd hook
#
# Runs before each prompt is displayed.  Sequence:
#   [133;D;<exit_code>]   end of previous command
#   [7;file://HOST/CWD]   current working directory
#   [133;A]               prompt start
# ---------------------------------------------------------------------------
__glass_precmd() {
    local exit_code=$?

    # End previous command (OSC 133;D with exit code)
    printf '\e]133;D;%d\e\\' "$exit_code"

    # Report CWD
    __glass_osc7

    # Prompt start
    printf '\e]133;A\e\\'

    # Clean up temp files from previous pipeline captures
    __glass_cleanup_stages 2>/dev/null
}

# ---------------------------------------------------------------------------
# preexec hook
#
# Runs after the user presses Enter but before the command executes.
#   [133;B]   command-input start (end of prompt region)
#   [133;C]   command is being executed (output start)
# ---------------------------------------------------------------------------
__glass_preexec() {
    printf '\e]133;B\e\\'
    printf '\e]133;C\e\\'
}

# ---------------------------------------------------------------------------
# Register hooks via add-zsh-hook (standard zsh mechanism)
# ---------------------------------------------------------------------------
autoload -Uz add-zsh-hook
add-zsh-hook precmd __glass_precmd
add-zsh-hook preexec __glass_preexec

# ---------------------------------------------------------------------------
# Pipeline capture: tee rewriting + OSC 133;S/P emission
#
# Intercepts piped commands at Enter, rewrites them to insert tee between
# stages, captures intermediate output to temp files, and emits OSC 133;S
# (pipeline start) and 133;P (per-stage data) so the terminal can display
# pipe stage output.
# ---------------------------------------------------------------------------

# State variables for pipeline capture
__glass_capture_tmpdir=""
__glass_capture_stage_count=0

# Detect whether a command line contains unquoted top-level pipes (not ||).
# Correctly handles: backslash escapes, $() subshells, backtick subshells,
# parenthesized subshells, and quoted strings.
__glass_has_pipes() {
    local cmd="$1"
    # Skip internal functions
    [[ "$cmd" == __glass_* ]] && return 1
    # Skip --no-glass commands
    [[ "$cmd" == *"--no-glass"* ]] && return 1
    # Walk characters, tracking quote state and nesting depth
    local in_sq=0 in_dq=0 depth=0 i=0 len=${#cmd}
    while [[ $i -lt $len ]]; do
        local c="${cmd:$i:1}"
        if [[ $in_sq -eq 1 ]]; then
            [[ "$c" == "'" ]] && in_sq=0
        elif [[ $in_dq -eq 1 ]]; then
            if [[ "$c" == '\\' ]]; then
                ((i++))  # skip escaped char inside double quotes
            elif [[ "$c" == '"' ]]; then
                in_dq=0
            fi
        elif [[ "$c" == '\\' ]]; then
            ((i++))  # skip escaped char
        elif [[ "$c" == "'" ]]; then
            in_sq=1
        elif [[ "$c" == '"' ]]; then
            in_dq=1
        elif [[ "$c" == '$' && "${cmd:$((i+1)):1}" == '(' ]]; then
            ((depth++))
            ((i++))  # skip the '('
        elif [[ "$c" == '(' ]]; then
            ((depth++))
        elif [[ "$c" == ')' ]]; then
            ((depth > 0)) && ((depth--))
        elif [[ "$c" == '`' ]]; then
            # Skip to matching backtick
            ((i++))
            while [[ $i -lt $len ]]; do
                [[ "${cmd:$i:1}" == '\\' ]] && ((i++))
                [[ "${cmd:$i:1}" == '`' ]] && break
                ((i++))
            done
        elif [[ "$c" == '|' && $depth -eq 0 ]]; then
            local next="${cmd:$((i+1)):1}"
            [[ "$next" != '|' ]] && return 0  # found unquoted top-level pipe
        fi
        ((i++))
    done
    return 1
}

# Rewrite a pipeline command to insert tee between top-level stages.
# Sets __glass_capture_stage_count as a side effect.
# Correctly handles: backslash escapes, $() subshells, backtick subshells,
# parenthesized subshells, and quoted strings.
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
                current+="${cmd:$i:1}"  # append escaped char
            elif [[ "$c" == '"' ]]; then
                in_dq=0
            fi
        elif [[ "$c" == '\\' ]]; then
            current+="$c"
            ((i++))
            current+="${cmd:$i:1}"  # append escaped char
        elif [[ "$c" == "'" ]]; then
            in_sq=1
            current+="$c"
        elif [[ "$c" == '"' ]]; then
            in_dq=1
            current+="$c"
        elif [[ "$c" == '$' && "${cmd:$((i+1)):1}" == '(' ]]; then
            ((depth++))
            current+='$('
            ((i++))  # skip the '('
        elif [[ "$c" == '(' ]]; then
            ((depth++))
            current+="$c"
        elif [[ "$c" == ')' ]]; then
            ((depth > 0)) && ((depth--))
            current+="$c"
        elif [[ "$c" == '`' ]]; then
            # Copy backtick-delimited subshell verbatim
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
# Called in __glass_precmd -- by the time we get back to the prompt the
# terminal has already read the temp files.
# Note: use $~pattern for glob expansion in zsh (globs in vars need ~ prefix).
__glass_cleanup_stages() {
    local pattern="${TMPDIR:-/tmp}/glass_*"
    for d in ${~pattern}; do
        [[ -d "$d" ]] && rm -rf "$d" 2>/dev/null
    done
}

# ---------------------------------------------------------------------------
# Enter key interception via zle widget
# ---------------------------------------------------------------------------
__glass_accept_line_widget() {
    [[ "$GLASS_PIPES_DISABLED" == "1" ]] && { zle accept-line; return; }
    local cmd="$BUFFER"
    if [[ -n "$cmd" ]] && __glass_has_pipes "$cmd"; then
        local tmpdir
        tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/glass_XXXXXXXX") || { zle accept-line; return; }
        chmod 700 "$tmpdir"
        __glass_capture_tmpdir="$tmpdir"
        local rewritten
        rewritten=$(__glass_tee_rewrite "$cmd" "$tmpdir")
        BUFFER="${rewritten}; __glass_pipestatus=(\${pipestatus[@]}); __glass_emit_stages"
        CURSOR=${#BUFFER}
    fi
    zle accept-line
}
zle -N __glass_accept_line_widget
bindkey '^M' __glass_accept_line_widget
