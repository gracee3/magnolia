use magnolia_domain::{DeliveryPolicy, EntityId, RuntimeEpochId, SchemaVersion};
use magnolia_observe::{AnalyzerFrame, AnalyzerKind, ObservationHub};
use magnolia_protocol::{
    encode_telemetry_payload, TelemetryClock, TelemetryEnvelope, TelemetryLease, TelemetryPayload,
    TelemetrySubscription, PROTOCOL_VERSION, SYNTHETIC_CAPTION_SESSION_ID,
    SYNTHETIC_CAPTION_STREAM_ID, SYNTHETIC_DIAGNOSTICS_STREAM_ID, SYNTHETIC_METER_STREAM_ID,
    SYNTHETIC_SPECTRUM_STREAM_ID, SYNTHETIC_WAVEFORM_STREAM_ID,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Arc, Mutex, MutexGuard},
    time::Instant,
};
use thiserror::Error;
use tokio::sync::Notify;

pub const METER_STREAM: EntityId = SYNTHETIC_METER_STREAM_ID;
pub const WAVEFORM_STREAM: EntityId = SYNTHETIC_WAVEFORM_STREAM_ID;
pub const SPECTRUM_STREAM: EntityId = SYNTHETIC_SPECTRUM_STREAM_ID;
pub const DIAGNOSTICS_STREAM: EntityId = SYNTHETIC_DIAGNOSTICS_STREAM_ID;
pub const CAPTION_STREAM: EntityId = SYNTHETIC_CAPTION_STREAM_ID;
pub const CAPTION_SESSION: EntityId = SYNTHETIC_CAPTION_SESSION_ID;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntheticStreamKind {
    Meter,
    Waveform,
    Spectrum,
    Diagnostics,
    Caption,
}

impl SyntheticStreamKind {
    #[must_use]
    pub const fn stream_id(self) -> EntityId {
        match self {
            Self::Meter => METER_STREAM,
            Self::Waveform => WAVEFORM_STREAM,
            Self::Spectrum => SPECTRUM_STREAM,
            Self::Diagnostics => DIAGNOSTICS_STREAM,
            Self::Caption => CAPTION_STREAM,
        }
    }

    #[must_use]
    pub const fn delivery(self) -> DeliveryPolicy {
        match self {
            Self::Meter | Self::Caption => DeliveryPolicy::Latest,
            Self::Waveform | Self::Spectrum | Self::Diagnostics => DeliveryPolicy::DropOldest,
        }
    }

    #[must_use]
    pub const fn all() -> [Self; 5] {
        [
            Self::Meter,
            Self::Waveform,
            Self::Spectrum,
            Self::Diagnostics,
            Self::Caption,
        ]
    }
}

fn stream_kind(stream_id: EntityId) -> Option<SyntheticStreamKind> {
    SyntheticStreamKind::all()
        .into_iter()
        .find(|kind| kind.stream_id() == stream_id)
}

#[derive(Clone)]
pub struct TelemetryHub {
    inner: Arc<Mutex<TelemetryState>>,
    started: Instant,
    observation: Option<ObservationHub>,
}

#[derive(Default)]
struct TelemetryState {
    sessions: BTreeMap<String, BTreeMap<EntityId, TelemetryLease>>,
    sequences: BTreeMap<EntityId, u64>,
    dropped: BTreeMap<EntityId, u64>,
    discontinuities: BTreeSet<EntityId>,
    diagnostic_entries_lost: u64,
    emission_accumulators: BTreeMap<(String, EntityId), u32>,
    active_connections: u64,
    total_connections: u64,
    released_leases: u64,
    flood_multiplier: u32,
}

impl Default for TelemetryHub {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(TelemetryState {
                flood_multiplier: 1,
                ..TelemetryState::default()
            })),
            started: Instant::now(),
            observation: None,
        }
    }
}

impl TelemetryHub {
    #[must_use]
    pub fn with_observation(observation: ObservationHub) -> Self {
        Self {
            observation: Some(observation),
            ..Self::default()
        }
    }

