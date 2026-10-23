use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use image::{Rgba, RgbaImage, imageops};
use sbgen::closest::{self, Options};
use sbgen::pak::Assets;
use std::hint::black_box;
use std::path::PathBuf;

/// Blocky pixel art with a palette of 24 colors on a transparent ground, so that most windows are opaque enough to be looked at
fn asset(size: u32) -> RgbaImage {
    let palette: Vec<Rgba<u8>> = (0..24u32)
        .map(|i| {
            Rgba([
                (i * 53 % 256) as u8,
                (i * 91 % 256) as u8,
                (i * 17 % 256) as u8,
                255,
            ])
        })
        .collect();
    let mut image = RgbaImage::new(size, size);
    let mut state = 12345u32;
    for by in 0..size / 4 {
        for bx in 0..size / 4 {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            if (state >> 24) % 5 == 0 {
                continue;
            }
            let color = palette[(state >> 16) as usize % palette.len()];
            for y in 0..4 {
                for x in 0..4 {
                    image.put_pixel(bx * 4 + x, by * 4 + y, color);
                }
            }
        }
    }
    image
}

/// A window of the asset with its colors rotated and flipped, which is a sprite the asset can make
fn sprite_of(asset: &RgbaImage, x: u32, y: u32, size: u32) -> RgbaImage {
    let mut sprite = imageops::crop_imm(asset, x, y, size, size).to_image();
    for pixel in sprite.pixels_mut() {
        if pixel[3] != 0 {
            *pixel = Rgba([pixel[2], pixel[0], pixel[1], pixel[3]]);
        }
    }
    imageops::flip_horizontal_in_place(&mut sprite);
    sprite
}

fn bench_match(c: &mut Criterion) {
    let mut group = c.benchmark_group("match_image");
    for size in [128u32, 256, 512] {
        let asset = asset(size);
        // some window with enough opaque pixels
        let (x, y) = (size / 2 + 3, size / 3 + 1);
        let hit = sprite_of(&asset, x, y, 43);
        let mut miss = hit.clone();
        miss.put_pixel(20, 20, Rgba([1, 2, 3, 255]));
        miss.put_pixel(21, 20, Rgba([4, 5, 6, 255]));
        let windows = (size - 42) as u64 * (size - 42) as u64 * 4;
        group.throughput(Throughput::Elements(windows));
        for (label, target) in [("hit", &hit), ("miss", &miss)] {
            group.bench_with_input(BenchmarkId::new(label, size), target, |b, target| {
                b.iter(|| {
                    closest::match_image(black_box(target), "/asset.png", black_box(&asset), 0, 128)
                });
            });
        }
    }
    group.finish();
}

/// The game's assets, from `SBGEN_BENCH_ASSETS` (packs and directories separated by `:`) or the default ones
fn game_assets() -> Option<Assets> {
    let opened = match std::env::var("SBGEN_BENCH_ASSETS") {
        Ok(paths) => Assets::open(&paths.split(':').map(PathBuf::from).collect::<Vec<_>>()),
        Err(_) => Assets::open_default(),
    };
    match opened {
        Ok(assets) => Some(assets),
        Err(error) => {
            eprintln!(
                "no game assets for the search benchmarks ({error:#}), set SBGEN_BENCH_ASSETS"
            );
            None
        }
    }
}

fn bench_search(c: &mut Criterion) {
    let Some(assets) = game_assets() else { return };
    let Ok(sheet) = assets.read("/humanoid/human/malebody.png") else {
        return;
    };
    let sheet = sbgen::generator::decode_rgba(&sheet).unwrap();
    let frame = sprite_of(&sheet, 43, 43, 43);

    let mut group = c.benchmark_group("search");
    group.sample_size(10);
    let humanoid = Options {
        under: vec!["/humanoid/".to_string()],
        ..Options::default()
    };
    group.bench_function("humanoid_frame", |b| {
        b.iter(|| closest::search(black_box(&frame), &assets, &humanoid).unwrap())
    });
    group.bench_function("whole_pack_frame", |b| {
        b.iter(|| closest::search(black_box(&frame), &assets, &Options::default()).unwrap())
    });
    group.finish();
}

criterion_group!(benches, bench_match, bench_search);
criterion_main!(benches);
