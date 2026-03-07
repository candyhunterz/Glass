# Phase 26: Performance Profiling & Optimization - Research

**Researched:** 2026-03-07
**Domain:** Rust benchmarking, tracing instrumentation, performance optimization
**Confidence:** HIGH

## Summary

This phase adds automated performance benchmarks (criterion), tracing instrumentation behind a cargo feature flag, and an optimization pass to meet documented targets. The project already has ad-hoc performance measurement in main.rs (cold start timing via `Instant`, memory via `memory-stats` crate, key latency timing). The v1.0 baseline shows 360ms cold start, 3-7us key latency, 86MB idle memory -- all well within the <500ms, <5ms, <120MB targets.

The key challenge is that criterion benchmarks require extractable, unit-testable code paths. The main hot paths (PTY read loop, render frame, event dispatch) are deeply embedded in the runtime. Benchmarks must target the decomposable sub-operations: `snapshot_term()`, `GridRenderer::build_rects()`, `GridRenderer::build_text_buffers()`, `OscScanner::scan()`, and `resolve_color()`. Cold start and input latency are integration-level metrics best measured via the existing `Instant`-based approach, with results recorded to a baseline file.

**Primary recommendation:** Use criterion 0.5 for micro-benchmarks on hot-path sub-functions, tracing-chrome behind a `perf` cargo feature for trace file generation, and commit a `PERFORMANCE.md` baseline document with measured values.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| PERF-01 | Automated criterion benchmarks for cold start, input latency, and idle memory | criterion 0.5 benchmark harness; cold start/memory measured via process-spawn benchmark; input latency via snapshot_term + build_rects microbenchmarks |
| PERF-02 | Tracing instrumentation on hot paths (PTY read, render loop, event dispatch) | tracing-chrome 0.7 behind `perf` cargo feature; `#[tracing::instrument]` on glass_pty_loop, pty_read_with_scan, draw_frame, draw_multi_pane_frame, snapshot_term |
| PERF-03 | Performance optimization pass based on profiling results (startup time, memory, rendering throughput) | Profile-guided optimization targeting snapshot_term Vec pre-allocation, glyph cache hits, rect instance reuse; baseline doc with targets |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| criterion | 0.5 | Statistical micro-benchmarking | De facto Rust benchmark harness; stable, statistical analysis, HTML reports |
| tracing | 0.1.44 | Instrumentation spans | Already in workspace dependencies |
| tracing-subscriber | 0.3 | Subscriber infrastructure | Already in workspace dependencies |
| tracing-chrome | 0.7 | Chrome trace file output | Generates JSON trace files viewable in ui.perfetto.dev; integrates with existing tracing stack |
| memory-stats | 1.2 | Process memory measurement | Already in workspace dependencies |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| std::time::Instant | stdlib | Wall-clock timing | Already used for cold start and key latency; continue for integration-level metrics |
| std::hint::black_box | stdlib (1.66+) | Prevent dead code elimination | Use in benchmarks; criterion also re-exports this |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| criterion 0.5 | criterion 0.8 | 0.8 requires Rust 1.88+; 0.5 is proven stable, widely documented |
| tracing-chrome | tracing-flame | tracing-flame generates folded stacks for flamegraph SVG; tracing-chrome gives interactive timeline in Perfetto which is better for understanding render pipeline ordering |
| tracing-chrome | cargo-flamegraph | cargo-flamegraph uses perf/dtrace sampling (OS-level); tracing-chrome gives application-level span-based traces with named operations |

**Installation:**
```bash
# Dev dependencies (benchmarks only)
cargo add --dev criterion --features html_reports

# Optional perf feature dependencies
# Added to workspace Cargo.toml under [features] and individual crate Cargo.toml
cargo add tracing-chrome  # only compiled when feature = "perf"
```

## Architecture Patterns

