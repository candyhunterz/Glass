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
function __glass_urlencode
    set -l string $argv[1]
    set -l encoded ""
    for i in (string split "" -- $string)
        switch $i
            case a b c d e f g h i j k l m n o p q r s t u v w x y z \
                 A B C D E F G H I J K L M N O P Q R S T U V W X Y Z \
                 0 1 2 3 4 5 6 7 8 9 / _ . '~' : -
                set encoded "$encoded$i"
            case '*'
                set encoded "$encoded"(printf '%%%02X' "'$i")
        end
    end
    echo -n $encoded
end

function __glass_osc7
    printf '\e]7;file://%s%s\e\\' (hostname) (__glass_urlencode $PWD)
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

    # Clean up temp files from previous pipeline captures
    __glass_cleanup_stages 2>/dev/null
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

# ---------------------------------------------------------------------------
# Pipeline capture: tee rewriting + OSC 133;S/P emission
#
# Intercepts piped commands at Enter, rewrites them to insert tee
# between stages, captures intermediate output to temp files, and
# emits OSC 133;S (pipeline start) and 133;P (per-stage data) so
# the terminal can display pipe stage output.
# ---------------------------------------------------------------------------

# State variables for pipeline capture
set -g __glass_capture_tmpdir ""
set -g __glass_capture_stage_count 0

# Detect whether a command line contains unquoted top-level pipes (not ||).
# Correctly handles: backslash escapes, $() subshells, backtick subshells,
# parenthesized subshells, and quoted strings.
function __glass_has_pipes
    set -l cmd $argv[1]
    # Skip internal functions
    if string match -q '__glass_*' -- $cmd
        return 1
    end
    # Skip --no-glass commands
    if string match -q '*--no-glass*' -- $cmd
        return 1
    end
    # Walk characters, tracking quote state and nesting depth
    set -l in_sq 0
    set -l in_dq 0
    set -l depth 0
    set -l i 1
    set -l len (string length -- $cmd)
    while test $i -le $len
        set -l c (string sub -s $i -l 1 -- $cmd)
        set -l i1 (math $i + 1)
        if test $in_sq -eq 1
            if test "$c" = "'"
                set in_sq 0
            end
        else if test $in_dq -eq 1
            if test "$c" = '\\'
                set i (math $i + 1)  # skip escaped char inside double quotes
            else if test "$c" = '"'
                set in_dq 0
            end
        else if test "$c" = '\\'
            set i (math $i + 1)  # skip escaped char
        else if test "$c" = "'"
            set in_sq 1
        else if test "$c" = '"'
            set in_dq 1
        else if test "$c" = '$'
            set -l next (string sub -s $i1 -l 1 -- $cmd)
            if test "$next" = '('
                set depth (math $depth + 1)
                set i (math $i + 1)  # skip the '('
            end
        else if test "$c" = '('
            set depth (math $depth + 1)
        else if test "$c" = ')'
            if test $depth -gt 0
                set depth (math $depth - 1)
            end
        else if test "$c" = '`'
            # Skip to matching backtick
            set i (math $i + 1)
            while test $i -le $len
                set -l bc (string sub -s $i -l 1 -- $cmd)
                if test "$bc" = '\\'
                    set i (math $i + 1)
                else if test "$bc" = '`'
                    break
                end
                set i (math $i + 1)
            end
        else if test "$c" = '|'; and test $depth -eq 0
            set -l next (string sub -s $i1 -l 1 -- $cmd)
            if test "$next" != '|'
                return 0  # found unquoted top-level pipe
            end
        end
        set i (math $i + 1)
    end
    return 1
end

