use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use log::{info, warn};
use tokio::time::Instant;

use crate::AudioInputSettings;
use crate::backend::{default_backend, AudioInputBackend, BackendStream};
use crate::settings::AudioDeviceEntry;
use magnolia_core::{DataType, ModuleSchema, Port, PortDirection, Signal, Source};
use magnolia_signals::ring_buffer::{self, RingBufferReceiver};

const DEFAULT_CAPACITY: usize = 16384;

/// Audio input source using CPAL, emitting buffered Audio signals.
pub struct AudioInputSource {
    id: String,
    enabled: bool,
    stream: Option<BackendStream>,
    receiver: RingBufferReceiver<f32>,
    sample_rate: u32,
    channels: u16,
    last_capture_us: Arc<AtomicU64>,
    settings: Arc<AudioInputSettings>,
    backend: Mutex<Box<dyn AudioInputBackend>>,
}

impl AudioInputSource {
    pub fn new(id: &str, settings: Arc<AudioInputSettings>) -> anyhow::Result<Self> {
        let last_capture_us = Arc::new(AtomicU64::new(0));
        let backend = default_backend()?;

        let mut source = Self {
            id: id.to_string(),
            enabled: true,
            stream: None,
            receiver: ring_buffer::channel::<f32>(DEFAULT_CAPACITY).1,
            sample_rate: 44100,
            channels: 2,
            last_capture_us,
            settings,
            backend: Mutex::new(backend),
        };

        if let Err(e) = source.initialize() {
            // Keep the module alive so the user can retry via settings UI.
            source.settings.set_last_error(Some(e.to_string()));
        }
        Ok(source)
    }

    fn initialize(&mut self) -> anyhow::Result<()> {
        if self.stream.is_some() {
            return Ok(());
        }

        // Refresh device list in settings (best-effort).
        match self
            .backend
            .lock()
            .map_err(|_| anyhow::anyhow!("AudioInputSource backend lock poisoned"))?
            .refresh_devices()
        {
            Ok(devs) => {
                let entries = devs
                    .into_iter()
                    .map(|d| AudioDeviceEntry { id: d.id, name: d.name })
                    .collect::<Vec<_>>();
                self.settings.set_devices(entries);
            }
            Err(e) => {
                warn!("AudioInputSource: failed to refresh devices: {e}");
            }
        }

        let selected = self.settings.selected();

        // Re-create ring buffer channel each initialization so we always have a valid producer handle.
        let (tx, rx) = ring_buffer::channel::<f32>(DEFAULT_CAPACITY);
        self.receiver = rx;
        let capture_us = self.last_capture_us.clone();

        let (stream, fmt, resolved_name) = self
            .backend
            .lock()
            .map_err(|_| anyhow::anyhow!("AudioInputSource backend lock poisoned"))?
            .start(&selected, tx, capture_us)?;

        self.settings.set_last_error(None);
        self.settings.set_active_device(Some(resolved_name.clone()));
        self.settings.set_format(fmt.sample_rate, fmt.channels);

        info!(
            "AudioInputSource initialized. SR: {}, Ch: {}, Device: {}",
            fmt.sample_rate, fmt.channels, resolved_name
        );
        self.stream = Some(stream);
        self.sample_rate = fmt.sample_rate;
        self.channels = fmt.channels;
        Ok(())
    }
}

#[async_trait]
impl Source for AudioInputSource {
    fn name(&self) -> &str {
        "Audio Input"
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            id: self.id.clone(),
            name: "Audio Input".to_string(),
            description: "Captures audio from the system input device (PipeWire on Linux, CPAL elsewhere)".to_string(),
            ports: vec![Port {
                id: "audio_out".to_string(),
                label: "Audio Out".to_string(),
                data_type: DataType::Audio,
                direction: PortDirection::Output,
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

    async fn poll(&mut self) -> Option<Signal> {
        if self.settings.take_pending() {
            self.stream = None;
            let _ = self.initialize();
        }

        if !self.enabled || self.settings.is_muted() {
            // Drain receiver to avoid buildup
            while self.receiver.try_recv().is_some() {}
            tokio::time::sleep(Duration::from_millis(10)).await;
            return Some(Signal::Pulse);
        }

        let frame_samples = self.settings.frame_samples() as usize;
        let max_batch_wait = Duration::from_millis(self.settings.max_batch_wait_ms() as u64);

        let target_samples = frame_samples * self.channels as usize;
        let mut data = Vec::with_capacity(target_samples);

        let deadline = Instant::now() + max_batch_wait;
        while data.len() < target_samples {
            if let Some(sample) = self.receiver.try_recv() {
                data.push(sample);
                continue;
            }

            // If we've already captured *some* audio, stop waiting quickly so viz/output doesn't lag.
            if !data.is_empty() {
                if Instant::now() >= deadline {
                    break;
                }
                tokio::task::yield_now().await;
                continue;
            }

            // No samples yet; avoid a tight loop.
            if Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }

        if data.is_empty() {
            return Some(Signal::Pulse);
        }

        let timestamp_us = self.last_capture_us.load(Ordering::Relaxed);
        Some(Signal::Audio {
            sample_rate: self.sample_rate,
            channels: self.channels,
            timestamp_us,
            data,
        })
    }
}
