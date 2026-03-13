# Windows Installation

## MSI Installer (recommended)

1. Download the latest `.msi` file from the [GitHub Releases](https://github.com/candyhunterz/Glass/releases) page.
2. Double-click the MSI to launch the installer.
3. Glass installs to `C:\Program Files\Glass Terminal\` and adds `glass` to your system PATH.
4. Launch Glass from the Start menu or by running `glass` in any terminal.

> **Note:** The MSI uses UpgradeCode `D5F79758-7183-4EBE-9B63-DADD19B1D42C`. Subsequent installs automatically upgrade in place.

## Winget

Once the package is published to the winget repository:

```
winget install Glass.Terminal
```

## SmartScreen warning

The binary is currently unsigned. Windows SmartScreen may show a warning on first launch:

1. Click **"More info"** in the SmartScreen dialog.
2. Click **"Run anyway"**.

This only happens once per version. Code signing will be added in a future release.

## System requirements

- Windows 10 version 1903 or later (Windows 11 recommended)
- GPU with Vulkan or DirectX 12 support
- 64-bit (x86_64) processor
