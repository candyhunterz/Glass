# Command Blocks

Command blocks are the core concept in Glass. Instead of showing terminal output as a continuous stream of text, Glass renders each command as a distinct, structured block.

## How it works

Glass uses shell integration to detect when a command starts and ends. Each block captures:

- **The command itself** -- The exact command line you typed
- **Output** -- All stdout and stderr produced by the command
- **Exit code** -- Displayed as a badge: green checkmark for exit code 0, red X for non-zero
- **Duration** -- Wall-clock time from command start to finish
- **Working directory** -- The directory the command executed in

## Structured scrollback

Traditional terminals give you a flat buffer of text. In Glass, scrollback is organized by command. You can visually scan through your history and immediately identify:

- Which commands succeeded or failed
- How long each command took
- Where each command ran

This makes it dramatically easier to debug build failures, trace deployment steps, or review a sequence of operations.

## Exit code badges

Each block displays a small badge indicating the command's exit status:

- **Green checkmark** -- Exit code 0 (success)
- **Red X** -- Non-zero exit code (failure), with the exit code number shown

The color coding makes it trivial to spot failures when scrolling through a long session.

## Shell integration

Glass automatically integrates with your shell to detect command boundaries. This works with common shells (bash, zsh, fish, PowerShell) without any manual configuration or prompt modifications.

The integration is transparent -- Glass does not modify your shell prompt or inject visible markers. Everything happens behind the scenes.
