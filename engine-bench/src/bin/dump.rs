//! Prints the operations of a case's directives, one per line
fn main() -> anyhow::Result<()> {
    let name = std::env::args().nth(1).unwrap_or("male_body".into());
    let case = engine_bench::harness::humanoid_cases()
        .into_iter()
        .find(|case| case.name == name)
        .expect("no such case");
    for op in case.directives()?.split('?').filter(|op| !op.is_empty()) {
        println!("{op}");
    }
    Ok(())
}
