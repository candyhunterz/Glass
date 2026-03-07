# Phase 30: Documentation & Distribution - Research

**Researched:** 2026-03-07
**Domain:** mdBook documentation, GitHub Pages deployment, winget/Homebrew package manifests, README authoring
**Confidence:** HIGH

## Summary

Phase 30 covers three distinct work areas: (1) creating an mdBook documentation site with feature guides, (2) deploying it to GitHub Pages via CI, and (3) creating package manager manifests for winget (Windows) and Homebrew (macOS). Additionally, the GitHub README needs a complete overhaul with screenshots, install instructions, feature overview, and CI badges.

mdBook is the standard Rust ecosystem documentation tool -- it generates static HTML from Markdown, requires no runtime, and has first-class GitHub Actions support with an official starter workflow. The winget submission requires a multi-file YAML manifest (version + installer + defaultLocale) submitted as a PR to microsoft/winget-pkgs. The Homebrew cask requires a Ruby formula in a custom tap (GitHub repo) pointing at the DMG release asset.

**Primary recommendation:** Use mdBook with the official GitHub Actions Pages workflow. Create a custom Homebrew tap repo and winget multi-file manifest. The README should be written from scratch since none currently exists.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| DOCS-01 | mdBook documentation site with feature guides (undo, pipes, MCP, search, config) | mdBook setup, book.toml config, SUMMARY.md structure, chapter organization |
| DOCS-02 | GitHub Pages deployment for docs site | Official GitHub Actions starter workflow for mdBook + deploy-pages |
| DOCS-03 | README overhaul with screenshots, install instructions, feature overview, and badges | README structure, badge URLs from shields.io, platform install commands |
| PKG-05 | Winget package manifest for Windows package manager | Multi-file manifest YAML (version + installer + defaultLocale), PR to microsoft/winget-pkgs |
| PKG-06 | Homebrew cask formula for macOS package manager | Custom tap repo, Ruby cask formula with DMG URL, sha256, livecheck |
</phase_requirements>

## Standard Stack

### Core
| Tool | Version | Purpose | Why Standard |
|------|---------|---------|--------------|
| mdBook | latest (0.4.x) | Static documentation site generator | Rust ecosystem standard, used by The Rust Book itself |
| GitHub Pages | N/A | Free static site hosting | Already using GitHub for CI; zero-cost, zero-config hosting |
| winget-create | latest | Manifest generation helper | Official Microsoft tool for winget manifest creation |

### Supporting
| Tool | Purpose | When to Use |
|------|---------|-------------|
| peaceiris/actions-mdbook | GitHub Action to install mdBook in CI | In the docs deploy workflow |
| actions/deploy-pages | GitHub Action for Pages deployment | Deploy built book to GitHub Pages |
| actions/upload-pages-artifact | Upload static files for Pages | Pair with deploy-pages |
| shields.io | CI badge generation | README badges for build status |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| mdBook | Docusaurus | Heavier (Node.js), more features but overkill for a terminal emulator |
| Custom tap | homebrew-cask PR to official repo | Official repo requires 50+ GitHub stars and notarization; custom tap works immediately |
| winget-create | Manual YAML authoring | winget-create auto-fills metadata from GitHub releases; manual is fine for a single manifest |

## Architecture Patterns

### mdBook Project Structure
```
docs/
  book.toml             # mdBook configuration
  src/
    SUMMARY.md          # Table of contents (required)
    introduction.md     # Landing page
    installation/
      windows.md        # MSI + winget instructions
      macos.md          # DMG + brew instructions
      linux.md          # .deb instructions
    getting-started.md  # First launch, shell integration
    features/
      blocks.md         # Command blocks and status bars
      search.md         # Ctrl+Shift+F search overlay
      undo.md           # Ctrl+Shift+Z file undo system
      pipes.md          # Pipeline inspection
      tabs-panes.md     # Tabs and split panes
      history.md        # History database and CLI queries
    configuration.md    # Full config.toml reference
    mcp-server.md       # MCP server setup for AI assistants
    troubleshooting.md  # Common issues (Gatekeeper, etc.)
```

