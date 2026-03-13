# Troubleshooting

## macOS: Gatekeeper blocks Glass from opening

**Symptom:** macOS shows "Glass.app cannot be opened because it is from an unidentified developer."

**Fix:** Right-click Glass.app, select Open, then click Open in the dialog. Alternatively, run:

```bash
xattr -cr /Applications/Glass.app
```

This only needs to be done once per version. See [macOS Installation](./installation/macos.md) for details.

## GPU driver issues

**Symptom:** Glass fails to start with an error about Vulkan, OpenGL, or the GPU.

**Fix:**

- **Windows:** Update your GPU drivers from the manufacturer's website (NVIDIA, AMD, or Intel).
- **macOS:** Ensure you are running macOS 11.0 (Big Sur) or later. All supported Macs have Metal support.
- **Linux:** Install appropriate GPU drivers:
  ```bash
  # NVIDIA
  sudo apt install nvidia-driver-535

  # AMD/Intel (Mesa)
  sudo apt install mesa-vulkan-drivers
  ```

Verify Vulkan support with `vulkaninfo --summary` (Linux).

## Shell not detected

**Symptom:** Glass opens but no shell prompt appears, or the wrong shell is used.

**Fix:** Explicitly set your shell in `~/.glass/config.toml`:

```toml
shell = "/usr/bin/zsh"
```

On Windows:
```toml
shell = "C:\\Program Files\\PowerShell\\7\\pwsh.exe"
```

## Config parse errors

**Symptom:** Glass shows an error overlay mentioning a config error with a line number.

**Fix:** The error overlay displays the exact line, column, and a snippet of the problematic configuration. Common causes:

- **Typo in key name** -- Check spelling against the [Configuration](./configuration.md) reference.
- **Wrong value type** -- For example, `font_size = "big"` instead of `font_size = 16.0`.
- **Missing quotes** -- String values must be quoted: `font_family = "Consolas"`.

Fix the error in `~/.glass/config.toml` and save. Glass hot-reloads automatically -- no restart needed.

## Font not found

**Symptom:** Terminal text appears in an unexpected font.

**Fix:** Glass falls back to the platform default font if the configured font is not found. Verify the font name matches exactly what your system reports:

- **Windows:** Check the font name in Settings > Personalization > Fonts.
- **macOS:** Check the font name in Font Book.
- **Linux:** Run `fc-list | grep "FontName"`.

Update `font_family` in `~/.glass/config.toml` with the correct name.

## Auto-update issues

**Symptom:** Glass reports an update is available but the update fails, or Ctrl+Shift+U does nothing.

**Fix:**

- Ensure you have an internet connection.
- On Windows, the updater downloads an MSI and runs it. If the MSI download fails, try downloading it manually from the [GitHub Releases](https://github.com/candyhunterz/Glass/releases) page.
- On macOS, download the latest DMG manually and replace Glass.app in Applications.
- On Linux, download the latest deb package and install with `sudo dpkg -i glass_*.deb`.

## Agent mode: Claude CLI not found

**Symptom:** Agent mode stays disabled despite `agent.enabled = true` in the config.

**Fix:** Ensure the `claude` CLI is installed and available in your PATH. Glass checks for the binary at startup and disables agent mode gracefully when it is missing. A config hint will appear in the status bar indicating the binary could not be found. Install the Claude CLI and restart Glass, or verify it is accessible:

```bash
which claude
claude --version
```

If the binary exists but is not on PATH when Glass launches, add its directory to your shell's PATH configuration (`.bashrc`, `.zshrc`, or equivalent).

## Agent mode: proposals not appearing

**Symptom:** Agent mode is active but no proposals appear in the UI.

**Check the following config values in `~/.glass/config.toml`:**

- **`agent.cooldown_secs`** -- The default is 30 seconds, meaning at least 30 seconds must pass between proposals. If commands are running quickly, the cooldown may be suppressing output.
- **`agent.max_budget_usd`** -- Proposals stop generating once cumulative spend exceeds this limit. Check whether your budget has been reached.
- **`agent.quiet_rules`** -- Rules in this list can filter out events that would otherwise trigger a proposal. Review whether any rules are matching your current workflow.
- **`agent.mode`** -- If set to `"watch"`, the agent only reacts to errors and will not produce proactive proposals. Set to `"active"` to enable proactive suggestions.

## SOI: no decorations on command blocks

**Symptom:** Commands complete but no SOI (Structured Output Intelligence) label or decoration appears on the command block.

**Check the following:**

- **`soi.enabled`** -- Must be set to `true`. This is the default, but verify it has not been explicitly disabled in `~/.glass/config.toml`.
- **`soi.min_lines`** -- Output must meet or exceed this line threshold before SOI attempts classification. The default is 5 lines. Commands that produce fewer lines will not receive a decoration.
- **Output type recognition** -- SOI classifies output into known types (tables, lists, error traces, etc.). Output that does not match a recognized type is left as `Freeform` and receives no decoration. This is expected behavior.

## Getting help

If your issue is not listed here:

1. Check the [GitHub Issues](https://github.com/candyhunterz/Glass/issues) page for known issues.
2. Open a new issue with your Glass version, operating system, and a description of the problem.