### Recommended Project Structure
```
benches/
    perf_benchmarks.rs     # criterion benchmarks (single file, all groups)
src/
    main.rs                # Feature-gated tracing-chrome subscriber init
crates/
    glass_terminal/
        src/
            pty.rs         # #[cfg_attr(feature="perf", tracing::instrument)] on hot paths
            grid_snapshot.rs  # instrument snapshot_term
            osc_scanner.rs    # instrument scan()
    glass_renderer/
        src/
            frame.rs       # instrument draw_frame, draw_multi_pane_frame
            grid_renderer.rs  # instrument build_rects, build_text_buffers
PERFORMANCE.md             # Committed baseline numbers
```

### Pattern 1: Feature-Gated Tracing Instrumentation
**What:** Add `#[tracing::instrument]` to hot-path functions only when the `perf` cargo feature is enabled, using `cfg_attr` to avoid overhead in normal builds.
**When to use:** All hot-path functions that should appear in trace files.
**Example:**
```rust
// In crates/glass_terminal/src/pty.rs
#[cfg_attr(feature = "perf", tracing::instrument(skip_all))]
fn pty_read_with_scan(
    pty: &mut tty::Pty,
    terminal: &Arc<FairMutex<Term<EventProxy>>>,
    // ... all params
) -> io::Result<()> {
    // existing implementation unchanged
}

// In crates/glass_renderer/src/frame.rs
#[cfg_attr(feature = "perf", tracing::instrument(skip_all))]
pub fn draw_frame(&mut self, /* params */) {
    // existing implementation unchanged
}
```

### Pattern 2: Criterion Benchmark with Synthetic Data
**What:** Benchmark hot sub-functions using synthetic GridSnapshot data to avoid needing a real PTY/GPU.
**When to use:** Micro-benchmarks that isolate CPU-bound operations.
**Example:**
```rust
// benches/perf_benchmarks.rs
use criterion::{criterion_group, criterion_main, Criterion, black_box};
use glass_terminal::{GridSnapshot, RenderedCell, DefaultColors, resolve_color};

fn make_snapshot(cols: usize, lines: usize) -> GridSnapshot {
    // Construct synthetic snapshot with realistic cell data
    let cells: Vec<RenderedCell> = (0..cols * lines)
        .map(|i| RenderedCell {
            point: alacritty_terminal::index::Point {
                line: alacritty_terminal::index::Line((i / cols) as i32),
                column: alacritty_terminal::index::Column(i % cols),
            },
            c: 'A',
            fg: alacritty_terminal::vte::ansi::Rgb { r: 204, g: 204, b: 204 },
            bg: alacritty_terminal::vte::ansi::Rgb { r: 26, g: 26, b: 26 },
            flags: alacritty_terminal::term::cell::Flags::empty(),
            zerowidth: vec![],
        })
        .collect();

    GridSnapshot {
        cells,
        cursor: /* default cursor */,
        display_offset: 0,
        history_size: 0,
        mode: alacritty_terminal::term::TermMode::empty(),
        columns: cols,
        screen_lines: lines,
    }
}

fn bench_resolve_color(c: &mut Criterion) {
    let colors = alacritty_terminal::term::color::Colors::default();
    let defaults = DefaultColors::default();
    c.bench_function("resolve_color_spec", |b| {
        b.iter(|| {
            resolve_color(
                black_box(alacritty_terminal::vte::ansi::Color::Spec(
                    alacritty_terminal::vte::ansi::Rgb { r: 255, g: 0, b: 0 }
                )),
                &colors,
                &defaults,
                alacritty_terminal::term::cell::Flags::empty(),
            )
        })
    });
}

fn bench_osc_scanner(c: &mut Criterion) {
    use glass_terminal::OscScanner;
    let mut scanner = OscScanner::new();
    let data = b"Hello world \x1b]133;A\x07 more text \x1b]133;C\x07";
    c.bench_function("osc_scan_mixed", |b| {
        b.iter(|| {
            scanner.scan(black_box(data))
        })
    });
}

criterion_group!(benches, bench_resolve_color, bench_osc_scanner);
criterion_main!(benches);
```

