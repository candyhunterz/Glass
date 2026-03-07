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

## Panes

Panes let you split a single tab into multiple side-by-side terminal views.

| Action | Shortcut |
|---|---|
| Split vertically | Ctrl+Shift+D |
| Close pane | Ctrl+Shift+W |

- Each pane runs its own shell session, independent of other panes.
- Click a pane to give it focus.
- When you close the last pane in a tab, the tab closes.

## Workflow tips

- Use **tabs** to separate different projects or contexts (e.g., one tab for frontend, another for backend).
- Use **panes** to see multiple views within the same context (e.g., editor in one pane, build output in another).
- All Glass features (blocks, search, undo, pipes) work independently in each pane.
