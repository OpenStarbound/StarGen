//! Looks for a game asset that a sprite can be made from with a few directives, instead of building the sprite from nothing.
//!
//! A sprite is made of an asset when some window of the asset, flipped or not, has opaque pixels wherever the sprite has them, and every color of the asset maps to one color of the sprite.
//! The directives are then `?crop` (the window), `?flip*` and one `?replace` (the colors).
//! Pixels where the colors of the asset do not map to one color of the sprite are the mismatches: the directives cannot make them right.
use crate::generator::{self, Input, Inputs, create_replacer_string, decode_rgba};
use crate::pak::{Assets, png_size};
use anyhow::Result;
use image::RgbaImage;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flip {
    None,
    X,
    Y,
    Both,
}

impl Flip {
    pub const ALL: [Flip; 4] = [Flip::None, Flip::X, Flip::Y, Flip::Both];

    fn operation(self) -> &'static str {
        match self {
            Flip::None => "",
            Flip::X => "?flipx",
            Flip::Y => "?flipy",
            Flip::Both => "?flipxy",
        }
    }
}

type Color = [u8; 4];

/// The sprite, or the sprite flipped, with what the search needs of it
struct Variant {
    flip: Flip,
    width: usize,
    height: usize,
    pixels: Vec<Color>,
    /// every pixel of the sprite, as x and y: the opaque ones first, each kind in an order that jumps around
    order: Vec<(u32, u32)>,
    /// the words of every row that has an opaque pixel: the row, then the bits of its opaque pixels
    rows: Vec<(usize, Vec<u64>)>,
}

impl Variant {
    fn new(target: &RgbaImage, flip: Flip) -> Variant {
        let (width, height) = (target.width() as usize, target.height() as usize);
        let mut pixels = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                let (sx, sy) = match flip {
                    Flip::None => (x, y),
                    Flip::X => (width - 1 - x, y),
                    Flip::Y => (x, height - 1 - y),
                    Flip::Both => (width - 1 - x, height - 1 - y),
                };
                pixels.push(target.get_pixel(sx as u32, sy as u32).0);
            }
        }
        let words = width.div_ceil(64);
        let mut rows = Vec::new();
        for y in 0..height {
            let mut bits = vec![0u64; words];
            let mut any = false;
            for x in 0..width {
                if pixels[y * width + x][3] != 0 {
                    bits[x / 64] |= 1 << (x % 64);
                    any = true;
                }
            }
            if any {
                rows.push((y, bits));
            }
        }
        // the rows with the most pixels are the likeliest to fail, so they go first
        rows.sort_by_key(|(_, bits)| {
            std::cmp::Reverse(bits.iter().map(|word| word.count_ones()).sum::<u32>())
        });
        let mut order: Vec<(u32, u32)> = Vec::with_capacity(width * height);
        for opaque in [true, false] {
            let mut kind: Vec<(u32, u32)> = (0..width * height)
                .filter(|i| (pixels[*i][3] != 0) == opaque)
                .map(|i| ((i % width) as u32, (i / width) as u32))
                .collect();
            // a fixed shuffle: the pixel at index i goes to i * step, with a step that has no common factor with the length
            let len = kind.len();
            if len > 2 {
                let mut step = (len as f64 * 0.618) as usize | 1;
                while gcd(step, len) != 1 {
                    step += 2;
                }
                kind = (0..len).map(|i| kind[i * step % len]).collect();
            }
            order.extend(kind);
        }
        Variant {
            flip,
            width,
            height,
            pixels,
            order,
            rows,
        }
    }
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// The opaque pixels of an asset, one bit each, with a word of padding at the end of every row
struct Mask {
    stride: usize,
    bits: Vec<u64>,
}

impl Mask {
    fn new(image: &RgbaImage) -> Mask {
        let stride = (image.width() as usize).div_ceil(64) + 1;
        let mut bits = vec![0u64; stride * image.height() as usize];
        for (x, y, pixel) in image.enumerate_pixels() {
            if pixel[3] != 0 {
                bits[y as usize * stride + x as usize / 64] |= 1 << (x % 64);
            }
        }
        Mask { stride, bits }
    }

    /// the 64 pixels of a row from `x` on
    #[inline]
    fn word(&self, y: usize, x: usize) -> u64 {
        let row = &self.bits[y * self.stride..(y + 1) * self.stride];
        let (index, shift) = (x / 64, x % 64);
        if shift == 0 {
            row[index]
        } else {
            row[index] >> shift | row[index + 1] << (64 - shift)
        }
    }

