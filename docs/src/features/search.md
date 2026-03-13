# Search

Glass provides full-text search across your entire command history, powered by SQLite FTS5. Search persists across sessions -- you can find commands and output from days or weeks ago.

## Using search

Press **Ctrl+Shift+F** to open the search overlay.

1. Type your search query in the search bar.
2. Results appear in real time as you type.
3. Navigate between results using the overlay controls.
4. Press **Escape** to close the search overlay.

## What gets searched

Search indexes:

- **Commands** -- The command lines you typed
- **Output** -- The stdout/stderr produced by each command
- **Metadata** -- Working directories, timestamps

## Scope

Search works across all tabs and all past sessions. Results are not limited to the current tab or the current session -- any command recorded in the history database is included.

## FTS5-powered

Glass stores all command history in a SQLite database with FTS5 full-text indexing. This means:

- **Fast** -- Even with thousands of commands, search returns results instantly
- **Persistent** -- History survives across sessions and restarts
- **Flexible** -- Standard search query syntax with implicit AND between terms

## Search tips

- Search for error messages to find when and where they first appeared
- Search for filenames to find commands that operated on specific files
- Combine terms to narrow results (e.g., `build error` finds commands containing both words)
