//! Lists every frame of a case with how many pixels the engine got wrong
use engine_bench::{
    engine::Directives,
    harness::{self, Assets, Mode},
};

fn main() -> anyhow::Result<()> {
    let case_name = std::env::args().nth(1).unwrap_or("male_body".into());
    let assets = Assets::open_default()?;
    let case = harness::humanoid_cases()
        .into_iter()
        .find(|case| case.name == case_name)
        .expect("no such case");
    let mut engine = Directives::new(&case.directives()?)?;
    for frame in case.frames(&assets)? {
        let report = harness::verify(&mut engine, std::slice::from_ref(&frame), Mode::Engine)?;
        let visible = frame
            .expected
            .rgba
            .chunks_exact(4)
            .filter(|p| p[3] != 0)
            .count();
        let base_visible = frame
            .base
            .rgba
            .chunks_exact(4)
            .filter(|p| p[3] != 0)
            .count();
        println!(
            "{:22} bad {:4} of {:4} expected visible, vanilla has {:4} visible",
            frame.name, report.bad_pixels, visible, base_visible
        );
    }
    Ok(())
}
