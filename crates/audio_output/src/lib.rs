mod settings;
#[cfg(feature = "tile-rendering")]
pub mod tile;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use log::{error, info, warn};

use talisman_core::{DataType, ModuleSchema, Port, PortDirection, Signal, Sink};
use talisman_signals::ring_buffer::{self, RingBufferSender};

pub use settings::AudioOutputSettings;

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
    inner: Mutex<AudioOutputInner>,
    state: Arc<AudioOutputState>,
    settings: Arc<AudioOutputSettings>,
}

struct AudioOutputInner {
    _stream: Option<SendStream>,
    sender: RingBufferSender<f32>,
    sample_rate: u32,
    channels: u16,
    warned_mismatch: AtomicBool,
}

impl AudioOutputSink {
    pub fn new(
        id: &str,
        settings: Arc<AudioOutputSettings>,
    ) -> anyhow::Result<(Self, Arc<AudioOutputState>)> {
        let state = Arc::new(AudioOutputState::default());

        let (inner, devices) = Self::build_stream(&settings)?;
        settings.set_devices(devices);

        Ok((
            Self {
                id: id.to_string(),
                enabled: true,
                inner: Mutex::new(inner),
                state: state.clone(),
                settings,
            },
            state,
        ))
    }

    fn build_stream(
        settings: &AudioOutputSettings,
    ) -> anyhow::Result<(AudioOutputInner, Vec<String>)> {
        let (tx, rx) = ring_buffer::channel::<f32>(OUTPUT_CAPACITY);

        let host = cpal::default_host();
        let available = host
            .output_devices()
            .map(|devices| devices.filter_map(|d| d.name().ok()).collect::<Vec<_>>())
            .unwrap_or_default();

        let selected = settings.selected();
        let device = if selected == "Default" {
            host.default_output_device()
        } else {
            host.output_devices().ok().and_then(|mut devices| {
                devices.find(|d| d.name().ok().as_deref() == Some(&selected))
            })
        }
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
        info!(
            "AudioOutputSink initialized. SR: {}, Ch: {}, Device: {}",
            sample_rate, channels, selected
        );

        Ok((
            AudioOutputInner {
                _stream: Some(SendStream { _stream: stream }),
                sender: tx,
                sample_rate,
                channels,
                warned_mismatch: AtomicBool::new(false),
            },
            available,
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

        if self.settings.take_pending() {
            if let Ok(mut inner) = self.inner.lock() {
                let (next, devices) = Self::build_stream(&self.settings)?;
                *inner = next;
                self.settings.set_devices(devices);
            }
        }

        let Signal::Audio {
            sample_rate,
            channels,
            timestamp_us,
            data,
        } = signal
        else {
            return Ok(None);
        };

        let inner = self.inner.lock().unwrap();
        if sample_rate != inner.sample_rate || channels != inner.channels {
            if !inner.warned_mismatch.swap(true, Ordering::Relaxed) {
                warn!(
                    "AudioOutputSink: format mismatch ({}Hz/{}ch) != output ({}Hz/{}ch)",
                    sample_rate, channels, inner.sample_rate, inner.channels
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
            let _ = inner.sender.try_send(*sample);
        }

        if !data.is_empty() {
            let rms = (sum / data.len() as f64).sqrt();
            let level_milli = (rms * 1000.0) as u64;
            self.state.level_milli.store(level_milli, Ordering::Relaxed);
        }

        Ok(None)
    }
}
