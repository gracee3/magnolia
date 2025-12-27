use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub mod patch_bay;
pub use patch_bay::{PatchBay, PatchBayError};

pub mod runtime;
pub use runtime::{ModuleRuntime, ModuleHost, ModuleHandle, ExecutionModel, Priority};

pub mod adapters;
pub use adapters::{SourceAdapter, SinkAdapter};

pub mod ring_buffer;
pub use ring_buffer::{SPSCRingBuffer, RingBufferSender, RingBufferReceiver};

pub mod audio_frame;
pub use audio_frame::AudioFrame;

pub mod shared_data;
pub use shared_data::{AudioData, BlobData};

pub mod plugin_loader;
pub use plugin_loader::{PluginLoader, PluginLibrary};
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LayoutConfig {
    pub columns: Vec<String>, // e.g. "30%", "1fr", "200px"
    pub rows: Vec<String>,
    pub tiles: Vec<TileConfig>,
    #[serde(default)]
    pub patches: Vec<Patch>,
    #[serde(default)]
    pub is_sleeping: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TileConfig {
    pub id: String,
    pub col: usize,
    pub row: usize,
    pub colspan: Option<usize>,
    pub rowspan: Option<usize>,
    pub module: String, // e.g. "editor", "word_count"
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool { true }

use std::fmt::Debug;
use schemars::JsonSchema;

pub type Result<T> = std::result::Result<T, anyhow::Error>;

// ============================================================================
// PATCH BAY TYPES
// ============================================================================

/// Data types for type-safe port connections.
/// These define compatibility between module inputs and outputs.
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

/// Port direction - whether a port receives or emits data
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum PortDirection {
    Input,
    Output,
}

/// A typed port on a module for connecting to other modules
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Port {
    /// Unique identifier within the module
    pub id: String,
    /// Human-readable label
    pub label: String,
    /// Type of data this port handles
    pub data_type: DataType,
    /// Whether this port receives (Input) or emits (Output) data
    pub direction: PortDirection,
}

/// Schema describing a module's capabilities and interface
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModuleSchema {
    /// Unique module identifier
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description of what the module does
    pub description: String,
    /// Available input/output ports
    pub ports: Vec<Port>,
    /// Optional JSON Schema for settings UI
    pub settings_schema: Option<serde_json::Value>,
}

/// A connection between two ports on different modules
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Patch {
    /// Unique patch identifier
    pub id: String,
    /// Source module ID
    pub source_module: String,
    /// Source port ID (must be Output direction)
    pub source_port: String,
    /// Sink module ID
    pub sink_module: String,
    /// Sink port ID (must be Input direction)
    pub sink_port: String,
}

// ============================================================================
// SIGNAL TYPES
// ============================================================================

/// The Alchemical Consignment.
///
/// A `Signal` represents any discrete unit of information flowing through the system.
/// It acts as the "Standardized Substance" that allows modules to transmute data.
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
    SharedAudio(Arc<AudioData>),
    /// Real-time audio stream handle (ring buffer for minimal latency)
    /// Contains receiver end - SPSC: only ONE module can consume this!
    /// First module to receive this signal gets exclusive access
    #[serde(skip)]
    AudioStream {
        sample_rate: u32,
        channels: u16,
        receiver: RingBufferReceiver<AudioFrame>,
    },
    /// Shared blob data (Arc-wrapped) - one allocation, many readers
    /// Use this for large files/images to avoid copying overhead  
    #[serde(skip)]
    SharedBlob(Arc<BlobData>),
    /// A control signal for the system (e.g., "Shutdown", "Reload")
    Control(ControlSignal),
    /// Computed/Processed Data (Source, Content)
    Computed {
        source: String,
        content: String,
    },
    /// Empty signal, used for heartbeat or triggers
    Pulse,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub enum ControlSignal {
    Shutdown,
    ReloadConfig,
}

// ============================================================================
// MODULE TRAITS
// ============================================================================

/// A Source emits Signals into the Patch Bay.
///
/// Examples: Clipboard monitor, Keyboard listener, Timer, HTTP Server, Astrology Clock.
#[async_trait]
pub trait Source: Send + Sync {
    /// The name of this source (e.g., "clipboard_monitor")
    fn name(&self) -> &str;
    
    /// Returns the schema describing this module's ports and capabilities
    fn schema(&self) -> ModuleSchema;
    
    /// Whether this module is currently enabled
    fn is_enabled(&self) -> bool { true }
    
    /// Enable or disable this module
    fn set_enabled(&mut self, enabled: bool);

    /// Wait for the next signal from this source.
    /// Returns `None` if the source is exhausted/closed.
    async fn poll(&mut self) -> Option<Signal>;
}

/// A Sink consumes Signals from the Patch Bay.
///
/// Examples: Log file, TTS Speaker, Sigil Renderer, HTTP Client, Screen Display.
#[async_trait]
pub trait Sink: Send + Sync {
    /// The name of this sink
    fn name(&self) -> &str;
    
    /// Returns the schema describing this module's ports and capabilities
    fn schema(&self) -> ModuleSchema;
    
    /// Whether this module is currently enabled
    fn is_enabled(&self) -> bool { true }
    
    /// Enable or disable this module
    fn set_enabled(&mut self, enabled: bool);
    
    /// Render the current output state as a string for clipboard copy
    fn render_output(&self) -> Option<String> { None }

    /// Consume a signal.
    /// Returns Ok(()) if processed, or an error if something went wrong.
    async fn consume(&self, signal: Signal) -> Result<()>;
}

/// A Processor is both a Source and Sink - it transforms signals (middleware).
///
/// Examples: Text sanitizer, Format converter, Rate limiter, Aggregator.
#[async_trait]
pub trait Processor: Send + Sync {
    /// The name of this processor
    fn name(&self) -> &str;
    
    /// Returns the schema describing this module's ports and capabilities
    fn schema(&self) -> ModuleSchema;
    
    /// Whether this module is currently enabled
    fn is_enabled(&self) -> bool { true }
    
    /// Enable or disable this module
    fn set_enabled(&mut self, enabled: bool);
    
    /// Process an input signal and optionally emit an output signal
    async fn process(&mut self, signal: Signal) -> Result<Option<Signal>>;
}

/// A Transform modifies a Signal in flight (synchronous version).
/// (Optional advanced feature for later, but good to have the trait)
#[async_trait]
pub trait Transform: Send + Sync {
    async fn apply(&self, signal: Signal) -> Result<Signal>;
}

