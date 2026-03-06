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
# Compatible with PowerShell 5.1+ and PowerShell 7+.

# ESC character that works on all PowerShell versions (5.1+).
$Global:__GlassESC = [char]0x1b

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

    $E = $Global:__GlassESC

    # End previous command (OSC 133;D with exit code)
    if ($Global:__GlassLastHistoryId -ne -1) {
        if ($LastHistoryEntry.Id -eq $Global:__GlassLastHistoryId) {
            # User pressed Enter without typing a command -- no exit code
            $out += "$E]133;D$([char]7)"
        } else {
            $out += "$E]133;D;$gle$([char]7)"
        }
    }

    # Report CWD via OSC 7  (backslashes -> forward slashes)
    $loc = $executionContext.SessionState.Path.CurrentLocation
    $cwd = $loc.Path.Replace('\', '/')
    $out += "$E]7;file://$($env:COMPUTERNAME)/$cwd$([char]7)"

    # Prompt start (OSC 133;A)
    $out += "$E]133;A$([char]7)"

    # Call the original prompt (preserves Oh My Posh / Starship styling)
    if ($Global:__GlassOriginalPrompt) {
        $out += & $Global:__GlassOriginalPrompt
    } else {
        $out += "PS $loc> "
    }

    # Command-input start (OSC 133;B)
    $out += "$E]133;B$([char]7)"

    # Clean up temp files from previous pipeline captures
    __Glass-Cleanup-Stages

    # Remember the current history ID for the next prompt cycle
    $Global:__GlassLastHistoryId = $LastHistoryEntry.Id

    return $out
}

# ---------------------------------------------------------------------------
# Pipeline capture: Tee-Object rewriting + OSC 133;S/P emission
#
# Intercepts piped commands at Enter, rewrites them to insert Tee-Object
# between stages, captures intermediate output to temp files, and emits
# OSC 133;S (pipeline start) and 133;P (per-stage data) so the terminal
# can display pipe stage output.
# ---------------------------------------------------------------------------

# State variables for pipeline capture
$Global:__GlassCaptureDir = $null
$Global:__GlassCaptureStageCount = 0

# Detect and rewrite pipeline commands with Tee-Object capture.
# Returns $null if the command is not a pipeline or should be skipped.
function Global:__Glass-Rewrite-Pipeline {
    param([string]$Command)

    # Skip --no-glass commands
    if ($Command -match '--no-glass') { return $null }
    # Skip internal functions
    if ($Command -match '^__Glass') { return $null }

    # Split on unquoted pipes (not ||)
    $stages = @()
    $current = ""
    $inSingle = $false
    $inDouble = $false

    for ($i = 0; $i -lt $Command.Length; $i++) {
        $c = $Command[$i]
        if ($c -eq "'" -and -not $inDouble) { $inSingle = -not $inSingle }
        elseif ($c -eq '"' -and -not $inSingle) { $inDouble = -not $inDouble }
        elseif ($c -eq '|' -and -not $inSingle -and -not $inDouble) {
            # Check for ||
            if ($i + 1 -lt $Command.Length -and $Command[$i + 1] -eq '|') {
                $current += '||'
                $i++
                continue
            }
            $stages += $current.Trim()
            $current = ""
            continue
        }
        $current += $c
    }
    $stages += $current.Trim()

    # Not a pipeline if only one stage
    if ($stages.Count -le 1) { return $null }

    # Create temp directory
    $tmpdir = Join-Path ([System.IO.Path]::GetTempPath()) "glass_$PID`_$([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())"
    [System.IO.Directory]::CreateDirectory($tmpdir) | Out-Null
    $Global:__GlassCaptureDir = $tmpdir
    $Global:__GlassCaptureStageCount = $stages.Count

    # Build rewritten command with Tee-Object between stages
    $parts = @()
    for ($i = 0; $i -lt $stages.Count; $i++) {
        if ($i -gt 0) {
            $parts += "|"
        }
        $parts += $stages[$i]
        if ($i -lt ($stages.Count - 1)) {
            $path = Join-Path $tmpdir "stage_$i.txt"
            $parts += "| Tee-Object -FilePath '$path'"
        }
    }

    return ($parts -join ' ') + "; __Glass-Emit-Stages"
}

# Emit OSC 133;S (pipeline start) and 133;P (per-stage) sequences
function Global:__Glass-Emit-Stages {
    $tmpdir = $Global:__GlassCaptureDir
    if (-not $tmpdir -or -not (Test-Path $tmpdir)) { return }

    $E = [char]0x1b
    $count = $Global:__GlassCaptureStageCount
    if (-not $count -or $count -eq 0) { return }

    # Pipeline start marker
    [Console]::Write("$E]133;S;$count$E\")

    for ($i = 0; $i -lt $count; $i++) {
        $path = Join-Path $tmpdir "stage_$i.txt"
        if (Test-Path $path) {
            $size = (Get-Item $path).Length
            [Console]::Write("$E]133;P;$i;$size;$path$E\")
        }
    }

    $Global:__GlassCaptureDir = $null
    $Global:__GlassCaptureStageCount = 0
}

# Clean up temp dirs from previous pipeline executions
function Global:__Glass-Cleanup-Stages {
    $pattern = Join-Path ([System.IO.Path]::GetTempPath()) "glass_$PID`_*"
    Get-ChildItem $pattern -Directory -ErrorAction SilentlyContinue |
        Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
}

# ---------------------------------------------------------------------------
# PSReadLine Enter-key handler -- emits OSC 133;C and intercepts pipelines
#
# 133;C marks the exact moment between "user finished typing" and "command
# starts executing".  We hook the Enter key so the marker appears before
# PowerShell begins processing.  Pipeline commands are rewritten to insert
# Tee-Object capture before execution.
# ---------------------------------------------------------------------------
if (Get-Module PSReadLine) {
    Set-PSReadLineKeyHandler -Key Enter -ScriptBlock {
        $line = $null
        $cursor = $null
        [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$line, [ref]$cursor)

        # Try to rewrite pipeline commands
        $rewritten = __Glass-Rewrite-Pipeline $line
        if ($rewritten) {
            [Microsoft.PowerShell.PSConsoleReadLine]::Replace(0, $line.Length, $rewritten)
        }

        [Microsoft.PowerShell.PSConsoleReadLine]::AcceptLine()
        [Console]::Write("$([char]0x1b)]133;C$([char]7)")
    }
}
