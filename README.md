# Glass

![CI](https://github.com/<GITHUB_USER>/glass/actions/workflows/ci.yml/badge.svg)
![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)

<!-- TODO: Add screenshot -->

**GPU-accelerated terminal emulator with command structure awareness.**

Glass treats terminal output as structured data. Every command produces a visually
distinct block showing exit code, duration, and working directory. Built with Rust
and [wgpu](https://wgpu.rs/) for GPU-accelerated rendering.

## Features

- **Command blocks** with exit codes, duration, and working directory
- **Ctrl+Shift+F** full-text search across terminal history (FTS5/SQLite)
- **Ctrl+Shift+Z** file undo -- restore files modified by commands
- **Pipeline inspection** with intermediate stage output
- **Tabs** (Ctrl+Shift+T) and **split panes** (Ctrl+Shift+D)
- **MCP server** for AI assistant integration
- **Hot-reloadable configuration** -- edit config, changes apply instantly
- **Auto-update notifications** -- never miss a release
- **Cross-platform:** Windows, macOS, Linux

## Installation

### Windows

Download the `.msi` installer from
[Releases](https://github.com/<GITHUB_USER>/glass/releases/latest), or install
with winget:

```
winget install Glass.Terminal
```

### macOS

Download the `.dmg` disk image from
[Releases](https://github.com/<GITHUB_USER>/glass/releases/latest), or install
with Homebrew:

```
brew install <GITHUB_USER>/glass/glass
```

> **Note:** The DMG is currently unsigned. If blocked by Gatekeeper, run
> `xattr -cr /Applications/Glass.app` or right-click the app and choose **Open**.

### Linux

Download the `.deb` package from
[Releases](https://github.com/<GITHUB_USER>/glass/releases/latest) and install:

```
sudo dpkg -i glass_*.deb
```

## Quick Start

1. Launch Glass -- it detects your default shell automatically.
2. Edit `~/.glass/config.toml` to customize fonts, colors, and behavior. Changes
   apply immediately (no restart needed).
3. See the [full documentation](https://<GITHUB_USER>.github.io/glass/) for the
   complete configuration reference and feature guides.

## Documentation

Full documentation is available at
**[https://\<GITHUB_USER\>.github.io/glass/](https://<GITHUB_USER>.github.io/glass/)**.

## Building from Source

### Prerequisites

- [Rust toolchain](https://rustup.rs/) (stable)
- Platform-specific dependencies:
  - **Windows:** No additional dependencies
  - **macOS:** No additional dependencies
  - **Linux:** `libwayland-dev libxkbcommon-dev libx11-dev libxi-dev libxtst-dev`

### Build

```bash
cargo build --release
```

The binary will be at `target/release/glass` (or `target\release\glass.exe` on
Windows).

## License

MIT
