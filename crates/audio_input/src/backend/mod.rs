use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use talisman_signals::ring_buffer::RingBufferSender;

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    /// Stable identifier (backend-specific). For CPAL we use the device name.
    pub id: String,
    /// Human-readable display name.
    pub name: String,
}

#[derive(Clone, Copy, Debug)]
pub struct NegotiatedFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

/// Opaque backend stream handle; dropping this stops the stream.
pub struct BackendStream {
    _inner: Box<dyn Send + Sync>,
}

impl BackendStream {
    pub fn new<T: Send + Sync + 'static>(inner: T) -> Self {
        Self {
            _inner: Box::new(inner),
        }
    }
}

pub trait AudioInputBackend: Send {
    fn refresh_devices(&mut self) -> anyhow::Result<Vec<DeviceInfo>>;

    /// Start capture on the selected device.
    ///
    /// `device_id` is either `"Default"` or a backend-specific stable id.
    ///
    /// Returns `(stream_handle, negotiated_format, resolved_device_name)`.
    fn start(
        &mut self,
        device_id: &str,
        tx: RingBufferSender<f32>,
        capture_us: Arc<AtomicU64>,
    ) -> anyhow::Result<(BackendStream, NegotiatedFormat, String)>;
}

#[cfg(target_os = "linux")]
mod pipewire;
#[cfg(not(target_os = "linux"))]
mod cpal;

pub fn default_backend() -> anyhow::Result<Box<dyn AudioInputBackend>> {
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(pipewire::PipeWireInputBackend::new()?))
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(Box::new(cpal::CpalInputBackend::new()))
    }
}


