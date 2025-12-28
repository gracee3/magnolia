use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use std::sync::Arc;
use std::any::Any;

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

/// The Alchemical Consignment.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
    Astrology {
        sun_sign: String,
        moon_sign: String,
        rising_sign: String,
        planetary_positions: Vec<(String, f64)>, // Planet name, degree
    },
    /// Raw bytes (e.g., Image, Audio buffer)
    Blob {
        mime_type: String,
        bytes: Vec<u8>,
    },
    /// Audio Signal (PCM) - buffered, copied to each module
    Audio {
        sample_rate: u32,
        channels: u16,
        data: Vec<f32>,
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
    Computed {
        source: String,
        content: String,
    },
    /// Pointer to WGPU Context types (Device, Queue) - Unsafe!
    #[serde(skip)]
    GpuContext {
        device: usize, // cast to *const wgpu::Device
        queue: usize,  // cast to *const wgpu::Queue
    },
    /// GPU Texture Handle (for Compositor)
    #[serde(skip)]
    Texture {
        id: u64,
        view: usize, // cast to *const wgpu::TextureView
        width: u32,
        height: u32,
    },
    /// Empty signal, used for heartbeat or triggers
    Pulse,
}
