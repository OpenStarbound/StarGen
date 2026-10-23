//! Cases for the humanoid generator:
//! the vanilla sprite sheets are the drawables' images and the sheets the directives were made from are what the engine should turn them into.
use crate::engine::{Directives, Image, Pak};
use anyhow::{Context, Result, bail};
use stargen::generator::{
    self, Inputs,
    data::{self, FrameSet},
};
use std::path::PathBuf;

const TEMPLATES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/generator/templates");

/// One sprite sheet that is given to the generator
pub struct Part {
    /// the name of the sheet in the frame set, like "malebody"
    pub key: &'static str,
    pub columns: u32,
    /// the sheet that the directives are generated from, and should reproduce
    pub custom: PathBuf,
    /// where the vanilla sheet that they are applied to is in the asset pack
    pub vanilla: &'static str,
}

pub struct Case {
    pub name: &'static str,
    pub frame_set: &'static FrameSet,
    pub id: &'static str,
    pub parts: Vec<Part>,
}

/// A drawable, an image and the image that it should come out as
pub struct Frame {
    pub name: String,
    pub base: Image,
    pub expected: Image,
}

pub fn humanoid_cases() -> Vec<Case> {
    let body = |name, file: &str, key, vanilla, frame_set| Case {
        name,
        frame_set,
        id: "humanoid",
        parts: vec![Part {
            key,
            columns: 9,
            custom: PathBuf::from(TEMPLATES).join(file),
            vanilla,
        }],
    };
    vec![
        body(
            "male_body",
            "malebody.png",
            "malebody",
            "/humanoid/human/malebody.png",
            &data::MALE,
        ),
        body(
            "female_body",
            "femalebody.png",
            "femalebody",
            "/humanoid/human/femalebody.png",
            &data::FEMALE,
        ),
        body(
            "male_body_greyscale",
            "malebody_greyscale.png",
            "malebody",
            "/humanoid/human/malebody.png",
            &data::MALE,
        ),
        body(
            "female_body_greyscale",
            "femalebody_greyscale.png",
            "femalebody",
            "/humanoid/human/femalebody.png",
            &data::FEMALE,
        ),
    ]
}

impl Case {
    /// The directives made by the generator
    pub fn directives(&self) -> Result<String> {
        let mut inputs = Inputs::new();
        for part in &self.parts {
            inputs.insert(
                part.key.to_string(),
                (generator::load_input(&part.custom)?, part.columns),
            );
        }
        Ok(generator::do_generate(&inputs, self.frame_set, self.id)?.directives)
    }

    /// Every frame that the generator made directives for
    pub fn frames(&self, assets: &Assets) -> Result<Vec<Frame>> {
        let mut frames = Vec::new();
        for part in &self.parts {
            let custom = Image::decode(
                &std::fs::read(&part.custom)
                    .with_context(|| format!("reading {}", part.custom.display()))?,
            )?;
            // BASE=custom applies the directives to the sheets they were made from, which only checks that they are consistent
            let vanilla = if std::env::var("BASE").is_ok_and(|base| base == "custom") {
                custom.clone()
            } else {
                Image::decode(&assets.read(part.vanilla)?)?
            };
            if (custom.width, custom.height) != (vanilla.width, vanilla.height) {
                bail!(
                    "{} is {}x{}, but the vanilla sheet is {}x{}",
                    part.custom.display(),
                    custom.width,
                    custom.height,
                    vanilla.width,
                    vanilla.height
                );
            }
            // frames are square, like the generator assumes
            let size = custom.width / part.columns;
            // The engine's images start at the lower left, the generator counts the cells of a sheet from the top: its row y is the engine's row (rows - 1 - y)
            let rows = custom.height / size;
            for entry in self
                .frame_set
                .entries
                .iter()
                .filter(|entry| entry.image == part.key)
            {
                let [x, y] = entry.pos;
                if (x + 1) * size > custom.width || (y + 1) * size > custom.height {
                    continue;
                }
                frames.push(Frame {
                    name: format!("{}:{}", part.key, entry.frame),
                    base: vanilla.crop(x * size, (rows - 1 - y) * size, size, size),
                    expected: custom.crop(x * size, (rows - 1 - y) * size, size, size),
                });
            }
        }
        Ok(frames)
    }
}

