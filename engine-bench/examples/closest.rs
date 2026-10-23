//! Applies the image that `stargen sprite --closest` made (an asset path followed by directives) with the real engine and compares it to the sprite.
//! Usage: closest <sprite.png> "<asset path><directives>" [iterations]
//! With iterations it only applies them that many times, to count instructions with `perf stat`.
use engine_bench::{
    engine::{Directives, Image},
    harness::Assets,
};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (sprite, image) = (&args[1], &args[2]);
    let (path, directives) = image.split_at(image.find('?').unwrap_or(image.len()));
    let assets = Assets::open_default()?;
    let base = Image::decode(&assets.read(path)?)?;
    let want = Image::decode(&std::fs::read(sprite)?)?;
    let mut directives = Directives::new(directives)?;
    directives.load()?;
    // a fixed amount of work for `perf stat`, with the setup above being the same for every length
    if let Some(iterations) = args.get(3).and_then(|n| n.parse::<u32>().ok()) {
        for _ in 0..iterations {
            std::hint::black_box(directives.apply_in_place(std::hint::black_box(&base))?.2[0]);
        }
        return Ok(());
    }
    let (width, height, rgba) = directives.apply_in_place(&base)?;
    if (width, height) != (want.width, want.height) {
        println!(
            "wrong size: {width}x{height}, wanted {}x{}",
            want.width, want.height
        );
        std::process::exit(1);
    }
    for (i, (got, want)) in rgba
        .chunks_exact(4)
        .zip(want.rgba.chunks_exact(4))
        .enumerate()
        .filter(|(_, (got, want))| !(want[3] == 0 && got[3] == 0) && got != want)
        .take(5)
    {
        eprintln!(
            "pixel {},{}: got {got:?}, wanted {want:?}",
            i as u32 % width,
            i as u32 / width
        );
    }
    let bad = rgba
        .chunks_exact(4)
        .zip(want.rgba.chunks_exact(4))
        // a transparent pixel is the same whatever its color
        .filter(|(got, want)| !(want[3] == 0 && got[3] == 0) && got != want)
        .count();
    println!(
        "{bad} of {} pixels differ ({} chars of directives)",
        width * height,
        directives_len(image)
    );
    std::process::exit((bad > 0) as i32);
}

fn directives_len(image: &str) -> usize {
    image.len()
}
