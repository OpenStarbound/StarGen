//! What the engine pays for the directives that sbgen generates: parsing them when they arrive, and applying them to a
//! drawable's image. The frames are the ones that the directives reproduce exactly.
use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use engine_bench::{
    engine::Directives,
    harness::{self, Assets, Frame},
};
use std::hint::black_box;

struct Prepared {
    name: &'static str,
    text: String,
    frames: Vec<Frame>,
}

fn prepare() -> Vec<Prepared> {
    let assets = Assets::open_default().expect("the game's assets");
    harness::humanoid_cases()
        .into_iter()
        .map(|case| {
            let text = case.directives().unwrap();
            let mut directives = Directives::new(&text).unwrap();
            directives.load().unwrap();
            let frames =
                harness::exact_frames(&mut directives, case.frames(&assets).unwrap()).unwrap();
            // empty frames are dropped as soon as the colors are looked up, they only show the cost of the prefix
            let frames = frames
                .into_iter()
                .filter(|frame| frame.expected.rgba.chunks_exact(4).any(|p| p[3] != 0))
                .collect();
            Prepared {
                name: case.name,
                text,
                frames,
            }
        })
        .collect()
}

fn parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");
    for case in prepare() {
        group.throughput(Throughput::Bytes(case.text.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(case.name),
            &case.text,
            |b, text| {
                b.iter_batched(
                    || (),
                    |()| {
                        let directives = Directives::new(black_box(text)).unwrap();
                        directives.load().unwrap();
                        directives
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }
    group.finish();
}

fn apply(c: &mut Criterion) {
    for (group_name, sequential) in [("apply_frame", false), ("apply_frame_in_order", true)] {
        let mut group = c.benchmark_group(group_name);
        for case in prepare() {
            let mut directives = Directives::new(&case.text).unwrap();
            directives.load().unwrap();
            // a walking frame, one of the larger ones: it is what a character spends most of its time in
            let frame = case
                .frames
                .iter()
                .find(|frame| frame.name == "malebody:run.1" || frame.name == "femalebody:run.1")
                .unwrap_or(&case.frames[0]);
            group.throughput(Throughput::Elements(1));
            group.bench_with_input(BenchmarkId::from_parameter(case.name), frame, |b, frame| {
                b.iter(|| {
                    let (_, _, pixels) = if sequential {
                        directives.apply_sequential_in_place(black_box(&frame.base))
                    } else {
                        directives.apply_in_place(black_box(&frame.base))
                    }
                    .unwrap();
                    black_box(pixels[0])
                })
            });
        }
        group.finish();
    }
}

fn sheet(c: &mut Criterion) {
    let mut group = c.benchmark_group("apply_sheet");
    for case in prepare() {
        let mut directives = Directives::new(&case.text).unwrap();
        directives.load().unwrap();
        group.throughput(Throughput::Elements(case.frames.len() as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(case.name),
            &case.frames,
            |b, frames| {
                b.iter(|| {
                    for frame in frames {
                        let (_, _, pixels) =
                            directives.apply_in_place(black_box(&frame.base)).unwrap();
                        black_box(pixels[0]);
                    }
                })
            },
        );
    }
    group.finish();
}

criterion_group!(benches, parse, apply, sheet);
criterion_main!(benches);