### Pattern 3: Feature-Gated Subscriber Init
**What:** When `perf` feature is enabled, replace the default tracing subscriber with one that includes a tracing-chrome layer.
**When to use:** In main.rs, gated behind `#[cfg(feature = "perf")]`.
**Example:**
```rust
// In main.rs, terminal launch branch
#[cfg(feature = "perf")]
{
    use tracing_chrome::ChromeLayerBuilder;
    use tracing_subscriber::prelude::*;

    let (chrome_layer, _guard) = ChromeLayerBuilder::new()
        .file("glass-trace.json".to_string())
        .build();
    tracing_subscriber::registry()
        .with(chrome_layer)
        .with(tracing_subscriber::fmt::layer()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env()))
        .init();
    // _guard must live until program exit - store in Processor or use static
}

#[cfg(not(feature = "perf"))]
{
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
}
```

### Pattern 4: Cold Start Benchmark via Process Spawn
**What:** Benchmark cold start by spawning the Glass binary as a subprocess and timing until first output or exit.
**When to use:** For PERF-01 cold start metric -- not a micro-benchmark but a real integration measurement.
**Example:**
```rust
fn bench_cold_start(c: &mut Criterion) {
    let mut group = c.benchmark_group("startup");
    group.sample_size(10); // Lower sample size for slow operations
    group.measurement_time(std::time::Duration::from_secs(30));

    group.bench_function("cold_start_help", |b| {
        b.iter(|| {
            // Measure time to run `glass --help` (avoids PTY/GPU)
            std::process::Command::new("cargo")
                .args(["run", "--release", "--", "--help"])
                .output()
                .expect("failed to run glass")
        })
    });
    group.finish();
}
```

### Anti-Patterns to Avoid
- **Instrumenting inside tight loops:** Do NOT put `#[instrument]` on `resolve_color()` or per-cell iteration functions. The tracing overhead would dominate. Instrument the outer function (`snapshot_term`, `build_rects`) instead.
- **Benchmarking with GPU:** Criterion benchmarks must not require a GPU context. Benchmark the CPU-side data preparation (rect building, text buffer building, snapshot extraction) separately from GPU submission.
- **Storing trace guard in local scope:** The `FlushGuard` from tracing-chrome must outlive the entire program. Store it in the Processor struct or use a `static` to prevent premature drop.
- **Using `skip_all` without `level`:** On high-frequency functions, set `level = "trace"` to allow filtering: `#[cfg_attr(feature = "perf", tracing::instrument(skip_all, level = "trace"))]`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Statistical benchmarking | Custom timing loops with `Instant` | criterion | Handles warm-up, outlier detection, regression comparison, HTML reports |
| Trace file format | Custom JSON trace writer | tracing-chrome | Chrome Trace Event Format is complex; tracing-chrome handles thread IDs, timestamps, nesting |
| Memory measurement | Manual `/proc/self/status` parsing | memory-stats 1.2 | Already cross-platform in workspace; handles Windows/macOS/Linux |
| Flamegraph visualization | Custom SVG generation | Perfetto UI (ui.perfetto.dev) | Free web viewer for tracing-chrome output; supports filtering, zoom, search |

**Key insight:** The benchmarking infrastructure (criterion) and tracing infrastructure (tracing-chrome) are mature, well-maintained crates that integrate directly with the existing tracing stack. Custom solutions would be worse in every dimension.

## Common Pitfalls

### Pitfall 1: Benchmarking GPU-Dependent Code
**What goes wrong:** Trying to benchmark `draw_frame()` end-to-end in criterion requires a wgpu device, surface, and window -- none of which are available in a headless benchmark environment.
**Why it happens:** The render pipeline is tightly coupled to wgpu state.
**How to avoid:** Benchmark only the CPU-side data preparation: `build_rects()`, `build_text_buffers()`, `snapshot_term()`. These are the actual bottlenecks (cell iteration, color resolution, buffer allocation). GPU submission is I/O-bound and not meaningfully benchmarkable via criterion.
**Warning signs:** Benchmark fails with "Failed to create adapter" or similar wgpu errors.

### Pitfall 2: Tracing Overhead in Production
**What goes wrong:** Leaving `#[tracing::instrument]` on hot-path functions in release builds adds measurable overhead (span creation, memory allocation per span entry/exit).
**Why it happens:** `instrument` is not zero-cost when tracing is active.
**How to avoid:** Use `#[cfg_attr(feature = "perf", tracing::instrument(skip_all))]` so instrumentation is compiled out entirely when the `perf` feature is not enabled. The existing `tracing::info!` calls for PERF logging are fine -- they use static dispatch and are near-zero-cost when the log level filters them.
**Warning signs:** Release build is slower than expected; `cargo bench` numbers regress after adding instrumentation.

