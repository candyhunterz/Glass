# Multi-Agent Coordination

Glass provides a coordination layer for projects where multiple AI agents work simultaneously — for example, one agent refactoring a module while another writes tests. Without coordination, agents can conflict: two agents editing the same file at once, one agent undoing work the other just completed, or agents duplicating effort on the same task.

---

## Overview

When multiple AI agents operate on the same project inside Glass tabs, Glass coordinates them through a shared SQLite database at `~/.glass/agents.db`, opened in WAL mode to allow concurrent readers and writers without contention. Coordination is scoped by project root path, so agents on different projects do not interfere with each other.

Coordination is entirely advisory — Glass does not enforce locks at the filesystem level. Agents that follow the protocol gain safe concurrent operation; agents that ignore it are unaffected by other agents' advisory locks.

---

## Features

### Agent Registry

Each agent registers on session start and deregisters on exit. The registry records:

- Agent name and type (e.g., `claude-code`, `cursor`)
- Project root path
- Registration timestamp and last heartbeat
- Current status and task description
- OS process ID (PID)

Glass uses heartbeat timestamps together with PID detection to identify stale registrations from agents that crashed or disconnected without calling `glass_agent_deregister`. Stale registrations are automatically cleaned up and their locks released.

### Advisory File Locks

Agents acquire advisory locks on files before editing them. Locking is:

- **Atomic and all-or-nothing**: if an agent requests locks on five files and one is already held, no locks are acquired and the call returns a conflict identifying the holder.
- **Path-canonicalized**: lock keys are resolved to canonical absolute paths so that `./src/main.rs` and `/home/user/project/src/main.rs` refer to the same lock.
- **Conflict-transparent**: a conflict response includes the holding agent's name, type, ID, and current task description, giving the requesting agent enough context to decide whether to wait, ask for a release, or work on something else.

Locks are released explicitly via `glass_agent_unlock`, or automatically when an agent deregisters or its registration expires.

### Inter-Agent Messaging

Agents can communicate through a message bus backed by the same SQLite database:

- **Broadcast**: send a message to all agents registered on the project.
- **Directed send**: send a message to a specific agent by ID.
- **Mark-as-read semantics**: `glass_agent_messages` returns only unread messages and marks them as read, so agents do not process the same message twice.

A standard message type, `request_unlock`, is used to ask a lock holder to release a file so another agent can edit it.

### Status Tracking

Agents publish their current task via `glass_agent_status`. Any agent (or a human via the Glass GUI) can call `glass_agent_list` to see all registered agents, what each one is working on, which files each holds locks on, and when each last sent a heartbeat.

---

## MCP Tools

All coordination is exposed through the Glass MCP server. The 11 coordination tools are:

| Tool | Description |
|------|-------------|
| `glass_agent_register` | Register an agent with a name, type, and project root. Returns an agent ID for all subsequent calls. |
| `glass_agent_deregister` | Deregister the agent and release all held locks. Call this on session end. |
| `glass_agent_list` | List all registered agents for the project with their status, task, held locks, and heartbeat time. |
| `glass_agent_status` | Update the calling agent's status string and current task description. |
| `glass_agent_heartbeat` | Send a liveness signal. Glass uses this together with PID detection to expire stale registrations. |
| `glass_agent_lock` | Acquire advisory locks on one or more file paths (atomic, all-or-nothing). |
| `glass_agent_unlock` | Release advisory locks on one or more file paths held by the calling agent. |
| `glass_agent_locks` | List all active advisory locks for the project, including which agent holds each path. |
| `glass_agent_broadcast` | Send a message to all registered agents on the project. |
| `glass_agent_send` | Send a directed message to a specific agent by ID. |
| `glass_agent_messages` | Retrieve and mark as read all unread messages for the calling agent. |

---

## Protocol for AI Agents

AI agents operating in a Glass-managed project should follow this protocol to participate safely in multi-agent coordination.

**On session start** — Call `glass_agent_register` with your agent name, type (e.g., `claude-code`), and the project root path. Store the returned agent ID; it is required for all subsequent coordination calls.

**Before editing files** — Call `glass_agent_lock` with the list of file paths you intend to modify. If the call returns a conflict, do not edit the locked files. Instead, use `glass_agent_send` with `msg_type: "request_unlock"` to ask the holder to release the files, or choose a different task that does not require those files.

**After editing files** — Call `glass_agent_unlock` to release the locks on files you have finished editing. Do not hold locks longer than necessary; other agents may be waiting.

**Periodically** — Call `glass_agent_messages` to check for messages from other agents. Process `request_unlock` messages by releasing the requested files if you are done with them, then sending a reply or acknowledgment.

**When changing tasks** — Call `glass_agent_status` to update your status and task description. This keeps the shared registry accurate so other agents can make informed decisions about what to work on.

**On session end** — Call `glass_agent_deregister` to clean up your registration and release all held locks. If your process exits without deregistering, Glass will eventually expire your registration via heartbeat timeout and PID detection, but explicit deregistration is faster and cleaner.

---

## GUI Integration

The Glass status bar displays live coordination state when agents are active:

- **Agent count**: shows the number of agents currently registered on the project.
- **Lock count**: shows the total number of advisory file locks currently held across all agents.
- **Tab lock indicators**: tabs that have agents holding locks display a lock indicator in the tab bar.
- **Conflict warning overlay**: when an agent fails to acquire a lock due to a conflict, Glass surfaces a brief overlay in the terminal identifying the conflicting agent and the files involved, so the human operator is aware of coordination activity.
