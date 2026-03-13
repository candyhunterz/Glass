# Phase 55: Agent Activity Stream - Research

**Researched:** 2026-03-13
**Domain:** Rust channel design, bounded MPSC, token budgeting, deduplication, rate limiting
**Confidence:** HIGH

## Summary

Phase 55 bridges the completed SOI pipeline (Phase 50) and the upcoming agent runtime (Phase 56). When a `SoiReady` event arrives on the main thread, a compressed `ActivityEvent` is produced and sent into a bounded, in-process channel. A consumer half (held by the future agent runtime struct) drains this channel. Because Phase 56 doesn't exist yet, the channel receiver is simply stored in `Processor` and left unread for now — the key deliverable is the sender-side infrastructure: noise filter, rolling budget window, and rate limiter.

The implementation is entirely in `src/main.rs` and a new lightweight module (either a `glass_agent` crate or a module within `glass_core`). No new crate is strictly required — the activity stream is an in-process data structure, not a subprocess protocol. Given the project pattern of putting shared in-process types in `glass_core` (coordination poller, IPC types), the activity stream types belong in `glass_core` as a new `activity_stream` module.

The channel mechanism is `std::sync::mpsc` — this matches the established pattern in `glass_terminal/pty.rs` and `glass_snapshot/watcher.rs`. The `Processor` on the winit main thread holds the `Sender<ActivityEvent>`, and the future agent runtime will hold the `Receiver<ActivityEvent>`. Using `std::sync::mpsc::sync_channel(N)` (bounded) is the correct choice: it gives back-pressure and enforces the capacity limit without requiring a tokio runtime on the main thread.

**Primary recommendation:** Add an `activity_stream` module to `glass_core` containing `ActivityEvent`, `ActivityStreamConfig`, `ActivityFilter` (noise + rate limiter + budget window). Add `activity_stream_tx: Option<std::sync::mpsc::SyncSender<ActivityEvent>>` to `Processor`. In the `SoiReady` handler, after updating `session.last_soi_summary`, call into `ActivityFilter::process()` and send the result through the channel sender if Some.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| AGTA-01 | Activity stream feeds compressed SOI summaries to agent runtime via bounded channel | `std::sync::mpsc::sync_channel(N)` creates a bounded channel; `Processor` holds `SyncSender`, agent runtime holds `Receiver`. Feed from `SoiReady` handler. |
| AGTA-02 | Rolling budget window constrains activity context to configurable token limit (default 4096) | `ActivityWindow` struct holds a `VecDeque<ActivityEvent>` and tracks cumulative `token_cost`. On push, sum existing + new; if over budget, drain oldest until budget is satisfied. |
| AGTA-03 | Noise filtering deduplicates and collapses repetitive success events | Track `last_fingerprint: Option<String>` per session. Fingerprint = hash of `(command_text, severity, summary)`. Consecutive identical fingerprints with Success/Info severity are collapsed into a `collapsed_count` counter rather than emitted individually. |
| AGTA-04 | Rate limiting prevents flooding on rapid command execution | Token-bucket or leaky-bucket per channel: track `last_emit_time: Instant` and `tokens: f64`. Refill at `rate` events/sec (configurable, default 5/sec). On each candidate event, check if a token is available. If not, drop or defer. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `std::sync::mpsc` | stdlib | Bounded MPSC channel | Matches `glass_terminal/pty.rs` and `glass_snapshot/watcher.rs` patterns. No tokio runtime needed on winit main thread. |
| `std::collections::VecDeque` | stdlib | Rolling budget window | O(1) front-push/back-pop, exactly right for sliding window |
| `std::time::Instant` | stdlib | Rate limiter timestamps | Monotonic clock, no external dep |
| `serde_json` | 1.0 (workspace) | Token cost estimation (word count heuristic) | Already in root Cargo.toml and glass_core does not yet use it directly — keep estimate as string word count if serde_json is not added to glass_core |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `tracing` | workspace | Logging dropped/collapsed events | Project convention |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `std::sync::mpsc::sync_channel` | `tokio::sync::mpsc::channel` | tokio channel requires async receiver; agent runtime in Phase 56 is a struct in Processor (not async), so std mpsc is correct |
| `std::sync::mpsc::sync_channel` | `crossbeam-channel` | crossbeam not in workspace; std mpsc is sufficient for 1-sender, 1-receiver usage |
| Rolling VecDeque budget window | Circular ring buffer (fixed cap) | VecDeque is simpler, stdlib, adequate for ~100-event windows |
| Fingerprint via SHA-256 | Fingerprint via simple string concat | String concat is deterministic and zero-dep; hash only if collisions become a concern |