### Pitfall 3: criterion Harness Conflicts
**What goes wrong:** Cargo complains about multiple `main` functions or benchmark binaries not running.
**Why it happens:** criterion requires `harness = false` in the `[[bench]]` section.
**How to avoid:** Ensure Cargo.toml has:
```toml
[[bench]]
name = "perf_benchmarks"
harness = false
```

### Pitfall 4: OscScanner Visibility
**What goes wrong:** `OscScanner` and other internal types may not be public enough for benchmark crate access.
**Why it happens:** Benchmarks in `benches/` are external to the crate.
**How to avoid:** Either make benchmarkable functions `pub` (preferred for `snapshot_term`, `resolve_color` which are already pub) or add `pub use` re-exports in the crate's `lib.rs`. For truly internal functions, consider in-crate benchmarks or `#[cfg(test)]` bench helpers.

### Pitfall 5: Misleading Cold Start Numbers
**What goes wrong:** Measuring cold start via `cargo run` includes cargo compilation checking overhead. Measuring via `Instant::now()` at the start of `main()` misses process creation and dynamic linking.
**Why it happens:** "Cold start" can mean different things.
**How to avoid:** Define cold start clearly as "wall-clock time from process exec to first frame presented" -- which is what the existing `self.cold_start.elapsed()` at first frame measures. For benchmarks, use the pre-built release binary directly, not `cargo run`.

### Pitfall 6: Memory Baseline Variability
**What goes wrong:** Idle memory numbers vary significantly between runs due to GPU driver allocations, font cache warming, and OS page caching.
**Why it happens:** wgpu/DX12 allocates GPU buffers non-deterministically; font system loads vary by installed fonts.
**How to avoid:** Take multiple measurements and report the median. Document the measurement conditions (OS, GPU driver version, terminal size). The 120MB target already accounts for ~80MB GPU baseline.

## Code Examples

### Cargo.toml Feature Configuration
```toml
# Root Cargo.toml additions
[features]
perf = ["glass_terminal/perf", "glass_renderer/perf", "tracing-chrome"]

[dependencies]
tracing-chrome = { version = "0.7", optional = true }

[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }

[[bench]]
name = "perf_benchmarks"
harness = false
```

```toml
# crates/glass_terminal/Cargo.toml additions
[features]
perf = []
```

```toml
# crates/glass_renderer/Cargo.toml additions
[features]
perf = []
```

