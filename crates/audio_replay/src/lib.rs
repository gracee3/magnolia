use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::time::Duration;

use magnolia_core::{DataType, ModuleSchema, Port, PortDirection, Signal, Source};

/// Deterministic WAV replay source for demos/tests.
///
/// Emits `Signal::Audio` chunks with the WAV's sample rate/channels.
/// (Downstream modules can resample; e.g. parakeet_stt handles it internally.)
pub struct WavReplaySource {
    id: String,
    enabled: bool,
    wav_path: PathBuf,
    chunk_ms: u32,
    realtime: bool,

    started: bool,
    pos: usize,
    sample_rate: u32,
    channels: u16,
    audio: Vec<f32>,
    t0_us: u64,
}

impl WavReplaySource {
    pub fn new(id: &str, wav_path: PathBuf, chunk_ms: u32, realtime: bool) -> anyhow::Result<Self> {
        let (sample_rate, channels, audio) = load_wav_f32(&wav_path)?;
        Ok(Self {
            id: id.to_string(),
            enabled: true,
            wav_path,
            chunk_ms: chunk_ms.max(10),
            realtime,
            started: false,
            pos: 0,
            sample_rate,
            channels,
            audio,
            t0_us: 0,
        })
    }
}

/// Load a WAV into interleaved f32 samples (normalized to [-1,1] for PCM int input).
///
/// This is intentionally simple and deterministic for testing.
pub fn load_wav_f32(path: &Path) -> anyhow::Result<(u32, u16, Vec<f32>)> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let sr = spec.sample_rate;
    let ch = spec.channels;

    let audio: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap()).collect(),
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            if bits == 0 || bits > 32 {
                anyhow::bail!("Unsupported PCM bit depth: {}", bits);
            }
            let denom = (1u64 << (bits - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.unwrap() as f32 / denom)
                .collect()
        }
    };

    Ok((sr, ch, audio))
}

/// Deterministically chunk an interleaved audio buffer into `Signal::Audio` events.
pub fn chunk_audio_signals(
    sample_rate: u32,
    channels: u16,
    audio_interleaved: &[f32],
    chunk_ms: u32,
) -> Vec<Signal> {
    let chunk_ms = chunk_ms.max(10);
    let samples_per_chunk = (sample_rate as u64 * chunk_ms as u64 / 1000) as usize;
    let take = (samples_per_chunk * channels as usize).max(1);
    let mut pos = 0usize;
    let mut out = Vec::new();
    while pos < audio_interleaved.len() {
        let end = (pos + take).min(audio_interleaved.len());
        let data = audio_interleaved[pos..end].to_vec();
        let ts_us = (pos as u64 / channels as u64) * 1_000_000u64 / sample_rate as u64;
        out.push(Signal::Audio {
            sample_rate,
            channels,
            timestamp_us: ts_us,
            data,
        });
        pos = end;
    }
    out
}

/// Convenience wrapper: load WAV then produce deterministic `Signal::Audio` chunks.
pub fn load_wav_audio_signals(
    wav_path: &Path,
    chunk_ms: u32,
) -> anyhow::Result<(u32, u16, Vec<Signal>)> {
    let (sr, ch, audio) = load_wav_f32(wav_path)?;
    let signals = chunk_audio_signals(sr, ch, &audio, chunk_ms);
    Ok((sr, ch, signals))
}

#[async_trait]
impl Source for WavReplaySource {
    fn name(&self) -> &str {
        "WAV Replay"
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            id: self.id.clone(),
            name: "WAV Replay".to_string(),
            description: format!("Replays WAV audio from {}", self.wav_path.display()),
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

        if !self.started {
            self.started = true;
            self.t0_us = 0;
        }

        if self.pos >= self.audio.len() {
            // End: keep emitting pulses.
            tokio::time::sleep(Duration::from_millis(10)).await;
            return Some(Signal::Pulse);
        }

        let samples_per_chunk = (self.sample_rate as u64 * self.chunk_ms as u64 / 1000) as usize;
        let take = (samples_per_chunk * self.channels as usize).max(1);
        let end = (self.pos + take).min(self.audio.len());
        let data = self.audio[self.pos..end].to_vec();
        let ts_us = self.t0_us
            + ((self.pos as u64 / self.channels as u64) * 1_000_000u64 / self.sample_rate as u64);
        self.pos = end;

        if self.realtime {
            tokio::time::sleep(Duration::from_millis(self.chunk_ms as u64)).await;
        } else {
            tokio::task::yield_now().await;
        }

        Some(Signal::Audio {
            sample_rate: self.sample_rate,
            channels: self.channels,
            timestamp_us: ts_us,
            data,
        })
    }
}

