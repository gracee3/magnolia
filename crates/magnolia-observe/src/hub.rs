use crate::{AnalyzerEngine, AnalyzerFrame, AnalyzerKind, BlockTiming};
use magnolia_audio::{AnalysisEdge, ConsumeOutcome};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex, MutexGuard,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Default)]
pub struct ObservationHub {
    inner: Arc<ObservationState>,
}

#[derive(Default)]
struct ObservationState {
    leases: [AtomicUsize; 4],
    latest: Mutex<BTreeMap<AnalyzerKind, AnalyzerFrame>>,
    processed_blocks: AtomicU64,
    ring_faults: AtomicU64,
    latencies: Mutex<BTreeMap<AnalyzerKind, VecDeque<u64>>>,
}

impl ObservationHub {
    pub fn acquire(&self, kind: AnalyzerKind) {
        self.inner.leases[kind_index(kind)].fetch_add(1, Ordering::Relaxed);
    }

    pub fn release(&self, kind: AnalyzerKind) {
        let lease = &self.inner.leases[kind_index(kind)];
        let _ = lease.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_sub(1)
        });
    }

    #[must_use]
    pub fn lease_count(&self, kind: AnalyzerKind) -> usize {
        self.inner.leases[kind_index(kind)].load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn latest(&self, kind: AnalyzerKind) -> Option<AnalyzerFrame> {
        self.latest_frames().get(&kind).cloned()
    }

    #[must_use]
    pub fn status(&self) -> ObservationStatus {
        let latencies = self
            .inner
            .latencies
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        ObservationStatus {
            processed_blocks: self.inner.processed_blocks.load(Ordering::Relaxed),
            ring_faults: self.inner.ring_faults.load(Ordering::Relaxed),
            active_leases: AnalyzerKind::ALL
                .into_iter()
                .map(|kind| self.lease_count(kind))
                .sum(),
            latency_p95_ns: latencies
                .iter()
                .map(|(kind, values)| (*kind, percentile(values, 0.95)))
                .collect(),
        }
    }

    pub fn attach(
        &self,
        edge: AnalysisEdge,
        sample_rate: u32,
        channels: usize,
    ) -> Result<ObservationWorker, std::io::Error> {
        ObservationWorker::spawn(self.clone(), edge, sample_rate, channels)
    }

    fn latest_frames(&self) -> MutexGuard<'_, BTreeMap<AnalyzerKind, AnalyzerFrame>> {
        self.inner
            .latest
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }
}

impl AnalyzerKind {
    pub const ALL: [Self; 4] = [
        Self::Meter,
        Self::Waveform,
        Self::Spectrum,
        Self::Diagnostics,
    ];
}

