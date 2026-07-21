use super::{AudioChunk, SttBackend, SttEvent};
use async_trait::async_trait;
use magnolia_core::{DataType, ModuleSchema, Port, PortDirection, Processor, Signal};

/// Magnolia adapter for a streaming STT backend.
///
/// The processor receives ordinary routed audio buffers, performs the cheap
/// downmix/resample step on its worker, and emits one serialized STT event at
/// a time. Model inference never runs in the audio capture callback.
pub struct SttProcessor {
    id: String,
    enabled: bool,
    backend: Box<dyn SttBackend>,
    started: bool,
}

impl SttProcessor {
    pub fn new(id: &str, backend: Box<dyn SttBackend>) -> Self {
        Self {
            id: id.to_string(),
            enabled: true,
            backend,
            started: false,
        }
    }

    fn event_signal(event: SttEvent) -> anyhow::Result<Signal> {
        Ok(Signal::Computed {
            source: "speech_to_text".to_string(),
            content: serde_json::to_string(&event)?,
        })
    }
}

#[async_trait]
impl Processor for SttProcessor {
    fn name(&self) -> &str {
        "Speech to Text"
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            id: self.id.clone(),
            name: "Speech to Text".to_string(),
            description: "Streaming microphone transcription with replaceable partial hypotheses"
                .to_string(),
            ports: vec![
                Port {
                    id: "audio_in".to_string(),
                    label: "Audio In".to_string(),
                    data_type: DataType::Audio,
                    direction: PortDirection::Input,
                },
                Port {
                    id: "text_out".to_string(),
                    label: "Text Events".to_string(),
                    data_type: DataType::Text,
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
            data,
        } = signal
        else {
            return Ok(None);
        };
        if !self.started {
            self.backend.start(&self.id)?;
            self.started = true;
        }
        let audio = normalize_audio(sample_rate, channels, &data, timestamp_us)?;
        self.backend.push_audio(audio)?;
        let mut events = Vec::new();
        self.backend.poll_events(&mut events)?;
        // Keep the newest event. The backend emits partials frequently, and
        // the router/display treats them as replaceable state.
        events.pop().map(Self::event_signal).transpose()
    }
}

fn normalize_audio(
    sample_rate: u32,
    channels: u16,
    interleaved: &[f32],
    timestamp_us: u64,
) -> anyhow::Result<AudioChunk> {
    anyhow::ensure!(sample_rate > 0, "audio sample rate must be non-zero");
    anyhow::ensure!(channels > 0, "audio channel count must be non-zero");
    let channels = channels as usize;
    let frames = interleaved.len() / channels;
    anyhow::ensure!(frames > 0, "audio buffer is empty");

    let mono: Vec<f32> = interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32)
        .collect();
    let samples = if sample_rate == 16_000 {
        mono
    } else {
        let output_len = ((mono.len() as u64 * 16_000) / sample_rate as u64).max(1) as usize;
        (0..output_len)
            .map(|i| {
                let position = i as f32 * sample_rate as f32 / 16_000.0;
                let left = position.floor() as usize;
                let right = (left + 1).min(mono.len() - 1);
                let fraction = position - left as f32;
                mono[left] * (1.0 - fraction) + mono[right] * fraction
            })
            .collect()
    };
    Ok(AudioChunk::mono_16khz(
        samples,
        std::time::Duration::from_micros(timestamp_us),
    ))
}

#[cfg(test)]
mod tests {
    use super::normalize_audio;

    #[test]
    fn normalize_audio_downmixes_and_resamples() {
        let audio = normalize_audio(8_000, 2, &[1.0, 0.0, 0.0, 1.0], 10).unwrap();
        assert_eq!(audio.sample_rate, 16_000);
        assert_eq!(audio.samples.len(), 4);
        assert_eq!(audio.timestamp.as_micros(), 10);
    }
}
