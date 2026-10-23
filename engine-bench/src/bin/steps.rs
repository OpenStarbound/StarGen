//! Follows a frame through the operations: the size and the number of visible pixels after each one
use engine_bench::{
    engine::Directives,
    harness::{self, Assets},
};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (case_name, frames) = (
        args.get(1).cloned().unwrap_or("male_body".into()),
        &args[2.min(args.len())..],
    );
    let assets = Assets::open_default()?;
    let case = harness::humanoid_cases()
        .into_iter()
        .find(|case| case.name == case_name)
        .expect("no such case");
    let text = case.directives()?;
    let ops: Vec<&str> = text.split('?').filter(|op| !op.is_empty()).collect();
    let mut engine = Directives::new(&text)?;
    let all = case.frames(&assets)?;
    for name in frames {
        let frame = all
            .iter()
            .find(|frame| &frame.name == name)
            .expect("no such frame");
        println!("{name}");
        for n in 0..=ops.len() {
            let (w, h, rgba) = engine.apply_first(&frame.base, n)?;
            let visible = rgba.chunks_exact(4).filter(|p| p[3] != 0).count();
            let op = if n == 0 { "(start)" } else { ops[n - 1] };
            let pixels = if w * h <= 4 {
                format!("{:?}", rgba.chunks_exact(4).collect::<Vec<_>>())
            } else {
                String::new()
            };
            println!(
                "  {n:2} {w:3}x{h:<3} {visible:5} visible  {:.60} {pixels}",
                op
            );
        }
    }
    Ok(())
}