    pub fn subscribe(
        &self,
        session_id: &str,
        requested: TelemetrySubscription,
    ) -> Result<TelemetryLease, TelemetryError> {
        let kind = stream_kind(requested.stream_id)
            .ok_or(TelemetryError::UnknownStream(requested.stream_id))?;
        if requested.delivery != kind.delivery() {
            return Err(TelemetryError::DeliveryMismatch {
                stream_id: requested.stream_id,
                expected: kind.delivery(),
                received: requested.delivery,
            });
        }
        let lease = TelemetryLease {
            stream_id: requested.stream_id,
            negotiated_rate_hz: requested.requested_rate_hz.clamp(1, 30),
            capacity: requested.capacity.clamp(1, 64),
            delivery: requested.delivery,
        };
        let replaced = self
            .lock()?
            .sessions
            .entry(session_id.to_owned())
            .or_default()
            .insert(lease.stream_id, lease.clone())
            .is_some();
        if !replaced {
            self.acquire_observer(kind);
        }
        Ok(lease)
    }

    pub fn release(&self, session_id: &str, stream_id: EntityId) -> Result<bool, TelemetryError> {
        let mut state = self.lock()?;
        let released = state
            .sessions
            .get_mut(session_id)
            .is_some_and(|leases| leases.remove(&stream_id).is_some());
        if released {
            state.released_leases = state.released_leases.saturating_add(1);
            drop(state);
            if let Some(kind) = stream_kind(stream_id) {
                self.release_observer(kind);
            }
        }
        Ok(released)
    }

    pub fn mark_connection_open(&self) -> Result<(), TelemetryError> {
        let mut state = self.lock()?;
        state.active_connections = state.active_connections.saturating_add(1);
        state.total_connections = state.total_connections.saturating_add(1);
        Ok(())
    }

    /// Disconnects are lease boundaries. A reconnect must explicitly recreate
    /// its non-durable subscriptions from the browser's visible tiles.
    pub fn disconnect(&self, session_id: &str) -> Result<(), TelemetryError> {
        let mut state = self.lock()?;
        let released = release_session_leases(&mut state, session_id);
        state.active_connections = state.active_connections.saturating_sub(1);
        drop(state);
        for stream_id in released {
            if let Some(kind) = stream_kind(stream_id) {
                self.release_observer(kind);
            }
        }
        Ok(())
    }

    pub fn release_session_leases(&self, session_id: &str) -> Result<(), TelemetryError> {
        let mut state = self.lock()?;
        let released = release_session_leases(&mut state, session_id);
        drop(state);
        for stream_id in released {
            if let Some(kind) = stream_kind(stream_id) {
                self.release_observer(kind);
            }
        }
        Ok(())
    }

    pub fn set_flood_multiplier(&self, multiplier: u32) -> Result<(), TelemetryError> {
        self.lock()?.flood_multiplier = multiplier.clamp(1, 2_000);
        Ok(())
    }

    pub fn status(&self) -> Result<TelemetryStatus, TelemetryError> {
        let state = self.lock()?;
        Ok(TelemetryStatus {
            active_connections: state.active_connections,
            total_connections: state.total_connections,
            active_leases: state.sessions.values().map(BTreeMap::len).sum(),
            released_leases: state.released_leases,
            cumulative_dropped: state.dropped.values().copied().sum(),
            per_stream_dropped: state.dropped.clone(),
            flood_multiplier: state.flood_multiplier,
        })
    }

