//! Prints a hash of what the engine makes of a lot of drawables, so that a change to the engine can be checked to leave every
//! pixel as it was: `golden > before.txt`, change the engine, `golden > after.txt`, `diff before.txt after.txt`.
//!
//! Two sets: the frames of the generator's cases, after every operation of their directives, and random images through the
//! operations that are worth testing on their own.
//! Each is applied the way drawables are and one operation after the other.
use engine_bench::{
    engine::{Directives, Image},
    harness::{self, Assets},
};

fn hash(width: u32, height: u32, rgba: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in width
        .to_le_bytes()
        .into_iter()
        .chain(height.to_le_bytes())
        .chain(rgba.iter().copied())
    {
        hash = (hash ^ byte as u64).wrapping_mul(0x100000001b3);
    }
    hash
}

struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    /// Images of all kinds: sizes from 1 to 63, and alpha that is solid, sparse, scattered, or anything
    fn image(&mut self) -> Image {
        let (width, height) = (1 + self.below(63) as u32, 1 + self.below(63) as u32);
        let kind = self.below(5);
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..width * height {
            let bits = self.next();
            let alpha = match kind {
                0 => 255,
                1 => {
                    if bits >> 40 & 15 == 0 {
                        255
                    } else {
                        0
                    }
                }
                2 => {
                    if bits >> 40 & 3 == 0 {
                        (bits >> 48) as u8
                    } else {
                        0
                    }
                }
                3 => (bits >> 48) as u8,
                _ => [0, 255, 128, 1, 254][(bits >> 48) as usize % 5],
            };
            // transparent pixels keep their color, to see that it is treated the same
            rgba.extend_from_slice(&[bits as u8, (bits >> 8) as u8, (bits >> 16) as u8, alpha]);
        }
        Image {
            width,
            height,
            rgba,
        }
    }
}

const DIRECTIVES: &[&str] = &[
    "?border=1;f00;00f",
    "?border=2;f00;00f",
    "?border=3;ff08;0f08;00f",
    "?border=2;fff",
    "?border=4;f00;00f",
    "?outline=2;f00;00f",
    "?outline=1;fff",
    "?flipx",
    "?flipy",
    "?flipxy",
    "?scale=.944",
    "?scale=.94",
    "?scale=1.013",
    "?scale=.506",
    "?scale=2.11",
    "?scale=.5;.7",
    "?scale=42.5;1",
    "?scale=1;42.5",
    "?scale=1",
    "?scale=3",
    "?scale=3;.25",
    "?scale=.3;1.7",
    "?scale=1;.999",
    "?scale=.944;1.013",
    "?scale=7;.1",
    "?scale=.05;.05",
    "?scale=5.5;2.2",
    "?scale=1.5;1.5",
    "?scalebilinear=1.5",
    "?scalenearest=2",
    "?scalenearest=.5",
    "?scalebicubic=1.5",
    "?saturation=100",
    "?saturation=22",
    "?saturation=-50",
    "?brightness=20",
    "?hue=30",
    "?setcolor=0f0",
    "?multiply=fff0",
    "?multiply=f80",
    "?scanlines=f11;.4;12f;.4",
    "?fade=f00;.3",
    "?crop=1;1;5;5",
    "?crop=0;0;2;40",
    "?replace;000=fff;fff=000",
    // the shape of the generator's prefix
    "?setcolor=0f0?border=2;f00;00f?replace;0f00=fff7?scanlines=f11;.4;12f;.4?scale=.944?saturation=100?flipxy?scale=.94?scale=1.013?saturation=22?flipxy?scale=.506?flipy?saturation=50?scale=.51?flipxy?scale=.52?scale=.7?flipy?scale=2.11?crop=3;3;5;5",
    "?scale=1;42.5?crop=0;0;2;43?scale=42.5;1?crop=0;0;43;43",
    "?border=2;f00;00f?scale=2?crop=3;3;40;40?saturation=30?flipx",
];

fn main() -> anyhow::Result<()> {
    let assets = Assets::open_default()?;
    for case in harness::humanoid_cases() {
        let text = case.directives()?;
        let ops: Vec<&str> = text.split('?').filter(|op| !op.is_empty()).collect();
        let mut engine = Directives::new(&text)?;
        for frame in case.frames(&assets)? {
            let (w, h, rgba) = engine.apply_in_place(&frame.base)?;
            println!(
                "{}/{} engine {:016x}",
                case.name,
                frame.name,
                hash(w, h, rgba)
            );
            for n in 0..=ops.len() {
                let (w, h, rgba) = engine.apply_first(&frame.base, n)?;
                println!(
                    "{}/{} step {n} {:016x}",
                    case.name,
                    frame.name,
                    hash(w, h, rgba)
                );
            }
        }
    }

    // Every cell of every humanoid sheet of the game, for all of its species, through the directives of the first case: real
    // sprites of all kinds of shapes through the prefix of the frameset. Hashed after the prefix and at the end.
    let case = harness::humanoid_cases().into_iter().next().unwrap();
    let text = case.directives()?;
    let ops: Vec<&str> = text.split('?').filter(|op| !op.is_empty()).collect();
    let prefix_ops = case
        .frame_set
        .prefix
        .split('?')
        .filter(|op| !op.is_empty())
        .count();
    let mut engine = Directives::new(&text)?;
    for species in [
        "apex", "avian", "floran", "glitch", "human", "hylotl", "novakid",
    ] {
        for sheet in [
            "malebody",
            "femalebody",
            "frontarm",
            "backarm",
            "malehead",
            "femalehead",
        ] {
            let path = format!("/humanoid/{species}/{sheet}.png");
            let Ok(bytes) = assets.read(&path) else {
                continue;
            };
            let image = Image::decode(&bytes)?;
            for row in 0..image.height / 43 {
                for column in 0..image.width / 43 {
                    let cell = image.crop(column * 43, row * 43, 43, 43);
                    let (w, h, rgba) = engine.apply_in_place(&cell)?;
                    println!("art {path} {column},{row} engine {:016x}", hash(w, h, rgba));
                    let (w, h, rgba) = engine.apply_first(&cell, prefix_ops)?;
                    println!("art {path} {column},{row} prefix {:016x}", hash(w, h, rgba));
                    let (w, h, rgba) = engine.apply_first(&cell, ops.len())?;
                    println!(
                        "art {path} {column},{row} in-order {:016x}",
                        hash(w, h, rgba)
                    );
                }
            }
        }
    }

    let mut random = Random(0x9e3779b97f4a7c15);
    let images: Vec<Image> = (0..60).map(|_| random.image()).collect();
    for directives in DIRECTIVES {
        let mut engine = Directives::new(directives)?;
        for (i, image) in images.iter().enumerate() {
            // an operation can fail on an image (a crop outside of it), which has to stay the same too
            let engine_hash = engine.apply_in_place(image).map_or_else(
                |error| format!("error {error}"),
                |(w, h, rgba)| format!("{:016x}", hash(w, h, rgba)),
            );
            println!("random {directives} #{i} engine {engine_hash}");
            let in_order_hash = engine.apply_sequential_in_place(image).map_or_else(
                |error| format!("error {error}"),
                |(w, h, rgba)| format!("{:016x}", hash(w, h, rgba)),
            );
            println!("random {directives} #{i} in-order {in_order_hash}");
        }
    }
    Ok(())
}