**Installation:** No new Cargo.toml dependencies required. `std::sync::mpsc`, `VecDeque`, and `Instant` are all stdlib.

## Architecture Patterns

### Recommended Module Structure

```
crates/glass_core/src/
├── activity_stream.rs        # ActivityEvent, ActivityFilter, ActivityStreamConfig (new)
├── event.rs                  # AppEvent (no change needed in Phase 55)
└── coordination_poller.rs    # Reference pattern for background-data structs

src/main.rs
└── Processor.activity_stream_tx: Option<SyncSender<ActivityEvent>>
    # Populated at app startup (once, in watcher_spawned block)
    # Fed from SoiReady handler
```

### Pattern 1: ActivityEvent Type

**What:** A single activity item sent through the channel. Carries enough for the agent to understand what happened without re-querying the DB.

**When to use:** Created once per qualifying `SoiReady` event.

```rust
// Source: crates/glass_core/src/activity_stream.rs (new file)
#[derive(Debug, Clone)]
pub struct ActivityEvent {
    /// History DB row id for drill-down via glass_query MCP tool.
    pub command_id: i64,
    /// One-line compressed summary from SOI.
    pub summary: String,
    /// Highest severity: "Error" | "Warning" | "Info" | "Success"
    pub severity: String,
    /// Wall-clock timestamp (Unix seconds).
    pub timestamp_secs: u64,
    /// Approximate token cost of this event for budget accounting.
    pub token_cost: usize,
    /// How many identical events were collapsed into this one (1 = not collapsed).
    pub collapsed_count: u32,
}
```

### Pattern 2: ActivityStreamConfig (sourced from [agent] config, defaulted here)

```rust
// Source: crates/glass_core/src/activity_stream.rs (new file)
#[derive(Debug, Clone)]
pub struct ActivityStreamConfig {
    /// Channel capacity (max pending events). Default 256.
    pub channel_capacity: usize,
    /// Rolling token budget for the window. Default 4096 (AGTA-02).
    pub token_budget: usize,
    /// Max events emitted per second (rate limiter). Default 5.0 (AGTA-04).
    pub max_rate_per_sec: f64,
}

impl Default for ActivityStreamConfig {
    fn default() -> Self {
        Self {
            channel_capacity: 256,
            token_budget: 4096,
            max_rate_per_sec: 5.0,
        }
    }
}
```

### Pattern 3: ActivityFilter — noise filter + rate limiter + budget window

```rust
// Source: crates/glass_core/src/activity_stream.rs (new file)

use std::collections::VecDeque;
use std::time::Instant;

pub struct ActivityFilter {
    config: ActivityStreamConfig,
    /// Rolling window of emitted events for budget accounting.
    window: VecDeque<ActivityEvent>,
    /// Cumulative token cost of all events in the window.
    window_token_cost: usize,
    /// Rate limiter: tokens available (leaky bucket).
    rate_tokens: f64,
    /// Rate limiter: last refill instant.
    last_refill: Instant,
    /// Fingerprint of the last emitted event (for dedup/collapse).
    last_fingerprint: Option<String>,
    /// How many events have been collapsed since last emit.
    pending_collapsed: u32,
}

impl ActivityFilter {
    pub fn new(config: ActivityStreamConfig) -> Self { ... }

    /// Fingerprint for a candidate event. Success/Info events are candidates for collapse.
    fn fingerprint(summary: &str, severity: &str) -> String {
        format!("{}:{}", severity, summary)
    }

    /// Returns Some(ActivityEvent) if the event passes filters, None if dropped.
    pub fn process(
        &mut self,
        command_id: i64,
        summary: String,
        severity: String,
    ) -> Option<ActivityEvent> { ... }

    /// Returns a snapshot of the current window contents (for agent context queries).
    pub fn window_events(&self) -> impl Iterator<Item = &ActivityEvent> { ... }
}
```