    /// whether the asset is opaque, at the window, wherever the sprite is
    #[inline]
    fn covers(&self, variant: &Variant, x: usize, y: usize) -> bool {
        for (row, bits) in &variant.rows {
            for (index, want) in bits.iter().enumerate() {
                if self.word(y + row, x + index * 64) & want != *want {
                    return false;
                }
            }
        }
        true
    }
}

#[derive(Clone, Debug)]
pub struct Candidate {
    pub path: String,
    pub flip: Flip,
    /// the window in the asset, counted from the top left
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub asset_width: u32,
    pub asset_height: u32,
    /// pixels that the colors cannot make right
    pub mismatches: usize,
    /// asset color to sprite color, only where they are different
    pub replacements: Vec<(Color, Color)>,
}

impl Candidate {
    /// the operations that turn the asset into the sprite
    pub fn directives(&self) -> String {
        let mut directives = String::new();
        if (self.width, self.height) != (self.asset_width, self.asset_height) {
            // the engine counts rows from the bottom
            let (x1, y1) = (self.x + self.width, self.asset_height - self.y);
            let y0 = y1 - self.height;
            directives += &format!("?crop={};{};{};{}", self.x, y0, x1, y1);
        }
        directives += self.flip.operation();
        directives += &create_replacer_string(self.replacements.clone(), false, |_| true);
        directives
    }

    /// the asset and the operations, which is what an image path in an item looks like
    pub fn image(&self) -> String {
        format!("{}{}", self.path, self.directives())
    }
}

/// Which sprite color each asset color has been seen with in the window being looked at, a hash table that is emptied by counting up
struct ColorMap {
    mask: usize,
    stamp: u32,
    stamps: Vec<u32>,
    from: Vec<u32>,
    to: Vec<u32>,
    entries: Vec<(Color, Color)>,
}

impl ColorMap {
    fn new(max_colors: usize) -> ColorMap {
        let size = (max_colors * 4).next_power_of_two().max(64);
        ColorMap {
            mask: size - 1,
            stamp: 0,
            stamps: vec![0; size],
            from: vec![0; size],
            to: vec![0; size],
            entries: Vec::new(),
        }
    }

    fn clear(&mut self) {
        self.stamp += 1;
        self.entries.clear();
    }

    /// Notes that `from` is `to` in the window. False if `from` was another color before, or if there are more than `max_colors` colors
    #[inline]
    fn add(&mut self, from: Color, to: Color, max_colors: usize) -> bool {
        let (key, value) = (u32::from_le_bytes(from), u32::from_le_bytes(to));
        let mut slot = (key.wrapping_mul(0x9E37_79B1) >> 7) as usize & self.mask;
        loop {
            if self.stamps[slot] != self.stamp {
                if self.entries.len() == max_colors {
                    return false;
                }
                self.stamps[slot] = self.stamp;
                self.from[slot] = key;
                self.to[slot] = value;
                self.entries.push((from, to));
                return true;
            }
            if self.from[slot] == key {
                return self.to[slot] == value;
            }
            slot = (slot + 1) & self.mask;
        }
    }
}

/// The colors to replace if every color of the asset in the window is one color of the sprite
fn exact_colors(
    variant: &Variant,
    asset: &[u8],
    asset_width: usize,
    x: usize,
    y: usize,
    max_colors: usize,
    map: &mut ColorMap,
) -> Option<Vec<(Color, Color)>> {
    map.clear();
    for &(dx, dy) in &variant.order {
        let at = ((y + dy as usize) * asset_width + x + dx as usize) * 4;
        let from: Color = asset[at..at + 4].try_into().unwrap();
        if from[3] != 0
            && !map.add(
                from,
                variant.pixels[dy as usize * variant.width + dx as usize],
                max_colors,
            )
        {
            return None;
        }
    }
    Some(
        map.entries
            .iter()
            .filter(|(from, to)| from != to)
            .copied()
            .collect(),
    )
}

/// Whether the asset has more than `limit` different opaque colors
fn too_many_colors(asset: &RgbaImage, limit: usize) -> bool {
    let mut colors: std::collections::HashSet<Color, foldhash::fast::FixedState> =
        Default::default();
    for pixel in asset.as_raw().as_chunks::<4>().0 {
        if pixel[3] != 0 && colors.insert(*pixel) && colors.len() > limit {
            return true;
        }
    }
    false
}

