pub mod sinks;
pub mod save_file;

pub use sinks::{WordCountSink, DevowelizerSink};
pub use save_file::{SaveFileSink, OutputFormat};
