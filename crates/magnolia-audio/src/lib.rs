//! Native audio primitives for Magnolia.
//!
//! Allocation and graph preparation happen on control threads. The callback
//! API works only with fixed-capacity blocks and slices.

mod block;
#[cfg(target_os = "linux")]
mod capture;
mod convert;
#[cfg(target_os = "linux")]
mod output;
mod pool;
mod quantum;
mod registry;
mod rt_audit;

#[cfg(target_os = "linux")]
pub mod pipewire;

pub use block::{AudioBlock, AudioFormat, BlockIndex, Discontinuity};
#[cfg(target_os = "linux")]
pub use capture::{
    CaptureConfiguration, CaptureError, CaptureSnapshot, CaptureState, NativeSampleFormat,
    PipeWireCapture,
};
pub use convert::{
    downmix_to_mono, f32_le_to_f32, i16_le_to_f32, i32_le_to_f32, LinearResampler, ProcessError,
};
#[cfg(target_os = "linux")]
pub use output::{OutputConfiguration, OutputError, OutputSnapshot, PipeWireOutput};
pub use pool::{
    block_channel, BlockConsumer, BlockProducer, ConsumeOutcome, EdgeCounters, EdgeSnapshot,
    PublishOutcome,
};
pub use quantum::{
    DiscontinuityReason, QuantumAdapter, QuantumBlockMeta, QuantumError, MAGNOLIA_BLOCK_FRAMES,
    MAX_PIPEWIRE_QUANTUM_FRAMES,
};
pub use registry::{
    deterministic_runtime_id, DeviceDirection, DeviceRegistry, DeviceResolutionError,
    RegistryDevice,
};
pub use rt_audit::{
    callback_allocation_counts, reset_callback_allocation_counts, CallbackCountingAllocator,
    CallbackScope,
};