### Pattern 1: book.toml Configuration
**What:** mdBook configuration file at the docs root
**When to use:** Always -- required for mdBook to build
**Example:**
```toml
[book]
title = "Glass Terminal"
description = "GPU-accelerated terminal emulator with command structure awareness"
authors = ["Glass Contributors"]
language = "en"
src = "src"

[build]
build-dir = "book"

[output.html]
default-theme = "navy"
preferred-dark-theme = "navy"
git-repository-url = "https://github.com/user/glass"
edit-url-template = "https://github.com/user/glass/edit/main/docs/{path}"
```

### Pattern 2: SUMMARY.md Structure
**What:** The table of contents that defines navigation and build order
**When to use:** Required -- mdBook uses this to determine which files to include
**Example:**
```markdown
# Summary

[Introduction](./introduction.md)

# Getting Started

- [Installation](./installation/windows.md)
  - [Windows](./installation/windows.md)
  - [macOS](./installation/macos.md)
  - [Linux](./installation/linux.md)
- [Getting Started](./getting-started.md)

# Features

- [Command Blocks](./features/blocks.md)
- [Search](./features/search.md)
- [Undo](./features/undo.md)
- [Pipe Inspection](./features/pipes.md)
- [Tabs & Panes](./features/tabs-panes.md)
- [History](./features/history.md)

# Reference

- [Configuration](./configuration.md)
- [MCP Server](./mcp-server.md)
- [Troubleshooting](./troubleshooting.md)
```

### Pattern 3: GitHub Actions Pages Deployment
**What:** CI workflow that builds mdBook and deploys to GitHub Pages
**When to use:** On push to main branch
**Example:**
```yaml
name: Deploy Docs

on:
  push:
    branches: [main]
  workflow_dispatch:

permissions:
  contents: read
  pages: write
  id-token: write

concurrency:
  group: pages
  allow-concurrent: false

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Setup mdBook
        uses: peaceiris/actions-mdbook@v2
        with:
          mdbook-version: 'latest'
      - name: Build docs
        run: mdbook build docs
      - name: Upload artifact
        uses: actions/upload-pages-artifact@v3
        with:
          path: docs/book

  deploy:
    needs: build
    runs-on: ubuntu-latest
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}
    steps:
      - name: Deploy to GitHub Pages
        id: deployment
        uses: actions/deploy-pages@v4
```

### Pattern 4: Winget Multi-File Manifest
**What:** Three YAML files submitted as a PR to microsoft/winget-pkgs
**When to use:** For each new release version
**Structure:**
```
manifests/g/Glass/Terminal/<version>/
  Glass.Terminal.yaml                    # version manifest
  Glass.Terminal.installer.yaml          # installer manifest
  Glass.Terminal.locale.en-US.yaml       # default locale manifest
```

**Version manifest:**
```yaml
PackageIdentifier: "Glass.Terminal"
PackageVersion: "0.1.0"
DefaultLocale: "en-US"
ManifestType: "version"
ManifestVersion: "1.6.0"
```

**Installer manifest:**
```yaml
PackageIdentifier: "Glass.Terminal"
PackageVersion: "0.1.0"
MinimumOSVersion: "10.0.18362.0"
InstallerType: "msi"
InstallModes:
  - "silent"
  - "silentWithProgress"
Installers:
  - Architecture: "x64"
    InstallerUrl: "https://github.com/<user>/glass/releases/download/v0.1.0/glass-0.1.0-x86_64.msi"
    InstallerSha256: "<sha256>"
    UpgradeBehavior: "install"
    ProductCode: "{PRODUCT-CODE-GUID}"
ManifestType: "installer"
ManifestVersion: "1.6.0"
```

**Default locale manifest:**
```yaml
PackageIdentifier: "Glass.Terminal"
PackageVersion: "0.1.0"
PackageLocale: "en-US"
Publisher: "Glass Contributors"
PackageName: "Glass Terminal"
License: "MIT"
LicenseUrl: "https://github.com/<user>/glass/blob/main/LICENSE"
ShortDescription: "GPU-accelerated terminal emulator with command structure awareness"
Description: "Glass is a GPU-accelerated terminal emulator that understands command structure. It renders each command's output as a visually distinct block with exit code, duration, and status information. Features include command-level undo, pipe inspection, tabs, split panes, and an MCP server for AI assistant integration."
Tags:
  - "Terminal"
  - "Console"
  - "Command-Line"
  - "GPU"
  - "Developer-Tools"
ManifestType: "defaultLocale"
ManifestVersion: "1.6.0"
```