### PERFORMANCE.md Baseline Format
```markdown
# Glass Performance Baseline

**Measured:** 2026-03-XX
**Platform:** Windows 11, [GPU], Rust [version]
**Terminal size:** 80x24 (default)
**Build:** `cargo build --release`

## Targets

| Metric | Target | Measured | Status |
|--------|--------|----------|--------|
| Cold start (first frame) | <500ms | XXXms | PASS/FAIL |
| Input latency (key -> render) | <5ms | X.Xus | PASS/FAIL |
| Idle memory (physical) | <120MB | XXmb | PASS/FAIL |

## Criterion Benchmarks

Run: `cargo bench`
Reports: `target/criterion/report/index.html`

| Benchmark | Mean | Std Dev |
|-----------|------|---------|
| resolve_color_spec | Xns | Xns |
| osc_scan_mixed | Xns | Xns |
| snapshot_term_80x24 | Xus | Xus |
| build_rects_80x24 | Xus | Xus |

## Profiling

Run: `cargo run --release --features perf`
Output: `glass-trace.json`
View: Open in https://ui.perfetto.dev
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Ad-hoc `Instant` timing in main.rs | criterion statistical benchmarks + baseline doc | This phase | Reproducible, regression-detectable measurements |
| No tracing instrumentation | Feature-gated `tracing::instrument` + tracing-chrome | This phase | Interactive trace visualization of hot paths |
| Verbal performance claims | Committed PERFORMANCE.md with measured values | This phase | Documented, verifiable performance characteristics |

**Deprecated/outdated:**
- criterion 0.3.x: Old API, use 0.5+ for current macro syntax
- `test::Bencher` (nightly-only): Unstable, criterion is the stable alternative

## Open Questions

1. **OscScanner public API for benchmarking**
   - What we know: `OscScanner` is `pub(crate)` or `pub` -- need to verify visibility
   - What's unclear: Whether `scan()` can be called standalone without full PTY context
   - Recommendation: Check visibility; if not pub, add a `pub use` or make the benchmark an in-crate test

2. **GridRenderer benchmarkability without wgpu**
   - What we know: `build_rects()` and `build_text_buffers()` take a `GridSnapshot` and return data structures; they should not need GPU
   - What's unclear: Whether `build_text_buffers` requires a `FontSystem` that needs initialization
   - Recommendation: `FontSystem::new()` is CPU-only (font discovery); should work in benchmark context but verify

3. **Optimization targets after profiling**
   - What we know: v1.0 baseline already meets all targets comfortably (360ms < 500ms, 86MB < 120MB)
   - What's unclear: Whether v2.0 (multi-tab, split panes) has regressed from v1.0 numbers
   - Recommendation: Measure first, then identify if any optimization is actually needed

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | criterion 0.5 (benchmarks) + cargo test (existing 436 tests) |
| Config file | benches/perf_benchmarks.rs (new -- Wave 0) |
| Quick run command | `cargo bench -- --quick` |
| Full suite command | `cargo bench` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| PERF-01 | criterion benchmarks produce statistical output for cold start, latency, memory | integration | `cargo bench 2>&1 \| grep -q "cold_start\|resolve_color\|osc_scan"` | -- Wave 0 |
| PERF-02 | `perf` feature produces trace file | smoke | `cargo run --release --features perf -- --help && ls glass-trace.json` | -- Wave 0 |
| PERF-03 | Performance meets targets after optimization | manual | Manual: run glass, check PERF logs, compare to PERFORMANCE.md | manual-only: requires GPU + PTY |

### Sampling Rate
- **Per task commit:** `cargo bench -- --quick` (fast mode, fewer iterations)
- **Per wave merge:** `cargo bench` (full statistical run)
- **Phase gate:** All benchmarks run without error; PERFORMANCE.md committed with measured values

### Wave 0 Gaps
- [ ] `benches/perf_benchmarks.rs` -- benchmark file with criterion harness (PERF-01)
- [ ] Root `Cargo.toml` -- `[features] perf = [...]`, `[[bench]]` section, criterion dev-dep
- [ ] `crates/glass_terminal/Cargo.toml` -- `[features] perf = []`
- [ ] `crates/glass_renderer/Cargo.toml` -- `[features] perf = []`

## Sources

### Primary (HIGH confidence)
- Project source code: `src/main.rs` lines 743-749 (existing PERF measurement), `crates/glass_terminal/src/pty.rs` (PTY hot path), `crates/glass_renderer/src/frame.rs` (render pipeline)
- [criterion docs](https://docs.rs/criterion/latest/criterion/) - benchmark API and harness setup
- [tracing-chrome docs](https://docs.rs/tracing-chrome/latest/tracing_chrome/) - ChromeLayerBuilder API

### Secondary (MEDIUM confidence)
- [criterion.rs GitHub](https://github.com/bheisler/criterion.rs) - version 0.5 confirmed as stable release
- [tracing-chrome crates.io](https://crates.io/crates/tracing-chrome) - version 0.7.1 latest

### Tertiary (LOW confidence)
- criterion 0.8 availability -- search results mention it but version compatibility with current Rust toolchain unverified

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - all libraries are well-established, criterion and tracing-chrome are de facto standards
- Architecture: HIGH - feature-gated instrumentation is a well-documented Rust pattern; existing tracing stack makes integration straightforward
- Pitfalls: HIGH - based on direct code inspection of the project's hot paths and architecture

**Research date:** 2026-03-07
**Valid until:** 2026-04-07 (stable domain, 30 days)
