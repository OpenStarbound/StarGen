use image::{GenericImageView, Pixel};
use image::{RgbaImage, SubImage};
use serde_json::{json, to_string_pretty};
use std::fmt::Write;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Instant;
type HashMap<K, V> = std::collections::HashMap<K, V, FixedState>;
type HashSet<T> = std::collections::HashSet<T, FixedState>;
use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use compact_str::{CompactString, format_compact};
use foldhash::fast::FixedState;
use indexmap::IndexMap;
use std::hash::{BuildHasher, Hash, Hasher};

pub mod data;
use data::*;

// the bytes of row `y` of a frame
fn frame_row_bytes<'a>(frame: &'a SubImage<&RgbaImage>, y: u32) -> &'a [u8] {
    let buffer: &RgbaImage = frame.inner();
    let (offset_x, offset_y) = frame.offsets();
    let start = ((offset_y + y) as usize * buffer.width() as usize + offset_x as usize) * 4;
    &buffer.as_raw()[start..start + frame.width() as usize * 4]
}

fn hash_image(image: SubImage<&RgbaImage>) -> u64 {
    let mut hasher = FixedState::default().build_hasher();
    let (width, height) = image.dimensions();
    hasher.write_u32(width);
    hasher.write_u32(height);
    for y in 0..height {
        hasher.write(frame_row_bytes(&image, y));
    }
    hasher.finish()
}

fn frame_is_empty(frame: &SubImage<&RgbaImage>) -> bool {
    (0..frame.height()).all(|y| {
        frame_row_bytes(frame, y)
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 0)
    })
}

// the pixels of a row, hashed as one block of bytes instead of element by element
#[derive(PartialEq, Eq)]
struct RowKey(Vec<[u8; 4]>);

impl Hash for RowKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write(self.0.as_flattened());
    }
}

// sorts replacements by the byte order of `(from, to)` because having them as plain integers sorts a lot faster
fn sort_replacements(replacements: &mut Vec<([u8; 4], [u8; 4])>) {
    let mut keys: Vec<u64> = replacements
        .iter()
        .map(|&([a, b, c, d], [e, f, g, h])| u64::from_be_bytes([a, b, c, d, e, f, g, h]))
        .collect();
    keys.sort_unstable();
    replacements.clear();
    replacements.extend(keys.into_iter().map(|key| {
        let [a, b, c, d, e, f, g, h] = key.to_be_bytes();
        ([a, b, c, d], [e, f, g, h])
    }));
}

fn color_hex_len(color: [u8; 4]) -> u8 {
    let is_rgb_compact = color[0..=2].iter().all(|&c| c.is_multiple_of(17));
    if color[3] == 255 {
        if is_rgb_compact { 3 } else { 6 }
    } else if is_rgb_compact && color[3].is_multiple_of(17) {
        4
    } else {
        8
    }
}

fn color_hex_len_opaque(mut color: [u8; 4]) -> u8 {
    color[3] = 255;
    color_hex_len(color)
}

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

// writes the shortest form that decodes to the same color (rgb, rgba, rrggbb or rrggbbaa) and returns its length
fn write_hex(color: [u8; 4], out: &mut [u8]) -> usize {
    let compact = color.iter().all(|&c| c.is_multiple_of(17));
    let channels = if color[3] == 255 { 3 } else { 4 };
    let mut len = 0;
    for &c in &color[..channels] {
        if compact {
            out[len] = HEX_DIGITS[(c / 17) as usize];
            len += 1;
        } else {
            out[len] = HEX_DIGITS[(c >> 4) as usize];
            out[len + 1] = HEX_DIGITS[(c & 0xF) as usize];
            len += 2;
        }
    }
    len
}

fn color_to_hex(color: [u8; 4]) -> CompactString {
    let mut buf = [0u8; 8];
    let len = write_hex(color, &mut buf);
    CompactString::from(std::str::from_utf8(&buf[..len]).unwrap())
}

// "from=to" as ASCII, padded with zeroes. The padding sorts before every character, so this orders like the string would
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Replacement([u8; 17]);

impl Replacement {
    fn new(from: [u8; 4], to: [u8; 4]) -> Self {
        let mut buf = [0u8; 17];
        let mut len = write_hex(from, &mut buf);
        buf[len] = b'=';
        len += 1;
        write_hex(to, &mut buf[len..]);
        Self(buf)
    }

    fn as_bytes(&self) -> &[u8] {
        let len = self.0.iter().position(|&b| b == 0).unwrap_or(self.0.len());
        &self.0[..len]
    }
}

fn join_replacements(replacements: &[Replacement]) -> String {
    let mut joined = Vec::with_capacity(replacements.len() * 18);
    for (i, replacement) in replacements.iter().enumerate() {
        if i > 0 {
            joined.push(b';');
        }
        joined.extend_from_slice(replacement.as_bytes());
    }
    String::from_utf8(joined).unwrap()
}

type RowId = u16;

fn compact_row(x: u16) -> u16 {
    match x {
        0 => x,
        1..256 => {
            let (a, b) = (x & 0xF0, x & 0x0F);
            a << 8 | a << 4 | b << 4 | b
        }
        256.. => {
            let high = x >> 8;
            let low = x & 0xFF;
            if high.is_multiple_of(17) && low.is_multiple_of(17) {
                return match high & 0xF0 | low & 0x0F {
                    v @ 0..=15 => v * 17,
                    v => v - 16 + v / 16,
                };
            }
            x
        }
    }
}

fn palette_id_to_color(val: u16) -> [u8; 4] {
    let (a, b, c) = (
        ((val >> 8) & 0xF) as u8,
        ((val >> 4) & 0xF) as u8,
        (val & 0xF) as u8,
    );
    [(a << 4) | a, (b << 4) | b, (c << 4) | c, 255]
}

