# Glass Shell Integration for PowerShell
#
# Emits OSC 133 (command lifecycle) and OSC 7 (CWD) escape sequences
# so that Glass can identify command boundaries and track the working directory.
#
# Usage:
#   . $env:USERPROFILE\.glass\shell-integration\glass.ps1
#
# Compatible with Oh My Posh and Starship -- this script wraps around
# any existing prompt function rather than replacing it.
#
# Requires: PowerShell 7+ (uses `e escape character)

# ---------------------------------------------------------------------------
# State: track the last history entry ID so we can tell whether the user
# actually ran a command (new ID) or just pressed Enter on an empty prompt.
# ---------------------------------------------------------------------------
$Global:__GlassLastHistoryId = -1

# ---------------------------------------------------------------------------
# Exit code helper
#
# PowerShell has two independent error signals:
#   $?             -- $false after a cmdlet/function failure
#   $LASTEXITCODE  -- set only by external (native) programs
#
# We need to unify them into a single integer for OSC 133;D.
# ---------------------------------------------------------------------------
function Global:__Glass-Get-LastExitCode {
    if ($? -eq $True) { return 0 }

    $LastHistoryEntry = $(Get-History -Count 1)
    # If the most recent $Error came from the same history entry as the last
    # command, it was a PowerShell cmdlet error (report as -1).
    if ($Error.Count -gt 0 -and
        $Error[0].InvocationInfo -and
        $Error[0].InvocationInfo.HistoryId -eq $LastHistoryEntry.Id) {
        return -1
    }

    # Otherwise it was an external program -- use its exit code.
    if ($null -ne $LASTEXITCODE) { return $LASTEXITCODE }
    return -1
}

# ---------------------------------------------------------------------------
# Stash the existing prompt function so Oh My Posh / Starship styling is
# preserved.  This MUST happen after those tools have initialised (i.e.
# source glass.ps1 at the end of your $PROFILE).
# ---------------------------------------------------------------------------
if ($function:Prompt) {
    $Global:__GlassOriginalPrompt = $function:Prompt
}

# ---------------------------------------------------------------------------
# Replacement prompt function
#
# Sequence emitted on every prompt:
#   [133;D;<exit_code>]   end of previous command (skipped on first prompt)
#   [7;file://HOST/CWD]   current working directory
#   [133;A]               prompt start
#   <original prompt>     Oh My Posh / Starship / default PS1
#   [133;B]               command-input start
# ---------------------------------------------------------------------------
function prompt {
    $gle = $(__Glass-Get-LastExitCode)
    $LastHistoryEntry = $(Get-History -Count 1)

    $out = ""

    # End previous command (OSC 133;D with exit code)
    if ($Global:__GlassLastHistoryId -ne -1) {
        if ($LastHistoryEntry.Id -eq $Global:__GlassLastHistoryId) {
            # User pressed Enter without typing a command -- no exit code
            $out += "`e]133;D`a"
        } else {
            $out += "`e]133;D;$gle`a"
        }
    }

    # Report CWD via OSC 7  (backslashes -> forward slashes)
    $loc = $executionContext.SessionState.Path.CurrentLocation
    $cwd = $loc.Path.Replace('\', '/')
    $out += "`e]7;file://$($env:COMPUTERNAME)/$cwd`a"

    # Prompt start (OSC 133;A)
    $out += "`e]133;A`a"

    # Call the original prompt (preserves Oh My Posh / Starship styling)
    if ($Global:__GlassOriginalPrompt) {
        $out += & $Global:__GlassOriginalPrompt
    } else {
        $out += "PS $loc> "
    }

    # Command-input start (OSC 133;B)
    $out += "`e]133;B`a"

    # Remember the current history ID for the next prompt cycle
    $Global:__GlassLastHistoryId = $LastHistoryEntry.Id

    return $out
}

# ---------------------------------------------------------------------------
# PSReadLine Enter-key handler -- emits OSC 133;C
#
# 133;C marks the exact moment between "user finished typing" and "command
# starts executing".  We hook the Enter key so the marker appears before
# PowerShell begins processing.
# ---------------------------------------------------------------------------
if (Get-Module PSReadLine) {
    Set-PSReadLineKeyHandler -Key Enter -ScriptBlock {
        [Microsoft.PowerShell.PSConsoleReadLine]::AcceptLine()
        [Console]::Write("`e]133;C`a")
    }
}
