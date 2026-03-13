# Activity Stream Design Spec

## Overview

Glass gains an **Activity Stream** — a visual layer that surfaces what AI agents are doing behind the scenes. It shows agent lifecycle events, file lock operations, inter-agent communication, Agent Mode observations, and command context in a unified view that scales from single-agent transparency to multi-agent orchestration.

### Design Principles

- **Progressive disclosure**: compact ambient indicator that expands into a full overlay on demand
- **Write-only for AI**: agents emit events but never read the stream — zero token cost
- **Zero cost when solo**: UI elements appear only when agents are active
- **Human-facing view**: renders data that already exists in the coordination and history databases

## Architecture

### Data Layer: Activity Events

A new `coordination_events` table in the existing coordination database (`~/.glass/agents.db`):

```sql
CREATE TABLE coordination_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp INTEGER NOT NULL,
    project TEXT NOT NULL,
    category TEXT NOT NULL,      -- 'agent', 'lock', 'message', 'observe', 'command'
    agent_id TEXT,
    agent_name TEXT,             -- denormalized for display after deregistration
    event_type TEXT NOT NULL,
    summary TEXT NOT NULL,       -- human-readable one-liner
    detail TEXT,                 -- optional JSON payload
    pinned INTEGER DEFAULT 0
);
CREATE INDEX idx_coord_events_project_ts ON coordination_events(project, timestamp);
```

**Note on naming:** The existing `ActivityEvent` and `ActivityFilter` types in `glass_core::activity_stream` are SOI pipeline types (command-level events for agent consumption). The new coordination events are a separate concept — human-facing UI events about agent behavior. To avoid collision, the new type is named `CoordinationEvent` and the table is `coordination_events`.

**Retention**: 1000 non-pinned events per project, pruned on each poller cycle. Pinned events are exempt from count-based pruning but expire after 24 hours.

### Event Sources

Events are emitted as side-effects of existing `CoordinationDb` methods — no new API for agents to call:

- `register()` → `agent.registered`
- `deregister()` → `agent.deregistered`
- `update_status()` → `agent.status_changed`, `agent.task_changed`
- `lock_files()` → `lock.acquired` or `lock.conflict` (pinned)
- `unlock_file()` / `unlock_all()` → `lock.released`
- `send_message()` → `message.sent`
- `broadcast()` → `message.broadcast`
- Poller detects stale heartbeat → `agent.heartbeat_lost` (pinned)
- Agent Mode hooks → `observe.*` events
- OSC 133 boundaries → `command.*` events

### Integration Points

No new crates. Changes to existing crates:

| Crate | Changes |
|---|---|
| `glass_coordination` | `coordination_events` table, `CoordinationEvent` type, event emission in DB methods |
| `glass_core` | Enrich `CoordinationState` with `agents: Vec<AgentDisplayInfo>` and `recent_events: Vec<CoordinationEvent>`, `ticker_event: Option<CoordinationEvent>` |
| `glass_renderer` | New `ActivityOverlayRenderer`, extend `StatusBarRenderer` for two-line mode |
| `src/main.rs` | Hotkey handler, overlay state, pass data to frame renderer |

### Data Flow

```
CoordinationDb methods
  → INSERT into coordination_events

coordination_poller (every 5s)
  → SELECT recent events + agent summaries
  → AppEvent::CoordinationUpdate(enriched state)

main.rs event loop
  → stores state for renderer

FrameRenderer::draw_frame()
  → StatusBarRenderer (compact layer, always)
  → ActivityOverlayRenderer (only when overlay toggled)
```

No new async runtime, threads, or database connections.

## UI: Compact Layer — Two-Line Contextual Status Bar

### Behavior

- **No agents registered** → standard single-line status bar (unchanged from today)
- **>=1 agent registered** → status bar grows to two lines
  - Top line: agent activity indicators
  - Bottom line: existing CWD + git info (unchanged)

### Top Line Layout

```
● claude-code editing main.rs  │  ● cursor idle  │  ⚡ 2 locks      ▼ Ctrl+Shift+G
```

- Each agent: colored dot + name + status/task (truncated)
- Agents separated by `│` divider
- Lock summary on the right
- Expand hint at far right
- More than fits: show first 2 + `+N more`

### Color Coding

- **Agent dots**: unique color per agent from palette (purple, blue, teal, amber)
- **Status text**: green (active/editing), gray (idle), red (conflict)
- **Lock indicator**: amber

### Event Ticker

When a notable event occurs, the agent area briefly shows the event text for one poller cycle (5 seconds) before returning to steady-state agent indicators.

### Implementation

- Extend `StatusBarRenderer` with two-line mode
- `CoordinationState` gains `agents: Vec<AgentDisplayInfo>` and `ticker_event: Option<CoordinationEvent>` (the most recent notable event, cleared after one display cycle)
- `AgentDisplayInfo` struct:
  ```rust
  pub struct AgentDisplayInfo {
      pub id: String,
      pub name: String,
      pub agent_type: String,
      pub status: String,          // "active", "idle", "editing"
      pub task: Option<String>,
      pub lock_count: usize,
      pub locked_files: Vec<String>,
  }
  ```
  Constructed by the poller by joining the `agents` and `file_locks` tables. This is a display-oriented projection of the existing `AgentInfo` type — it adds `lock_count` and `locked_files` which `AgentInfo` does not carry.
- Frame layout adjusts viewport height based on status bar line count

## UI: Expanded Overlay — Fullscreen Activity Stream

### Activation

- `Ctrl+Shift+G` toggles overlay
- `Esc` closes (consistent with search/proposal overlays)

### Layout

Two-column with header bar:

**Header**: "Activity Stream" title + filter tabs (All | Agents | Locks | Observations | Messages) + close hint

