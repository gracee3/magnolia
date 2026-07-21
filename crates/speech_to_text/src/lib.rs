//! Backend-neutral streaming speech-to-text contracts.
//!
//! The audio callback and the renderer should not know which recognizer is in
//! use. Backends consume normalized mono PCM on a worker and emit replaceable
//! partial hypotheses plus durable final events.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[cfg(feature = "sherpa")]
mod sherpa;

#[cfg(feature = "sherpa")]
pub use sherpa::{LocalSherpaBackend, SherpaConfig};

#[derive(Debug, Clone, PartialEq)]
pub struct AudioChunk {
    pub sample_rate: u32,
    pub samples: Vec<f32>,
    pub timestamp: Duration,
}

impl AudioChunk {
    pub fn mono_16khz(samples: Vec<f32>, timestamp: Duration) -> Self {
        Self {
            sample_rate: 16_000,
            samples,
            timestamp,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SttStatus {
    Starting,
    Listening,
    Paused,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SttEvent {
    Partial {
        session_id: String,
        segment_id: u64,
        text: String,
        audio_end_ms: u64,
        sequence: u64,
    },
    Final {
        session_id: String,
        segment_id: u64,
        text: String,
        start_ms: u64,
        end_ms: u64,
        sequence: u64,
    },
    Status {
        status: SttStatus,
    },
    Error {
        message: String,
    },
}

pub trait SttBackend: Send {
    fn start(&mut self, session_id: &str) -> Result<()>;
    fn push_audio(&mut self, audio: AudioChunk) -> Result<()>;
    fn finish_utterance(&mut self) -> Result<()>;
    fn reset(&mut self) -> Result<()>;
    fn poll_events(&mut self, output: &mut Vec<SttEvent>) -> Result<()>;
    fn shutdown(&mut self);
}

/// A backend useful for reducer, routing, and demo tests before a model exists.
#[derive(Default)]
pub struct MockBackend {
    events: Vec<SttEvent>,
}

impl MockBackend {
    pub fn push_event(&mut self, event: SttEvent) {
        self.events.push(event);
    }
}

impl SttBackend for MockBackend {
    fn start(&mut self, session_id: &str) -> Result<()> {
        self.events.push(SttEvent::Status {
            status: SttStatus::Starting,
        });
        self.events.push(SttEvent::Status {
            status: SttStatus::Listening,
        });
        let _ = session_id;
        Ok(())
    }
    fn push_audio(&mut self, _audio: AudioChunk) -> Result<()> {
        Ok(())
    }
    fn finish_utterance(&mut self) -> Result<()> {
        Ok(())
    }
    fn reset(&mut self) -> Result<()> {
        self.events.clear();
        Ok(())
    }
    fn poll_events(&mut self, output: &mut Vec<SttEvent>) -> Result<()> {
        output.append(&mut self.events);
        Ok(())
    }
    fn shutdown(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_events_are_drained_in_order() {
        let mut backend = MockBackend::default();
        backend.push_event(SttEvent::Partial {
            session_id: "s".into(),
            segment_id: 1,
            text: "hello".into(),
            audio_end_ms: 200,
            sequence: 1,
        });
        let mut events = Vec::new();
        backend.poll_events(&mut events).unwrap();
        assert_eq!(events.len(), 1);
        assert!(backend.events.is_empty());
    }
}
