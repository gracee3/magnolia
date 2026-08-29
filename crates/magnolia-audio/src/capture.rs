use crate::{
    block_channel, f32_le_to_f32, i16_le_to_f32, i32_le_to_f32, AudioFormat, BlockConsumer,
    BlockIndex, BlockProducer, BlockProvenance, CallbackScope, CallbackTiming, EdgeCounters,
    PublishOutcome, QuantumAdapter, StereoLinearResampler, MAX_PIPEWIRE_QUANTUM_FRAMES,
};
use pipewire as pw;
use pw::{properties::properties, spa};
use spa::{param::format::MediaSubtype, param::format::MediaType, pod::Pod};
use std::{
    io::Cursor,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Instant,
};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureConfiguration {
    pub target_node_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CaptureState {
    Preparing = 0,
    Running = 1,
    Paused = 2,
    Failed = 3,
    Stopped = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureSnapshot {
    pub state: CaptureState,
    pub sample_format: Option<NativeSampleFormat>,
    pub sample_rate: u32,
    pub channels: u32,
    pub channel_layout: Option<NativeChannelLayout>,
    pub configuration_error: Option<CaptureConfigurationError>,
    pub quantum_frames: u32,
    pub callbacks: u64,
    pub source_frames: u64,
    pub emitted_blocks: u64,
    pub faults: u64,
    pub dropped_frames: u64,
    pub callback_max_ns: u64,
    pub callback_p99_ns: u64,
    pub callback_p999_ns: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NativeSampleFormat {
    F32Le = 1,
    S16Le = 2,
    S32Le = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NativeChannelLayout {
    Mono = 1,
    Stereo = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CaptureConfigurationError {
    UnsupportedSampleFormat = 1,
    UnsupportedChannelLayout = 2,
    UnsupportedSampleRate = 3,
}

#[derive(Debug)]
struct CaptureCounters {
    state: AtomicU8,
    sample_format: AtomicU8,
    sample_rate: AtomicU32,
    channels: AtomicU32,
    channel_layout: AtomicU8,
    configuration_supported: AtomicBool,
    configuration_error: AtomicU8,
    quantum_frames: AtomicU32,
    timing: CallbackTiming,
    source_frames: AtomicU64,
    emitted_blocks: AtomicU64,
    faults: AtomicU64,
    capture_muted: AtomicBool,
}

impl Default for CaptureCounters {
    fn default() -> Self {
        Self {
            state: AtomicU8::new(CaptureState::Preparing as u8),
            sample_format: AtomicU8::new(0),
            sample_rate: AtomicU32::new(0),
            channels: AtomicU32::new(0),
            channel_layout: AtomicU8::new(0),
            configuration_supported: AtomicBool::new(false),
            configuration_error: AtomicU8::new(0),
            quantum_frames: AtomicU32::new(0),
            timing: CallbackTiming::default(),
            source_frames: AtomicU64::new(0),
            emitted_blocks: AtomicU64::new(0),
            faults: AtomicU64::new(0),
            capture_muted: AtomicBool::new(false),
        }
    }
}

pub struct PipeWireCapture {
    counters: Arc<CaptureCounters>,
    stop: Option<pw::channel::Sender<()>>,
    worker: Option<JoinHandle<()>>,
    consumer: Option<BlockConsumer>,
    monitor_edge_enabled: Arc<AtomicBool>,
    monitor_edge_primed: Arc<AtomicBool>,
    edge_counters: Arc<EdgeCounters>,
    analysis_consumer: Option<BlockConsumer>,
    analysis_edge_enabled: Arc<AtomicBool>,
    analysis_edge_counters: Arc<EdgeCounters>,
}

pub struct MonitorEdge {
    pub(crate) consumer: BlockConsumer,
    pub(crate) enabled: Arc<AtomicBool>,
    pub(crate) primed: Arc<AtomicBool>,
}

pub struct AnalysisEdge {
    consumer: BlockConsumer,
    enabled: Arc<AtomicBool>,
}

impl AnalysisEdge {
    pub fn consume<F, R>(&mut self, consume: F) -> crate::ConsumeOutcome<R>
    where
        F: FnOnce(&crate::AudioBlock) -> R,
    {
        self.consumer.consume(consume)
    }
}

impl Drop for AnalysisEdge {
    fn drop(&mut self) {
        self.enabled.store(false, Ordering::Release);
    }
}

impl Drop for MonitorEdge {
    fn drop(&mut self) {
        self.enabled.store(false, Ordering::Release);
        self.primed.store(false, Ordering::Release);
    }
}

impl PipeWireCapture {
    pub fn start(configuration: CaptureConfiguration) -> Result<Self, CaptureError> {
        if configuration.target_node_name.trim().is_empty() {
            return Err(CaptureError::BlankTarget);
        }
        let counters = Arc::new(CaptureCounters::default());
        let monitor_edge_enabled = Arc::new(AtomicBool::new(false));
        let monitor_edge_primed = Arc::new(AtomicBool::new(false));
        let worker_monitor_edge_enabled = Arc::clone(&monitor_edge_enabled);
        let worker_monitor_edge_primed = Arc::clone(&monitor_edge_primed);
        let analysis_edge_enabled = Arc::new(AtomicBool::new(false));
        let worker_analysis_edge_enabled = Arc::clone(&analysis_edge_enabled);
        let format = AudioFormat::new(48_000, 2, 256).map_err(|_| CaptureError::InternalFormat)?;
        let (producer, consumer, edge_counters) = block_channel(format, 16);
        let (analysis_producer, analysis_consumer, analysis_edge_counters) =
            block_channel(format, 32);
        let worker_counters = Arc::clone(&counters);
        let (stop, receiver) = pw::channel::channel();
        let worker = thread::Builder::new()
            .name("magnolia-pipewire-capture".to_owned())
            .spawn(move || {
                if run_capture_loop(
                    configuration,
                    receiver,
                    Arc::clone(&worker_counters),
                    CaptureEdges {
                        monitor_producer: producer,
                        monitor_enabled: worker_monitor_edge_enabled,
                        monitor_primed: worker_monitor_edge_primed,
                        analysis_producer,
                        analysis_enabled: worker_analysis_edge_enabled,
                    },
                )
                .is_err()
                {
                    worker_counters
                        .state
                        .store(CaptureState::Failed as u8, Ordering::Release);
                }
            })
            .map_err(CaptureError::Thread)?;
        Ok(Self {
            counters,
            stop: Some(stop),
            worker: Some(worker),
            consumer: Some(consumer),
            monitor_edge_enabled,
            monitor_edge_primed,
            edge_counters,
            analysis_consumer: Some(analysis_consumer),
            analysis_edge_enabled,
            analysis_edge_counters,
        })
    }

    #[must_use]
    pub fn snapshot(&self) -> CaptureSnapshot {
        let sample_format = match self.counters.sample_format.load(Ordering::Relaxed) {
            1 => Some(NativeSampleFormat::F32Le),
            2 => Some(NativeSampleFormat::S16Le),
            3 => Some(NativeSampleFormat::S32Le),
            _ => None,
        };
        let mut state = decode_state(self.counters.state.load(Ordering::Acquire));
        if state == CaptureState::Running
            && !self
                .counters
                .configuration_supported
                .load(Ordering::Acquire)
        {
            state = CaptureState::Failed;
        }
        let channel_layout = match self.counters.channel_layout.load(Ordering::Relaxed) {
            1 => Some(NativeChannelLayout::Mono),
            2 => Some(NativeChannelLayout::Stereo),
            _ => None,
        };
        let configuration_error = match self.counters.configuration_error.load(Ordering::Relaxed) {
            1 => Some(CaptureConfigurationError::UnsupportedSampleFormat),
            2 => Some(CaptureConfigurationError::UnsupportedChannelLayout),
            3 => Some(CaptureConfigurationError::UnsupportedSampleRate),
            _ => None,
        };
        let timing = self.counters.timing.snapshot();
        let edge = self.edge_counters.snapshot();
        let analysis_edge = self.analysis_edge_counters.snapshot();
        CaptureSnapshot {
            state,
            sample_format,
            sample_rate: self.counters.sample_rate.load(Ordering::Relaxed),
            channels: self.counters.channels.load(Ordering::Relaxed),
            channel_layout,
            configuration_error,
            quantum_frames: self.counters.quantum_frames.load(Ordering::Relaxed),
            callbacks: timing.callbacks,
            source_frames: self.counters.source_frames.load(Ordering::Relaxed),
            emitted_blocks: self.counters.emitted_blocks.load(Ordering::Relaxed),
            faults: self
                .counters
                .faults
                .load(Ordering::Relaxed)
                .saturating_add(edge.faults)
                .saturating_add(analysis_edge.faults),
            dropped_frames: edge
                .dropped
                .saturating_add(analysis_edge.dropped)
                .saturating_mul(256),
            callback_max_ns: timing.maximum_ns,
            callback_p99_ns: timing.p99_ns,
            callback_p999_ns: timing.p999_ns,
        }
    }

    pub fn take_monitor_edge(&mut self) -> Option<MonitorEdge> {
        let consumer = self.consumer.take()?;
        self.monitor_edge_primed.store(false, Ordering::Release);
        Some(MonitorEdge {
            consumer,
            enabled: Arc::clone(&self.monitor_edge_enabled),
            primed: Arc::clone(&self.monitor_edge_primed),
        })
    }

    #[must_use]
    pub fn monitor_edge_available(&self) -> bool {
        self.consumer.is_some()
    }

    pub fn take_analysis_edge(&mut self) -> Option<AnalysisEdge> {
        let consumer = self.analysis_consumer.take()?;
        self.analysis_edge_enabled.store(true, Ordering::Release);
        Some(AnalysisEdge {
            consumer,
            enabled: Arc::clone(&self.analysis_edge_enabled),
        })
    }

    pub fn set_muted(&self, muted: bool) {
        self.counters.capture_muted.store(muted, Ordering::Release);
    }
}

impl Drop for PipeWireCapture {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.counters
            .state
            .store(CaptureState::Stopped as u8, Ordering::Release);
    }
}

fn decode_state(value: u8) -> CaptureState {
    match value {
        1 => CaptureState::Running,
        2 => CaptureState::Paused,
        3 => CaptureState::Failed,
        4 => CaptureState::Stopped,
        _ => CaptureState::Preparing,
    }
}

struct CallbackData {
    format: spa::param::audio::AudioInfoRaw,
    native_format: Option<NativeSampleFormat>,
    converted: Box<[f32]>,
    stereo: Box<[f32]>,
    resampled: Box<[f32]>,
    resampler: Option<StereoLinearResampler>,
    adapter: Option<QuantumAdapter>,
    counters: Arc<CaptureCounters>,
    producer: BlockProducer,
    monitor_edge_enabled: Arc<AtomicBool>,
    monitor_edge_primed: Arc<AtomicBool>,
    analysis_producer: BlockProducer,
    analysis_edge_enabled: Arc<AtomicBool>,
    monotonic_epoch: Instant,
}

struct CaptureEdges {
    monitor_producer: BlockProducer,
    monitor_enabled: Arc<AtomicBool>,
    monitor_primed: Arc<AtomicBool>,
    analysis_producer: BlockProducer,
    analysis_enabled: Arc<AtomicBool>,
}

fn run_capture_loop(
    configuration: CaptureConfiguration,
    stop: pw::channel::Receiver<()>,
    counters: Arc<CaptureCounters>,
    edges: CaptureEdges,
) -> Result<(), CaptureError> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let stream = pw::stream::StreamBox::new(
        &core,
        "magnolia-capture",
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Production",
            "target.object" => configuration.target_node_name,
        },
    )?;
    let callback_data = CallbackData {
        format: spa::param::audio::AudioInfoRaw::new(),
        native_format: None,
        converted: vec![0.0; MAX_PIPEWIRE_QUANTUM_FRAMES * 2].into_boxed_slice(),
        stereo: vec![0.0; MAX_PIPEWIRE_QUANTUM_FRAMES * 2].into_boxed_slice(),
        resampled: vec![0.0; MAX_PIPEWIRE_QUANTUM_FRAMES * 12].into_boxed_slice(),
        resampler: None,
        adapter: None,
        counters: Arc::clone(&counters),
        producer: edges.monitor_producer,
        monitor_edge_enabled: edges.monitor_enabled,
        monitor_edge_primed: edges.monitor_primed,
        analysis_producer: edges.analysis_producer,
        analysis_edge_enabled: edges.analysis_enabled,
        monotonic_epoch: Instant::now(),
    };
    let _listener = stream
        .add_local_listener_with_user_data(callback_data)
        .state_changed({
            let counters = Arc::clone(&counters);
            move |_stream, _data, _old, state| {
                let value = match state {
                    pw::stream::StreamState::Streaming => CaptureState::Running,
                    pw::stream::StreamState::Paused => CaptureState::Paused,
                    pw::stream::StreamState::Error(_) => CaptureState::Failed,
                    pw::stream::StreamState::Unconnected => CaptureState::Stopped,
                    _ => CaptureState::Preparing,
                };
                counters.state.store(value as u8, Ordering::Release);
            }
        })
        .param_changed(|_, data, id, param| {
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let Some(param) = param else {
                return;
            };
            let Ok((media_type, media_subtype)) = spa::param::format_utils::parse_format(param)
            else {
                data.counters.faults.fetch_add(1, Ordering::Relaxed);
                return;
            };
            if media_type != MediaType::Audio || media_subtype != MediaSubtype::Raw {
                data.counters.faults.fetch_add(1, Ordering::Relaxed);
                return;
            }
            if data.format.parse(param).is_err() {
                data.counters.faults.fetch_add(1, Ordering::Relaxed);
                return;
            }
            data.native_format = match data.format.format() {
                spa::param::audio::AudioFormat::F32LE => Some(NativeSampleFormat::F32Le),
                spa::param::audio::AudioFormat::S16LE => Some(NativeSampleFormat::S16Le),
                spa::param::audio::AudioFormat::S32LE => Some(NativeSampleFormat::S32Le),
                _ => None,
            };
            let channels = data.format.channels();
            let rate = data.format.rate();
            let positions = data.format.position();
            data.counters
                .configuration_supported
                .store(false, Ordering::Release);
            let channel_layout = match channels {
                1 if positions[0] == 0 || positions[0] == spa::sys::SPA_AUDIO_CHANNEL_MONO => {
                    Some(NativeChannelLayout::Mono)
                }
                2 if (positions[0] == 0 && positions[1] == 0)
                    || (positions[0] == spa::sys::SPA_AUDIO_CHANNEL_FL
                        && positions[1] == spa::sys::SPA_AUDIO_CHANNEL_FR) =>
                {
                    Some(NativeChannelLayout::Stereo)
                }
                _ => None,
            };
            let rate_supported = (8_000..=192_000).contains(&rate);
            data.adapter = (channel_layout.is_some() && rate_supported)
                .then(|| QuantumAdapter::new(2, 48_000).ok())
                .flatten();
            data.resampler = (channel_layout.is_some() && rate_supported)
                .then(|| StereoLinearResampler::new(rate, 48_000).ok())
                .flatten();
            data.counters.sample_format.store(
                data.native_format.map_or(0, |format| format as u8),
                Ordering::Relaxed,
            );
            data.counters.sample_rate.store(rate, Ordering::Relaxed);
            data.counters.channels.store(channels, Ordering::Relaxed);
            data.counters.channel_layout.store(
                channel_layout.map_or(0, |layout| layout as u8),
                Ordering::Relaxed,
            );
            let supported =
                data.native_format.is_some() && data.adapter.is_some() && data.resampler.is_some();
            let configuration_error = if data.native_format.is_none() {
                CaptureConfigurationError::UnsupportedSampleFormat as u8
            } else if channel_layout.is_none() {
                CaptureConfigurationError::UnsupportedChannelLayout as u8
            } else if !rate_supported {
                CaptureConfigurationError::UnsupportedSampleRate as u8
            } else {
                0
            };
            data.counters
                .configuration_error
                .store(configuration_error, Ordering::Relaxed);
            data.counters
                .configuration_supported
                .store(supported, Ordering::Release);
            if !supported {
                data.counters.faults.fetch_add(1, Ordering::Relaxed);
            }
        })
        .process(process_capture)
        .register()?;
    let _stop = stop.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        move |_| mainloop.quit()
    });
    let mut encoded = [Vec::new(), Vec::new(), Vec::new()];
    for (slot, format) in encoded.iter_mut().zip([
        spa::param::audio::AudioFormat::F32LE,
        spa::param::audio::AudioFormat::S16LE,
        spa::param::audio::AudioFormat::S32LE,
    ]) {
        *slot = serialize_format(format)?;
    }
    let mut params = [
        Pod::from_bytes(&encoded[0]).ok_or(CaptureError::InvalidPod)?,
        Pod::from_bytes(&encoded[1]).ok_or(CaptureError::InvalidPod)?,
        Pod::from_bytes(&encoded[2]).ok_or(CaptureError::InvalidPod)?,
    ];
    stream.connect(
        spa::utils::Direction::Input,
        None,
        pw::stream::StreamFlags::AUTOCONNECT
            | pw::stream::StreamFlags::MAP_BUFFERS
            | pw::stream::StreamFlags::RT_PROCESS,
        &mut params,
    )?;
    mainloop.run();
    Ok(())
}

