//! Native audio primitives for Magnolia.
//!
//! Allocation and graph preparation happen on control threads. The callback
//! API works only with fixed-capacity blocks and slices.

mod block;
mod convert;
mod pool;

#[cfg(target_os = "linux")]
pub mod pipewire;

pub use block::{AudioBlock, AudioFormat, BlockIndex, Discontinuity};
pub use convert::{downmix_to_mono, i16_le_to_f32, LinearResampler, ProcessError};
pub use pool::{
    block_channel, BlockConsumer, BlockProducer, ConsumeOutcome, EdgeCounters, EdgeSnapshot,
    PublishOutcome,
};
