use super::{AudioChunk, SttBackend, SttEvent, SttStatus};
use anyhow::{bail, Result};
use sherpa_onnx::{OnlineRecognizer, OnlineRecognizerConfig, OnlineStream};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct SherpaConfig {
    pub encoder: PathBuf,
    pub decoder: PathBuf,
    pub joiner: PathBuf,
    pub tokens: PathBuf,
    pub num_threads: i32,
    pub endpointing: bool,
}

pub struct LocalSherpaBackend {
    config: SherpaConfig,
    recognizer: Option<OnlineRecognizer>,
    stream: Option<OnlineStream>,
    session_id: String,
    segment_id: u64,
    sequence: u64,
    events: VecDeque<SttEvent>,
    segment_start: Duration,
    segment_end: Duration,
}

impl LocalSherpaBackend {
    pub fn new(config: SherpaConfig) -> Self {
        Self {
            config,
            recognizer: None,
            stream: None,
            session_id: String::new(),
            segment_id: 0,
            sequence: 0,
            events: VecDeque::new(),
            segment_start: Duration::ZERO,
            segment_end: Duration::ZERO,
        }
    }

    fn emit_hypothesis(&mut self, audio_end: Duration) {
        let (Some(recognizer), Some(stream)) = (&self.recognizer, &self.stream) else {
            return;
        };
        let Some(result) = recognizer.get_result(stream) else {
            return;
        };
        self.sequence += 1;
        self.events.push_back(SttEvent::Partial {
            session_id: self.session_id.clone(),
            segment_id: self.segment_id,
            text: result.text,
            audio_end_ms: audio_end.as_millis() as u64,
            sequence: self.sequence,
        });
    }
}

impl SttBackend for LocalSherpaBackend {
    fn start(&mut self, session_id: &str) -> Result<()> {
        let mut config = OnlineRecognizerConfig::default();
        config.model_config.transducer.encoder = Some(self.config.encoder.display().to_string());
        config.model_config.transducer.decoder = Some(self.config.decoder.display().to_string());
        config.model_config.transducer.joiner = Some(self.config.joiner.display().to_string());
        config.model_config.tokens = Some(self.config.tokens.display().to_string());
        config.model_config.num_threads = self.config.num_threads;
        config.enable_endpoint = self.config.endpointing;
        config.decoding_method = Some("greedy_search".into());
        let recognizer = OnlineRecognizer::create(&config)
            .ok_or_else(|| anyhow::anyhow!("failed to create Sherpa recognizer"))?;
        self.stream = Some(recognizer.create_stream());
        self.recognizer = Some(recognizer);
        self.session_id = session_id.to_string();
        self.segment_id = 0;
        self.sequence = 0;
        self.events.push_back(SttEvent::Status {
            status: SttStatus::Listening,
        });
        Ok(())
    }

    fn push_audio(&mut self, audio: AudioChunk) -> Result<()> {
        if audio.sample_rate != 16_000 {
            bail!("Sherpa backend requires 16 kHz mono audio")
        }
        let (Some(recognizer), Some(stream)) = (&self.recognizer, &self.stream) else {
            bail!("Sherpa backend is not started")
        };
        stream.accept_waveform(audio.sample_rate as i32, &audio.samples);
        while recognizer.is_ready(stream) {
            recognizer.decode(stream);
        }
        self.segment_end =
            audio.timestamp + Duration::from_secs_f32(audio.samples.len() as f32 / 16_000.0);
        self.emit_hypothesis(self.segment_end);
        Ok(())
    }

    fn finish_utterance(&mut self) -> Result<()> {
        let (Some(recognizer), Some(stream)) = (&self.recognizer, &self.stream) else {
            return Ok(());
        };
        stream.input_finished();
        while recognizer.is_ready(stream) {
            recognizer.decode(stream);
        }
        let result = recognizer
            .get_result(stream)
            .ok_or_else(|| anyhow::anyhow!("Sherpa returned no final result"))?;
        self.sequence += 1;
        self.events.push_back(SttEvent::Final {
            session_id: self.session_id.clone(),
            segment_id: self.segment_id,
            text: result.text,
            start_ms: self.segment_start.as_millis() as u64,
            end_ms: self.segment_end.as_millis() as u64,
            sequence: self.sequence,
        });
        self.segment_id += 1;
        self.stream = Some(recognizer.create_stream());
        self.segment_start = self.segment_end;
        Ok(())
    }

    fn reset(&mut self) -> Result<()> {
        self.stream = self
            .recognizer
            .as_ref()
            .map(OnlineRecognizer::create_stream);
        self.events.clear();
        Ok(())
    }
    fn poll_events(&mut self, output: &mut Vec<SttEvent>) -> Result<()> {
        output.extend(self.events.drain(..));
        Ok(())
    }
    fn shutdown(&mut self) {
        self.stream = None;
        self.recognizer = None;
    }
}