/// The best window of one asset for one variant of the sprite: fewest mismatches, then fewest colors to replace
fn best_window(
    variant: &Variant,
    asset: &RgbaImage,
    mask: &Mask,
    max_mismatch: usize,
    max_colors: usize,
) -> Option<(usize, usize, usize, Vec<(Color, Color)>)> {
    let (width, height) = (asset.width() as usize, asset.height() as usize);
    if variant.width > width || variant.height > height {
        return None;
    }
    let asset_pixels = asset.as_raw();
    let mut best: Option<((usize, usize), usize, usize, Vec<(Color, Color)>)> = None;
    // counts of target colors for every asset color, reused between windows
    let mut counts: HashMap<Color, HashMap<Color, usize>> = HashMap::new();
    let mut seen = ColorMap::new(max_colors);
    for y in 0..=height - variant.height {
        for x in 0..=width - variant.width {
            if !mask.covers(variant, x, y) {
                continue;
            }
            if max_mismatch == 0 {
                // nothing may be wrong, so the first asset color that is two colors of the sprite ends it
                if let Some(replacements) =
                    exact_colors(variant, asset_pixels, width, x, y, max_colors, &mut seen)
                {
                    let score = (0, replacements.len());
                    if best.as_ref().is_none_or(|best| score < best.0) {
                        best = Some((score, x, y, replacements));
                    }
                }
                continue;
            }
            counts.clear();
            let mut too_many = false;
            'rows: for row in 0..variant.height {
                let source = &asset_pixels[((y + row) * width + x) * 4..][..variant.width * 4];
                let wanted = &variant.pixels[row * variant.width..][..variant.width];
                for (from, to) in source.as_chunks::<4>().0.iter().zip(wanted) {
                    if from[3] != 0 {
                        *counts.entry(*from).or_default().entry(*to).or_default() += 1;
                        if counts.len() > max_colors {
                            too_many = true;
                            break 'rows;
                        }
                    }
                }
            }
            if too_many {
                continue;
            }
            let mut mismatches = 0;
            let mut replacements = Vec::new();
            for (from, targets) in &counts {
                let total: usize = targets.values().sum();
                // the most common color, the asset color itself if it ties
                let (to, count) = targets
                    .iter()
                    .max_by_key(|(to, count)| (**count, **to == *from, std::cmp::Reverse(**to)))
                    .map(|(to, count)| (*to, *count))
                    .unwrap();
                mismatches += total - count;
                if to != *from {
                    replacements.push((*from, to));
                }
            }
            if mismatches > max_mismatch {
                continue;
            }
            let score = (mismatches, replacements.len());
            if best.as_ref().is_none_or(|best| score < best.0) {
                best = Some((score, x, y, replacements));
            }
        }
    }
    best.map(|((mismatches, _), x, y, replacements)| (mismatches, x, y, replacements))
}

/// Looks for the sprite in one asset
pub fn match_image(
    target: &RgbaImage,
    path: &str,
    asset: &RgbaImage,
    max_mismatch: usize,
    max_colors: usize,
) -> Option<Candidate> {
    let variants: Vec<Variant> = Flip::ALL
        .iter()
        .map(|flip| Variant::new(target, *flip))
        .collect();
    match_asset(&variants, path, asset, max_mismatch, max_colors)
}

/// The best candidates for one asset: one for each way to flip the sprite that found a window, the best of them first
fn match_asset(
    variants: &[Variant],
    path: &str,
    asset: &RgbaImage,
    max_mismatch: usize,
    max_colors: usize,
) -> Option<Candidate> {
    let mask = Mask::new(asset);
    let mut best: Option<Candidate> = None;
    for variant in variants {
        let Some((mismatches, x, y, mut replacements)) =
            best_window(variant, asset, &mask, max_mismatch, max_colors)
        else {
            continue;
        };
        replacements.sort();
        let candidate = Candidate {
            path: path.to_owned(),
            flip: variant.flip,
            x: x as u32,
            y: y as u32,
            width: variant.width as u32,
            height: variant.height as u32,
            asset_width: asset.width(),
            asset_height: asset.height(),
            mismatches,
            replacements,
        };
        let key = |c: &Candidate| (c.mismatches, c.replacements.len(), c.flip != Flip::None);
        if best.as_ref().is_none_or(|best| key(&candidate) < key(best)) {
            best = Some(candidate);
        }
    }
    best
}

