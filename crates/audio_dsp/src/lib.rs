use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use async_trait::async_trait;

use talisman_core::{Processor, ModuleSchema, Port, PortDirection, DataType, Signal};

#[cfg(feature = "tile-rendering")]
pub mod tile;

fn load_f32(atom: &AtomicU32) -> f32 {
    f32::from_bits(atom.load(Ordering::Relaxed))
}

fn store_f32(atom: &AtomicU32, value: f32) {
    atom.store(value.to_bits(), Ordering::Relaxed);
}

#[derive(Default)]
pub struct AudioDspState {
    gain: AtomicU32,
    lowpass_hz: AtomicU32,
    lowpass_enabled: AtomicBool,
}

impl AudioDspState {
    pub fn new() -> Arc<Self> {
        let state = Arc::new(Self::default());
        store_f32(&state.gain, 1.0);
        store_f32(&state.lowpass_hz, 2000.0);
        state
    }

    pub fn gain(&self) -> f32 {
        load_f32(&self.gain)
    }

    pub fn set_gain(&self, gain: f32) {
        store_f32(&self.gain, gain);
    }

    pub fn lowpass_hz(&self) -> f32 {
        load_f32(&self.lowpass_hz)
    }

    pub fn set_lowpass_hz(&self, hz: f32) {
        store_f32(&self.lowpass_hz, hz);
    }

    pub fn lowpass_enabled(&self) -> bool {
        self.lowpass_enabled.load(Ordering::Relaxed)
    }

    pub fn set_lowpass_enabled(&self, enabled: bool) {
        self.lowpass_enabled.store(enabled, Ordering::Relaxed);
    }
}

/// Simple DSP processor that applies gain and optional lowpass.
pub struct AudioDspProcessor {
    id: String,
    enabled: bool,
    state: Arc<AudioDspState>,
    last_samples: Vec<f32>,
}

impl AudioDspProcessor {
    pub fn new(id: &str, state: Arc<AudioDspState>) -> Self {
        Self {
            id: id.to_string(),
            enabled: true,
            state,
            last_samples: Vec::new(),
        }
    }
}

#[async_trait]
impl Processor for AudioDspProcessor {
    fn name(&self) -> &str {
        "Audio DSP"
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            id: self.id.clone(),
            name: "Audio DSP".to_string(),
            description: "Applies gain and lowpass to audio buffers".to_string(),
            ports: vec![
                Port {
                    id: "audio_in".to_string(),
                    label: "Audio In".to_string(),
                    data_type: DataType::Audio,
                    direction: PortDirection::Input,
                },
                Port {
                    id: "audio_out".to_string(),
                    label: "Audio Out".to_string(),
                    data_type: DataType::Audio,
                    direction: PortDirection::Output,
                },
            ],
            settings_schema: None,
        }
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    async fn process(&mut self, signal: Signal) -> anyhow::Result<Option<Signal>> {
        let Signal::Audio {
            sample_rate,
            channels,
            timestamp_us,
            mut data,
        } = signal else {
            return Ok(None);
        };

        let gain = self.state.gain();
        let lowpass_enabled = self.state.lowpass_enabled();
        let lowpass_hz = self.state.lowpass_hz().max(10.0);

        if self.last_samples.len() != channels as usize {
            self.last_samples = vec![0.0; channels as usize];
        }

        let dt = 1.0 / sample_rate as f32;
        let rc = 1.0 / (2.0 * std::f32::consts::PI * lowpass_hz);
        let alpha = dt / (rc + dt);

        for (i, sample) in data.iter_mut().enumerate() {
            let mut x = *sample * gain;
            if lowpass_enabled {
                let ch = i % channels as usize;
                let y_prev = self.last_samples[ch];
                let y = y_prev + alpha * (x - y_prev);
                self.last_samples[ch] = y;
                x = y;
            }
            *sample = x;
        }

        Ok(Some(Signal::Audio {
            sample_rate,
            channels,
            timestamp_us,
            data,
        }))
    }
}
