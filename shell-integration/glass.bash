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
fi
