//! Text Tools - Text processing sinks
//!
//! Provides text processing modules for the Magnolia system.

mod save_file;
mod sinks;

pub use save_file::{OutputFormat, SaveFileSink};
pub use sinks::{DevowelizerSink, WordCountSink};
