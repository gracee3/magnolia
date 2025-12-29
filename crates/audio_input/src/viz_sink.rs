use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;

use talisman_core::{DataType, ModuleSchema, Port, PortDirection, Signal, Sink};
use talisman_signals::ring_buffer::RingBufferSender;

fn now_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

/// Sink that updates a shared audio buffer for visualization.
pub struct AudioVizSink {
    id: String,
    enabled: bool,
    buffer: Arc<Mutex<Vec<f32>>>,
    latency_us: Arc<AtomicU64>,
    sample_rate_hz: Arc<AtomicU32>,
}

impl AudioVizSink {
    pub fn new(
        id: &str,
        buffer: Arc<Mutex<Vec<f32>>>,
        latency_us: Arc<AtomicU64>,
        sample_rate_hz: Arc<AtomicU32>,
    ) -> Self {
        Self {
            id: id.to_string(),
            enabled: true,
            buffer,
            latency_us,
            sample_rate_hz,
        }
    }
}

/// Sink that forwards audio into an SPSC ring buffer for low-latency visualization on the UI thread.
///
/// This avoids `Mutex<Vec<f32>>` copies/locks on the hot UI path.
pub struct AudioVizRingSink {
    id: String,
    enabled: bool,
    tx: RingBufferSender<f32>,
    latency_us: Arc<AtomicU64>,
    sample_rate_hz: Arc<AtomicU32>,
    channels: Arc<AtomicU32>,
}

impl AudioVizRingSink {
    pub fn new(
        id: &str,
        tx: RingBufferSender<f32>,
        latency_us: Arc<AtomicU64>,
        sample_rate_hz: Arc<AtomicU32>,
        channels: Arc<AtomicU32>,
    ) -> Self {
        Self {
            id: id.to_string(),
            enabled: true,
            tx,
            latency_us,
            sample_rate_hz,
            channels,
        }
    }
}

#[async_trait]
impl Sink for AudioVizSink {
    fn name(&self) -> &str {
        "Audio Visualizer Sink"
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            id: self.id.clone(),
            name: "Audio Viz".to_string(),
            description: "Updates shared buffer for audio visualization".to_string(),
            ports: vec![Port {
                id: "audio_in".to_string(),
                label: "Audio In".to_string(),
                data_type: DataType::Audio,
                direction: PortDirection::Input,
            }],
            settings_schema: None,
        }
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    async fn consume(&self, signal: Signal) -> anyhow::Result<Option<Signal>> {
        if !self.enabled {
            return Ok(None);
        }

        let Signal::Audio {
            timestamp_us,
            data,
            sample_rate,
            ..
        } = signal
        else {
            return Ok(None);
        };

        if timestamp_us > 0 {
            let now = now_micros();
            if now >= timestamp_us {
                self.latency_us.store(now - timestamp_us, Ordering::Relaxed);
            }
        }

        self.sample_rate_hz.store(sample_rate, Ordering::Relaxed);

        if let Ok(mut buf) = self.buffer.lock() {
            let n = data.len();
            let buf_len = buf.len();
            if n >= buf_len {
                // New data is larger than buffer, just take the end
                buf.copy_from_slice(&data[n - buf_len..]);
            } else {
                // Shift existing samples to the left
                buf.rotate_left(n);
                // Copy new samples to the end
                let start = buf_len - n;
                buf[start..].copy_from_slice(&data);
            }
        }

        Ok(None)
    }
}

#[async_trait]
impl Sink for AudioVizRingSink {
    fn name(&self) -> &str {
        "Audio Visualizer Ring Sink"
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            id: self.id.clone(),
            name: "Audio Viz".to_string(),
            description: "Streams audio into an SPSC ring buffer for visualization".to_string(),
            ports: vec![Port {
                id: "audio_in".to_string(),
                label: "Audio In".to_string(),
                data_type: DataType::Audio,
                direction: PortDirection::Input,
            }],
            settings_schema: None,
        }
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    async fn consume(&self, signal: Signal) -> anyhow::Result<Option<Signal>> {
        if !self.enabled {
            return Ok(None);
        }

        let Signal::Audio {
            timestamp_us,
            data,
            sample_rate,
            channels,
            ..
        } = signal
        else {
            return Ok(None);
        };

        if timestamp_us > 0 {
            let now = now_micros();
            if now >= timestamp_us {
                self.latency_us.store(now - timestamp_us, Ordering::Relaxed);
            }
        }

        self.sample_rate_hz.store(sample_rate, Ordering::Relaxed);
        self.channels
            .store((channels as u32).max(1), Ordering::Relaxed);

        // Best-effort: if the UI can't keep up, we drop samples rather than blocking.
        for s in data {
            let _ = self.tx.try_send(s);
        }

        Ok(None)
    }
}
