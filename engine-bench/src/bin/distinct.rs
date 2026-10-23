//! The test that the template cannot give: a custom sheet in which every cell is different from every other (the vanilla silhouettes,
//! with the color of each pixel made from the cell and the position), so that a frame cannot come out right by drawing another cell that
//! looks the same. Prints for every frame which cell of that sheet the engine draws.
use engine_bench::{
    engine::{Directives, Image},
    harness::{self, Assets},
};
use stargen::generator::{self, Inputs};

fn main() -> anyhow::Result<()> {
    let case_name = std::env::args().nth(1).unwrap_or("male_body".into());
    let assets = Assets::open_default()?;
    let mut case = harness::humanoid_cases()
        .into_iter()
        .find(|case| case.name == case_name)
        .expect("no such case");
    let part = &mut case.parts[0];

    // vanilla silhouettes, colors from the cell and the position
    let vanilla = Image::decode(&assets.read(part.vanilla)?)?;
    let size = vanilla.width / part.columns;
    let mut sheet = image::RgbaImage::new(vanilla.width, vanilla.height);
    for (x, y, pixel) in sheet.enumerate_pixels_mut() {
        let source = vanilla.pixel(x, y);
        if source[3] != 0 {
            let cell = (y / size) * part.columns + x / size;
            *pixel = image::Rgba([
                (cell * 23 + 5) as u8 | 1,
                ((x % size) * 5 + 3) as u8,
                ((y % size) * 5 + 7) as u8,
                255,
            ]);
        }
    }
    let path = std::env::temp_dir().join("stargen-distinct-sheet.png");
    sheet.save(&path)?;
    part.custom = path;

    let mut inputs = Inputs::new();
    inputs.insert(
        part.key.to_string(),
        (generator::load_input(&part.custom)?, part.columns),
    );
    let directives = generator::do_generate(&inputs, case.frame_set, case.id)?.directives;
    let mut engine = Directives::new(&directives)?;
    let frames = case.frames(&assets)?;
    let cell_of = |rgba: &[u8]| {
        rgba.chunks_exact(4)
            .find(|p| p[3] != 0)
            .map(|p| p[0] as u32)
    };
    let mut right = 0;
    for frame in &frames {
        let got = engine.apply(&frame.base)?;
        let hits: Vec<&str> = frames
            .iter()
            .filter(|other| {
                got.rgba
                    .chunks_exact(4)
                    .zip(other.expected.rgba.chunks_exact(4))
                    .all(|(g, w)| (g[3] == 0 && w[3] == 0) || g == w)
            })
            .map(|other| other.name.as_str())
            .collect();
        let expected_cell = cell_of(&frame.expected.rgba).map_or("-".to_string(), |r| {
            format!(
                "{}",
                (0..54u32)
                    .find(|c| ((c * 23 + 5) as u8 | 1) as u32 == r)
                    .map_or("?".to_string(), |c| format!(
                        "cell {} ({},{})",
                        c,
                        c % 9,
                        c / 9
                    ))
            )
        });
        let note = match (cell_of(&got.rgba), hits.as_slice()) {
            (None, _) => "empty".to_string(),
            (Some(_), [only]) if *only == frame.name => {
                right += 1;
                "own cell".to_string()
            }
            (Some(_), []) => "a mix of cells, or part of one".to_string(),
            (Some(_), hits) => format!("the cell of {}", hits.join(" ")),
        };
        let drawn = cell_of(&got.rgba).map_or("-".to_string(), |r| {
            format!(
                "{}",
                (0..54u32)
                    .find(|c| ((c * 23 + 5) as u8 | 1) as u32 == r)
                    .map_or("?".to_string(), |c| format!(
                        "cell {} ({},{})",
                        c,
                        c % 9,
                        c / 9
                    ))
            )
        });
        println!(
            "{:22} should draw {expected_cell:14} draws {drawn:16} {note}",
            frame.name
        );
    }
    println!("{right} of {} frames draw their own cell", frames.len());
    Ok(())
}
