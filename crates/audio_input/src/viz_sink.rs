use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;

use talisman_core::{DataType, ModuleSchema, Port, PortDirection, Signal, Sink};

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
}

impl AudioVizSink {
    pub fn new(id: &str, buffer: Arc<Mutex<Vec<f32>>>, latency_us: Arc<AtomicU64>) -> Self {
        Self {
            id: id.to_string(),
            enabled: true,
            buffer,
            latency_us,
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
            timestamp_us, data, ..
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

        if let Ok(mut buf) = self.buffer.lock() {
            if data.len() <= buf.len() {
                let start = buf.len() - data.len();
                buf[start..].copy_from_slice(&data);
            } else {
                let start = data.len() - buf.len();
                buf.copy_from_slice(&data[start..]);
            }
        }

        Ok(None)
    }
}
