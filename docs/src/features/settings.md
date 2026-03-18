# Settings Overlay

Glass includes an in-app settings editor accessible with **Ctrl+Shift+,** (Cmd+Shift+, on macOS). The overlay lets you browse and edit configuration without leaving the terminal.

---

## Layout

The overlay has three tabs, navigable with Tab/Shift+Tab:

### Settings Tab

A sidebar lists 8 configuration sections:

| Section | Controls |
|---|---|
| Font | Font family and size |
| Agent Mode | Enable/disable, autonomy mode, budget, cooldown, permissions |
| SOI | Enable/disable structured output parsing, shell summary, min lines |
| Snapshots | Enable/disable, max count, max size, retention days |
| Pipes | Enable/disable, max capture size, auto-expand |
| History | Output capture limits |
| Orchestrator | Enable/disable, silence timeout, PRD path, verify mode, metric guard, feedback loop |
| Scripting | Enable/disable Rhai engine, operation limits, timeout, scripts per hook |

Use arrow keys to navigate between sections and fields. Enter/Space toggles booleans. +/- adjusts numeric values.

Changes are written directly to `~/.glass/config.toml` and hot-reloaded immediately -- no restart required.

### Shortcuts Tab

A two-column keyboard shortcut cheatsheet covering all Glass keybindings organized by category (Core, Tabs, Panes, Navigation, Overlays).

### About Tab

Version info, platform details, and license.

---

## Controls

| Key | Action |
|---|---|
| Ctrl+Shift+, | Open/close settings overlay |
| Tab / Shift+Tab | Switch between tabs |
| Arrow Up/Down | Navigate sections and fields |
| Enter / Space | Toggle boolean values |
| +/- | Adjust numeric values |
| Escape | Close overlay |
