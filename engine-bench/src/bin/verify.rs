//! Checks that the directives the generator makes turn the vanilla sprites into the custom ones, in the real engine.
use engine_bench::{
    engine::Directives,
    harness::{self, Assets, Mode},
};
use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let assets = Assets::open_default()?;
    let mut failed = false;
    for case in harness::humanoid_cases() {
        let directives = case.directives()?;
        let frames = case.frames(&assets)?;
        let began = Instant::now();
        let mut engine = Directives::new(&directives)?;
        let operations = engine.load()?;
        let parse = began.elapsed();
        println!(
            "{}: {} chars, {operations} operations, parsed in {:.2?}",
            case.name,
            directives.len(),
            parse
        );
        for mode in [Mode::Engine, Mode::Sequential] {
            let report = harness::verify(&mut engine, &frames, mode)?;
            println!(
                "  {mode:?}: {} frames ({} empty): {} exact, {} wrong size, {} bad pixels, largest channel difference {}",
                report.frames,
                report.empty_frames,
                report.exact_frames,
                report.wrong_size_frames,
                report.bad_pixels,
                report.max_channel_diff
            );
            for (frame, bad) in &report.worst {
                println!("    {frame}: {bad} bad pixels");
            }
            failed |= mode == Mode::Engine && report.exact_frames != report.frames;
        }
    }
    if failed {
        std::process::exit(1);
    }
    Ok(())
}
