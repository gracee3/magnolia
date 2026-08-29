//! Bounded, off-callback ASR sessions and durable normalized transcript events.
//!
//! The Sherpa adapter is deliberately feature-gated. Production composition may
//! enable it only after the model and native library pass the provenance gate.

mod acquisition;
mod journal;
mod reducer;
mod worker;

pub use acquisition::*;
pub use journal::*;
pub use magnolia_protocol::{
    AsrEvent, AsrEventBody, AsrEventHeader, DiscontinuityReason, ModelProvenance, WordAlignment,
    ASR_EVENT_SCHEMA_MAJOR,
};
pub use reducer::*;
pub use worker::*;

pub const SHERPA_ADAPTER_VERSION: &str = "1.13.4";
pub const ACCEPTED_MODEL_NAME: &str = "sherpa-onnx-streaming-zipformer-en-2023-06-26";
pub const ACCEPTED_MODEL_ASSET_ID: u64 = 191_971_614;
pub const ACCEPTED_MODEL_ARCHIVE_BYTES: u64 = 310_414_022;
pub const ACCEPTED_MODEL_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-streaming-zipformer-en-2023-06-26.tar.bz2";
pub const ACCEPTED_NATIVE_NAME: &str = "sherpa-onnx-v1.13.4-linux-x64-shared-no-tts-lib";
pub const ACCEPTED_NATIVE_ARCHIVE_BYTES: u64 = 9_006_130;
pub const ACCEPTED_NATIVE_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.4/sherpa-onnx-v1.13.4-linux-x64-shared-no-tts-lib.tar.bz2";
pub const ACCEPTED_PROVIDER: &str = "cpu";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SherpaRuntimeConfig {
    pub threads: u32,
    pub empty_silence_seconds: f32,
    pub trailing_silence_seconds: f32,
    pub maximum_utterance_seconds: f32,
}

impl Default for SherpaRuntimeConfig {
    fn default() -> Self {
        Self {
            threads: 2,
            empty_silence_seconds: 2.4,
            trailing_silence_seconds: 0.8,
            maximum_utterance_seconds: 30.0,
        }
    }
}