### Pattern 4: Channel Creation and Storage in Processor

```rust
// Source: src/main.rs — Processor struct (add field)
struct Processor {
    // ... existing fields ...
    /// Sender half of the agent activity stream channel.
    /// Receiver held by future agent runtime (Phase 56).
    activity_stream_tx: Option<std::sync::mpsc::SyncSender<glass_core::activity_stream::ActivityEvent>>,
    /// Agent activity filter: dedup + rate limit + budget window.
    activity_filter: glass_core::activity_stream::ActivityFilter,
}

// Creation (in watcher_spawned block, once):
let (tx, rx) = std::sync::mpsc::sync_channel(config.channel_capacity);
self.activity_stream_tx = Some(tx);
// rx is stored for Phase 56 agent runtime (ignore for now, or store in Processor)
```

### Pattern 5: Feeding from SoiReady Handler

```rust
// Source: src/main.rs — AppEvent::SoiReady arm (add after existing last_soi_summary update)
if let Some(event) = self.activity_filter.process(command_id, summary.clone(), severity.clone()) {
    if let Some(tx) = &self.activity_stream_tx {
        // SyncSender::try_send drops on full channel (non-blocking, bounded)
        if tx.try_send(event).is_err() {
            tracing::debug!("Activity stream channel full or closed, dropping event");
        }
    }
}
```

### Anti-Patterns to Avoid

- **Blocking send on the main thread:** `SyncSender::send()` blocks when channel is full. Always use `try_send()` to drop on back-pressure, never block the winit event loop.
- **Unbounded `std::sync::mpsc::channel()`:** Creates unbounded queue that violates AGTA-02. Use `sync_channel(N)` exclusively.
- **Global/static channel:** Phase 55 channel must be scoped to the `Processor` instance. Static channels make testing impossible and create session-scoping issues.
- **Storing `Receiver` without a consumer:** The receiver will accumulate events. Store it in `Processor` for Phase 56 to pluck out, or drop it immediately (which closes the channel). Safest: store `Option<Receiver<ActivityEvent>>` in `Processor` to hand off to the agent runtime.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Bounded channel | Custom ring buffer | `std::sync::mpsc::sync_channel(N)` | stdlib, correct semantics, back-pressure built in |
| Rate limiting | Timer-per-event with sleep | Leaky bucket with `Instant::elapsed()` | Sleep on main thread is prohibited; leaky bucket is O(1) per check |
| Token counting | Full LLM tokenizer | Word-count heuristic (`summary.split_whitespace().count() + overhead`) | Exact token count requires a tokenizer crate not in workspace; heuristic is sufficient for budget enforcement |
| Deduplication | HashMap of recent events | Single `last_fingerprint: Option<String>` | Consecutive-collapse pattern (not global dedup); single field covers the `cargo check` loop case from AGTA-03 success criteria |

**Key insight:** The activity stream is an in-process filtering pipeline with four concerns (emit, dedup, rate-limit, budget). Each concern is independent and trivially testable with stdlib primitives. No external crate is justified.

## Common Pitfalls

### Pitfall 1: Blocking `send()` on the main thread
**What goes wrong:** `SyncSender::send()` blocks when the channel buffer is full. On the winit main thread this freezes the entire terminal UI.
**Why it happens:** Confusing `SyncSender::send` (blocking) with `SyncSender::try_send` (non-blocking).
**How to avoid:** Always use `try_send()`. Log drops with `tracing::debug!`.
**Warning signs:** Terminal becomes unresponsive during rapid command bursts.

### Pitfall 2: Rate limiter accumulates negative tokens
**What goes wrong:** Leaky bucket with `f64` tokens subtracts on each emit but never floors at 0. With a burst, `tokens` goes deeply negative and it takes many seconds before events flow again.
**Why it happens:** No floor check when subtracting.
**How to avoid:** After subtracting: `self.rate_tokens = self.rate_tokens.max(0.0);`
**Warning signs:** After a burst of 10+ commands, subsequent commands are silently dropped for much longer than the configured window.

