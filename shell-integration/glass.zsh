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
__glass_osc7() {
    printf '\e]7;file://%s%s\e\\' "${HOST}" "${PWD}"
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
