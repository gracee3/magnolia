//! Text Tools - Text processing sinks
//!
//! Provides text processing modules for the Talisman system.

mod sinks;
mod save_file;

pub use sinks::{WordCountSink, DevowelizerSink};
pub use save_file::{SaveFileSink, OutputFormat};
