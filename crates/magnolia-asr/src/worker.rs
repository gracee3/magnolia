use crate::{
    AsrEvent, AsrEventBody, AsrEventHeader, DiscontinuityReason, ModelProvenance,
    ASR_EVENT_SCHEMA_MAJOR,
};
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender, TrySendError},
        Arc,
    },
    thread::{self, JoinHandle},
};
use thiserror::Error;
use uuid::Uuid;

pub const ASR_SAMPLE_RATE: u32 = 16_000;

#[derive(Debug)]
pub struct AudioPacket {
    pub start_frame: u64,
    pub monotonic_ns: u64,
    pub samples: Vec<f32>,
    pub discontinuity: Option<(DiscontinuityReason, u64)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendUpdate {
    Partial {
        segment_id: Uuid,
        revision: u64,
        text: String,
    },
    Final {
        segment_id: Uuid,
        revision: u64,
        text: String,
    },
}

pub trait StreamingRecognizer: Send + 'static {
    fn accept(&mut self, packet: &AudioPacket) -> Result<Vec<BackendUpdate>, String>;
    fn reset(&mut self);
}

enum WorkerCommand {
    Audio(AudioPacket),
    Stop { cancelled: bool },
}

pub struct AsrWorker {
    commands: SyncSender<WorkerCommand>,
    events: Receiver<AsrEvent>,
    pending_dropped_frames: Arc<AtomicU64>,
    worker: Option<JoinHandle<()>>,
}

impl AsrWorker {
    pub fn start<B: StreamingRecognizer>(
        backend: B,
        capacity: usize,
        session_id: Uuid,
        provenance: ModelProvenance,
    ) -> Result<Self, WorkerError> {
        if capacity == 0 {
            return Err(WorkerError::InvalidCapacity);
        }
        let (commands, command_rx) = mpsc::sync_channel(capacity);
        let (event_tx, events) = mpsc::channel();
        let pending_dropped_frames = Arc::new(AtomicU64::new(0));
        let worker = thread::Builder::new()
            .name("magnolia-asr".to_owned())
            .spawn(move || run_worker(backend, command_rx, event_tx, session_id, provenance))?;
        Ok(Self {
            commands,
            events,
            pending_dropped_frames,
            worker: Some(worker),
        })
    }

    /// Non-blocking publication suitable for the off-callback audio fanout.
    pub fn try_send(&self, mut packet: AudioPacket) -> Result<(), WorkerError> {
        let prior_loss = self.pending_dropped_frames.swap(0, Ordering::AcqRel);
        if prior_loss > 0 {
            packet.discontinuity = Some(match packet.discontinuity {
                Some((reason, lost)) => (reason, lost.saturating_add(prior_loss)),
                None => (DiscontinuityReason::Overflow, prior_loss),
            });
        }
        self.commands
            .try_send(WorkerCommand::Audio(packet))
            .map_err(|error| match error {
                TrySendError::Full(WorkerCommand::Audio(packet)) => {
                    self.pending_dropped_frames.fetch_add(
                        prior_loss.saturating_add(packet.samples.len() as u64),
                        Ordering::Relaxed,
                    );
                    WorkerError::QueueFull
                }
                TrySendError::Full(WorkerCommand::Stop { .. }) => WorkerError::QueueFull,
                TrySendError::Disconnected(_) => WorkerError::Unavailable,
            })
    }

    pub fn try_recv(&self) -> Result<Option<AsrEvent>, WorkerError> {
        match self.events.try_recv() {
            Ok(event) => Ok(Some(event)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(WorkerError::Unavailable),
        }
    }

    pub fn stop(mut self, cancelled: bool) -> Result<Vec<AsrEvent>, WorkerError> {
        self.commands
            .send(WorkerCommand::Stop { cancelled })
            .map_err(|_| WorkerError::Unavailable)?;
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| WorkerError::Panicked)?;
        }
        Ok(self.events.try_iter().collect())
    }
}

