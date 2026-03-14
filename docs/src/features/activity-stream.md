# Activity Stream

The Activity Stream surfaces AI agent behavior in real time. When agents register, acquire locks, send messages, or propose changes, Glass records each action as a coordination event and displays it through a compact status bar summary and a fullscreen overlay.

---

## Compact Status Bar

When one or more agents are registered on the current project, the Glass status bar expands to two lines:

- **Line 1 (existing)**: shell status, CWD, git branch, SOI summary, agent mode indicator.
- **Line 2 (new)**: agent activity summary showing agent names, statuses, current tasks, and total lock count.

If more than two agents are active, the status bar shows the first two and a "+N more" overflow indicator.

### Ticker

When a new coordination event occurs (e.g., an agent acquires a lock or sends a message), the second line briefly shows the event summary before reverting to the steady-state agent display. The ticker auto-clears after one poll cycle (~5 seconds).

---

## Activity Overlay

Press **Ctrl+Shift+G** (Cmd+Shift+G on macOS) to open the fullscreen activity stream overlay. Press **Esc** or **Ctrl+Shift+G** again to close it.

### Layout

The overlay has two columns:

**Left column — Agent cards**: each registered agent is shown as a card with its name (color-coded), current status, active task, and locked files. Idle agents are dimmed.

**Right column — Event timeline**: a chronological list of coordination events grouped by minute. Each entry shows a timestamp, agent name badge (color-coded by name hash), event verb (color-coded by category), and a summary.

### Keyboard Controls

| Key | Action |
|-----|--------|
| Esc | Close overlay |
| Tab | Cycle to next filter tab |
| Shift+Tab | Cycle to previous filter tab |
| Arrow Up | Scroll timeline up |
| Arrow Down | Scroll timeline down |
| V | Toggle verbose mode (show/hide dismissed events) |

### Filter Tabs

The overlay header shows five filter tabs:

| Tab | Shows |
|-----|-------|
| All | All event categories |
| Agents | Agent registrations, deregistrations, status changes, task changes |
| Locks | Lock acquisitions, releases, and conflicts |
| Observations | Agent Mode observation events (output parsed, errors noticed, proposals) |
| Messages | Directed messages and broadcasts between agents |

The active tab is highlighted in purple. Switching tabs resets the scroll position to the top.

---

## Event Categories

Glass records events in four categories:

### Agent Events (`agent`)
- `registered` — Agent joined the project
- `deregistered` — Agent left the project
- `status_changed` — Agent changed its status (e.g., idle -> editing)
- `task_changed` — Agent updated its current task description

### Lock Events (`lock`)
- `acquired` — Agent locked one or more files
- `released` — Agent released locks
- `conflict` — Agent failed to acquire a lock (held by another agent). Conflicts are **pinned** and remain visible until dismissed.

### Observation Events (`observe`)
- `output_parsed` — Agent Mode analyzed command output via SOI
- `error_noticed` — Agent Mode detected an error or warning in command output
- `dismissed` — Agent Mode determined no action was needed
- `proposing` — Agent Mode created a code change proposal

### Message Events (`message`)
- `sent` — Directed message sent to a specific agent
- `broadcast` — Message sent to all agents on the project
- `request_unlock` — Special message type requesting a lock holder to release files

### Command Events (`command`)
- `started` — A shell command began executing (OSC 133 C boundary)
- `finished` — A shell command completed (OSC 133 D boundary), with exit code and duration

---

## Event Colors

Events are color-coded by verb to make scanning the timeline fast:

| Color | Event types |
|-------|-------------|
| Green | registered, status_changed, started, analyzed |
| Amber | acquired, locked, proposing |
| Red | conflict, error_noticed, heartbeat_lost |
| Blue | sent, broadcast, message |
| Gray | All other event types |

Agent names use a stable color from a six-color palette (purple, blue, teal, amber, coral, green), derived from a hash of the agent name so the same agent always appears in the same color.

---

## Data Storage

Coordination events are stored in the `coordination_events` table of `~/.glass/agents.db` alongside the existing agent registry and lock tables. Events are:

- **Scoped by project** — Each event records the project root path, so events from different projects do not mix.
- **Auto-pruned** — On each poll cycle, events beyond the most recent 1000 per project are deleted.
- **Pinned** — Lock conflict events are marked as pinned and remain visible in the overlay until explicitly dismissed.

The coordination poller fetches the most recent 200 events per poll cycle (~5 seconds) for the overlay timeline and surfaces the newest event as the ticker.
