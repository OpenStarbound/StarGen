//! The engine's images start at the lower left, the `image` crate's at the top left. Prints which of the two a png decodes like, as a check
//! that the cells of a sheet (counted from the top, like the generator does) are taken from the right end.
use engine_bench::engine::Image;
fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).unwrap();
    let bytes = std::fs::read(&path)?;
    let engine = Image::decode(&bytes)?;
    let other = image::load_from_memory(&bytes)?.to_rgba8();
    let same_up = (0..engine.height).all(|y| (0..engine.width).all(|x| engine.pixel(x, y) == other.get_pixel(x, y).0));
    let same_flipped = (0..engine.height).all(|y| (0..engine.width).all(|x| engine.pixel(x, y) == other.get_pixel(x, engine.height - 1 - y).0));
    println!("{path}: {}x{}; engine rows equal the image crate's: {same_up}; equal when flipped vertically: {same_flipped}", engine.width, engine.height);
    Ok(())
}