pub struct Options {
    /// only assets under these paths (all of them if there are none)
    pub under: Vec<String>,
    /// assets with more pixels than this are not looked at
    pub max_asset_pixels: u64,
    pub max_mismatch: usize,
    /// windows with more different colors than this are not looked at, since each costs a replacement
    pub max_colors: usize,
    /// assets with more different colors than this are not looked at: photographs and gradients, not sprites
    pub max_asset_colors: usize,
    /// how many candidates to keep
    pub keep: usize,
    pub threads: usize,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            under: Vec::new(),
            max_asset_pixels: 1 << 18,
            max_mismatch: 0,
            max_colors: 128,
            max_asset_colors: 2048,
            keep: 5,
            threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
        }
    }
}

#[derive(Default, Debug)]
pub struct Search {
    /// best first: the fewest mismatches, then the shortest directives
    pub candidates: Vec<Candidate>,
    /// the png assets that are big enough to hold the sprite
    pub considered: usize,
    pub decoded: usize,
}

/// Looks through the assets for the ones the sprite can be made from
pub fn search(target: &RgbaImage, assets: &Assets, options: &Options) -> Result<Search> {
    let variants: Vec<Variant> = Flip::ALL
        .iter()
        .map(|flip| Variant::new(target, *flip))
        .collect();
    let opaque = target.pixels().filter(|pixel| pixel[3] != 0).count();
    let (width, height) = target.dimensions();

    let mut paths: Vec<&str> = assets
        .paths()
        .filter(|path| path.len() > 4 && path[path.len() - 4..].eq_ignore_ascii_case(".png"))
        .filter(|path| {
            options.under.is_empty()
                || options
                    .under
                    .iter()
                    .any(|prefix| path.starts_with(prefix.as_str()))
        })
        .collect();
    paths.sort_unstable();

    let next = AtomicUsize::new(0);
    let found = Mutex::new(Vec::new());
    let (considered, decoded) = (AtomicUsize::new(0), AtomicUsize::new(0));
    std::thread::scope(|scope| {
        for _ in 0..options.threads.max(1) {
            scope.spawn(|| {
                let mut mine = Vec::new();
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = paths.get(index) else { break };
                    let Ok(header) = assets.read_prefix(path, 24) else {
                        continue;
                    };
                    let Some((w, h)) = png_size(&header) else {
                        continue;
                    };
                    if w < width || h < height || (w as u64) * (h as u64) > options.max_asset_pixels
                    {
                        continue;
                    }
                    considered.fetch_add(1, Ordering::Relaxed);
                    let Ok(data) = assets.read(path) else {
                        continue;
                    };
                    let Ok(asset) = decode_rgba(&data) else {
                        continue;
                    };
                    decoded.fetch_add(1, Ordering::Relaxed);
                    if asset.pixels().filter(|pixel| pixel[3] != 0).count() < opaque {
                        continue;
                    }
                    if too_many_colors(&asset, options.max_asset_colors) {
                        continue;
                    }
                    let started = std::time::Instant::now();
                    let found_one = match_asset(
                        &variants,
                        path,
                        &asset,
                        options.max_mismatch,
                        options.max_colors,
                    );
                    if std::env::var_os("STARGEN_TRACE").is_some()
                        && started.elapsed().as_millis() > 500
                    {
                        eprintln!(
                            "{:?} {path} {}x{}",
                            started.elapsed(),
                            asset.width(),
                            asset.height()
                        );
                    }
                    if let Some(candidate) = found_one {
                        mine.push(candidate);
                    }
                }
                found.lock().unwrap().extend(mine);
            });
        }
    });

    let mut candidates = found.into_inner().unwrap();
    let mut ranked: Vec<(usize, usize, String, Candidate)> = candidates
        .drain(..)
        .map(|candidate| {
            let length = candidate.image().len();
            (
                candidate.mismatches,
                length,
                candidate.path.clone(),
                candidate,
            )
        })
        .collect();
    ranked.sort_by(|a, b| (a.0, a.1, &a.2).cmp(&(b.0, b.1, &b.2)));
    ranked.truncate(options.keep.max(1));
    Ok(Search {
        candidates: ranked
            .into_iter()
            .map(|(.., candidate)| candidate)
            .collect(),
        considered: considered.into_inner(),
        decoded: decoded.into_inner(),
    })
}