fn run_worker<B: StreamingRecognizer>(
    mut backend: B,
    commands: Receiver<WorkerCommand>,
    events: Sender<AsrEvent>,
    session_id: Uuid,
    provenance: ModelProvenance,
) {
    let mut sequence = 0_u64;
    let _ = events.send(make_event(
        session_id,
        provenance.clone(),
        &mut sequence,
        None,
        0,
        0,
        0,
        0,
        AsrEventBody::SessionStart,
    ));
    while let Ok(command) = commands.recv() {
        match command {
            WorkerCommand::Audio(packet) => {
                let end = packet
                    .start_frame
                    .saturating_add(packet.samples.len() as u64);
                if let Some((reason, lost_frames)) = packet.discontinuity {
                    backend.reset();
                    for body in [
                        AsrEventBody::Discontinuity {
                            reason,
                            lost_frames,
                        },
                        AsrEventBody::Reset { reason },
                    ] {
                        let _ = events.send(make_event(
                            session_id,
                            provenance.clone(),
                            &mut sequence,
                            None,
                            packet.monotonic_ns,
                            packet.start_frame,
                            end,
                            0,
                            body,
                        ));
                    }
                }
                match backend.accept(&packet) {
                    Ok(updates) => {
                        for update in updates {
                            let (segment, revision, body) = match update {
                                BackendUpdate::Partial {
                                    segment_id,
                                    revision,
                                    text,
                                } => (
                                    segment_id,
                                    revision,
                                    if revision == 0 {
                                        AsrEventBody::PartialCreate { text }
                                    } else {
                                        AsrEventBody::PartialRevise { text }
                                    },
                                ),
                                BackendUpdate::Final {
                                    segment_id,
                                    revision,
                                    text,
                                } => (
                                    segment_id,
                                    revision,
                                    AsrEventBody::Final {
                                        text,
                                        words: Vec::new(),
                                    },
                                ),
                            };
                            let _ = events.send(make_event(
                                session_id,
                                provenance.clone(),
                                &mut sequence,
                                Some(segment),
                                packet.monotonic_ns,
                                packet.start_frame,
                                end,
                                revision,
                                body,
                            ));
                        }
                    }
                    Err(message) => {
                        let _ = events.send(make_event(
                            session_id,
                            provenance.clone(),
                            &mut sequence,
                            None,
                            packet.monotonic_ns,
                            packet.start_frame,
                            end,
                            0,
                            AsrEventBody::Warning {
                                code: "backend_error".to_owned(),
                                message,
                            },
                        ));
                    }
                }
            }
            WorkerCommand::Stop { cancelled } => {
                let _ = events.send(make_event(
                    session_id,
                    provenance,
                    &mut sequence,
                    None,
                    0,
                    0,
                    0,
                    0,
                    AsrEventBody::SessionEnd { cancelled },
                ));
                break;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn make_event(
    session_id: Uuid,
    provenance: ModelProvenance,
    sequence: &mut u64,
    segment_id: Option<Uuid>,
    runtime_monotonic_ns: u64,
    audio_start_frame: u64,
    audio_end_frame: u64,
    revision: u64,
    body: AsrEventBody,
) -> AsrEvent {
    let current = *sequence;
    *sequence = sequence.saturating_add(1);
    AsrEvent {
        header: AsrEventHeader {
            schema_major: ASR_EVENT_SCHEMA_MAJOR,
            schema_minor: 0,
            session_id,
            segment_id,
            revision,
            sequence: current,
            runtime_monotonic_ns,
            audio_start_frame,
            audio_end_frame,
            provenance,
        },
        body,
    }
}

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("ASR worker capacity must be non-zero")]
    InvalidCapacity,
    #[error("ASR worker queue is full")]
    QueueFull,
    #[error("ASR worker is unavailable")]
    Unavailable,
    #[error("ASR worker panicked")]
    Panicked,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
