use async_trait::async_trait;

use talisman_core::{Processor, ModuleSchema, Port, PortDirection, DataType, Signal};

/// Simple DSP processor that applies gain to audio buffers.
pub struct AudioDspProcessor {
    id: String,
    enabled: bool,
    gain: f32,
}

impl AudioDspProcessor {
    pub fn new(id: &str, gain: f32) -> Self {
        Self {
            id: id.to_string(),
            enabled: true,
            gain,
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
            description: "Applies gain to audio buffers".to_string(),
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

        for sample in &mut data {
            *sample *= self.gain;
        }

        Ok(Some(Signal::Audio {
            sample_rate,
            channels,
            timestamp_us,
            data,
        }))
    }
}
