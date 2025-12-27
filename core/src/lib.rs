use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use schemars::JsonSchema;

pub type Result<T> = std::result::Result<T, anyhow::Error>;

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

/// A Source emits Signals into the Patch Bay.
///
/// Examples: Clipboard monitor, Keyboard listener, Timer, HTTP Server, Astrology Clock.
#[async_trait]
pub trait Source: Send + Sync {
    /// The name of this source (e.g., "clipboard_monitor")
    fn name(&self) -> &str;

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

    /// Consume a signal.
    /// Returns Ok(()) if processed, or an error if something went wrong.
    async fn consume(&self, signal: Signal) -> Result<()>;
}

/// A Transform modifies a Signal in flight.
/// (Optional advanced feature for later, but good to have the trait)
#[async_trait]
pub trait Transform: Send + Sync {
    async fn apply(&self, signal: Signal) -> Result<Signal>;
}
