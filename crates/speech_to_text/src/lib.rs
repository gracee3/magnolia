//! Backend-neutral streaming speech-to-text contracts.
//!
//! The audio callback and the renderer should not know which recognizer is in
//! use. Backends consume normalized mono PCM on a worker and emit replaceable
//! partial hypotheses plus durable final events.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::Duration;

#[cfg(feature = "magnolia")]
mod processor;
#[cfg(feature = "sherpa")]
mod sherpa;

#[cfg(feature = "magnolia")]
pub use processor::{SttMetrics, SttMetricsSnapshot, SttProcessor};
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

impl SttEvent {
    /// Partial hypotheses may be replaced or dropped under backpressure;
    /// finals, status, and errors are loss-sensitive.
    pub fn is_replaceable(&self) -> bool {
        matches!(self, Self::Partial { .. })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SttQueueError {
    FullLossSensitive,
}

/// A bounded event queue that protects durable transcript and lifecycle events
/// from being displaced by an unbounded stream of partial hypotheses.
pub struct SttEventQueue {
    capacity: usize,
    events: VecDeque<SttEvent>,
    dropped_partials: u64,
}

impl SttEventQueue {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "STT event queue capacity must be positive");
        Self {
            capacity,
            events: VecDeque::with_capacity(capacity),
            dropped_partials: 0,
        }
    }

    pub fn push(&mut self, event: SttEvent) -> Result<(), SttQueueError> {
        if self.events.len() < self.capacity {
            self.events.push_back(event);
            return Ok(());
        }

        if event.is_replaceable() {
            self.dropped_partials += 1;
            return Ok(());
        }

        if let Some(index) = self.events.iter().position(SttEvent::is_replaceable) {
            self.events.remove(index);
            self.dropped_partials += 1;
            self.events.push_back(event);
            Ok(())
        } else {
            Err(SttQueueError::FullLossSensitive)
        }
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn dropped_partials(&self) -> u64 {
        self.dropped_partials
    }

    pub fn drain_into(&mut self, output: &mut Vec<SttEvent>) {
        output.extend(self.events.drain(..));
    }
}

pub trait SttBackend: Send + Sync {
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

    fn partial(sequence: u64) -> SttEvent {
        SttEvent::Partial {
            session_id: "s".into(),
            segment_id: 1,
            text: format!("p{sequence}"),
            audio_end_ms: sequence,
            sequence,
        }
    }

    fn final_event(sequence: u64) -> SttEvent {
        SttEvent::Final {
            session_id: "s".into(),
            segment_id: 1,
            text: "done".into(),
            start_ms: 0,
            end_ms: sequence,
            sequence,
        }
    }

    #[test]
    fn queue_drops_incoming_partials_when_full() {
        let mut queue = SttEventQueue::new(2);
        queue.push(partial(1)).unwrap();
        queue.push(partial(2)).unwrap();
        queue.push(partial(3)).unwrap();
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.dropped_partials(), 1);
    }

    #[test]
    fn queue_evicts_partial_for_loss_sensitive_event() {
        let mut queue = SttEventQueue::new(2);
        queue.push(partial(1)).unwrap();
        queue
            .push(SttEvent::Status {
                status: SttStatus::Listening,
            })
            .unwrap();
        queue.push(final_event(3)).unwrap();

        let mut events = Vec::new();
        queue.drain_into(&mut events);
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], SttEvent::Status { .. }));
        assert!(matches!(events[1], SttEvent::Final { .. }));
        assert_eq!(queue.dropped_partials(), 1);
    }

    #[test]
    fn queue_rejects_when_only_loss_sensitive_events_remain() {
        let mut queue = SttEventQueue::new(2);
        queue.push(final_event(1)).unwrap();
        queue
            .push(SttEvent::Status {
                status: SttStatus::Listening,
            })
            .unwrap();
        assert_eq!(
            queue.push(final_event(2)),
            Err(SttQueueError::FullLossSensitive)
        );
    }
}
