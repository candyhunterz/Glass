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
- On Windows, the updater downloads an MSI and runs it. If the MSI download fails, try downloading it manually from the [GitHub Releases](https://github.com/anthropics/glass/releases) page.
- On macOS, download the latest DMG manually and replace Glass.app in Applications.
- On Linux, download the latest deb package and install with `sudo dpkg -i glass_*.deb`.

## Getting help

If your issue is not listed here:

1. Check the [GitHub Issues](https://github.com/anthropics/glass/issues) page for known issues.
2. Open a new issue with your Glass version, operating system, and a description of the problem.
