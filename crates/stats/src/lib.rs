//! Spatial statistics (L3), implemented clean-room from the published literature. Spatial
//! weights come first: every other statistic takes one.
//!
//! - `weights`: contiguity, nearest neighbours, distance bands and kernels, standardised.
//! - `gal`: weights as GAL and GWT text.
//! - `autocorrelation`: global Moran's I, Geary's C and General G, with their inference.
//! - `inference`: normal p-values and the seeded generator permutation tests use.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod autocorrelation;
pub mod gal;
pub mod inference;
mod kdtree;
pub mod weights;

/// The most polygon vertices contiguity reads.
pub const MAX_VERTICES: usize = 50_000_000;

pub use weights::{Spec, build, min_threshold, standardize};