/// What `stargen sprite --closest` does: the image path of the best asset and its operations, or the sprite built from nothing when that is shorter or nothing fits
pub fn generate(
    image: &std::path::Path,
    packs: &[std::path::PathBuf],
    options: &Options,
    output: Option<std::path::PathBuf>,
) -> Result<()> {
    use anyhow::Context;
    let data =
        std::fs::read(image).with_context(|| format!("failed to read {}", image.display()))?;
    let target =
        decode_rgba(&data).with_context(|| format!("failed to decode {}", image.display()))?;
    let name = image.file_name().map_or_else(
        || "sprite".to_owned(),
        |name| name.to_string_lossy().trim_end_matches(".png").to_owned(),
    );

    let began = std::time::Instant::now();
    let assets = if packs.is_empty() {
        Assets::open_default()?
    } else {
        Assets::open(packs)?
    };
    let search = search(&target, &assets, options)?;
    eprintln!(
        "looked at {} of {} assets ({} could hold the sprite), in {:.2?}",
        search.decoded,
        assets.len(),
        search.considered,
        began.elapsed()
    );
    let built = generated_length(&data, &format!("{name}.png"));
    match &built {
        Ok(length) => eprintln!("built from nothing: {length} chars"),
        Err(error) => eprintln!("can not be built from nothing: {error}"),
    }
    let built = built.ok();
    for candidate in &search.candidates {
        let image = candidate.image();
        eprintln!(
            "  {} chars, {} mismatched pixels: {} at {},{}{}",
            image.len(),
            candidate.mismatches,
            candidate.path,
            candidate.x,
            candidate.y,
            if candidate.flip == Flip::None {
                String::new()
            } else {
                format!(", flipped {:?}", candidate.flip)
            },
        );
    }
    let best = search
        .candidates
        .first()
        .filter(|best| built.is_none_or(|built| best.image().len() < built));
    let content = match best {
        Some(best) => {
            if best.mismatches > 0 {
                eprintln!(
                    "(the {} mismatched pixels will not be right)",
                    best.mismatches
                );
            }
            best.image()
        }
        None => {
            eprintln!("no asset does better, building the sprite from nothing");
            let mut inputs = Inputs::new();
            let (width, height) = target.dimensions();
            inputs.insert(
                "image".to_string(),
                (
                    Input {
                        filename: format!("{name}.png"),
                        data,
                        width,
                        height,
                    },
                    0,
                ),
            );
            generator::do_generate(&inputs, &generator::data::SINGLE, "image")?.directives
        }
    };
    generator::write_output(output, &format!("{name}.txt"), content)
}