    pub(crate) fn generate_frames(
        &self,
        session_id: &str,
        epoch: RuntimeEpochId,
    ) -> Result<Vec<GeneratedFrame>, TelemetryError> {
        let mut state = self.lock()?;
        let leases: Vec<_> = state
            .sessions
            .get(session_id)
            .map(|leases| leases.values().cloned().collect())
            .unwrap_or_default();
        let multiplier = state.flood_multiplier;
        let now = u64::try_from(self.started.elapsed().as_nanos()).unwrap_or(u64::MAX);
        let mut frames = Vec::with_capacity(leases.len().saturating_mul(multiplier as usize));
        for lease in &leases {
            let accumulator = state
                .emission_accumulators
                .entry((session_id.to_owned(), lease.stream_id))
                .or_default();
            *accumulator = accumulator.saturating_add(lease.negotiated_rate_hz);
            if *accumulator < 30 {
                continue;
            }
            *accumulator -= 30;
            for _ in 0..multiplier {
                let sequence = state.sequences.entry(lease.stream_id).or_default();
                *sequence = sequence.saturating_add(1);
                let current = *sequence;
                let kind = stream_kind(lease.stream_id)
                    .ok_or(TelemetryError::UnknownStream(lease.stream_id))?;
                let diagnostic_entries_lost = if kind == SyntheticStreamKind::Diagnostics {
                    std::mem::take(&mut state.diagnostic_entries_lost)
                } else {
                    0
                };
                let native = self
                    .observation
                    .as_ref()
                    .and_then(|hub| analyzer_kind(kind).and_then(|kind| hub.latest(kind)));
                if self.observation.is_some()
                    && kind != SyntheticStreamKind::Caption
                    && native.is_none()
                {
                    continue;
                }
                let payload = native
                    .as_ref()
                    .map(native_payload)
                    .unwrap_or_else(|| synthetic_payload(kind, current, diagnostic_entries_lost));
                let discontinuity = state.discontinuities.remove(&lease.stream_id);
                let metadata = native.as_ref().map(frame_metadata);
                frames.push(GeneratedFrame {
                    envelope: TelemetryEnvelope {
                        protocol_version: PROTOCOL_VERSION,
                        runtime_epoch: epoch,
                        stream_id: lease.stream_id,
                        schema_version: SchemaVersion::new(1, 0),
                        clock: TelemetryClock::RuntimeMonotonic,
                        sequence: metadata.map_or(current, |value| value.0),
                        source_start: metadata
                            .map_or_else(|| current.saturating_mul(128), |value| value.1),
                        source_end: metadata.map_or_else(
                            || current.saturating_add(1).saturating_mul(128),
                            |value| value.2,
                        ),
                        emitted_monotonic_ns: metadata.map_or(now, |value| value.3),
                        queue_depth: 0,
                        cumulative_dropped: state
                            .dropped
                            .get(&lease.stream_id)
                            .copied()
                            .unwrap_or(0),
                        discontinuity: discontinuity || metadata.is_some_and(|value| value.4),
                        payload: encode_telemetry_payload(&payload)
                            .map_err(|error| TelemetryError::Encoding(error.to_string()))?,
                    },
                    delivery: lease.delivery,
                    capacity: usize::try_from(lease.capacity).unwrap_or(64),
                });
            }
        }
        Ok(frames)
    }

    pub(crate) fn record_drop(
        &self,
        stream_id: EntityId,
        count: u64,
    ) -> Result<u64, TelemetryError> {
        let mut state = self.lock()?;
        let total = state.dropped.entry(stream_id).or_default();
        *total = total.saturating_add(count);
        let current = *total;
        state.discontinuities.insert(stream_id);
        if stream_id == DIAGNOSTICS_STREAM {
            state.diagnostic_entries_lost = state
                .diagnostic_entries_lost
                .saturating_add(count.saturating_mul(DIAGNOSTIC_ENTRIES_PER_FRAME));
        }
        Ok(current)
    }

    fn lock(&self) -> Result<MutexGuard<'_, TelemetryState>, TelemetryError> {
        self.inner.lock().map_err(|_| TelemetryError::Poisoned)
    }

    fn acquire_observer(&self, kind: SyntheticStreamKind) {
        if let (Some(hub), Some(kind)) = (&self.observation, analyzer_kind(kind)) {
            hub.acquire(kind);
        }
    }

    fn release_observer(&self, kind: SyntheticStreamKind) {
        if let (Some(hub), Some(kind)) = (&self.observation, analyzer_kind(kind)) {
            hub.release(kind);
        }
    }
}