fn process_capture(stream: &pw::stream::Stream, data: &mut CallbackData) {
    let _callback_scope = CallbackScope::enter();
    let _callback_timing = data.counters.timing.start();
    let Some(mut buffer) = stream.dequeue_buffer() else {
        data.counters.faults.fetch_add(1, Ordering::Relaxed);
        return;
    };
    let Some(plane) = buffer.datas_mut().first_mut() else {
        data.counters.faults.fetch_add(1, Ordering::Relaxed);
        return;
    };
    let size = plane.chunk().size() as usize;
    let Some(bytes) = plane.data() else {
        data.counters.faults.fetch_add(1, Ordering::Relaxed);
        return;
    };
    if size > bytes.len() {
        data.counters.faults.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let converted = match data.native_format {
        Some(NativeSampleFormat::F32Le) => f32_le_to_f32(&bytes[..size], &mut data.converted),
        Some(NativeSampleFormat::S16Le) => i16_le_to_f32(&bytes[..size], &mut data.converted),
        Some(NativeSampleFormat::S32Le) => i32_le_to_f32(&bytes[..size], &mut data.converted),
        None => return,
    };
    let Ok(samples) = converted else {
        data.counters.faults.fetch_add(1, Ordering::Relaxed);
        return;
    };
    let channels = data.format.channels() as usize;
    if channels == 0 || !samples.is_multiple_of(channels) {
        data.counters.faults.fetch_add(1, Ordering::Relaxed);
        return;
    }
    let frames = samples / channels;
    if frames > MAX_PIPEWIRE_QUANTUM_FRAMES {
        data.counters.faults.fetch_add(1, Ordering::Relaxed);
        return;
    }
    data.counters
        .quantum_frames
        .store(frames as u32, Ordering::Relaxed);
    let source_position = data
        .counters
        .source_frames
        .fetch_add(frames as u64, Ordering::Relaxed);
    let stereo_samples = frames * 2;
    match channels {
        1 => {
            for (sample, frame) in data.converted[..samples]
                .iter()
                .zip(data.stereo[..stereo_samples].chunks_exact_mut(2))
            {
                frame[0] = *sample;
                frame[1] = *sample;
            }
        }
        2 => data.stereo[..stereo_samples].copy_from_slice(&data.converted[..samples]),
        _ => {
            data.counters.faults.fetch_add(1, Ordering::Relaxed);
            return;
        }
    }
    let Some(resampler) = data.resampler.as_mut() else {
        data.counters.faults.fetch_add(1, Ordering::Relaxed);
        return;
    };
    let Ok(resampled_frames) =
        resampler.process(&data.stereo[..stereo_samples], &mut data.resampled)
    else {
        data.counters.faults.fetch_add(1, Ordering::Relaxed);
        return;
    };
    let monotonic_epoch = &data.monotonic_epoch;
    let Some(adapter) = data.adapter.as_mut() else {
        data.counters.faults.fetch_add(1, Ordering::Relaxed);
        return;
    };
    let producer = &mut data.producer;
    let counters = &data.counters;
    let monitor_edge_enabled = &data.monitor_edge_enabled;
    let monitor_edge_primed = &data.monitor_edge_primed;
    let analysis_edge_enabled = &data.analysis_edge_enabled;
    let analysis_producer = &mut data.analysis_producer;
    let muted = counters.capture_muted.load(Ordering::Acquire);
    let monotonic_ns = monotonic_epoch.elapsed().as_nanos() as u64;
    let mut offset_frames = 0;
    let mut emitted_blocks = 0_u64;
    while offset_frames < resampled_frames {
        let available = MAX_PIPEWIRE_QUANTUM_FRAMES - adapter.buffered_frames();
        let chunk_frames = available.min(resampled_frames - offset_frames);
        let start_sample = offset_frames * 2;
        let end_sample = start_sample + chunk_frames * 2;
        let emitted = adapter.push(
            &data.resampled[start_sample..end_sample],
            source_position.saturating_add(offset_frames as u64),
            monotonic_ns,
            |samples, meta| {
                if monitor_edge_enabled.load(Ordering::Acquire) {
                    let outcome = producer.publish(BlockIndex(meta.sequence), 256, |destination| {
                        if muted {
                            destination.fill(0.0);
                        } else {
                            destination.copy_from_slice(samples);
                        }
                    });
                    if outcome != PublishOutcome::Published {
                        counters.faults.fetch_add(1, Ordering::Relaxed);
                    } else {
                        monitor_edge_primed.store(true, Ordering::Release);
                    }
                }
                if analysis_edge_enabled.load(Ordering::Acquire) {
                    let complete_ns = monotonic_epoch.elapsed().as_nanos() as u64;
                    let outcome = analysis_producer.publish_with_provenance(
                        BlockIndex(meta.sequence),
                        256,
                        BlockProvenance {
                            source_frame_position: meta.source_frame_position,
                            capture_monotonic_ns: meta.monotonic_ns,
                            block_complete_monotonic_ns: complete_ns,
                            graph_monotonic_ns: complete_ns,
                            dropped_frames_before: meta.dropped_frames_before,
                            discontinuity: meta.discontinuity.is_some(),
                        },
                        |destination| destination.copy_from_slice(samples),
                    );
                    if outcome != PublishOutcome::Published {
                        counters.faults.fetch_add(1, Ordering::Relaxed);
                    }
                }
            },
        );
        let Ok(blocks) = emitted else {
            data.counters.faults.fetch_add(1, Ordering::Relaxed);
            break;
        };
        emitted_blocks = emitted_blocks.saturating_add(blocks as u64);
        offset_frames += chunk_frames;
    }
    data.counters
        .emitted_blocks
        .fetch_add(emitted_blocks, Ordering::Relaxed);
}

fn serialize_format(format: spa::param::audio::AudioFormat) -> Result<Vec<u8>, CaptureError> {
    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(format);
    pw::spa::pod::serialize::PodSerializer::serialize(
        Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(pw::spa::pod::Object {
            type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
            id: pw::spa::param::ParamType::EnumFormat.as_raw(),
            properties: info.into(),
        }),
    )
    .map(|(cursor, _)| cursor.into_inner())
    .map_err(|error| CaptureError::Serialize(error.to_string()))
}

#[derive(Debug, Error)]
pub enum CaptureError {
    #[error("PipeWire capture target must not be blank")]
    BlankTarget,
    #[error("failed to start PipeWire capture thread: {0}")]
    Thread(std::io::Error),
    #[error("PipeWire capture failed: {0}")]
    PipeWire(#[from] pw::Error),
    #[error("failed to serialize PipeWire format: {0}")]
    Serialize(String),
    #[error("serialized PipeWire format pod is invalid")]
    InvalidPod,
    #[error("internal canonical audio format is invalid")]
    InternalFormat,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_capture_target_is_rejected_before_a_thread_or_device_is_opened() {
        assert!(matches!(
            PipeWireCapture::start(CaptureConfiguration {
                target_node_name: " ".to_owned()
            }),
            Err(CaptureError::BlankTarget)
        ));
    }
}
