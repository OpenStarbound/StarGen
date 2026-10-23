//! The OpenStarbound engine's drawable pipeline, for checking and timing the directives that stargen generates.
//!
//! `engine` is a thin wrapper over the C++ shim, `harness` builds the cases and compares the results.

pub mod engine;
pub mod harness;
