use std::time::Duration;

use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use glass_terminal::{DefaultColors, OscScanner, resolve_color};

fn bench_resolve_color(c: &mut Criterion) {
    let colors = Colors::default();
    let defaults = DefaultColors::default();

    let mut group = c.benchmark_group("resolve_color");

    group.bench_function("spec_truecolor", |b| {
        b.iter(|| {
            resolve_color(
                black_box(Color::Spec(Rgb { r: 255, g: 128, b: 0 })),
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

criterion_group!(benches, bench_resolve_color, bench_osc_scanner, bench_cold_start);
criterion_main!(benches);