**Left Column — Agent Cards** (fixed width ~280px):
- Each agent gets a card with unique color stripe on the left border
- Card contents: name, status badge, current task, held locks (with file paths), heartbeat age
- Idle agents rendered at lower opacity
- **Pinned Alerts** section below agent cards for conflicts and critical events (amber border, persist until dismissed)

**Right Column — Event Timeline** (flex):
- Events grouped by minute with time group headers
- Each event row: seconds timestamp, agent badge (colored), verb (color-coded), detail text
- Agent Mode observation chains get a subtle green left border on the most recent event
- Auto-scrolls to newest events; pauses auto-scroll when user scrolls up manually

### Filter Tabs

- **All** — every event
- **Agents** — lifecycle events (registered, deregistered, status changes)
- **Locks** — lock acquired, released, conflicts
- **Observations** — Agent Mode observation pipeline events
- **Messages** — inter-agent communication

### Rendering

Follows existing overlay patterns (`SearchOverlayRenderer`, `ProposalOverlayRenderer`):
- Semi-transparent backdrop
- Content panel with rects + glyphon text
- Rendered in the same pass as other overlays

## Agent Mode Transparency

Agent Mode's observation pipeline becomes visible through `observe.*` events:

| Stage | Event Type | Example |
|---|---|---|
| See | `observe.watching` | `agent-mode started watching terminal` |
| See | `observe.command_seen` | `agent-mode saw cargo build (exit: 1)` |
| Analyze | `observe.output_parsed` | `agent-mode analyzed build output — 3 errors` |
| Analyze | `observe.error_noticed` | `agent-mode noticed error[E0382]: use of moved value` |
| Decide | `observe.thinking` | `agent-mode thinking considering ownership fix` |
| Decide | `observe.dismissed` | `agent-mode dismissed cargo test (all passed)` |
| Act | `observe.proposing` | `agent-mode proposing fix borrow in pty.rs:142-145` |

These events are emitted by the Glass binary (Agent Mode runtime in `main.rs`), not by the AI agent. Zero token cost — they log what is already happening.

## Event Taxonomy

### Agent Lifecycle (`category: "agent"`)

| Event Type | Source | Pinned |
|---|---|---|
| `registered` | `register()` | No |
| `deregistered` | `deregister()` | No |
| `status_changed` | `update_status()` | No |
| `task_changed` | `update_status()` | No |
| `heartbeat_lost` | poller (>30s stale) | Yes |

### File Operations (`category: "lock"`)

| Event Type | Source | Pinned |
|---|---|---|
| `acquired` | `lock_files()` | No |
| `released` | `unlock_file()` / `unlock_all()` | No |
| `conflict` | `lock_files()` | Yes |

### Agent Communication (`category: "message"`)

| Event Type | Source | Pinned |
|---|---|---|
| `sent` | `send_message()` | No |
| `broadcast` | `broadcast()` | No |
| `request_unlock` | `send_message(request_unlock)` | No |

### Agent Mode Observation (`category: "observe"`)

| Event Type | Source | Pinned |
|---|---|---|
| `watching` | agent mode startup | No |
| `command_seen` | OSC 133 D | No |
| `output_parsed` | SOI analysis | No |
| `error_noticed` | SOI error detection | No |
| `thinking` | agent mode decision | No |
| `proposing` | proposal creation | No |
| `dismissed` | agent mode no-action | No |

### Command Context (`category: "command"`)

| Event Type | Source | Pinned |
|---|---|---|
| `started` | OSC 133 C | No |
| `finished` | OSC 133 D | No |
| `pipeline` | pipe stage detection | No |

### Filtering Rules

- `heartbeat_lost` and `conflict` events auto-pin (persist until dismissed)
- `dismissed` events hidden by default (shown with verbose toggle)
- Heartbeat events are never logged
- Consecutive same-type events from same agent within 2s are collapsed (e.g., locking 5 files = one "locked 5 files" entry)

## Keyboard & Interaction

### Hotkeys

| Key | Action |
|---|---|
| `Ctrl+Shift+G` | Toggle activity stream overlay |
| `Esc` | Close overlay |
| `Tab` / `Shift+Tab` | Cycle filter tabs (while overlay open) |
| `Up` / `Down` | Scroll event timeline |
| `D` | Dismiss selected pinned alert |
| `V` | Toggle verbose mode |

### State

```rust
// In main app state
activity_overlay_visible: bool,
activity_view_filter: ActivityViewFilter,  // enum: All, Agents, Locks, Observations, Messages
activity_scroll_offset: usize,
activity_pinned_selected: Option<usize>,
```

Note: `ActivityViewFilter` is a new enum distinct from the existing `ActivityFilter` struct in `glass_core::activity_stream` (which is an SOI rate-limiter).

All state resets on overlay close. No persistence needed.

### Interaction Model

- Read-only overlay — no text input, no cursor management
- Keyboard-only activation (`Ctrl+Shift+G`). Mouse click on status bar is deferred — requires new hit-testing infrastructure not currently in the renderer.
- Filter tabs are keyboard-navigable (Tab/Shift+Tab)

## Token & Performance Impact

- **Zero token cost for agents**: activity stream is write-only from agent perspective. Agents use existing MCP tools (`glass_context`, `glass_agent_list`) for their own awareness — those channels are unchanged.
- **Minimal DB overhead**: events are INSERTed as side-effects of existing operations (single extra INSERT per coordination call). Poller adds one SELECT per cycle (already running every 5s).
- **Rendering cost**: overlay only renders when visible. Compact bar adds one text line when agents are active. Both use existing rect + text rendering pipeline.
- **Memory**: ~1000 `CoordinationEvent` structs in memory (poller fetches recent window). Each event is small (strings + integer fields).