### Pattern 5: Homebrew Cask (Custom Tap)
**What:** A Ruby cask formula in a personal tap repository
**When to use:** For macOS distribution via `brew install --cask`
**Setup:** Create a GitHub repo named `homebrew-glass` with:
```
Casks/
  glass.rb
```

**Formula:**
```ruby
cask "glass" do
  version "0.1.0"
  sha256 "<sha256-of-dmg>"

  url "https://github.com/<user>/glass/releases/download/v#{version}/Glass-#{version}.dmg",
      verified: "github.com/<user>/glass/"
  name "Glass Terminal"
  desc "GPU-accelerated terminal emulator with command structure awareness"
  homepage "https://github.com/<user>/glass"

  livecheck do
    url :url
    strategy :github_latest
  end

  app "Glass.app"

  zap trash: [
    "~/.glass",
  ]
end
```

**Usage:** `brew install <user>/glass/glass` or `brew tap <user>/glass && brew install --cask glass`

### Anti-Patterns to Avoid
- **Submitting to homebrew-cask official repo without notarization:** The official Homebrew cask repo requires apps to be code-signed and notarized. Use a custom tap instead since macOS code signing is deferred (PKG-F04).
- **Using singleton winget manifest format:** Singleton manifests are deprecated in winget-pkgs. Always use multi-file format (version + installer + defaultLocale).
- **Putting docs in project root:** Keep mdBook files in a `docs/` subdirectory to avoid cluttering the project root. mdBook's `build-dir` will output to `docs/book/`.
- **Writing documentation before implementation is complete:** All 29 phases are done. Document the actual shipped behavior, not planned behavior.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Documentation site | Custom HTML/CSS | mdBook | Handles navigation, search, theming, responsive layout |
| CI deployment | Custom deploy script | actions/deploy-pages | Official GitHub action, handles artifact upload and deployment |
| mdBook installation in CI | cargo install mdbook | peaceiris/actions-mdbook | Cached binary, much faster than compiling from source |
| SHA-256 calculation | Manual hashing | `shasum -a 256` or `certutil -hashfile` | Standard CLI tools on macOS/Windows |
| Badge generation | Custom badge images | shields.io | Dynamic badges that auto-update with CI status |

**Key insight:** This phase is entirely about content authoring and manifest creation -- there is no Rust code to write. The work is Markdown, YAML, Ruby, and CI workflow configuration.

## Common Pitfalls

### Pitfall 1: SUMMARY.md Missing Entries
**What goes wrong:** mdBook silently ignores files not listed in SUMMARY.md -- pages exist on disk but don't appear in the book
**Why it happens:** Adding a new .md file without updating SUMMARY.md
**How to avoid:** Every .md file under src/ MUST have a corresponding entry in SUMMARY.md
**Warning signs:** Page count in built book doesn't match source file count

### Pitfall 2: GitHub Pages Not Enabled
**What goes wrong:** Deploy workflow runs but site is not accessible
**Why it happens:** GitHub Pages must be configured in repo Settings to use "GitHub Actions" as source (not "Deploy from a branch")
**How to avoid:** Document the one-time manual step: Settings > Pages > Source > GitHub Actions
**Warning signs:** 404 at the expected URL

### Pitfall 3: Winget InstallerSha256 Mismatch
**What goes wrong:** Winget manifest validation fails with hash mismatch
**Why it happens:** SHA-256 was computed from a different build artifact or the download URL redirects
**How to avoid:** Download the exact MSI from the GitHub Release URL and compute SHA-256 from that file
**Warning signs:** PR CI checks fail on microsoft/winget-pkgs

