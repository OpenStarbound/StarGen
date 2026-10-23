//! Prints how one frame compares: `.` transparent in both, `#` same, `X` different, `e` only expected, `g` only the engine's
use engine_bench::{engine::Directives, harness::{self, Assets}};

fn main() -> anyhow::Result<()> {
    let (case_name, frame_name) = (std::env::args().nth(1).unwrap_or("male_body".into()), std::env::args().nth(2).unwrap_or("malebody:idle.3".into()));
    let assets = Assets::open_default()?;
    let case = harness::humanoid_cases().into_iter().find(|case| case.name == case_name).expect("no such case");
    let mut engine = Directives::new(&case.directives()?)?;
    let frame = case.frames(&assets)?.into_iter().find(|frame| frame.name == frame_name).expect("no such frame");
    let got = engine.apply(&frame.base)?;
    println!("{}x{}", got.width, got.height);
    for y in 0..got.height {
        let row: String = (0..got.width).map(|x| {
            let (g, e) = (got.pixel(x, y), frame.expected.pixel(x, y));
            match (g[3] == 0, e[3] == 0) {
                (true, true) => '.',
                (true, false) => 'e',
                (false, true) => 'g',
                _ => if g == e { '#' } else { 'X' },
            }
        }).collect();
        println!("{row}");
    }
    Ok(())
}
