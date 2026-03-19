use std::time::Duration;

use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use glass_terminal::{resolve_color, DefaultColors, OscScanner};

fn bench_resolve_color(c: &mut Criterion) {
    let colors = Colors::default();
    let defaults = DefaultColors::default();

    let mut group = c.benchmark_group("resolve_color");

    group.bench_function("spec_truecolor", |b| {
        b.iter(|| {
            resolve_color(
                black_box(Color::Spec(Rgb {
                    r: 255,
                    g: 128,
                    b: 0,
                })),
                black_box(&colors),
                black_box(&defaults),
                black_box(Flags::empty()),
            )
        })
    });

    group.bench_function("named_with_bold", |b| {
        b.iter(|| {
            resolve_color(
                black_box(Color::Named(NamedColor::Blue)),
                black_box(&colors),
                black_box(&defaults),
                black_box(Flags::BOLD),
            )
        })
    });

    group.bench_function("indexed_196", |b| {
        b.iter(|| {
            resolve_color(
                black_box(Color::Indexed(196)),
                black_box(&colors),
                black_box(&defaults),
                black_box(Flags::empty()),
            )
        })
    });

    group.finish();
}

fn bench_osc_scanner(c: &mut Criterion) {
    let data = b"Hello world \x1b]133;A\x07 more text \x1b]133;C\x07 final chunk";

    c.bench_function("osc_scan", |b| {
        b.iter(|| {
            let mut scanner = OscScanner::new();
            scanner.scan(black_box(data))
        })
    });
}

fn bench_cold_start(c: &mut Criterion) {
    let mut group = c.benchmark_group("cold_start");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(30));

    group.bench_function("process_startup", |b| {
        b.iter(|| {
            std::process::Command::new(env!("CARGO_BIN_EXE_glass"))
                .arg("--help")
                .output()
                .expect("failed to run glass binary")
        })
    });

    group.finish();
}

/// Benchmark RenderedCell construction with the Option<Vec<char>> zerowidth field.
///
/// This measures the hot path of building a GridSnapshot's cell vec, which is
/// the core of `snapshot_term`. We use synthetic data because constructing a
/// real `Term<EventProxy>` requires a PTY and event loop.
fn bench_rendered_cell_construction(c: &mut Criterion) {
    use alacritty_terminal::index::{Column, Line, Point};
    use glass_terminal::RenderedCell;

    let mut group = c.benchmark_group("snapshot_term");

    // Simulate a 120x40 grid (4800 cells) — typical terminal size
    let cols = 120usize;
    let rows = 40usize;

    group.bench_function("build_4800_cells", |b| {
        b.iter(|| {
            let mut cells = Vec::with_capacity(cols * rows);
            for row in 0..rows {
                for col in 0..cols {
                    cells.push(RenderedCell {
                        point: Point {
                            line: Line(row as i32),
                            column: Column(col),
                        },
                        c: if col % 2 == 0 { 'A' } else { ' ' },
                        fg: black_box(Rgb {
                            r: 204,
                            g: 204,
                            b: 204,
                        }),
                        bg: black_box(Rgb {
                            r: 26,
                            g: 26,
                            b: 26,
                        }),
                        flags: Flags::empty(),
                        zerowidth: None, // 99%+ of cells have no zero-width chars
                    });
                }
            }
            black_box(cells)
        })
    });

    // Benchmark with 1% of cells having zero-width combining chars
    group.bench_function("build_4800_cells_1pct_zerowidth", |b| {
        b.iter(|| {
            let mut cells = Vec::with_capacity(cols * rows);
            for row in 0..rows {
                for col in 0..cols {
                    let idx = row * cols + col;
                    cells.push(RenderedCell {
                        point: Point {
                            line: Line(row as i32),
                            column: Column(col),
                        },
                        c: 'e',
                        fg: black_box(Rgb {
                            r: 204,
                            g: 204,
                            b: 204,
                        }),
                        bg: black_box(Rgb {
                            r: 26,
                            g: 26,
                            b: 26,
                        }),
                        flags: Flags::empty(),
                        zerowidth: if idx % 100 == 0 {
                            Some(vec!['\u{0301}']) // combining acute accent
                        } else {
                            None
                        },
                    });
                }
            }
            black_box(cells)
        })
    });

    group.finish();
}

fn bench_input_processing(c: &mut Criterion) {
    // Generate a realistic 50 KB output buffer (simulates large cargo build output)
    let line = "error[E0308]: mismatched types --> src/main.rs:42:5\n";
    let payload: String = line.repeat(50 * 1024 / line.len());
    assert!(payload.len() >= 50_000, "Payload should be ~50KB");

    c.bench_function("process_output_50kb", |b| {
        b.iter(|| {
            glass_history::output::process_output(
                black_box(Some(payload.as_bytes().to_vec())),
                black_box(50u32),
            )
        });
    });
}

criterion_group!(
    benches,
    bench_resolve_color,
    bench_osc_scanner,
    bench_cold_start,
    bench_input_processing,
    bench_rendered_cell_construction
);
criterion_main!(benches);