# Rewrite a pipeline command to insert tee between top-level stages.
# Sets __glass_capture_stage_count as a side effect.
# Correctly handles: backslash escapes, $() subshells, backtick subshells,
# parenthesized subshells, and quoted strings.
function __glass_tee_rewrite
    set -l cmd $argv[1]
    set -l tmpdir $argv[2]
    set -l result ""
    set -l current ""
    set -l stage_idx 0
    set -l in_sq 0
    set -l in_dq 0
    set -l depth 0
    set -l i 1
    set -l len (string length -- $cmd)

    while test $i -le $len
        set -l c (string sub -s $i -l 1 -- $cmd)
        set -l i1 (math $i + 1)
        if test $in_sq -eq 1
            set current "$current$c"
            if test "$c" = "'"
                set in_sq 0
            end
        else if test $in_dq -eq 1
            set current "$current$c"
            if test "$c" = '\\'
                set i (math $i + 1)
                set current "$current"(string sub -s $i -l 1 -- $cmd)
            else if test "$c" = '"'
                set in_dq 0
            end
        else if test "$c" = '\\'
            set current "$current$c"
            set i (math $i + 1)
            set current "$current"(string sub -s $i -l 1 -- $cmd)
        else if test "$c" = "'"
            set in_sq 1
            set current "$current$c"
        else if test "$c" = '"'
            set in_dq 1
            set current "$current$c"
        else if test "$c" = '$'
            set -l next (string sub -s $i1 -l 1 -- $cmd)
            if test "$next" = '('
                set depth (math $depth + 1)
                set current "$current\$("
                set i (math $i + 1)  # skip the '('
            else
                set current "$current$c"
            end
        else if test "$c" = '('
            set depth (math $depth + 1)
            set current "$current$c"
        else if test "$c" = ')'
            if test $depth -gt 0
                set depth (math $depth - 1)
            end
            set current "$current$c"
        else if test "$c" = '`'
            # Copy backtick-delimited subshell verbatim
            set current "$current\`"
            set i (math $i + 1)
            while test $i -le $len
                set -l bc (string sub -s $i -l 1 -- $cmd)
                set current "$current$bc"
                if test "$bc" = '\\'
                    set i (math $i + 1)
                    set current "$current"(string sub -s $i -l 1 -- $cmd)
                else if test "$bc" = '`'
                    break
                end
                set i (math $i + 1)
            end
        else if test "$c" = '|'; and test $depth -eq 0
            set -l next (string sub -s $i1 -l 1 -- $cmd)
            if test "$next" = '|'
                # Logical OR -- pass through
                set current "$current||"
                set i (math $i + 2)
                continue
            end
            # Pipe boundary: append current stage with tee, then pipe
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
    printf '%s' $result
end

# Emit OSC 133;S (pipeline start) and 133;P (per-stage) sequences
function __glass_emit_stages
    set -l tmpdir $__glass_capture_tmpdir
    if test -z "$tmpdir"; or not test -d "$tmpdir"
        return
    end

    set -l count $__glass_capture_stage_count
    if test -z "$count"; or test "$count" -eq 0
        return
    end

    # Emit pipeline start marker
    printf '\e]133;S;%d\e\\' $count

    # Emit each stage with temp file path
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

    # Clear state (temp files cleaned up on next prompt cycle)
    set -g __glass_capture_tmpdir ""
    set -g __glass_capture_stage_count 0
end

# Clean up temp dirs from previous pipeline executions.
# Called in __glass_prompt -- by the time we get back to the
# prompt the terminal has already read the temp files.
function __glass_cleanup_stages
    set -l pid %self
    set -l base (set -q TMPDIR; and echo $TMPDIR; or echo /tmp)
    for d in $base/glass_$pid_*
        if test -d "$d"
            rm -rf "$d" 2>/dev/null
        end
    end
end

# Enter key interception: rewrite pipeline commands before execution
function __glass_accept_line
    if test "$GLASS_PIPES_DISABLED" = "1"
        commandline -f execute
        return
    end
    set -l cmd (commandline)
    if test -n "$cmd"; and __glass_has_pipes "$cmd"
        set -l base (set -q TMPDIR; and echo $TMPDIR; or echo /tmp)
        set -l tmpdir (mktemp -d "$base/glass_XXXXXXXX")
        if test $status -ne 0
            commandline -f execute
            return
        end
        chmod 700 "$tmpdir"
        set -g __glass_capture_tmpdir "$tmpdir"
        set -l rewritten (__glass_tee_rewrite "$cmd" "$tmpdir")
        commandline -r "$rewritten; set -g __glass_pipestatus \$pipestatus; __glass_emit_stages"
    end
    commandline -f execute
end

bind \r __glass_accept_line
bind \n __glass_accept_line
