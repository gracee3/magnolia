use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

use async_trait::async_trait;

use magnolia_core::{DataType, ModuleSchema, Port, PortDirection, Processor, Signal};

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
    agc_enabled: AtomicBool,
    lowpass_hz: AtomicU32,
    lowpass_enabled: AtomicBool,
    is_muted: AtomicBool,
}

impl AudioDspState {
    pub fn new() -> Arc<Self> {
        let state = Arc::new(Self::default());
        store_f32(&state.gain, 1.0);
        state.agc_enabled.store(true, Ordering::Relaxed);
        store_f32(&state.lowpass_hz, 2000.0);
        state.is_muted.store(false, Ordering::Relaxed);
        state
    }

    pub fn gain(&self) -> f32 {
        load_f32(&self.gain)
    }

    pub fn set_gain(&self, gain: f32) {
        store_f32(&self.gain, gain);
    }

    pub fn agc_enabled(&self) -> bool {
        self.agc_enabled.load(Ordering::Relaxed)
    }

    pub fn set_agc_enabled(&self, enabled: bool) {
        self.agc_enabled.store(enabled, Ordering::Relaxed);
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

    pub fn is_muted(&self) -> bool {
        self.is_muted.load(Ordering::Relaxed)
    }

    pub fn set_muted(&self, muted: bool) {
        self.is_muted.store(muted, Ordering::Relaxed);
    }
}

/// Simple DSP processor that applies gain and optional lowpass.
pub struct AudioDspProcessor {
    id: String,
    enabled: bool,
    state: Arc<AudioDspState>,
    last_samples: Vec<f32>,
    agc_gain: f32,
}

impl AudioDspProcessor {
    pub fn new(id: &str, state: Arc<AudioDspState>) -> Self {
        Self {
            id: id.to_string(),
            enabled: true,
            state,
            last_samples: Vec::new(),
            agc_gain: 1.0,
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
        } = signal
        else {
            return Ok(None);
        };

        let gain = self.state.gain();
        let agc_enabled = self.state.agc_enabled();
        let lowpass_enabled = self.state.lowpass_enabled();
        let lowpass_hz = self.state.lowpass_hz().max(10.0);

        if self.state.is_muted() {
            for sample in data.iter_mut() {
                *sample = 0.0;
            }
            return Ok(Some(Signal::Audio {
                sample_rate,
                channels,
                timestamp_us,
                data,
            }));
        }

        if self.last_samples.len() != channels as usize {
            self.last_samples = vec![0.0; channels as usize];
        }

        if !agc_enabled {
            self.agc_gain = 1.0;
        }

        let dt = 1.0 / sample_rate as f32;
        let rc = 1.0 / (2.0 * std::f32::consts::PI * lowpass_hz);
        let alpha = dt / (rc + dt);

        let channel_count = channels as usize;
        for frame in data.chunks_exact_mut(channel_count) {
            let frame_rms = (frame.iter().map(|sample| *sample * *sample).sum::<f32>()
                / channel_count as f32)
                .sqrt();
            let desired_agc_gain = if agc_enabled {
                // Keep silence near unity so room noise is not aggressively amplified.
                if frame_rms < 0.003 {
                    1.0
                } else {
                    (0.10 / frame_rms).clamp(0.5, 8.0)
                }
            } else {
                1.0
            };
            let smoothing = if desired_agc_gain < self.agc_gain {
                // Fast attack, slower release: speech becomes audible quickly without
                // pumping badly between words.
                0.08
            } else {
                0.015
            };
            self.agc_gain += (desired_agc_gain - self.agc_gain) * smoothing;
            let frame_gain = if agc_enabled { self.agc_gain } else { 1.0 };

            for (channel, sample) in frame.iter_mut().enumerate() {
                let mut x = *sample * frame_gain * gain;
                if lowpass_enabled {
                    let y_prev = self.last_samples[channel];
                    let y = y_prev + alpha * (x - y_prev);
                    self.last_samples[channel] = y;
                    x = y;
                }
                *sample = x.clamp(-1.0, 1.0);
            }
        }

        Ok(Some(Signal::Audio {
            sample_rate,
            channels,
            timestamp_us,
            data,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::AudioDspState;

    #[test]
    fn automatic_gain_control_is_enabled_by_default_and_toggleable() {
        let state = AudioDspState::new();
        assert!(state.agc_enabled());

        state.set_agc_enabled(false);
        assert!(!state.agc_enabled());
    }
}
