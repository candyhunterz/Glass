# Codex exec JSON spike

## Codex version tested

- `codex-cli 0.128.0`
- Login status command is `codex login status`. `codex login --status` is not supported in this version.
- This machine is logged in using ChatGPT.

## Token file paths

- Windows: `%USERPROFILE%\.codex\auth.json`
- Unix assumption: `$HOME/.codex/auth.json`
- Glass should keep using file-existence only. The spike did not require reading token contents.

## Non-interactive JSON argv

Working default-model command:

```powershell
codex exec --json --cd C:\Users\nkngu\apps\Glass\.worktrees\openai-oauth --ephemeral --sandbox read-only --ignore-user-config "say hello and then call no tools"
```

Tool-use command:

```powershell
codex exec --json --cd C:\Users\nkngu\apps\Glass\.worktrees\openai-oauth --ephemeral --sandbox read-only --ignore-user-config "Use a shell command to list the file named Cargo.toml in the current directory, then answer done."
```

Flags confirmed:

- JSONL output: `--json`
- Model: `--model <MODEL>` or `-m <MODEL>`
- Working directory: `--cd <DIR>` or `-C <DIR>`
- Sandbox: `--sandbox <read-only|workspace-write|danger-full-access>`
- User config bypass: `--ignore-user-config`

No dedicated `--system-prompt-file` or `--mcp-config` flag appears in `codex exec --help` for 0.128.0. MCP servers are managed through `codex mcp` and Codex config. Runtime integration should inject Glass's system prompt as part of the first stdin turn unless a future CLI version exposes a dedicated system-prompt flag.

`gpt-5-codex` failed for the current ChatGPT account:

```json
{"type":"error","message":"{\"type\":\"error\",\"status\":400,\"error\":{\"type\":\"invalid_request_error\",\"message\":\"The 'gpt-5-codex' model is not supported when using Codex with a ChatGPT account.\"}}"}
```

The account default model from `~/.codex/config.toml` is `gpt-5.5`, and default-model `codex exec --json` succeeded.

## JSON event schema

Session start:

```json
{"type":"thread.started","thread_id":"019df94b-9e24-7571-9453-6a4c64c4dc22"}
```

Turn start:

```json
{"type":"turn.started"}
```

Assistant text is emitted as a completed item, not as a token delta in this fixture:

```json
{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"Hello."}}
```

Tool call start:

```json
{"type":"item.started","item":{"id":"item_0","type":"command_execution","command":"\"C:\\Program Files\\PowerShell\\7\\pwsh.exe\" -Command 'Get-ChildItem -Name Cargo.toml'","aggregated_output":"","exit_code":null,"status":"in_progress"}}
```

Tool result:

```json
{"type":"item.completed","item":{"id":"item_0","type":"command_execution","command":"\"C:\\Program Files\\PowerShell\\7\\pwsh.exe\" -Command 'Get-ChildItem -Name Cargo.toml'","aggregated_output":"Cargo.toml\r\n","exit_code":0,"status":"completed"}}
```

Turn end with usage:

```json
{"type":"turn.completed","usage":{"input_tokens":27176,"cached_input_tokens":24320,"output_tokens":59,"reasoning_output_tokens":0}}
```

Error shape:

```json
{"type":"turn.failed","error":{"message":"{\"type\":\"error\",\"status\":400,\"error\":{\"type\":\"invalid_request_error\",\"message\":\"The 'gpt-5-codex' model is not supported when using Codex with a ChatGPT account.\"}}"}}
```

## Stdin behavior

After `turn.completed`, `codex exec --json` printed `Reading additional input from stdin...` and waited for more input until stdin closed. Treat the process as multi-turn capable when Glass keeps stdin piped open. Send one newline-terminated turn at a time.

## Session reset command

No non-interactive session-reset command was found in `codex exec --help`. The interactive CLI help does not list slash commands. For the checkpoint-cycle implementer reset, treat Codex as a confirmed no-clear case unless a future interactive TUI spike verifies a slash command such as `/new`.

## Minimum Codex CLI version

Use `codex-cli 0.128.0` as the minimum version for this implementation. The event names and `codex login status` command were verified against that version.

## Stderr observations

Stdout remained JSONL, but stderr was noisy. This run emitted PowerShell shell-snapshot warnings, plugin manifest warnings, plugin sync 403 warnings, and analytics 403 warnings. The backend must drain stderr continuously and include only a short tail in crash reporting.
