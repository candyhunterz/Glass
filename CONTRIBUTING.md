# Contributing to Glass

Thank you for considering a contribution to Glass! This document covers everything you need to get started.

## Prerequisites

- **Rust toolchain** (stable) -- install from [rustup.rs](https://rustup.rs)
- **Git**

### Linux system dependencies

**Debian / Ubuntu:**
```bash
sudo apt install libxkbcommon-dev libwayland-dev libx11-dev libxi-dev libxtst-dev libfontconfig-dev
```

**Fedora:**
```bash
sudo dnf install libxkbcommon-devel wayland-devel libX11-devel libXi-devel libXtst-devel fontconfig-devel
```

**Arch Linux:**
```bash
sudo pacman -S libxkbcommon wayland libx11 libxi libxtst fontconfig
```

macOS and Windows require no additional system dependencies.

## Building

```bash
cargo build                  # Debug build
cargo build --release        # Optimized release build
cargo build --features perf  # Build with tracing instrumentation
```

## Testing

```bash
cargo test --workspace       # Run all tests (~420 tests)
```

Some tests use ConPTY and are gated with `#[cfg(target_os = "windows")]`. These only run on Windows.

## Linting

Both checks must pass before a PR will be merged:

```bash
cargo fmt --all -- --check                  # Check formatting
cargo clippy --workspace -- -D warnings     # Lint (all warnings are errors)
```

## Code Style

- Tests live in the same file as the code they test, inside `#[cfg(test)] mod tests`.
- The `alacritty_terminal` dependency is pinned to exact version `=0.25.1`. Do not change this pin without discussion.
- Use conventional commit messages: `feat:`, `fix:`, `docs:`, `chore:`, `perf:`, `ci:`, `refactor:`, `test:`.

## PR Process

1. Branch off `master` (the development branch).
2. Make your changes with clear, focused commits.
3. Ensure CI passes: `cargo fmt`, `cargo clippy`, `cargo test` on all three platforms (Linux, macOS, Windows).
4. Open a pull request targeting `main`.
5. A maintainer will review your PR. Address any feedback and push updates.

## Architecture

See [CLAUDE.md](CLAUDE.md) for the full crate map and architecture overview. See [ORCHESTRATOR.md](ORCHESTRATOR.md) for orchestrator internals.

## Configuration

See [config.example.toml](config.example.toml) for all configuration options with their defaults and descriptions.

## Screenshots

The README references `docs/assets/hero-screenshot.png`. To update:
1. Open Glass with a session showing command blocks, pipe visualization, exit badges, and split panes
2. Capture at 800px width minimum
3. Save as PNG to `docs/assets/hero-screenshot.png`
