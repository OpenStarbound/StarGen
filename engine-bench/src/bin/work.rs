//! A fixed amount of work, to count instructions with `perf stat` (wall clock times are noisy on this machine):
//! `work <case> parse|apply|sheet|upto<N> <iterations>`
use engine_bench::{
    engine::Directives,
    harness::{self, Assets},
};
use std::hint::black_box;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (case_name, what, iterations) = (&args[1], &args[2], args[3].parse::<u32>()?);
    let assets = Assets::open_default()?;
    let case = harness::humanoid_cases()
        .into_iter()
        .find(|case| &case.name == case_name)
        .expect("no such case");
    let text = case.directives()?;
    let mut directives = Directives::new(&text)?;
    directives.load()?;
    let frames = harness::exact_frames(&mut directives, case.frames(&assets)?)?;
    let frames: Vec<_> = frames
        .into_iter()
        .filter(|frame| frame.expected.rgba.chunks_exact(4).any(|p| p[3] != 0))
        .collect();

    // the setup above is the same for everything, so only the difference between runs of different lengths counts
    for _ in 0..iterations {
        match what.as_str() {
            "parse" => {
                let d = Directives::new(black_box(&text))?;
                d.load()?;
                black_box(&d);
            }
            "apply" => {
                black_box(directives.apply_in_place(black_box(&frames[0].base))?.2[0]);
            }
            "sheet" => {
                for frame in &frames {
                    black_box(directives.apply_in_place(black_box(&frame.base))?.2[0]);
                }
            }
            // only the first N operations, to see what each one costs
            upto if upto.starts_with("upto") => {
                black_box(
                    directives
                        .apply_first(black_box(&frames[0].base), upto[4..].parse()?)?
                        .2[0],
                );
            }
            _ => panic!("parse, apply or sheet"),
        }
    }
    Ok(())
}
