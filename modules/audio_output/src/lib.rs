pub mod tile;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use log::{error, info, warn};

use talisman_core::{Sink, ModuleSchema, Port, PortDirection, DataType, Signal};
use talisman_signals::ring_buffer::{self, RingBufferSender};

const OUTPUT_CAPACITY: usize = 32768;

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

#[derive(Default)]
pub struct AudioOutputState {
    latency_us: AtomicU64,
    level_milli: AtomicU64,
}

impl AudioOutputState {
    pub fn latency_us(&self) -> u64 {
        self.latency_us.load(Ordering::Relaxed)
    }

    pub fn level_milli(&self) -> u64 {
        self.level_milli.load(Ordering::Relaxed)
    }
}

pub struct AudioOutputSink {
    id: String,
    enabled: bool,
    _stream: Option<SendStream>,
    sender: RingBufferSender<f32>,
    sample_rate: u32,
    channels: u16,
    state: Arc<AudioOutputState>,
    warned_mismatch: AtomicBool,
}

impl AudioOutputSink {
    pub fn new(id: &str) -> anyhow::Result<(Self, Arc<AudioOutputState>)> {
        let (tx, rx) = ring_buffer::channel::<f32>(OUTPUT_CAPACITY);
        let state = Arc::new(AudioOutputState::default());

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow::anyhow!("No output device"))?;

        let config = device.default_output_config()?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels();

        let err_fn = |err| error!("cpal output error: {}", err);

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                &config.into(),
                move |data: &mut [f32], _| {
                    for sample in data {
                        *sample = rx.try_recv().unwrap_or(0.0);
                    }
                },
                err_fn,
                None,
            )?,
            _ => return Err(anyhow::anyhow!("Only F32 supported for now")),
        };

        stream.play()?;
        info!("AudioOutputSink initialized. SR: {}, Ch: {}", sample_rate, channels);

        Ok((
            Self {
                id: id.to_string(),
                enabled: true,
                _stream: Some(SendStream { _stream: stream }),
                sender: tx,
                sample_rate,
                channels,
                state: state.clone(),
                warned_mismatch: AtomicBool::new(false),
            },
            state,
        ))
    }

}

#[async_trait]
impl Sink for AudioOutputSink {
    fn name(&self) -> &str {
        "Audio Output"
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            id: self.id.clone(),
            name: "Audio Output".to_string(),
            description: "Plays audio buffers via CPAL output".to_string(),
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
            sample_rate,
            channels,
            timestamp_us,
            data,
        } = signal else {
            return Ok(None);
        };

        if sample_rate != self.sample_rate || channels != self.channels {
            if !self.warned_mismatch.swap(true, Ordering::Relaxed) {
                warn!(
                    "AudioOutputSink: format mismatch ({}Hz/{}ch) != output ({}Hz/{}ch)",
                    sample_rate,
                    channels,
                    self.sample_rate,
                    self.channels
                );
            }
            return Ok(None);
        }

        if timestamp_us > 0 {
            let now = now_micros();
            if now >= timestamp_us {
                self.state
                    .latency_us
                    .store(now - timestamp_us, Ordering::Relaxed);
            }
        }

        let mut sum = 0.0f64;
        for sample in &data {
            sum += (*sample as f64) * (*sample as f64);
            let _ = self.sender.try_send(*sample);
        }

        if !data.is_empty() {
            let rms = (sum / data.len() as f64).sqrt();
            let level_milli = (rms * 1000.0) as u64;
            self.state.level_milli.store(level_milli, Ordering::Relaxed);
        }

        Ok(None)
    }
}
