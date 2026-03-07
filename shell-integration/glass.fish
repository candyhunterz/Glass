# Glass Shell Integration for Fish
#
# Emits OSC 133 (command lifecycle) and OSC 7 (CWD) escape sequences
# so that Glass can identify command boundaries and track the working directory.
#
# Usage:
#   source /path/to/glass/shell-integration/glass.fish
#
# Compatible with Starship, Tide, and other fish prompt customizers --
# this script uses fish event handlers which cooperate with other hooks.

# Guard against double-sourcing
if set -q __GLASS_INTEGRATION_LOADED
    return
end
set -g __GLASS_INTEGRATION_LOADED 1

# ---------------------------------------------------------------------------
# CWD reporting via OSC 7
# ---------------------------------------------------------------------------
function __glass_osc7
    printf '\e]7;file://%s%s\e\\' (hostname) $PWD
end

# ---------------------------------------------------------------------------
# fish_prompt event handler
#
# Runs before each prompt is displayed.  Sequence:
#   [133;D;<exit_code>]   end of previous command
#   [7;file://HOST/CWD]   current working directory
#   [133;A]               prompt start
# ---------------------------------------------------------------------------
function __glass_prompt --on-event fish_prompt
    set -l exit_code $status

    # End previous command (OSC 133;D with exit code)
    printf '\e]133;D;%d\e\\' $exit_code

    # Report CWD
    __glass_osc7

    # Prompt start
    printf '\e]133;A\e\\'
end

# ---------------------------------------------------------------------------
# fish_preexec event handler
#
# Runs after the user presses Enter but before the command executes.
#   [133;B]   command-input start (end of prompt region)
#   [133;C]   command is being executed (output start)
# ---------------------------------------------------------------------------
function __glass_preexec --on-event fish_preexec
    printf '\e]133;B\e\\'
    printf '\e]133;C\e\\'
end
