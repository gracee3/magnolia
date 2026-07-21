use super::{AudioChunk, SttBackend, SttEvent, SttEventQueue, SttQueueError};
use async_trait::async_trait;
use magnolia_core::{DataType, ModuleSchema, Port, PortDirection, Processor, Signal};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Default)]
pub struct SttMetrics {
    pub audio_chunks: AtomicU64,
    pub emitted_events: AtomicU64,
    pub dropped_partials: AtomicU64,
    pub backend_errors: AtomicU64,
    pub queue_overflows: AtomicU64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SttMetricsSnapshot {
    pub audio_chunks: u64,
    pub emitted_events: u64,
    pub dropped_partials: u64,
    pub backend_errors: u64,
    pub queue_overflows: u64,
}

impl SttMetrics {
    pub fn snapshot(&self) -> SttMetricsSnapshot {
        SttMetricsSnapshot {
            audio_chunks: self.audio_chunks.load(Ordering::Relaxed),
            emitted_events: self.emitted_events.load(Ordering::Relaxed),
            dropped_partials: self.dropped_partials.load(Ordering::Relaxed),
            backend_errors: self.backend_errors.load(Ordering::Relaxed),
            queue_overflows: self.queue_overflows.load(Ordering::Relaxed),
        }
    }
}

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
    events: SttEventQueue,
    metrics: Arc<SttMetrics>,
}

impl SttProcessor {
    pub fn new(id: &str, backend: Box<dyn SttBackend>) -> Self {
        Self {
            id: id.to_string(),
            enabled: true,
            backend,
            started: false,
            events: SttEventQueue::new(64),
            metrics: Arc::new(SttMetrics::default()),
        }
    }

    pub fn metrics(&self) -> Arc<SttMetrics> {
        self.metrics.clone()
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
        self.metrics.audio_chunks.fetch_add(1, Ordering::Relaxed);
        if !self.started {
            if let Err(error) = self.backend.start(&self.id) {
                self.metrics.backend_errors.fetch_add(1, Ordering::Relaxed);
                return Err(error);
            }
            self.started = true;
        }
        let audio = normalize_audio(sample_rate, channels, &data, timestamp_us)?;
        if let Err(error) = self.backend.push_audio(audio) {
            self.metrics.backend_errors.fetch_add(1, Ordering::Relaxed);
            return Err(error);
        }
        let mut polled = Vec::new();
        if let Err(error) = self.backend.poll_events(&mut polled) {
            self.metrics.backend_errors.fetch_add(1, Ordering::Relaxed);
            return Err(error);
        }
        for event in polled {
            let result = self.events.push(event);
            if let Err(error) = result {
                self.metrics.queue_overflows.fetch_add(1, Ordering::Relaxed);
                return Err(match error {
                    SttQueueError::FullLossSensitive => {
                        anyhow::anyhow!("STT event queue full of loss-sensitive events")
                    }
                });
            }
        }
        self.metrics
            .dropped_partials
            .store(self.events.dropped_partials(), Ordering::Relaxed);
        let mut events = Vec::new();
        self.events.drain_into(&mut events);
        // Keep the newest event. The backend emits partials frequently, and
        // the router/display treats them as replaceable state.
        let signal = events.pop().map(Self::event_signal).transpose()?;
        if signal.is_some() {
            self.metrics.emitted_events.fetch_add(1, Ordering::Relaxed);
        }
        Ok(signal)
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