fn release_session_leases(state: &mut TelemetryState, session_id: &str) -> Vec<EntityId> {
    if let Some(leases) = state.sessions.remove(session_id) {
        state.released_leases = state
            .released_leases
            .saturating_add(u64::try_from(leases.len()).unwrap_or(u64::MAX));
        let released = leases.into_keys().collect();
        state
            .emission_accumulators
            .retain(|(candidate, _), _| candidate != session_id);
        return released;
    }
    state
        .emission_accumulators
        .retain(|(candidate, _), _| candidate != session_id);
    Vec::new()
}

const DIAGNOSTIC_ENTRIES_PER_FRAME: u64 = 2;

fn analyzer_kind(kind: SyntheticStreamKind) -> Option<AnalyzerKind> {
    match kind {
        SyntheticStreamKind::Meter => Some(AnalyzerKind::Meter),
        SyntheticStreamKind::Waveform => Some(AnalyzerKind::Waveform),
        SyntheticStreamKind::Spectrum => Some(AnalyzerKind::Spectrum),
        SyntheticStreamKind::Diagnostics => Some(AnalyzerKind::Diagnostics),
        SyntheticStreamKind::Caption => None,
    }
}

fn frame_metadata(frame: &AnalyzerFrame) -> (u64, u64, u64, u64, bool) {
    let header = match frame {
        AnalyzerFrame::Meter(frame) => &frame.header,
        AnalyzerFrame::Waveform(frame) => &frame.header,
        AnalyzerFrame::Spectrum(frame) => &frame.header,
        AnalyzerFrame::Diagnostics(frame) => &frame.header,
    };
    (
        header.sequence,
        header.source_start,
        header.source_end,
        header.analyzer_monotonic_ns,
        header.discontinuity,
    )
}

fn native_payload(frame: &AnalyzerFrame) -> TelemetryPayload {
    match frame {
        AnalyzerFrame::Meter(frame) => TelemetryPayload::Meter {
            level_milli: normalized_milli(frame.rms.iter().copied().fold(0.0, f32::max)),
            peak_milli: normalized_milli(frame.peak.iter().copied().fold(0.0, f32::max)),
        },
        AnalyzerFrame::Waveform(frame) => TelemetryPayload::Waveform {
            samples: frame
                .envelopes
                .iter()
                .flat_map(|value| [value.minimum, value.maximum])
                .map(normalized_i16)
                .collect(),
        },
        AnalyzerFrame::Spectrum(frame) => TelemetryPayload::Spectrum {
            bins: frame
                .bins_db
                .iter()
                .map(|value| normalized_milli((value.clamp(-120.0, 0.0) + 120.0) / 120.0))
                .collect(),
        },
        AnalyzerFrame::Diagnostics(frame) => TelemetryPayload::Diagnostics {
            entries: vec![magnolia_protocol::DiagnosticTelemetryEntry {
                sequence: frame.header.sequence,
                severity: magnolia_protocol::DiagnosticSeverity::Info,
                message: format!(
                    "native queue={} utilization={}ppm processing={}ns latency={}ns discontinuities={}",
                    frame.queue_depth,
                    frame.utilization_millionths,
                    frame.processing_ns,
                    frame.latency_ns,
                    frame.cumulative_discontinuities
                ),
            }],
            lost_since_previous: 0,
        },
    }
}

fn normalized_milli(value: f32) -> u16 {
    (value.clamp(0.0, 1.0) * 1_000.0).round() as u16
}