/// How long the directives are that build the sprite from nothing
pub fn generated_length(png: &[u8], filename: &str) -> Result<usize> {
    use crate::generator::{data::SINGLE, do_generate};
    let (width, height) = image::ImageReader::new(std::io::Cursor::new(png))
        .with_guessed_format()?
        .into_dimensions()?;
    let mut inputs = Inputs::new();
    inputs.insert(
        "image".to_string(),
        (
            Input {
                filename: filename.to_owned(),
                data: png.to_vec(),
                width,
                height,
            },
            0,
        ),
    );
    Ok(do_generate(&inputs, &SINGLE, "image")?.directives.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    const A: Color = [200, 10, 10, 255];
    const B: Color = [10, 200, 10, 255];
    const C: Color = [10, 10, 200, 255];

    fn image(width: u32, height: u32, rows: &[&str], colors: &[(char, Color)]) -> RgbaImage {
        let mut image = RgbaImage::new(width, height);
        for (y, row) in rows.iter().enumerate() {
            for (x, c) in row.chars().enumerate() {
                if let Some((_, color)) = colors.iter().find(|(name, _)| *name == c) {
                    image.put_pixel(x as u32, y as u32, Rgba(*color));
                }
            }
        }
        image
    }

    fn find(target: &RgbaImage, asset: &RgbaImage, max_mismatch: usize) -> Option<Candidate> {
        let variants: Vec<Variant> = Flip::ALL
            .iter()
            .map(|flip| Variant::new(target, *flip))
            .collect();
        match_asset(&variants, "/asset.png", asset, max_mismatch, 128)
    }

    fn asset() -> RgbaImage {
        image(
            6,
            5,
            &["......", ".aab..", ".abb..", ".bbc..", "......"],
            &[('a', A), ('b', B), ('c', C)],
        )
    }

    #[test]
    fn finds_a_window_and_counts_no_mismatches() {
        let target = image(
            3,
            3,
            &["aab", "abb", "bbc"],
            &[('a', A), ('b', B), ('c', C)],
        );
        let found = find(&target, &asset(), 0).unwrap();
        assert_eq!(
            (found.x, found.y, found.mismatches, found.flip),
            (1, 1, 0, Flip::None)
        );
        assert!(found.replacements.is_empty());
        // the window is the rows 1 to 3 of five, so from the bottom they are 2 to 4
        assert_eq!(found.directives(), "?crop=1;1;4;4");
    }

    #[test]
    fn the_crop_counts_rows_from_the_bottom() {
        let asset = image(
            4,
            4,
            &["a...", "....", "....", "..b."],
            &[('a', A), ('b', B)],
        );
        let top = image(1, 1, &["a"], &[('a', A)]);
        let found = find(&top, &asset, 0).unwrap();
        assert_eq!(found.directives(), "?crop=0;3;1;4");
        let bottom = image(1, 1, &["b"], &[('b', B)]);
        let found = find(&bottom, &asset, 0).unwrap();
        assert_eq!((found.x, found.y), (2, 3));
        assert_eq!(found.directives(), "?crop=2;0;3;1");
    }

    #[test]
    fn finds_flipped_and_recolored_windows() {
        let colors = [
            ('a', [255, 0, 0, 255]),
            ('b', [0, 0, 255, 255]),
            ('c', [255, 255, 0, 255]),
        ];
        // the window flipped left to right, with the three colors changed
        let target = image(3, 3, &["baa", "bba", "cbb"], &colors);
        let found = find(&target, &asset(), 0).unwrap();
        assert_eq!((found.flip, found.mismatches), (Flip::X, 0));
        assert_eq!(found.replacements.len(), 3);
        assert!(
            found
                .directives()
                .starts_with("?crop=1;1;4;4?flipx?replace;")
        );
    }

    #[test]
    fn colors_that_merge_are_fine_and_colors_that_split_are_mismatches() {
        let same = [('a', A), ('b', A), ('c', C)];
        let merged = image(3, 3, &["aab", "abb", "bbc"], &same);
        let found = find(&merged, &asset(), 0).unwrap();
        assert_eq!(found.mismatches, 0);
        // one pixel of b is another color than the rest of b
        let colors = [('a', A), ('b', B), ('c', C), ('d', [1, 2, 3, 255])];
        let split = image(3, 3, &["aab", "abb", "bdc"], &colors);
        assert!(find(&split, &asset(), 0).is_none());
        let found = find(&split, &asset(), 1).unwrap();
        assert_eq!(found.mismatches, 1);
    }

    #[test]
    fn needs_opaque_pixels_where_the_sprite_has_them() {
        let target = image(
            3,
            3,
            &["aab", "abb", "bbc"],
            &[('a', A), ('b', B), ('c', C)],
        );
        let hollow = image(
            6,
            5,
            &["......", ".aab..", ".a.b..", ".bbc..", "......"],
            &[('a', A), ('b', B), ('c', C)],
        );
        assert!(find(&target, &hollow, 100).is_none());
    }

    #[test]
    fn transparent_pixels_of_the_sprite_may_be_opaque_in_the_asset() {
        // a color of the asset cannot be nothing in some places and a color in others
        let target = image(3, 3, &["aa.", "ab.", "..."], &[('a', A), ('b', B)]);
        let asset = image(
            3,
            3,
            &["aab", "abb", "bbc"],
            &[('a', A), ('b', B), ('c', C)],
        );
        assert!(find(&target, &asset, 0).is_none());
        let found = find(&target, &asset, 1).unwrap();
        // b is nothing in five places and a color in one, c is nothing
        assert_eq!(found.mismatches, 1);
        assert!(found.replacements.contains(&(B, [0; 4])));
    }

    #[test]
    fn windows_wider_than_a_word_are_found() {
        let colors = [('a', A), ('b', B)];
        let row: String = (0..150)
            .map(|x| if x % 3 == 0 { 'a' } else { 'b' })
            .collect();
        let rows: Vec<&str> = vec![row.as_str(); 2];
        let asset = image(200, 4, &rows, &colors);
        let mut target = RgbaImage::new(130, 2);
        for y in 0..2 {
            for x in 0..130 {
                target.put_pixel(x, y, Rgba(if (x + 7) % 3 == 0 { A } else { B }));
            }
        }
        let found = find(&target, &asset, 0).unwrap();
        assert_eq!(found.mismatches, 0);
        assert_eq!(found.x % 3, 1);
    }
}
