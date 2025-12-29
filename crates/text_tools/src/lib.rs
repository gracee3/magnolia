//! Text Tools - Text processing sinks
//!
//! Provides text processing modules for the Talisman system.

mod save_file;
mod sinks;

pub use save_file::{OutputFormat, SaveFileSink};
pub use sinks::{DevowelizerSink, WordCountSink};
