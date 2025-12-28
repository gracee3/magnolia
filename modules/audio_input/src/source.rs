use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use log::{error, info};

use talisman_core::{Source, ModuleSchema, Port, PortDirection, DataType, Signal};
use talisman_signals::ring_buffer::{self, RingBufferReceiver, RingBufferSender};

const DEFAULT_CAPACITY: usize = 16384;
const DEFAULT_FRAME_SAMPLES: usize = 1024;

struct SendStream {
    _stream: cpal::Stream,
}
unsafe impl Send for SendStream {}
unsafe impl Sync for SendStream {}

fn now_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

/// Audio input source using CPAL, emitting buffered Audio signals.
pub struct AudioInputSource {
    id: String,
    enabled: bool,
    stream: Option<SendStream>,
    sender: Option<RingBufferSender<f32>>,
    receiver: RingBufferReceiver<f32>,
    sample_rate: u32,
    channels: u16,
    frame_samples: usize,
    last_capture_us: Arc<AtomicU64>,
}

impl AudioInputSource {
    pub fn new(id: &str) -> anyhow::Result<Self> {
        let (tx, rx) = ring_buffer::channel::<f32>(DEFAULT_CAPACITY);
        let last_capture_us = Arc::new(AtomicU64::new(0));

        let mut source = Self {
            id: id.to_string(),
            enabled: true,
            stream: None,
            sender: Some(tx),
            receiver: rx,
            sample_rate: 44100,
            channels: 2,
            frame_samples: DEFAULT_FRAME_SAMPLES,
            last_capture_us,
        };

        source.initialize()?;
        Ok(source)
    }

    fn initialize(&mut self) -> anyhow::Result<()> {
        if self.stream.is_some() {
            return Ok(());
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow::anyhow!("No input device"))?;

        let config = device.default_input_config()?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels();

        let tx = self.sender.take().ok_or_else(|| anyhow::anyhow!("No sender available"))?;
        let capture_us = self.last_capture_us.clone();

        let err_fn = |err| error!("cpal input error: {}", err);

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &config.into(),
                move |data: &[f32], _| {
                    capture_us.store(now_micros(), Ordering::Relaxed);
                    for &sample in data {
                        let _ = tx.try_send(sample);
                    }
                },
                err_fn,
                None,
            )?,
            _ => return Err(anyhow::anyhow!("Only F32 supported for now")),
        };

        stream.play()?;
        info!("AudioInputSource initialized. SR: {}, Ch: {}", sample_rate, channels);
        self.stream = Some(SendStream { _stream: stream });
        self.sample_rate = sample_rate;
        self.channels = channels;
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
            description: "Captures audio from default input device via CPAL".to_string(),
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
        if !self.enabled {
            tokio::time::sleep(Duration::from_millis(10)).await;
            return Some(Signal::Pulse);
        }

        let target_samples = self.frame_samples * self.channels as usize;
        let mut data = Vec::with_capacity(target_samples);

        while data.len() < target_samples {
            if let Some(sample) = self.receiver.try_recv() {
                data.push(sample);
            } else {
                if !data.is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
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