fn kind_index(kind: AnalyzerKind) -> usize {
    match kind {
        AnalyzerKind::Meter => 0,
        AnalyzerKind::Waveform => 1,
        AnalyzerKind::Spectrum => 2,
        AnalyzerKind::Diagnostics => 3,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationStatus {
    pub processed_blocks: u64,
    pub ring_faults: u64,
    pub active_leases: usize,
    pub latency_p95_ns: BTreeMap<AnalyzerKind, u64>,
}

pub struct ObservationWorker {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl ObservationWorker {
    fn spawn(
        hub: ObservationHub,
        mut edge: AnalysisEdge,
        sample_rate: u32,
        channels: usize,
    ) -> Result<Self, std::io::Error> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("magnolia-observation".to_owned())
            .spawn(move || {
                let Ok(mut analyzer) = AnalyzerEngine::new(sample_rate, channels) else {
                    return;
                };
                let mut discontinuities = 0_u64;
                let mut cumulative_dropped = 0_u64;
                while !worker_stop.load(Ordering::Acquire) {
                    for kind in AnalyzerKind::ALL {
                        analyzer.set_leased(kind, hub.lease_count(kind) > 0);
                    }
                    if analyzer.is_bypassed() {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    let started = Instant::now();
                    let outcome = edge.consume(|block| {
                        let provenance = block.provenance();
                        if provenance.discontinuity || block.discontinuity().is_some() {
                            discontinuities = discontinuities.saturating_add(1);
                        }
                        cumulative_dropped =
                            cumulative_dropped.saturating_add(provenance.dropped_frames_before);
                        let timing = BlockTiming {
                            sequence: block.index().0,
                            source_start: provenance.source_frame_position,
                            capture_monotonic_ns: provenance.capture_monotonic_ns,
                            block_complete_monotonic_ns: provenance.block_complete_monotonic_ns,
                            graph_monotonic_ns: provenance.graph_monotonic_ns,
                            analyzer_monotonic_ns: provenance
                                .block_complete_monotonic_ns
                                .saturating_add(
                                    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX),
                                ),
                            cumulative_dropped_frames: cumulative_dropped,
                            discontinuity: provenance.discontinuity
                                || block.discontinuity().is_some(),
                            queue_depth: 0,
                            utilization_millionths: 0,
                            processing_ns: u64::try_from(started.elapsed().as_nanos())
                                .unwrap_or(u64::MAX),
                            cumulative_discontinuities: discontinuities,
                        };
                        analyzer.process(block.samples(), timing).map(|mut frames| {
                            let processing_ns =
                                u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
                            let completed = provenance
                                .block_complete_monotonic_ns
                                .saturating_add(processing_ns);
                            for frame in &mut frames {
                                set_completion(frame, completed, processing_ns);
                            }
                            frames
                        })
                    });
                    match outcome {
                        ConsumeOutcome::Consumed(Ok(frames)) => {
                            let mut latest = hub.latest_frames();
                            for frame in frames {
                                let metadata = frame_header(&frame);
                                let latency = metadata
                                    .analyzer_monotonic_ns
                                    .saturating_sub(metadata.capture_monotonic_ns);
                                let mut latencies = hub
                                    .inner
                                    .latencies
                                    .lock()
                                    .unwrap_or_else(|error| error.into_inner());
                                let samples = latencies.entry(frame.kind()).or_default();
                                if samples.len() == 131_072 {
                                    samples.pop_front();
                                }
                                samples.push_back(latency);
                                latest.insert(frame.kind(), frame);
                            }
                            hub.inner.processed_blocks.fetch_add(1, Ordering::Relaxed);
                        }
                        ConsumeOutcome::Consumed(Err(_)) | ConsumeOutcome::RingFault => {
                            hub.inner.ring_faults.fetch_add(1, Ordering::Relaxed);
                        }
                        ConsumeOutcome::Empty => thread::sleep(Duration::from_millis(1)),
                    }
                }
            })?;
        Ok(Self {
            stop,
            worker: Some(worker),
        })
    }
}

fn frame_header(frame: &AnalyzerFrame) -> &crate::FrameHeader {
    match frame {
        AnalyzerFrame::Meter(frame) => &frame.header,
        AnalyzerFrame::Waveform(frame) => &frame.header,
        AnalyzerFrame::Spectrum(frame) => &frame.header,
        AnalyzerFrame::Diagnostics(frame) => &frame.header,
    }
}

fn set_completion(frame: &mut AnalyzerFrame, completed_ns: u64, processing_ns: u64) {
    let header = match frame {
        AnalyzerFrame::Meter(frame) => &mut frame.header,
        AnalyzerFrame::Waveform(frame) => &mut frame.header,
        AnalyzerFrame::Spectrum(frame) => &mut frame.header,
        AnalyzerFrame::Diagnostics(frame) => {
            frame.processing_ns = processing_ns;
            frame.latency_ns = completed_ns.saturating_sub(frame.header.capture_monotonic_ns);
            &mut frame.header
        }
    };
    header.analyzer_monotonic_ns = completed_ns;
}

fn percentile(values: &VecDeque<u64>, percentile: f64) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let mut sorted = values.iter().copied().collect::<Vec<_>>();
    sorted.sort_unstable();
    let index = ((sorted.len() - 1) as f64 * percentile).ceil() as usize;
    sorted[index]
}

impl Drop for ObservationWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl AnalyzerFrame {
    #[must_use]
    pub const fn kind(&self) -> AnalyzerKind {
        match self {
            Self::Meter(_) => AnalyzerKind::Meter,
            Self::Waveform(_) => AnalyzerKind::Waveform,
            Self::Spectrum(_) => AnalyzerKind::Spectrum,
            Self::Diagnostics(_) => AnalyzerKind::Diagnostics,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lease_counts_are_saturating_and_independent() {
        let hub = ObservationHub::default();
        hub.release(AnalyzerKind::Meter);
        hub.acquire(AnalyzerKind::Meter);
        hub.acquire(AnalyzerKind::Meter);
        hub.acquire(AnalyzerKind::Spectrum);
        assert_eq!(hub.lease_count(AnalyzerKind::Meter), 2);
        assert_eq!(hub.lease_count(AnalyzerKind::Spectrum), 1);
        hub.release(AnalyzerKind::Meter);
        assert_eq!(hub.lease_count(AnalyzerKind::Meter), 1);
    }
}
