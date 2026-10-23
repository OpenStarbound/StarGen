use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use stargen::generator::{self, Inputs, data};
use std::hint::black_box;
use std::path::Path;

const TEMPLATES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/generator/templates");

fn inputs(name: &str, key: &str, columns: u32) -> Inputs {
    let mut inputs = Inputs::new();
    let input = generator::load_input(&Path::new(TEMPLATES).join(name)).unwrap();
    inputs.insert(key.to_string(), (input, columns));
    inputs
}

fn bench_generate(c: &mut Criterion) {
    let mut group = c.benchmark_group("do_generate");
    // (label, template, sheet key, columns, frame set)
    let cases = [
        ("male_body", "malebody.png", "malebody", &data::MALE),
        ("female_body", "femalebody.png", "femalebody", &data::FEMALE),
        (
            "male_body_greyscale",
            "malebody_greyscale.png",
            "malebody",
            &data::MALE,
        ),
    ];
    for (label, file, key, frame_set) in cases {
        let attachments = inputs(file, key, 9);
        let pixels: u64 = attachments
            .values()
            .map(|(i, _)| (i.width * i.height) as u64)
            .sum();
        group.throughput(Throughput::Elements(pixels));
        group.bench_with_input(
            BenchmarkId::from_parameter(label),
            &attachments,
            |b, attachments| {
                b.iter(|| {
                    generator::do_generate(black_box(attachments), black_box(frame_set), "humanoid")
                        .unwrap()
                })
            },
        );
    }
    group.finish();
}

fn bench_replacer(c: &mut Criterion) {
    // a realistic replacement list, taken from the pixels of a template
    let img = image::open(Path::new(TEMPLATES).join("malebody.png"))
        .unwrap()
        .to_rgba8();
    let replacements: Vec<([u8; 4], [u8; 4])> = img
        .enumerate_pixels()
        .filter(|(_, _, p)| p[3] > 0)
        .map(|(x, y, p)| ([(y >> 8) as u8, y as u8, x as u8, 0], p.0))
        .collect();
    let mut group = c.benchmark_group("create_replacer_string");
    group.throughput(Throughput::Elements(replacements.len() as u64));
    group.bench_function("malebody_pixels", |b| {
        b.iter_batched(
            || replacements.clone(),
            |r| generator::create_replacer_string(r, true, |_| true),
            BatchSize::LargeInput,
        )
    });
    group.finish();
}

fn bench_load(c: &mut Criterion) {
    let path = Path::new(TEMPLATES).join("malebody.png");
    c.bench_function("load_input", |b| {
        b.iter(|| generator::load_input(black_box(&path)).unwrap())
    });
}

criterion_group!(benches, bench_generate, bench_replacer, bench_load);
criterion_main!(benches);