// returns palette assignments, then palette redirects, then color redirects
pub fn create_replacer<F>(
    replacements: Vec<([u8; 4], [u8; 4])>,
    split_from_alpha: bool,
    palette_tester: F,
) -> (Option<(String, String)>, Option<String>)
where
    F: Fn([u8; 4]) -> bool,
{
    enum ColorEntry {
        Count(u32, u32),
        PaletteColor([u8; 4]),
    }

    let mut colors: HashMap<[u8; 4], ColorEntry> = HashMap::default();
    for (from, to) in replacements.iter() {
        let from_len = if split_from_alpha {
            color_hex_len_opaque(*from)
        } else {
            color_hex_len(*from)
        } as u32;
        let to_len = color_hex_len(*to) as u32;
        if let ColorEntry::Count(opt_len, orig_len) = colors.entry(*to).or_insert_with(|| {
            // 6/5 is ';' + '=' + '????'/'???'
            ColorEntry::Count(2 + if split_from_alpha { 6 } else { 5 } + to_len, 0)
        }) {
            // 5 is ";=???"
            *opt_len += from_len + 5;
            // 2 is ';' + '='
            *orig_len += 2 + color_hex_len(*from) as u32 + to_len;
        }
    }

    let mut next_palette_id: u16 = 0;

    let mut palette_strings = Vec::new();
    let mut regular_strings = Vec::new();
    for (mut from, to) in replacements.into_iter() {
        let (new_to, is_palette) = match colors.get(&to).unwrap() {
            ColorEntry::Count(opt_len, orig_len) => {
                let mut new_to = to;
                let mut is_palette = false;
                // +10 is ?replace
                if (next_palette_id) <= 4096 && *orig_len > *opt_len {
                    loop {
                        let id = next_palette_id;
                        next_palette_id += 1;
                        let palette_as_color = palette_id_to_color(id);
                        if !colors.contains_key(&palette_as_color)
                            && palette_tester(palette_as_color)
                        {
                            colors.insert(to, ColorEntry::PaletteColor(palette_as_color));
                            new_to = palette_as_color;
                            is_palette = true;
                            break;
                        }
                    }
                }
                (new_to, is_palette)
            }
            ColorEntry::PaletteColor(color) => (*color, true),
        };
        if is_palette {
            if split_from_alpha {
                from[3] = 255;
            }
            palette_strings.push(Replacement::new(from, new_to))
        } else {
            regular_strings.push(Replacement::new(from, new_to));
        }
    }

    palette_strings.sort_unstable();
    regular_strings.sort_unstable();

    let palette_result = if palette_strings.is_empty() {
        None
    } else {
        let mut palette_redirects: Vec<Replacement> = colors
            .into_iter()
            .filter_map(|(k, x)| match x {
                ColorEntry::PaletteColor(v) => Some((k, v)),
                _ => None,
            })
            .map(|(color, mut palette)| {
                if split_from_alpha {
                    palette[3] = 0;
                }
                Replacement::new(palette, color)
            })
            .collect();
        palette_redirects.sort_unstable();
        Some((
            join_replacements(&palette_strings),
            join_replacements(&palette_redirects),
        ))
    };

    (
        palette_result,
        if regular_strings.is_empty() {
            None
        } else {
            Some(join_replacements(&regular_strings))
        },
    )
}

pub fn create_replacer_string<F: Fn([u8; 4]) -> bool>(
    replacements: Vec<([u8; 4], [u8; 4])>,
    split_from_alpha: bool,
    palette_tester: F,
) -> String {
    let (palette, colors) = create_replacer(replacements, split_from_alpha, palette_tester);
    if let Some((initial, assign)) = palette {
        if let Some(colors) = colors {
            format!("?replace;{colors};{initial}?replace;{assign}")
        } else {
            format!("?replace;{initial}?replace;{assign}")
        }
    } else if let Some(colors) = colors {
        format!("?replace;{colors}")
    } else {
        String::new()
    }
}

pub struct GenerationResult {
    pub description: String,
    pub directives: String,
}

