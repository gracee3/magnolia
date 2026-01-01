use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub mod ring_buffer;
use ring_buffer::RingBufferReceiver;

// ============================================================================
// DATA TYPES
// ============================================================================

/// Data types for type-safe port connections.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum DataType {
    /// String/text data
    Text,
    /// Raw bytes (images, generic binary)
    Blob,
    /// PCM or encoded audio streams
    Audio,
    /// Video frames or streams
    Video,
    /// Network packets or streams
    Network,
    /// Astrological data structures
    Astrology,
    /// Numeric values or metrics
    Numeric,
    /// Control signals (shutdown, reload, etc.)
    Control,
    /// Accepts any data type (universal transforms)
    Any,
}

// ============================================================================
// PORT DEFINITIONS
// ============================================================================

/// Port direction - whether a port receives or emits data
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PortDirection {
    Input,
    Output,
}

/// A typed port on a module for connecting to other modules
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PortDesc {
    /// Unique identifier within the module
    pub id: String,
    /// Human-readable label
    pub label: String,
    /// Type of data this port handles
    pub data_type: DataType,
    /// Whether this port receives (Input) or emits (Output) data
    pub direction: PortDirection,
}

// ============================================================================
// CONTROL MESSAGES
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum ControlMsg {
    /// Request to update configuration
    Configure(serde_json::Value),
    /// Signal that the module should reset/reload
    Reset,
    /// Custom control message
    Custom(String, serde_json::Value),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum ControlSignal {
    Shutdown,
    ReloadConfig,
    /// Apply settings update
    Settings(serde_json::Value),
}

// ============================================================================
// MANIFEST
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub author: String,
    /// Optional JSON Schema for settings UI
    pub settings_schema: Option<serde_json::Value>,
}

// ============================================================================
// SIGNAL
// ============================================================================

/// Handle to a host-managed audio buffer (zero-copy)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct AudioBufferHandle {
    pub id: u32,
    pub generation: u32,
    pub length: usize,
}

/// Handle to a host-managed binary blob (zero-copy)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct BlobHandle {
    pub id: u32,
    pub generation: u32,
    pub size: usize,
}

/// Handle to a host-managed GPU Texture (zero-copy)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct GpuTextureHandle {
    pub id: u64,
    pub generation: u32,
    pub width: u32,
    pub height: u32,
}

/// Handle to a host-managed GPU Buffer (zero-copy)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct GpuBufferHandle {
    pub id: u64,
    pub generation: u32,
    pub size: u64,
}

/// Astrological Data Payload
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AstrologyData {
    pub sun_sign: String,
    pub moon_sign: String,
    pub rising_sign: String,
    pub planetary_positions: Vec<(String, f64)>, // Planet name, degree
}

/// The Alchemical Consignment.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "data")]
pub enum Signal {
    /// Pure text content (e.g., from Clipboard, Keyboard, LLM)
    Text(String),
    /// A structured command or intent
    Intent {
        action: String,
        parameters: Vec<String>,
    },
    /// Astrological Data
    Astrology(AstrologyData),
    /// Raw bytes (e.g., Image, Audio buffer)
    Blob { mime_type: String, bytes: Vec<u8> },
    /// Host-managed Blob Handle (zero-copy)
    BlobHandle {
        handle: BlobHandle,
        mime_type: String,
    },
    /// Audio Signal (PCM) - buffered, copied to each module
    Audio {
        sample_rate: u32,
        channels: u16,
        timestamp_us: u64,
        data: Vec<f32>,
    },
    /// Host-managed Audio Buffer Handle (zero-copy)
    AudioHandle {
        handle: AudioBufferHandle,
        sample_rate: u32,
        channels: u16,
    },
    /// Shared audio data (Arc-wrapped) - one allocation, many readers
    /// Use this for large audio buffers to avoid copying overhead
    #[serde(skip)]
    SharedAudio(Arc<Vec<f32>>), // Simplified from core::AudioData for now
    /// Real-time audio stream handle (ring buffer for minimal latency)
    /// Contains receiver end - SPSC: only ONE module can consume this!
    #[serde(skip)]
    AudioStream {
        sample_rate: u32,
        channels: u16,
        // We use the RingBufferReceiver from this crate
        // AudioFrame is usually just f32 or a struct?
        // Let's use f32 to match ring_buffer tests for now, or define AudioFrame here.
        // Core used AudioFrame.
        // Let's define AudioFrame here.
        receiver: RingBufferReceiver<f32>,
    },
    /// Shared blob data (Arc-wrapped) - one allocation, many readers
    #[serde(skip)]
    SharedBlob(Arc<Vec<u8>>),
    /// A control signal for the system (e.g., "Shutdown", "Reload")
    Control(ControlSignal),
    /// Computed/Processed Data (Source, Content)
    Computed { source: String, content: String },
    /// Pointer to WGPU Context types (Device, Queue) - Unsafe!
    /// DEPRECATED: Use HostGpuApi instead of passing raw pointers
    #[serde(skip)]
    GpuContext {
        device: usize, // cast to *const wgpu::Device
        queue: usize,  // cast to *const wgpu::Queue
    },
    /// GPU Texture Handle (for Compositor)
    #[serde(skip)]
    Texture {
        handle: GpuTextureHandle,
        start_time: f64, // Optional timestamp
    },
    /// Empty signal, used for heartbeat or triggers
    Pulse,
}

impl Clone for Signal {
    fn clone(&self) -> Self {
        match self {
            Signal::Text(text) => Signal::Text(text.clone()),
            Signal::Intent { action, parameters } => Signal::Intent {
                action: action.clone(),
                parameters: parameters.clone(),
            },
            Signal::Astrology(data) => Signal::Astrology(data.clone()),
            Signal::Blob { mime_type, bytes } => Signal::Blob {
                mime_type: mime_type.clone(),
                bytes: bytes.clone(),
            },
            Signal::BlobHandle { handle, mime_type } => Signal::BlobHandle {
                handle: *handle,
                mime_type: mime_type.clone(),
            },
            Signal::Audio {
                sample_rate,
                channels,
                timestamp_us,
                data,
            } => Signal::Audio {
                sample_rate: *sample_rate,
                channels: *channels,
                timestamp_us: *timestamp_us,
                data: data.clone(),
            },
            Signal::AudioHandle {
                handle,
                sample_rate,
                channels,
            } => Signal::AudioHandle {
                handle: *handle,
                sample_rate: *sample_rate,
                channels: *channels,
            },
            Signal::SharedAudio(data) => Signal::SharedAudio(Arc::clone(data)),
            Signal::AudioStream { .. } => {
                panic!("Signal::AudioStream cannot be cloned (SPSC receiver)");
            }
            Signal::SharedBlob(data) => Signal::SharedBlob(Arc::clone(data)),
            Signal::Control(signal) => Signal::Control(signal.clone()),
            Signal::Computed { source, content } => Signal::Computed {
                source: source.clone(),
                content: content.clone(),
            },
            Signal::GpuContext { device, queue } => Signal::GpuContext {
                device: *device,
                queue: *queue,
            },
            Signal::Texture { handle, start_time } => Signal::Texture {
                handle: *handle,
                start_time: *start_time,
            },
            Signal::Pulse => Signal::Pulse,
        }
    }
}
