# macOS Installation

## DMG Installer (recommended)

1. Download the latest `.dmg` file from the [GitHub Releases](https://github.com/anthropics/glass/releases) page.
2. Open the DMG and drag **Glass.app** into your Applications folder.
3. Launch Glass from Applications or Spotlight.

### Gatekeeper workaround

The app is currently unsigned. macOS Gatekeeper will block it on first launch. Use one of these workarounds:

**Option A -- Right-click:**
1. Right-click (or Control-click) Glass.app in Applications.
2. Select **Open** from the context menu.
3. Click **Open** in the confirmation dialog.

**Option B -- Terminal command:**
```bash
xattr -cr /Applications/Glass.app
```

This only needs to be done once per version.

## Homebrew

Once the tap is published:

```bash
brew install <GITHUB_USER>/glass/glass
```

## System requirements

- macOS 11.0 (Big Sur) or later
- Apple Silicon (arm64) or Intel (x86_64) processor
- Metal-capable GPU (all supported Macs have this)
- Bundle identifier: `com.glass.terminal`