#[derive(Default, Debug)]
pub struct Report {
    pub frames: usize,
    /// frames without any visible pixel
    pub empty_frames: usize,
    pub exact_frames: usize,
    pub wrong_size_frames: usize,
    pub bad_pixels: usize,
    pub max_channel_diff: u8,
    /// the frames with the most bad pixels
    pub worst: Vec<(String, usize)>,
}

/// Applies the directives to every frame and compares the result with what was expected.
/// where the expected pixel is transparent the result must be too, anywhere else the colors must be the same.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// the way drawables are drawn, which lets the engine reorder and combine operations
    Engine,
    /// one operation after the other, as written
    Sequential,
}

pub fn verify(directives: &mut Directives, frames: &[Frame], mode: Mode) -> Result<Report> {
    let mut report = Report {
        frames: frames.len(),
        ..Report::default()
    };
    for frame in frames {
        let (width, height, rgba) = match mode {
            Mode::Engine => directives.apply_in_place(&frame.base)?,
            Mode::Sequential => directives.apply_sequential_in_place(&frame.base)?,
        };
        if (width, height) != (frame.expected.width, frame.expected.height) {
            report.wrong_size_frames += 1;
            continue;
        }
        let mut bad = 0;
        let mut visible = false;
        for (got, want) in rgba
            .chunks_exact(4)
            .zip(frame.expected.rgba.chunks_exact(4))
        {
            visible |= want[3] != 0;
            if want[3] == 0 && got[3] == 0 {
                continue;
            }
            if got != want {
                bad += 1;
                let diff = got
                    .iter()
                    .zip(want)
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap();
                report.max_channel_diff = report.max_channel_diff.max(diff);
            }
        }
        report.empty_frames += !visible as usize;
        report.exact_frames += (bad == 0) as usize;
        report.bad_pixels += bad;
        if bad > 0 {
            report.worst.push((frame.name.clone(), bad));
        }
    }
    report.worst.sort_by_key(|(_, bad)| std::cmp::Reverse(*bad));
    report.worst.truncate(5);
    Ok(report)
}

/// Where the game finds its assets. OpenStarbound's own assets are laid over the vanilla pack
pub struct Assets {
    pub pak: Pak,
    /// directories that are searched before the pack, like OpenStarbound's assets/opensb
    pub overlays: Vec<PathBuf>,
}

impl Assets {
    /// The vanilla pack from `STARBOUND_PAK` and OpenStarbound's assets from `OPENSB_ASSETS`, by default the ones of the user's checkout
    pub fn open_default() -> Result<Assets> {
        let overlay = std::env::var("OPENSB_ASSETS").unwrap_or_else(|_| {
            format!(
                "{}/code/game/OpenStarbound/assets/opensb",
                std::env::var("HOME").unwrap_or_default()
            )
        });
        Ok(Assets {
            pak: Pak::open_default()?,
            overlays: vec![PathBuf::from(overlay)],
        })
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        for overlay in &self.overlays {
            if let Ok(bytes) = std::fs::read(overlay.join(path.trim_start_matches('/'))) {
                return Ok(bytes);
            }
        }
        self.pak.read(path)
    }
}

/// The frames that the directives reproduce exactly.
/// Those that they don't are dropped by the engine early on, which makes them cheaper than they should be to time.
pub fn exact_frames(directives: &mut Directives, frames: Vec<Frame>) -> Result<Vec<Frame>> {
    let mut exact = Vec::new();
    for frame in frames {
        if verify(directives, std::slice::from_ref(&frame), Mode::Engine)?.exact_frames == 1 {
            exact.push(frame);
        }
    }
    Ok(exact)
}
