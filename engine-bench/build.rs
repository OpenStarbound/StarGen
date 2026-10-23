// Builds the C++ shim against an OpenStarbound build and links the engine's objects into everything in this crate.
// ./setup.sh collects the flags and the link line from that build.
use std::{env, fs, path::PathBuf};

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    // ENGINE_DIR picks another set of flags and objects, to compare engines
    println!("cargo:rerun-if-env-changed=ENGINE_DIR");
    let engine = env::var("ENGINE_DIR").map_or_else(|_| root.join("engine"), PathBuf::from);
    let flags = engine.join("cxxflags.txt");
    let link = engine.join("link.txt");
    for path in [&flags, &link, &root.join("shim/shim.cpp")] {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let (Ok(flags), Ok(link)) = (fs::read_to_string(&flags), fs::read_to_string(&link)) else {
        panic!(
            "engine/ is missing, run ./setup.sh first (it needs a built OpenStarbound checkout)"
        );
    };

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .file("shim/shim.cpp")
        .warnings(false)
        .opt_level(3);
    for flag in flags.split_whitespace() {
        build.flag(flag);
    }
    build.compile("starshim");

    // the engine's objects are not position independent
    println!("cargo:rustc-link-arg=-no-pie");
    for arg in link.split_whitespace() {
        println!("cargo:rustc-link-arg={arg}");
    }
    println!("cargo:rustc-link-lib=stdc++");
}