### Pitfall 4: Homebrew Cask SHA-256 Stale
**What goes wrong:** `brew install` fails with checksum mismatch after a new release
**Why it happens:** DMG was rebuilt/replaced on the release but cask sha256 was not updated
**How to avoid:** Use `livecheck` stanza so Homebrew can detect new versions; update sha256 with each release
**Warning signs:** `brew audit --cask glass` reports errors

### Pitfall 5: winget PackageIdentifier Naming
**What goes wrong:** Package rejected during PR review
**Why it happens:** PackageIdentifier must match publisher/product naming conventions (Publisher.Package)
**How to avoid:** Use a clear identifier like `Glass.Terminal` -- must match what appears in Add/Remove Programs after MSI install
**Warning signs:** Automated validation bot comments on PR

### Pitfall 6: No Remote Origin Configured
**What goes wrong:** Cannot reference GitHub URLs in manifests, README badges, or docs site
**Why it happens:** This project currently has no git remote configured
**How to avoid:** A GitHub repository must be created and remote added BEFORE writing manifests and docs that reference it
**Warning signs:** All `https://github.com/<user>/glass` URLs are placeholders

## Code Examples

### Configuration Reference Content (for docs)
Based on the actual `GlassConfig` struct in `crates/glass_core/src/config.rs`:

```toml
# ~/.glass/config.toml

# Font settings
font_family = "Cascadia Code"    # Default: Consolas (Win), Menlo (Mac), Monospace (Linux)
font_size = 14.0                 # Default: 14.0

# Shell override (optional -- auto-detected if omitted)
shell = "pwsh"

# History settings
[history]
max_output_capture_kb = 50       # Max output capture per command (KB)

# Snapshot/undo settings
[snapshot]
enabled = true                   # Enable file snapshot capture
max_count = 1000                 # Max snapshots to retain
max_size_mb = 500                # Max total blob storage (MB)
retention_days = 30              # Days to keep snapshots

# Pipe visualization settings
[pipes]
enabled = true                   # Enable pipe stage capture
max_capture_mb = 10              # Max capture per stage (MB)
auto_expand = true               # Auto-expand pipeline blocks on failure
```