fn normalized_i16(value: f32) -> i16 {
    (value.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
}

fn synthetic_payload(
    kind: SyntheticStreamKind,
    sequence: u64,
    diagnostics_lost: u64,
) -> TelemetryPayload {
    let phase = u16::try_from(sequence % 200).unwrap_or(0);
    match kind {
        SyntheticStreamKind::Meter => TelemetryPayload::Meter {
            level_milli: if phase <= 100 {
                phase.saturating_mul(9)
            } else {
                (200 - phase).saturating_mul(9)
            },
            peak_milli: 930,
        },
        SyntheticStreamKind::Waveform => {
            let samples = (0_u64..128)
                .map(|index| {
                    let saw = ((index + sequence.saturating_mul(3)) % 64) as i32 - 32;
                    i16::try_from(saw.saturating_mul(900)).unwrap_or(0)
                })
                .collect();
            TelemetryPayload::Waveform { samples }
        }
        SyntheticStreamKind::Spectrum => {
            let center = usize::try_from(sequence % 48).unwrap_or(0) + 8;
            let bins = (0_usize..64)
                .map(|index| {
                    let distance = index.abs_diff(center).min(24);
                    u16::try_from((24 - distance) * 38).unwrap_or(0)
                })
                .collect();
            TelemetryPayload::Spectrum { bins }
        }
        SyntheticStreamKind::Diagnostics => {
            let first = sequence.saturating_mul(DIAGNOSTIC_ENTRIES_PER_FRAME);
            TelemetryPayload::Diagnostics {
                entries: [first.saturating_sub(1), first]
                    .into_iter()
                    .map(
                        |entry_sequence| magnolia_protocol::DiagnosticTelemetryEntry {
                            sequence: entry_sequence,
                            severity: magnolia_protocol::DiagnosticSeverity::Info,
                            message: format!("synthetic runtime tick {entry_sequence}"),
                        },
                    )
                    .collect(),
                lost_since_previous: diagnostics_lost,
            }
        }
        SyntheticStreamKind::Caption => TelemetryPayload::PartialCaption {
            segment_id: EntityId::from_u128(0x2_200 + u128::from(sequence / 30)),
            segment_revision: sequence % 30,
            text: format!("synthetic phrase {:02}", sequence / 30),
        },
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TelemetryStatus {
    pub active_connections: u64,
    pub total_connections: u64,
    pub active_leases: usize,
    pub released_leases: u64,
    pub cumulative_dropped: u64,
    pub per_stream_dropped: BTreeMap<EntityId, u64>,
    pub flood_multiplier: u32,
}

pub(crate) struct GeneratedFrame {
    pub envelope: TelemetryEnvelope,
    pub delivery: DeliveryPolicy,
    pub capacity: usize,
}

#[derive(Debug, Error)]
pub enum TelemetryError {
    #[error("unknown synthetic telemetry stream {0}")]
    UnknownStream(EntityId),
    #[error("stream {stream_id} requires delivery {expected:?}, received {received:?}")]
    DeliveryMismatch {
        stream_id: EntityId,
        expected: DeliveryPolicy,
        received: DeliveryPolicy,
    },
    #[error("telemetry state lock was poisoned")]
    Poisoned,
    #[error("telemetry encoding failed: {0}")]
    Encoding(String),
}

pub(crate) struct BoundedTelemetryQueue {
    state: Mutex<QueueState>,
    notify: Notify,
}

struct QueueState {
    capacity: usize,
    closed: bool,
    frames: VecDeque<TelemetryEnvelope>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct DropReport {
    pub total: u64,
    pub per_stream: BTreeMap<EntityId, u64>,
}

impl BoundedTelemetryQueue {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            state: Mutex::new(QueueState {
                capacity: capacity.max(1),
                closed: false,
                frames: VecDeque::new(),
            }),
            notify: Notify::new(),
        }
    }

    pub(crate) fn push(
        &self,
        mut frame: TelemetryEnvelope,
        delivery: DeliveryPolicy,
        stream_capacity: usize,
    ) -> Result<DropReport, TelemetryError> {
        let mut state = self.state.lock().map_err(|_| TelemetryError::Poisoned)?;
        if state.closed {
            return Ok(DropReport::default());
        }
        let mut report = DropReport::default();
        if delivery == DeliveryPolicy::Latest {
            let mut retained = VecDeque::with_capacity(state.frames.len());
            while let Some(queued) = state.frames.pop_front() {
                if queued.stream_id == frame.stream_id {
                    record_queue_drop(&mut report, queued.stream_id);
                } else {
                    retained.push_back(queued);
                }
            }
            state.frames = retained;
        }
        let stream_capacity = stream_capacity.clamp(1, state.capacity);
        while state
            .frames
            .iter()
            .filter(|queued| queued.stream_id == frame.stream_id)
            .count()
            >= stream_capacity
        {
            let Some(remove_at) = state
                .frames
                .iter()
                .position(|queued| queued.stream_id == frame.stream_id)
            else {
                break;
            };
            if let Some(removed) = state.frames.remove(remove_at) {
                record_queue_drop(&mut report, removed.stream_id);
            }
        }
        while state.frames.len() >= state.capacity {
            let remove_at = if delivery == DeliveryPolicy::DropOldest {
                state
                    .frames
                    .iter()
                    .position(|queued| queued.stream_id == frame.stream_id)
                    .unwrap_or(0)
            } else {
                0
            };
            if let Some(removed) = state.frames.remove(remove_at) {
                record_queue_drop(&mut report, removed.stream_id);
            }
        }
        let dropped_for_frame = report
            .per_stream
            .get(&frame.stream_id)
            .copied()
            .unwrap_or(0);
        if dropped_for_frame > 0 {
            frame.discontinuity = true;
            frame.cumulative_dropped = frame.cumulative_dropped.saturating_add(dropped_for_frame);
        }
        state.frames.push_back(frame);
        drop(state);
        self.notify.notify_one();
        Ok(report)
    }

    pub(crate) async fn pop(&self) -> Result<Option<TelemetryEnvelope>, TelemetryError> {
        loop {
            let listener = self.notify.notified();
            {
                let mut state = self.state.lock().map_err(|_| TelemetryError::Poisoned)?;
                if let Some(mut frame) = state.frames.pop_front() {
                    frame.queue_depth = u32::try_from(state.frames.len()).unwrap_or(u32::MAX);
                    return Ok(Some(frame));
                }
                if state.closed {
                    return Ok(None);
                }
            }
            listener.await;
        }
    }

    pub(crate) fn close(&self) -> Result<(), TelemetryError> {
        self.state
            .lock()
            .map_err(|_| TelemetryError::Poisoned)?
            .closed = true;
        self.notify.notify_waiters();
        Ok(())
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.state.lock().unwrap().frames.len()
    }
}

fn record_queue_drop(report: &mut DropReport, stream_id: EntityId) {
    report.total = report.total.saturating_add(1);
    let dropped = report.per_stream.entry(stream_id).or_default();
    *dropped = dropped.saturating_add(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(stream_id: EntityId, sequence: u64) -> TelemetryEnvelope {
        TelemetryEnvelope {
            protocol_version: PROTOCOL_VERSION,
            runtime_epoch: RuntimeEpochId::from_u128(1),
            stream_id,
            schema_version: SchemaVersion::new(1, 0),
            clock: TelemetryClock::RuntimeMonotonic,
            sequence,
            source_start: sequence,
            source_end: sequence + 1,
            emitted_monotonic_ns: sequence,
            queue_depth: 0,
            cumulative_dropped: 0,
            discontinuity: false,
            payload: Vec::new(),
        }
    }

    #[tokio::test]
    async fn bounded_queue_replaces_latest_and_drops_oldest_without_growing() {
        let queue = BoundedTelemetryQueue::new(2);
        assert_eq!(
            queue
                .push(frame(METER_STREAM, 1), DeliveryPolicy::Latest, 2)
                .unwrap(),
            DropReport::default()
        );
        assert_eq!(
            queue
                .push(frame(METER_STREAM, 2), DeliveryPolicy::Latest, 2)
                .unwrap()
                .per_stream
                .get(&METER_STREAM),
            Some(&1)
        );
        assert_eq!(queue.len(), 1);
        assert_eq!(
            queue
                .push(frame(WAVEFORM_STREAM, 3), DeliveryPolicy::DropOldest, 1)
                .unwrap(),
            DropReport::default()
        );
        assert_eq!(
            queue
                .push(frame(WAVEFORM_STREAM, 4), DeliveryPolicy::DropOldest, 1)
                .unwrap()
                .per_stream
                .get(&WAVEFORM_STREAM),
            Some(&1)
        );
        assert_eq!(queue.len(), 2);
        let first = queue.pop().await.unwrap().unwrap();
        let second = queue.pop().await.unwrap().unwrap();
        assert_eq!((first.sequence, second.sequence), (2, 4));
        assert!(second.discontinuity);
    }

    #[test]
    fn leases_are_bounded_typed_and_released_on_disconnect() {
        let hub = TelemetryHub::default();
        let session = "session";
        let lease = hub
            .subscribe(
                session,
                TelemetrySubscription {
                    stream_id: WAVEFORM_STREAM,
                    requested_rate_hz: 1_000,
                    capacity: 1_000,
                    delivery: DeliveryPolicy::DropOldest,
                },
            )
            .unwrap();
        assert_eq!(lease.negotiated_rate_hz, 30);
        assert_eq!(lease.capacity, 64);
        hub.mark_connection_open().unwrap();
        assert_eq!(hub.status().unwrap().active_leases, 1);
        hub.disconnect(session).unwrap();
        let status = hub.status().unwrap();
        assert_eq!(status.active_connections, 0);
        assert_eq!(status.active_leases, 0);
        assert_eq!(status.released_leases, 1);
    }

    #[test]
    fn native_analyzer_leases_track_unique_browser_subscriptions() {
        let observation = ObservationHub::default();
        let hub = TelemetryHub::with_observation(observation.clone());
        let request = TelemetrySubscription {
            stream_id: METER_STREAM,
            requested_rate_hz: 30,
            capacity: 4,
            delivery: DeliveryPolicy::Latest,
        };
        hub.subscribe("browser", request.clone()).unwrap();
        hub.subscribe("browser", request).unwrap();
        assert_eq!(observation.lease_count(AnalyzerKind::Meter), 1);
        hub.release_session_leases("browser").unwrap();
        assert_eq!(observation.lease_count(AnalyzerKind::Meter), 0);
    }

    #[test]
    fn negotiated_rate_capacity_and_diagnostic_loss_are_observable() {
        let hub = TelemetryHub::default();
        let session = "diagnostics";
        hub.subscribe(
            session,
            TelemetrySubscription {
                stream_id: DIAGNOSTICS_STREAM,
                requested_rate_hz: 10,
                capacity: 1,
                delivery: DeliveryPolicy::DropOldest,
            },
        )
        .unwrap();
        hub.set_flood_multiplier(3).unwrap();
        let epoch = RuntimeEpochId::from_u128(9);
        assert!(hub.generate_frames(session, epoch).unwrap().is_empty());
        assert!(hub.generate_frames(session, epoch).unwrap().is_empty());
        let burst = hub.generate_frames(session, epoch).unwrap();
        assert_eq!(burst.len(), 3);

        let queue = BoundedTelemetryQueue::new(64);
        for generated in burst {
            let report = queue
                .push(generated.envelope, generated.delivery, generated.capacity)
                .unwrap();
            for (stream_id, count) in report.per_stream {
                hub.record_drop(stream_id, count).unwrap();
            }
        }
        assert_eq!(queue.len(), 1);
        assert_eq!(hub.status().unwrap().cumulative_dropped, 2);

        assert!(hub.generate_frames(session, epoch).unwrap().is_empty());
        assert!(hub.generate_frames(session, epoch).unwrap().is_empty());
        let resumed = hub.generate_frames(session, epoch).unwrap();
        assert!(resumed[0].envelope.discontinuity);
        let payload = magnolia_protocol::decode_synthetic_payload(&resumed[0].envelope).unwrap();
        let TelemetryPayload::Diagnostics {
            entries,
            lost_since_previous,
        } = payload
        else {
            panic!("expected diagnostics payload");
        };
        assert_eq!(entries.len(), DIAGNOSTIC_ENTRIES_PER_FRAME as usize);
        assert_eq!(lost_since_previous, 4);
    }
}