pub struct Input {
    pub filename: String,
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub type Inputs = IndexMap<String, (Input, u32)>;

// expands one row of packed palette indices, DEPTH bits each, most significant bits first
fn expand_row<const DEPTH: usize, const PER_BYTE: usize>(
    src: &[u8],
    dst: &mut [[u8; 4]],
    lut: &[[u8; 4]; 256],
) {
    let mask = ((1u16 << DEPTH) - 1) as u8;
    let pixel_of = |byte: u8, k: usize| lut[((byte >> (8 - DEPTH * (k + 1))) & mask) as usize];
    let (full, tail) = dst.as_chunks_mut::<PER_BYTE>();
    for (&byte, group) in src.iter().zip(full.iter_mut()) {
        for (k, pixel) in group.iter_mut().enumerate() {
            *pixel = pixel_of(byte, k);
        }
    }
    // the last byte of a row may hold fewer pixels than the others
    if let Some(&byte) = src.get(full.len()) {
        for (k, pixel) in tail.iter_mut().enumerate() {
            *pixel = pixel_of(byte, k);
        }
    }
}

// Indexed PNGs of up to 8 bits per pixel are expanded through a palette lookup here. The generic path in `image` takes
// several times as long for them. Anything else (or anything unusual) returns None, and `decode_rgba` falls back to `image`.
// Fully transparent palette entries are stored as [0; 4], see `decode_rgba`.
fn decode_indexed_png(data: &[u8]) -> Option<RgbaImage> {
    let mut decoder = png::Decoder::new(Cursor::new(data));
    decoder.set_transformations(png::Transformations::IDENTITY);
    let mut reader = decoder.read_info().ok()?;
    let info = reader.info();
    let (width, height) = (info.width, info.height);
    let palette = info.palette.as_deref()?;
    if info.interlaced || palette.len() % 3 != 0 || width == 0 || height == 0 {
        return None;
    }
    let (color_type, bit_depth) = reader.output_color_type();
    let depth = bit_depth as usize;
    if color_type != png::ColorType::Indexed || depth > 8 {
        return None;
    }

    // like the png crate does: a tRNS chunk with more entries than the palette is ignored, and unused entries are opaque black
    let trns = info.trns.as_deref().unwrap_or(&[]);
    let trns = if trns.len() <= palette.len() / 3 {
        trns
    } else {
        &[]
    };
    let mut lut = [[0, 0, 0, 255]; 256];
    for (i, (rgb, entry)) in palette.chunks_exact(3).zip(lut.iter_mut()).enumerate() {
        let alpha = trns.get(i).copied().unwrap_or(255);
        *entry = if alpha == 0 {
            [0; 4]
        } else {
            [rgb[0], rgb[1], rgb[2], alpha]
        };
    }

    let mut buffer = vec![0; reader.output_buffer_size()?];
    let line_size = reader.next_frame(&mut buffer).ok()?.line_size;
    let row_len = width as usize * 4;
    let mut pixels = vec![0u8; row_len * height as usize];
    for (src, dst) in buffer
        .chunks_exact(line_size)
        .zip(pixels.chunks_exact_mut(row_len))
    {
        let dst = dst.as_chunks_mut::<4>().0;
        match depth {
            1 => expand_row::<1, 8>(src, dst, &lut),
            2 => expand_row::<2, 4>(src, dst, &lut),
            4 => expand_row::<4, 2>(src, dst, &lut),
            _ => expand_row::<8, 1>(src, dst, &lut),
        }
    }
    RgbaImage::from_raw(width, height, pixels)
}

/// decodes an image to RGBA8, with every fully transparent pixel set to [0; 4]
pub fn decode_rgba(data: &[u8]) -> Result<RgbaImage, image::ImageError> {
    if let Some(image) = decode_indexed_png(data) {
        return Ok(image);
    }
    let mut buffer = image::load_from_memory(data)?.into_rgba8();
    for pixel in buffer.as_chunks_mut::<4>().0 {
        if pixel[3] == 0 {
            *pixel = [0; 4];
        }
    }
    Ok(buffer)
}

pub fn load_input(path: &Path) -> Result<Input> {
    let filename = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let data = std::fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let dimensions = image::ImageReader::new(Cursor::new(&data))
        .with_guessed_format()?
        .into_dimensions();
    let Ok((width, height)) = dimensions else {
        bail!("{filename} is not an image")
    };
    Ok(Input {
        filename,
        data,
        width,
        height,
    })
}

fn write_output(output: Option<PathBuf>, default: &str, content: String) -> Result<()> {
    match output {
        Some(path) if path.as_os_str() == "-" => println!("{content}"),
        path => {
            let path = path.unwrap_or_else(|| PathBuf::from(default));
            std::fs::write(&path, content)
                .with_context(|| format!("failed to write {}", path.display()))?;
            eprintln!("wrote {}", path.display());
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn do_generate_simple(input: &Input) -> Result<String> {
    let filename = &input.filename;
    let (width, height) = (input.width, input.height);
    if width > 256 || height > 256 {
        bail!("{filename} ({width}x{height}) is too big")
    } else if width == 0 || height == 0 {
        return Ok(format!("?crop=0;{width};0;{height}")); // yeah ok dude sure here
    }

    let Ok(decoded) = image::load_from_memory(&input.data) else {
        bail!("{filename} failed to decode")
    };
    let w = width as u8;
    let h = height as u8;

    let image = decoded.to_rgba8();
    let mut replacements: Vec<([u8; 4], [u8; 4])> = image
        .enumerate_pixels()
        .filter_map(|(x, y, pixel)| {
            if pixel[3] == 0 {
                None
            } else {
                Some((
                    [x as u8, h - 1 - y as u8, 0, 0],
                    pixel.channels().try_into().unwrap(),
                ))
            }
        })
        .collect();

    replacements.sort();

    let directives = format!(
        "?crop=0;0;1;1?multiply=0000?replace;0000=4444?border=1;ccc0;0000?scale=.8;.6?replace;9990={};4440={};8881={}?scale={};{}?crop=0;0;{w};{h}{}",
        color_to_hex([w, 0, 0, 0]),
        color_to_hex([0, h, 0, 0]),
        color_to_hex([w, h, 0, 0]),
        (w as f32) - 0.5,
        (h as f32) - 0.5,
        create_replacer_string(replacements, false, |_| true)
    );

    Ok(directives)
}

pub fn do_generate(
    attachments: &Inputs,
    frame_set: &FrameSet,
    id: &str,
) -> Result<GenerationResult> {
    let mut warning = None;
    let mut size_estimates: HashMap<String, (u32, u32)> = HashMap::default();
    for (sheet_name, (input, columns)) in attachments.iter() {
        let filename = &input.filename;
        let (width, height) = (input.width, input.height);
        if *columns != 0 {
            if width % columns != 0 {
                bail!("{filename} ({sheet_name}) does not have exactly {columns} columns")
            }
            let size_estimate: u32 = width / columns;
            if size_estimate > 128 {
                bail!("frame size in {filename} ({sheet_name}) is above the limit of 128x128")
            } else if size_estimate < 4 {
                bail!("frame size in {filename} ({sheet_name}) is too small (<= 4x4)")
            } else if size_estimate.is_multiple_of(2) {
                warning = Some(format!(
                    "frame size {size_estimate} is not an odd number, your sprite may be offset in-game"
                ));
            }
            let height_limit = size_estimate * 12;
            if height > height_limit {
                bail!(
                    "{filename} ({sheet_name}) sheet is too tall ({height}, limit is {height_limit})"
                )
            }
            size_estimates.insert(sheet_name.clone(), (width / columns, width / columns));
        } else {
            if width == 0 || height == 0 {
                bail!("{filename} has no pixels");
            } else if width > 256 || height > 256 {
                bail!("frame size in {filename} is above the limit of 256x256");
            } else if width <= 2 && height <= 2 {
                warning = Some("generator is not optimized for such small sprites".to_string());
            }
            size_estimates.insert(sheet_name.clone(), (width, height));
        }
    }
    if size_estimates.is_empty() {
        bail!("no inputs were provided")
    } else if size_estimates
        .values()
        .any(|v| v != size_estimates.values().next().unwrap())
    {
        bail!("mismatching frame sizes: {size_estimates:?}")
    }
    let (width, height) = *size_estimates.values().next().unwrap();
    struct Image {
        buffer: RgbaImage,
        frames: u16,
    }

    let mut images: IndexMap<String, Image> = IndexMap::new();
    for (name, (input, _)) in attachments.iter() {
        let Ok(buffer) = decode_rgba(&input.data) else {
            bail!("failed to decode {name}")
        };
        images.insert(
            name.clone(),
            Image {
                buffer,
                frames: 0u16,
            },
        );
    }

    let width_u8: u8 = (width.min(255)) as u8;
    let height_u8: u8 = (height.min(255)) as u8;
    let frame_begin = 128 - (width.div_ceil(2) as u8);
    let entries = &frame_set.entries;

    let began = Instant::now();
    // Vec(row_id, left), min_start, max_start

    #[derive(Debug, Clone, Copy)]
    enum RowKind {
        Alone,
        Pointed,
        Pointer,
        Sliding,
    }

    struct Row {
        entries: Vec<(RowId, u8, RowKind)>,     // Row ID, start, kind
        offsets: IndexMap<u8, u32, FixedState>, // start, entries index
        min_start: u32,
        max_start: u32,
    }

    let mut rows: IndexMap<RowKey, Row, FixedState> = IndexMap::default();
    let mut frames: HashMap<u64, RowId> = HashMap::default();

    let mut legacy_len: u32 = 0;

    let row_from_color = |color: [u8; 4]| -> u16 { (color[0] as u16) << 8 | color[1] as u16 };

    let row_to_color =
        |row: u16, off: u8| -> [u8; 4] { [(row >> 8) as u8, (row & 0xFF) as u8, off, 0] };

    let row_to_color_opaque =
        |row: u16, off: u8| -> [u8; 4] { [(row >> 8) as u8, (row & 0xFF) as u8, off, 255] };
    let mut base_replace: Vec<([u8; 4], [u8; 4])> = vec![];
    let mut base_rows: HashSet<RowId> = HashSet::default();

    for entry in entries.iter() {
        let image = {
            let image = images.get_mut(entry.image.as_str());
            if image.is_none() {
                continue;
            }
            image.unwrap()
        };
        let [x, y] = entry.pos;
        let frame = if image
            .buffer
            .in_bounds(((x + 1) * width) - 1, ((y + 1) * height) - 1)
        {
            Some(image.buffer.view(x * width, y * height, width, height))
        } else {
            None
        };
        if frame.is_none() || frame_is_empty(&frame.unwrap()) {
            // Don't bother if this is an empty frame
            let mut remove_color = |color: &[u8; 4]| {
                if color[3] > 0 {
                    base_replace.push((*color, [255, 255, 255, 0]))
                }
            };
            remove_color(&entry.lower_left);
            remove_color(&entry.lower_right);
            remove_color(&entry.upper_left);
            remove_color(&entry.upper_right);
        } else if let Some(frame) = frame {
            image.frames += 1;

            let mut this_lowest = height;
            let mut this_highest = 0;
            let mut frame_rows = Vec::new();

            let mut frame_legacy_len: u32 = 0;
            for y in 0..height {
                let row_pixels: &[[u8; 4]] =
                    frame_row_bytes(&frame, height - y - 1).as_chunks::<4>().0;
                let (mut start, mut end) = (width, 0);
                let mut solid = false;
                for (x, pixel) in (0u32..).zip(row_pixels) {
                    if pixel[3] > 0 {
                        solid = true;
                        frame_legacy_len += 10 + color_hex_len(*pixel) as u32;
                        if start == width {
                            start = x;
                        }
                        end = x;
                    }
                }
                if solid {
                    if this_lowest == height {
                        this_lowest = y;
                    }
                    if y > this_highest {
                        this_highest = y;
                    }
                    let row_vec = row_pixels[start as usize..=end as usize].to_vec();
                    frame_rows.push((row_vec, start, y));
                }
            }
            legacy_len += frame_legacy_len;

            let crop_height = (this_highest - this_lowest) + 1;
            let cropped_frame =
                frame.view(0, height - this_lowest - crop_height, width, crop_height);
            let frame_hash = hash_image(cropped_frame);

            let frame_column_begin = 128 - crop_height.div_ceil(2) as u16;
            let unique_frame_count = frames.len() as u16;
            let start_row = frames.entry(frame_hash).or_insert_with(|| {
                let start_row: u16 = (unique_frame_count * 255) + frame_column_begin;
                for (row_vec, start, y) in frame_rows.into_iter() {
                    let y_from_lowest = y - this_lowest;
                    let this_row = start_row + y_from_lowest as u16;
                    let sliding_begin = width.div_ceil(2) as u8 - (row_vec.len().div_ceil(2) as u8);
                    let row = rows.entry(RowKey(row_vec)).or_insert_with(|| Row {
                        entries: vec![(0, sliding_begin, RowKind::Sliding)],
                        offsets: IndexMap::default(),
                        min_start: u32::MAX,
                        max_start: 0,
                    });
                    row.offsets
                        .entry(start as u8)
                        .or_insert(row.entries.len() as u32);
                    row.entries.push((this_row, start as u8, RowKind::Alone));
                    if row.min_start > start {
                        row.min_start = row.entries.len() as u32;
                    }
                    if row.max_start < start {
                        row.max_start = entries.len() as u32;
                    }
                }
                start_row
            });

            let this_start_row = (*start_row as i32 - this_lowest as i32) as RowId;
            let this_end_row = this_start_row + height_u8 as RowId;
            base_rows.extend(this_start_row..this_end_row);
            if height_u8 == 255 {
                base_rows.insert(this_end_row);
            }

            base_replace.extend([
                (
                    entry.lower_left,
                    row_to_color_opaque(this_start_row, frame_begin),
                ),
                (
                    entry.lower_right,
                    row_to_color_opaque(this_start_row, frame_begin + width_u8),
                ),
                (
                    entry.upper_left,
                    row_to_color_opaque(this_end_row, frame_begin),
                ),
                (
                    entry.upper_right,
                    row_to_color_opaque(this_end_row, frame_begin + width_u8),
                ),
            ]);
        }
    }

    let frameless = images.values().filter(|i| i.frames == 0).count();
    if frameless > 0 && frameless != images.len() {
        let (a, b) = if frameless == 1 {
            (" is", "at")
        } else {
            ("s are", "ose")
        };
        warning = Some(format!(
            "{frameless} image{a} empty, what a waste of time! simply do not include th{b}"
        ));
    }

    let mut next_compact_row: u16 = 0;
    let mut reserve_sliding_row = |taken: &HashSet<u16>| -> Option<u16> {
        loop {
            if next_compact_row == 256 {
                return None;
            } else {
                let id = next_compact_row;
                next_compact_row += 1;
                let compacted = compact_row(255 - id);
                if !taken.contains(&compacted) {
                    return Some(compacted);
                }
            }
        }
    };

    let mut row_pixels: HashMap<RowId, (&[[u8; 4]], u8)> = HashMap::default();
    let mut row_replace: Vec<([u8; 4], [u8; 4])> = vec![];
    let total_rows: usize = rows.values().map(|row| row.entries.len()).sum();
    let effective_rows: usize = rows.len();

    let mut regular_starts: HashMap<u8, (u32, u8, u8)> = HashMap::default(); // start, row id
    let mut sliding_starts: HashMap<u8, (u32, u8, u8)> = HashMap::default();
    for (pixels, row_set) in rows.iter_mut() {
        let pixels = &pixels.0;
        let effective_entries = row_set.entries.len() - 1; // there is currently always 1 sliding row
        // if it is not worth it, then simply insert it without deduplication
        if pixels.len() * effective_entries <= (effective_entries * 2) + pixels.len() {
            for (row, start, kind) in row_set.entries.iter() {
                if let RowKind::Sliding = kind {
                    continue;
                }
                row_pixels.insert(*row, (pixels.as_slice(), frame_begin + start));
            }
        } else {
            let sweet_spot = (width.div_ceil(2)) as i16 - (pixels.len().div_ceil(2) as i16);
            row_set.offsets.sort_by(|start_a, _, start_b, _| {
                let off_a = *start_a as i16 - sweet_spot;
                let off_b = *start_b as i16 - sweet_spot;
                let score_a = if off_a > 0 { 2 * off_a - 1 } else { -2 * off_a };
                let score_b = if off_b > 0 { 2 * off_b - 1 } else { -2 * off_b };
                score_a.cmp(&score_b)
            });
            regular_starts.clear();
            sliding_starts.clear();

            for this_start in row_set.offsets.keys().cloned() {
                let mut referenced: Option<(u32, u8, u8)> = None;
                let mut referenced_slider: Option<(u32, u8, u8)> = None;
                // find a candidate to slide from - firstly from already inserted
                //*
                let test = |index: u32, start: u8, output: &mut Option<(u32, u8, u8)>| {
                    let diff = start as i16 - this_start as i16;
                    let begin = frame_begin as i16 + diff;
                    if begin >= 0 && (begin + width_u8 as i16) < 256 {
                        *output = Some((index, begin as u8, start));
                        true
                    } else {
                        false
                    }
                };

                for (index, _, start) in regular_starts.values() {
                    if test(*index, *start, &mut referenced) {
                        break;
                    }
                }
                for (index, _, start) in sliding_starts.values() {
                    if test(*index, *start, &mut referenced_slider) {
                        break;
                    }
                }
                //*/
                if referenced.is_none() || referenced_slider.is_none() {
                    for (index, (_, start, kind)) in row_set.entries.iter().enumerate() {
                        let diff = *start as i16 - this_start as i16;
                        let begin = frame_begin as i16 + diff;
                        if begin >= 0 && (begin + width_u8 as i16) < 256 {
                            if matches!(kind, RowKind::Sliding) && referenced_slider.is_none() {
                                referenced_slider = Some((index as u32, begin as u8, *start));
                            } else if !matches!(kind, RowKind::Sliding) && referenced.is_none() {
                                referenced = Some((index as u32, begin as u8, *start));
                            }
                        }
                        if referenced_slider.is_some() && referenced.is_some() {
                            break;
                        }
                    }
                }

                if let Some(ref_row) = referenced {
                    regular_starts.insert(this_start, ref_row);
                }

                if let Some(ref_row) = referenced_slider {
                    sliding_starts.insert(this_start, ref_row);
                }
            }

            let regular_indexes = regular_starts
                .values()
                .map(|(index, _, _)| *index)
                .collect::<HashSet<u32>>();
            let mut sliding_indexes = sliding_starts
                .values()
                .map(|(index, _, _)| *index)
                .collect::<HashSet<u32>>();

            let mut use_sliding = sliding_indexes.len() < regular_indexes.len();

            // we need to assign sliding rows now, and then decide priorities again in case any fail
            for this_start in row_set.offsets.keys() {
                if !use_sliding && regular_starts.contains_key(this_start) {
                    continue;
                }
                if let Some(index) = sliding_starts.get(this_start).map(|(index, _, _)| *index) {
                    if !sliding_indexes.contains(&index) {
                        sliding_starts.remove(this_start);
                        continue;
                    } else {
                        let entry = row_set.entries.get_mut(index as usize).unwrap();
                        if entry.0 == 0 {
                            if let Some(reserved) = reserve_sliding_row(&base_rows) {
                                base_rows.insert(reserved);
                                entry.0 = reserved;
                            } else {
                                sliding_indexes.remove(&index);
                                sliding_starts.remove(this_start);
                            }
                        }
                    }
                }
            }

            use_sliding = sliding_indexes.len() < regular_indexes.len();

            let len = row_set.entries.len();
            for i in 0..len {
                let (this_row, this_start, this_kind) = *row_set.entries.get(i).unwrap();
                if matches!(this_kind, RowKind::Sliding) || matches!(this_kind, RowKind::Pointed) {
                    continue;
                }
                //let res = { let i = i as u32; regular_indexes.contains(&i) }; if res { continue; } broken

                let reference = if use_sliding {
                    sliding_starts
                        .get(&this_start)
                        .or_else(|| regular_starts.get(&this_start))
                } else {
                    regular_starts
                        .get(&this_start)
                        .or_else(|| sliding_starts.get(&this_start))
                }
                .cloned();

                if let Some((ref_index, ref_offset, _)) = reference {
                    if ref_index as usize == i {
                        continue;
                    }
                    row_set.entries.get_mut(i).unwrap().2 = RowKind::Pointer;
                    let ref_entry = row_set.entries.get_mut(ref_index as usize).unwrap();
                    ref_entry.2 = RowKind::Pointed;
                    base_rows.remove(&this_row);
                    let left = row_to_color_opaque(this_row, frame_begin);
                    let right = row_to_color_opaque(this_row, frame_begin + width_u8);
                    let left_to = row_to_color_opaque(ref_entry.0, ref_offset);
                    let right_to = row_to_color_opaque(ref_entry.0, ref_offset + width_u8);
                    row_replace.extend([(left, left_to), (right, right_to)]);
                }
            }

            for i in 0..len {
                let (this_row, this_start, this_kind) = row_set.entries.get(i).unwrap();
                if matches!(this_kind, RowKind::Pointed) {
                    row_pixels.insert(*this_row, (pixels.as_slice(), frame_begin + this_start));
                }
            }

            /*
            let (ref_row, ref_start) = row.entries.first().unwrap().to_owned();
            let mut new_row = None;
            let sliding = row.entries.iter().any(|(_, start)| *start != ref_start);
            let row_begin = 128 - (pixels.len().div_ceil(2) as u8);

            row.entries.into_iter().for_each(|(row, start)| {
                let this_can_slide = start <= row_begin
                    && ((row_begin - start) as i16 + width_u8 as i16) < 255;
                let off = if sliding && this_can_slide {row_begin - start} else {frame_begin};
                if !this_can_slide {
                    row_pixels.insert(row, (pixels.as_slice(), frame_begin + start));
                } else if row != ref_row || (sliding && start != off) {
                    base_rows.remove(&row);
                    let target_row = *new_row.get_or_insert_with(|| {
                        let mut new_row = ref_row;
                        if row == ref_row {
                            if let Some(reserved_row) = reserve_sliding_row(&base_rows) {
                                new_row = reserved_row;
                            }
                        }
                        new_row
                    });
                    let left = row_to_color_opaque(row, frame_begin);
                    let right = row_to_color_opaque(row, frame_begin + width_u8);
                    let left_to = row_to_color_opaque(target_row, off);
                    let right_to = row_to_color_opaque(target_row, off + width_u8);
                    row_replace.extend([(left, left_to), (right, right_to)]);
                };
            });
            if let Some(row) = new_row {
                row_pixels.insert(row, (pixels, if sliding {row_begin} else {frame_begin + ref_start}));
            }
            */
        }
    }

    let mut color_replace: Vec<([u8; 4], [u8; 4])> = row_pixels
        .into_iter()
        .flat_map(move |(row, (colors, start))| {
            colors
                .iter()
                .copied()
                .enumerate()
                .filter_map(move |(x, color)| {
                    if color[3] > 0 {
                        Some((row_to_color(row, start + x as u8), color))
                    } else {
                        None
                    }
                })
        })
        .collect();

    let color_count = color_replace.len();
    if color_count > 75000 {
        bail!("too many color entries ({color_count} > 75000)")
    }

    sort_replacements(&mut row_replace);
    sort_replacements(&mut color_replace);

    let scale_value = |size: u32| SCALE_FIXERS.get(size as usize - 1).unwrap();

    let min_x = if width > 255 { 1 } else { 0 };
    let min_y = if height > 255 { 1 } else { 0 };
    let max_x = if width > 255 { width + 1 } else { width };
    let max_y = if height > 255 { height + 1 } else { height };

    //*
    let (palette, colors) = create_replacer(color_replace, true, |color| {
        !base_rows.contains(&row_from_color(color))
    });
    let initial_replace = if let Some((ref initial, _)) = palette {
        format!("?replace;{initial}")
    } else {
        String::new()
    };

    let final_replace = if let Some((_, assign)) = palette {
        if let Some(colors) = colors {
            format!("?replace;{assign};{colors}")
        } else {
            format!("?replace;{assign}")
        }
    } else if let Some(colors) = colors {
        format!("?replace;{colors}")
    } else {
        String::new()
    };
    let directives = format!(
        "{}{}?scale=1;{}?crop=0;{min_y};2;{max_y}{}?scale={};1?crop={min_x};0;{max_x};{height}{initial_replace}?multiply=fff0{final_replace}",
        frame_set.prefix,
        create_replacer_string(base_replace, false, |_| true),
        scale_value(height),
        create_replacer_string(row_replace, false, |color: [u8; 4]| !base_rows
            .contains(&row_from_color(color))
            || (color[2] != 0 && color[2] != width_u8)),
        scale_value(width)
    );

    let elapsed = Instant::now() - began;

    let length = directives.len();
    let score = (((1. - (effective_rows as f32 / total_rows as f32)) * 100.) + 0.5).round();
    let legacy_prefix_len: u32 = match id {
        "pants" => 842,
        "frog_pants" => 883,
        "chest" => 1562,
        "back" => 1027,
        _ => 0,
    };

    let millis = {
        let micros = elapsed.as_micros() as u32;
        format_compact!("{}.{:03}", micros / 1000, micros % 1000)
    };
    let mut description = format!(
        "generated!\ndirectives length: {length} chars, score: {score}%, time: {millis} ms"
    );
    if legacy_prefix_len != 0 {
        // comparison against https://silverfeelin.github.io/Starbound-NgOutfitGenerator/
        let silver_length = legacy_len + legacy_prefix_len;
        let efficiency = (((1. - (length as f32 / silver_length as f32)) * 100.) + 0.5).round();
        write!(
            description,
            "\nold method: {silver_length} chars, savings: {efficiency}%"
        )
        .unwrap();
    }
    if let Some(warning) = warning {
        write!(description, "\n!! {warning} !!").unwrap();
    }
    Ok(GenerationResult {
        description,
        directives,
    })
}

#[derive(Clone, Copy, ValueEnum)]
pub enum SpriteOutputType {
    Raw,
    Hat,
    Object,
    FloatingObject,
}

pub fn generate_sprite(
    image: &Path,
    output_type: SpriteOutputType,
    output: Option<PathBuf>,
) -> Result<()> {
    let image = load_input(image)?;
    let name = image.filename.trim_end_matches(".png").to_string();
    let mut attachments = Inputs::new();
    attachments.insert("image".to_string(), (image, 0));
    let result = do_generate(&attachments, &SINGLE, "image")?;
    let directives = &result.directives;
    eprintln!("{}", result.description);
    let content = match output_type {
        SpriteOutputType::Raw => result.directives,
        SpriteOutputType::Hat => to_string_pretty(&json!({
            "name" : "eyepatchhead",
            "count" : 1,
            "parameters" : {
                "shortdescription" : "Custom Hat",
                "description" : "Generated Hat",
                "tooltipKind" : "object",
                "inventoryIcon" : "head.png:normal",
                "directives" : directives
            }
        }))
        .unwrap(),
        SpriteOutputType::Object => to_string_pretty(&json!({
            "name" : "perfectlygenericitem",
            "count" : 1,
            "parameters" : {
                "shortdescription" : "Custom Object",
                "description" : "Generated Object",
                "inventoryIcon" : format!("/assetmissing.png{directives}"),
                "color" : format!("default{directives};")
            }
        }))
        .unwrap(),
        SpriteOutputType::FloatingObject => {
            eprintln!("(floating objects are not flippable)");
            to_string_pretty(&json!({
                "name" : "protectoratebooks",
                "count" : 1,
                "parameters" : {
                    "shortdescription" : "Custom Object",
                    "description" : "Generated Object",
                    "inventoryIcon" : format!("/assetmissing.png{directives}"),
                    "color" : format!("default{directives};")
                }
            }))
            .unwrap()
        }
    };
    write_output(output, &format!("{name}.txt"), content)
}

pub fn generate_shirt(
    chest_female: Option<PathBuf>,
    chest_male: Option<PathBuf>,
    front_sleeve: Option<PathBuf>,
    back_sleeve: Option<PathBuf>,
    output: Option<PathBuf>,
) -> Result<()> {
    let mut attachments = Inputs::new();
    if let Some(x) = chest_female {
        attachments.insert("chestf".to_string(), (load_input(&x)?, 2));
    }
    if let Some(x) = chest_male {
        attachments.insert("chestm".to_string(), (load_input(&x)?, 2));
    }
    if let Some(x) = front_sleeve {
        attachments.insert("fsleeve".to_string(), (load_input(&x)?, 9));
    }
    if let Some(x) = back_sleeve {
        attachments.insert("bsleeve".to_string(), (load_input(&x)?, 9));
    }
    let result = do_generate(&attachments, &CHEST, "chest")?;
    let mut item = ITEM_TEMPLATE.clone();
    merge_json(
        &mut item,
        json!({
            "name" : "aviantier6schest",
            "parameters" : {
                "shortdescription" : "Custom Shirt",
                "inventoryIcon" : format!("chest{}.png:idle.1", if attachments.contains_key("chestf") {"f"} else {"m"}),
                "directives" : result.directives
            }
        }),
    );
    eprintln!("{}", result.description);
    write_output(
        output,
        "custom_shirt.json",
        to_string_pretty(&item).unwrap(),
    )
}

fn tail_advice(str: &mut String) {
    let tail_advice = "if there is a tail, wings or other back accessory, consider moving it to a back item for the sake of optimization";
    write!(str, "\n({tail_advice})").unwrap();
}

pub fn generate_pants(pants: &Path, hide_body: bool, output: Option<PathBuf>) -> Result<()> {
    let (frame_set, id) = if hide_body {
        (&FROG_PANTS, "frog_pants")
    } else {
        (&PANTS, "pants")
    };
    let mut attachments = Inputs::new();
    attachments.insert("pants".to_string(), (load_input(pants)?, 9));
    let mut result = do_generate(&attachments, frame_set, id)?;
    let mut item = ITEM_TEMPLATE.clone();
    merge_json(
        &mut item,
        json!({
            "name" : if hide_body {"froggmerchantlegs"} else {"florantier6apants"},
            "parameters" : {
                "shortdescription" : "Custom Pants",
                "inventoryIcon" : if hide_body {"pantsM.png:idle.1"} else {"pants.png:idle.1"},
                "directives" : result.directives
            }
        }),
    );
    tail_advice(&mut result.description);
    eprintln!("{}", result.description);
    write_output(
        output,
        "custom_pants.json",
        to_string_pretty(&item).unwrap(),
    )
}

pub fn generate_back(back: &Path, output: Option<PathBuf>) -> Result<()> {
    let mut attachments = Inputs::new();
    attachments.insert("back".to_string(), (load_input(back)?, 9));
    let result = do_generate(&attachments, &BACK, "back")?;
    let mut item = ITEM_TEMPLATE.clone();
    merge_json(
        &mut item,
        json!({
            "name" : "tigertailback",
            "parameters" : {
                "shortdescription" : "Custom Back",
                "inventoryIcon" : "back.png:idle.1",
                "directives" : result.directives
            }
        }),
    );
    eprintln!("{}", result.description);
    write_output(output, "custom_back.json", to_string_pretty(&item).unwrap())
}

#[derive(Clone, Copy, ValueEnum, PartialOrd, PartialEq)]
pub enum BodyType {
    Male,
    Female,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Species {
    Apex,
    Avian,
    Floran,
    Glitch,
    Human,
    Hylotl,
    Novakid,
    Other,
}

pub fn generate_humanoid(
    body_type: BodyType,
    species: Species,
    body: Option<PathBuf>,
    head: Option<PathBuf>,
    front_arm: Option<PathBuf>,
    back_arm: Option<PathBuf>,
    output: Option<PathBuf>,
) -> Result<()> {
    let mut attachments = Inputs::new();
    let (frame_set, body_name, head_name) = match body_type {
        BodyType::Male => (
            match species {
                Species::Glitch => &MALE_GLITCH,
                Species::Novakid => &MALE_NOVAKID,
                Species::Hylotl => &MALE_HYLOTL,
                _ => &MALE,
            },
            "malebody",
            "malehead",
        ),
        BodyType::Female => (
            match species {
                Species::Glitch => &FEMALE_GLITCH,
                Species::Novakid => &FEMALE_NOVAKID,
                _ => &FEMALE,
            },
            "femalebody",
            "femalehead",
        ),
    };
    if let Some(x) = body {
        attachments.insert(body_name.to_string(), (load_input(&x)?, 9));
    }
    if let Some(x) = head {
        attachments.insert(head_name.to_string(), (load_input(&x)?, 1));
    }
    if let Some(x) = front_arm {
        attachments.insert("frontarm".to_string(), (load_input(&x)?, 9));
    }
    if let Some(x) = back_arm {
        attachments.insert("backarm".to_string(), (load_input(&x)?, 9));
    }
    let mut result = do_generate(&attachments, frame_set, "humanoid")?;
    tail_advice(&mut result.description);
    eprintln!("{}", result.description);
    write_output(output, "body_directives.txt", result.directives)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn reference(data: &[u8]) -> RgbaImage {
        let mut image = image::load_from_memory(data).unwrap().into_rgba8();
        for pixel in image.pixels_mut() {
            if pixel[3] == 0 {
                *pixel = Rgba([0; 4]);
            }
        }
        image
    }

    fn encode(
        width: u32,
        height: u32,
        depth: png::BitDepth,
        color: png::ColorType,
        palette: Option<Vec<u8>>,
        trns: Option<Vec<u8>>,
        data: &[u8],
    ) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(color);
        encoder.set_depth(depth);
        if let Some(palette) = palette {
            encoder.set_palette(palette);
        }
        if let Some(trns) = trns {
            encoder.set_trns(trns);
        }
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(data).unwrap();
        writer.finish().unwrap();
        out
    }

    #[test]
    fn templates_decode_like_image() {
        for template in [
            &include_bytes!("templates/malebody.png")[..],
            &include_bytes!("templates/femalebody.png")[..],
            &include_bytes!("templates/malebody_greyscale.png")[..],
            &include_bytes!("templates/femalebody_greyscale.png")[..],
        ] {
            assert!(decode_indexed_png(template).is_some());
            assert_eq!(decode_rgba(template).unwrap(), reference(template));
        }
    }

    #[test]
    fn indexed_decode_like_image() {
        let mut seed = 0x2545F491u32;
        let mut next = move || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 16) as u8
        };
        for (depth, bits) in [
            (png::BitDepth::One, 1),
            (png::BitDepth::Two, 2),
            (png::BitDepth::Four, 4),
            (png::BitDepth::Eight, 8),
        ] {
            for width in [1u32, 2, 3, 5, 8, 13, 31] {
                // palettes that are shorter than the index range, with tRNS that is shorter, as long as, or longer than the palette
                for (palette_len, trns_len) in [
                    (2usize, 0usize),
                    (2, 1),
                    (2, 2),
                    (2, 3),
                    (3, 2),
                    (16, 9),
                    (200, 100),
                    (256, 256),
                ] {
                    if palette_len > 1 << bits {
                        continue;
                    }
                    let height = 7;
                    let row_bytes = (width as usize * bits).div_ceil(8);
                    let data: Vec<u8> = (0..row_bytes * height).map(|_| next()).collect();
                    let palette: Vec<u8> = (0..palette_len * 3).map(|_| next()).collect();
                    let trns: Vec<u8> = (0..trns_len)
                        .map(|i| if i % 3 == 0 { 0 } else { next() })
                        .collect();
                    let png = encode(
                        width,
                        height as u32,
                        depth,
                        png::ColorType::Indexed,
                        Some(palette),
                        (trns_len > 0).then_some(trns),
                        &data,
                    );
                    let fast = decode_indexed_png(&png)
                        .unwrap_or_else(|| panic!("no fast path: depth {bits}, width {width}"));
                    assert_eq!(
                        fast,
                        reference(&png),
                        "depth {bits}, width {width}, palette {palette_len}, trns {trns_len}"
                    );
                }
            }
        }
    }

    #[test]
    fn other_images_use_the_fallback() {
        let rgba = encode(
            2,
            2,
            png::BitDepth::Eight,
            png::ColorType::Rgba,
            None,
            None,
            &[0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 255, 0, 9, 9, 0],
        );
        assert!(decode_indexed_png(&rgba).is_none());
        let decoded = decode_rgba(&rgba).unwrap();
        assert_eq!(decoded, reference(&rgba));
        assert_eq!(decoded.get_pixel(1, 1), &Rgba([0; 4]));
    }
}
