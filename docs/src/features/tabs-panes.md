# Tabs and Panes

Glass supports multiple tabs and split panes, letting you organize your terminal workspace without external multiplexers.

## Tabs

Tabs appear in a tab bar at the top of the Glass window.

| Action | Shortcut |
|---|---|
| New tab | Ctrl+Shift+T |
| Close tab | Ctrl+Shift+W |

- Each tab runs its own independent shell session.
- Tab names are displayed in the tab bar.
- Click a tab to switch to it.
- Hover a tab to reveal its close button ("x"); click it to close the tab.
- Click the "+" button at the right end of the tab bar to open a new tab.
- Middle-click a tab to close it without switching to it first.
- Drag a tab left or right to reorder tabs within the tab bar.

## Panes

Panes let you split a single tab into multiple side-by-side terminal views.

| Action | Shortcut |
|---|---|
| Split horizontally | Ctrl+Shift+D |
| Split vertically | Ctrl+Shift+E |
| Close pane | Ctrl+Shift+W |
| Focus left / right / up / down | Alt+Left / Right / Up / Down |
| Resize pane | Alt+Shift+Left / Right / Up / Down |

- Each pane runs its own shell session, independent of other panes.
- Click a pane to give it focus, or use Alt+Arrow to navigate between panes without the mouse.
- Alt+Shift+Arrow resizes the focused pane incrementally.
- When you close the last pane in a tab, the tab closes.

## Independent state per tab and pane

Each tab and each pane maintains its own independent state:

- **History** -- Command history is scoped per pane session.
- **Snapshots** -- File undo snapshots are tracked per command block, regardless of which pane it originated from.
- **SOI** -- Output classification runs independently in each pane.

## Workflow tips

- Use **tabs** to separate different projects or contexts (e.g., one tab for frontend, another for backend).
- Use **panes** to see multiple views within the same context (e.g., editor in one pane, build output in another).
- All Glass features (blocks, search, undo, pipes) work independently in each pane.
