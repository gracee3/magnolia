//! Kamea - Generative Sigil System
//!
//! Creates visual sigils from text input using SHA256 hash
//! and random walk on a grid (Digital Kamea method).

mod generator;
mod sink;

#[cfg(feature = "tile-rendering")]
pub mod tile;

pub use generator::{generate_path, SigilConfig};
pub use sink::KameaSink;

#[cfg(feature = "tile-rendering")]
pub use tile::KameaTile;

