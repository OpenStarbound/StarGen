{
  description = "stargen dev shell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in
      {
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            # toolchain
            rustc
            cargo
            clippy
            rustfmt
            rust-analyzer
            clang # C compiler for mimalloc's build script, also provides the linker driver
            mold # fast linker
            pkg-config

            # cargo helpers
            cargo-nextest
            cargo-watch
            cargo-edit
            cargo-outdated
            cargo-udeps

            # benchmarking
            hyperfine # end-to-end CLI benchmarks
            cargo-criterion # runs criterion benches (add `criterion` as a dev-dependency)
            poop # compare peak-perf counters between commands

            # profiling
            perf # linux perf
            cargo-flamegraph
            samply # sampling profiler, opens in the Firefox profiler
            valgrind # callgrind / cachegrind / massif / dhat
            heaptrack # allocation profiling
            gdb

            # code size / codegen inspection
            cargo-show-asm
            cargo-llvm-lines
            cargo-bloat
          ];

          RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          # keep frame pointers + symbols in release builds so profilers get usable stacks
          RUSTFLAGS = "-C force-frame-pointers=yes -C link-arg=-fuse-ld=mold";
        };
      });
}