### README Structure
```markdown
# Glass

<badges: CI status, license, version>

<screenshot or GIF>

GPU-accelerated terminal emulator with command structure awareness.

## Features
- Command blocks with exit codes, duration, CWD
- Ctrl+Shift+F search across terminal history
- Ctrl+Shift+Z undo file changes
- Pipeline inspection with intermediate stage output
- Tabs (Ctrl+Shift+T) and split panes (Ctrl+Shift+D)
- MCP server for AI assistant integration
- SQLite-backed command history with FTS5 search

## Installation

### Windows
- **Installer:** Download `.msi` from [Releases](link)
- **winget:** `winget install Glass.Terminal`

### macOS
- **DMG:** Download `.dmg` from [Releases](link)
- **Homebrew:** `brew install <user>/glass/glass`

### Linux (Debian/Ubuntu)
- **deb:** Download `.deb` from [Releases](link)

## Configuration
Edit `~/.glass/config.toml` -- see [Configuration Reference](docs-url)

## Documentation
Full documentation at [docs-url]

## License
MIT
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Singleton winget manifest | Multi-file manifest (version+installer+locale) | 2023 (schema 1.4+) | Singleton is deprecated in winget-pkgs repo |
| gh-pages branch deploy | GitHub Actions artifact deploy (actions/deploy-pages) | 2022 | Simpler, no orphan branch management |
| peaceiris/actions-gh-pages | actions/upload-pages-artifact + actions/deploy-pages | 2023 | Official GitHub-maintained actions |
| Homebrew appcast for updates | livecheck block in cask | 2020 | appcast deprecated, livecheck is standard |

## Open Questions

1. **GitHub Repository URL**
   - What we know: No git remote is currently configured. The project needs a public GitHub repo for docs deployment, release URLs, and package manifests.
   - What's unclear: The exact GitHub username/org and repo name (e.g., `user/glass`)
   - Recommendation: The planner should use placeholders like `<GITHUB_USER>` and `<GITHUB_REPO>` in templates. The implementer will need to substitute actual values. A GitHub repo must exist before any of this work can be deployed.

2. **DMG Filename Convention**
   - What we know: `packaging/macos/build-dmg.sh` builds the DMG. The release workflow uploads from `target/macos/*.dmg`
   - What's unclear: Exact DMG filename pattern produced (needed for Homebrew cask URL)
   - Recommendation: Check `build-dmg.sh` output naming during implementation; use version-interpolated URL in cask

3. **MSI ProductCode for Winget**
   - What we know: Winget best practice includes ProductCode in installer manifest for upgrade detection
   - What's unclear: The exact ProductCode GUID from the built MSI (it is auto-generated by cargo-wix)
   - Recommendation: Build an MSI and extract ProductCode, or omit it initially (it is optional)

4. **Screenshots for README**
   - What we know: README needs screenshots per DOCS-03 success criteria
   - What's unclear: No screenshots currently exist in the repo
   - Recommendation: Capture screenshots during implementation showing: terminal with command blocks, search overlay, undo notification, pipeline view, tabs and panes

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Manual verification (content authoring phase -- no Rust code) |
| Config file | N/A |
| Quick run command | `mdbook build docs && echo "Build OK"` |
| Full suite command | `mdbook build docs && mdbook test docs` |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| DOCS-01 | mdBook site covers all features | manual | `mdbook build docs` (build success) | No -- Wave 0 |
| DOCS-02 | Docs deployed to GitHub Pages | manual | Check URL after CI runs | No -- Wave 0 |
| DOCS-03 | README has screenshots, install instructions, features, badges | manual | Visual inspection | No -- Wave 0 |
| PKG-05 | winget manifest valid | smoke | `winget validate --manifest <path>` (requires winget-cli) | No -- Wave 0 |
| PKG-06 | Homebrew cask formula valid | smoke | `brew audit --cask glass` (requires macOS) | No -- Wave 0 |

### Sampling Rate
- **Per task commit:** `mdbook build docs` (verifies no broken links or syntax errors)
- **Per wave merge:** Full build + manual review of rendered output
- **Phase gate:** Docs site live on GitHub Pages, README rendered correctly, manifests pass validation

### Wave 0 Gaps
- [ ] `docs/book.toml` -- mdBook configuration
- [ ] `docs/src/SUMMARY.md` -- Table of contents
- [ ] `README.md` -- Does not currently exist
- [ ] `packaging/winget/` -- Winget manifest directory
- [ ] `homebrew-glass/` -- Homebrew tap (separate repo or directory for cask)
- [ ] `.github/workflows/docs.yml` -- GitHub Pages deployment workflow
- [ ] mdBook installation: `cargo install mdbook` or use CI action

## Sources

### Primary (HIGH confidence)
- [mdBook official docs](https://rust-lang.github.io/mdBook/) - Setup, SUMMARY.md format, book.toml configuration, CI deployment
- [Microsoft Learn - winget manifest](https://learn.microsoft.com/en-us/windows/package-manager/package/manifest) - Multi-file manifest format, required fields, examples
- [Homebrew Cask Cookbook](https://docs.brew.sh/Cask-Cookbook) - Required stanzas, DMG cask format, livecheck
- [GitHub starter-workflows/pages/mdbook.yml](https://github.com/actions/starter-workflows/blob/main/pages/mdbook.yml) - Official GitHub Actions workflow for mdBook

### Secondary (MEDIUM confidence)
- [winget-pkgs manifest schema 1.6.0](https://github.com/microsoft/winget-pkgs/tree/master/doc/manifest/schema/1.6.0) - Current schema version docs
- [Homebrew Tap docs](https://docs.brew.sh/How-to-Create-and-Maintain-a-Tap) - Custom tap creation
- [peaceiris/actions-mdbook](https://github.com/peaceiris/actions-mdbook) - mdBook CI action

### Tertiary (LOW confidence)
- None -- all findings verified with official documentation

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - mdBook is the undisputed Rust docs standard; winget/Homebrew are platform standards
- Architecture: HIGH - mdBook structure, winget manifest format, and cask formula are well-documented
- Pitfalls: HIGH - Common issues are well-known in each ecosystem's documentation

**Research date:** 2026-03-07
**Valid until:** 2026-04-07 (stable tools, slow-moving specifications)