### Pitfall 3: VecDeque budget window never shrinks
**What goes wrong:** Window entries are pushed but oldest are not evicted when budget is exceeded. `window_token_cost` grows unbounded.
**Why it happens:** Budget check code is only reached on new events, not on every push.
**How to avoid:** After pushing new event, loop: `while window_token_cost > budget { pop_front, subtract cost }`.
**Warning signs:** AGTA-02 success criterion fails — 20 consecutive `cargo check` runs show the channel receiving all 20 entries.

### Pitfall 4: Collapse counter lost on severity change
**What goes wrong:** `pending_collapsed` counter is reset when a different-severity event arrives, losing the collapse count of the previous run.
**Why it happens:** The collapsed event is only emitted when the fingerprint changes, but the counter isn't flushed to the last emitted event retroactively.
**How to avoid:** When fingerprint changes and `pending_collapsed > 0`, update the most-recently-emitted event's `collapsed_count` field. Simpler: emit a flush event with the collapse summary before processing the new event.
**Warning signs:** AGTA-03 success criterion "20 consecutive cargo check results in collapsed events" passes but collapse count field is wrong.

### Pitfall 5: Channel receiver dropped immediately
**What goes wrong:** `Receiver<ActivityEvent>` is created but immediately dropped (not stored). All subsequent `try_send` calls return `Err(Disconnected)`. Events are silently discarded.
**Why it happens:** The receiver is created in the `watcher_spawned` block but not stored anywhere (Phase 56 consumer doesn't exist yet).
**How to avoid:** Store `Option<std::sync::mpsc::Receiver<ActivityEvent>>` on `Processor`. Phase 56 takes it with `.take()`.
**Warning signs:** All `try_send` calls log "channel full or closed" immediately.

## Code Examples

Verified patterns from project codebase:

### Bounded std::sync::mpsc channel (from glass_terminal/pty.rs pattern)
```rust
// Source: crates/glass_terminal/src/pty.rs — established project pattern
use std::sync::mpsc::{self, Receiver, Sender};

let (tx, rx) = mpsc::sync_channel(256);
// tx.try_send(msg) — non-blocking, returns Err on full/disconnected
// rx.try_recv() — non-blocking drain in consumer
```

### Leaky bucket rate limiter (stdlib-only)
```rust
// Source: pattern from crates/glass_core/src/activity_stream.rs (new)
use std::time::Instant;

struct RateLimiter {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64,   // tokens per second
    last_refill: Instant,
}

impl RateLimiter {
    fn try_consume(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
        self.last_refill = now;

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}
```

### Rolling token budget window (VecDeque)
```rust
// Source: pattern from crates/glass_core/src/activity_stream.rs (new)
use std::collections::VecDeque;

struct BudgetWindow {
    events: VecDeque<ActivityEvent>,
    total_tokens: usize,
    budget: usize,
}

impl BudgetWindow {
    fn push(&mut self, event: ActivityEvent) {
        self.total_tokens += event.token_cost;
        self.events.push_back(event);

        // Evict oldest until under budget
        while self.total_tokens > self.budget {
            if let Some(oldest) = self.events.pop_front() {
                self.total_tokens = self.total_tokens.saturating_sub(oldest.token_cost);
            } else {
                break;
            }
        }
    }
}
```

### Token cost heuristic (no external tokenizer)
```rust
// Source: pattern consistent with glass_soi/src/types.rs OutputSummary.token_estimate
fn estimate_tokens(summary: &str) -> usize {
    // ~4 chars/token heuristic + fixed overhead for JSON envelope
    (summary.len() / 4) + 8
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `std::sync::mpsc::channel()` (unbounded) | `std::sync::mpsc::sync_channel(N)` (bounded) | Rust 1.0+ (both exist) | sync_channel enforces capacity; mandatory for AGTA-02 |
| N/A | Leaky bucket rate limiting | N/A (first rate limiter in Glass) | Must be O(1) per event; leaky bucket is standard |

**Deprecated/outdated:**
- `std::sync::mpsc::channel()` without a bound: violates AGTA-02 "does not grow unbounded" requirement.

## Open Questions

1. **Where does `ActivityStreamConfig` come from?**
   - What we know: Phase 60 (AGTC-01) adds the full `[agent]` config section. Phase 55 ships before Phase 60.
   - What's unclear: Should Phase 55 hardcode defaults or add a preliminary `[agent]` config section?
   - Recommendation: Use `ActivityStreamConfig::default()` hardcoded in Phase 55. Phase 60 will wire config hot-reload. Match the precedent of `CoordinationPoller` which also ignores config for now.

2. **Should the activity stream be per-session or global?**
   - What we know: Glass supports multiple sessions (tabs/panes). The agent runtime (Phase 56) is "a struct in Processor" — global, not per-session.
   - What's unclear: Does the agent care which session produced an event?
   - Recommendation: Single global channel on `Processor`. Include `session_id: SessionId` in `ActivityEvent` so Phase 56 can filter if needed.

3. **What is the correct collapsed-event emit strategy?**
   - What we know: AGTA-03 says "collapsed/deduplicated events rather than 20 identical entries." The success criterion is about what the agent *receives*, not what's stored.
   - What's unclear: Should collapse emit one event with `collapsed_count=20`, or emit nothing until a different event breaks the run?
   - Recommendation: Emit a single event with `collapsed_count=N` when the run breaks (fingerprint changes or app exits). For a burst of 20 identical events, only 1 event reaches the channel. This matches the success criterion.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test (`cargo test`) |
| Config file | None (inline `#[cfg(test)]`) |
| Quick run command | `cargo test -p glass_core -- activity_stream` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| AGTA-01 | SoiReady events reach channel sender | unit | `cargo test -p glass_core -- activity_stream::tests::test_channel_receives_event` | Wave 0 |
| AGTA-02 | 4096-token budget drops oldest on overflow | unit | `cargo test -p glass_core -- activity_stream::tests::test_budget_window_evicts_oldest` | Wave 0 |
| AGTA-03 | 20 identical success events produce <=1 collapsed event | unit | `cargo test -p glass_core -- activity_stream::tests::test_noise_filter_collapses_identical` | Wave 0 |
| AGTA-04 | Burst of 10 in <1s rate-limited to <=5 emitted | unit | `cargo test -p glass_core -- activity_stream::tests::test_rate_limiter_burst` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p glass_core -- activity_stream`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd:verify-work`

### Wave 0 Gaps
- [ ] `crates/glass_core/src/activity_stream.rs` — new file, covers all four AGTA-* requirements via unit tests

*(If the module is added in Wave 1, the test infrastructure is part of the same file — no separate fixture setup needed.)*

## Sources

### Primary (HIGH confidence)
- `crates/glass_terminal/src/pty.rs` — `std::sync::mpsc::sync_channel` usage in project (confirmed in codebase)
- `crates/glass_snapshot/src/watcher.rs` — `std::sync::mpsc` pattern confirmed
- `crates/glass_core/src/coordination_poller.rs` — background struct pattern for Processor-held state
- `crates/glass_core/src/event.rs` — `AppEvent::SoiReady` fields confirmed present
- `crates/glass_mux/src/session.rs` — `SoiSummary`, `last_soi_summary` confirmed present
- `src/main.rs` — `Processor` struct fields, `SoiReady` handler location confirmed (lines 3101-3155)
- `Cargo.toml` — workspace dependencies, no crossbeam or external channel crates

### Secondary (MEDIUM confidence)
- Rust reference on `std::sync::mpsc::sync_channel` — bounded capacity semantics, `try_send` non-blocking behavior (standard library, stable since Rust 1.0)
- Leaky bucket rate limiting — well-established O(1) algorithm, no reference needed

### Tertiary (LOW confidence)
- Token cost heuristic (4 chars/token) — rough approximation consistent with `OutputSummary.token_estimate` in `glass_soi/src/types.rs`; not validated against a real tokenizer

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — all stdlib, confirmed patterns in project
- Architecture: HIGH — follows established project conventions exactly
- Pitfalls: HIGH — all derived from analysis of the actual code paths
- Test design: HIGH — all four AGTA requirements map cleanly to unit-testable pure functions

**Research date:** 2026-03-13
**Valid until:** 2026-06-13 (stable Rust stdlib, no external deps)
