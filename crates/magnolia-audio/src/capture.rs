use crate::{
    f32_le_to_f32, i16_le_to_f32, i32_le_to_f32, QuantumAdapter, MAX_PIPEWIRE_QUANTUM_FRAMES,
};
use pipewire as pw;
use pw::{properties::properties, spa};
use spa::{param::format::MediaSubtype, param::format::MediaType, pod::Pod};
use std::{
    io::Cursor,
    sync::{
        atomic::{AtomicU32, AtomicU64, AtomicU8, Ordering},
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
    pub quantum_frames: u32,
    pub callbacks: u64,
    pub source_frames: u64,
    pub emitted_blocks: u64,
    pub faults: u64,
    pub callback_max_ns: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NativeSampleFormat {
    F32Le = 1,
    S16Le = 2,
    S32Le = 3,
}

#[derive(Debug)]
struct CaptureCounters {
    state: AtomicU8,
    sample_format: AtomicU8,
    sample_rate: AtomicU32,
    channels: AtomicU32,
    quantum_frames: AtomicU32,
    callbacks: AtomicU64,
    source_frames: AtomicU64,
    emitted_blocks: AtomicU64,
    faults: AtomicU64,
    callback_max_ns: AtomicU64,
}

impl Default for CaptureCounters {
    fn default() -> Self {
        Self {
            state: AtomicU8::new(CaptureState::Preparing as u8),
            sample_format: AtomicU8::new(0),
            sample_rate: AtomicU32::new(0),
            channels: AtomicU32::new(0),
            quantum_frames: AtomicU32::new(0),
            callbacks: AtomicU64::new(0),
            source_frames: AtomicU64::new(0),
            emitted_blocks: AtomicU64::new(0),
            faults: AtomicU64::new(0),
            callback_max_ns: AtomicU64::new(0),
        }
    }
}

pub struct PipeWireCapture {
    counters: Arc<CaptureCounters>,
    stop: Option<pw::channel::Sender<()>>,
    worker: Option<JoinHandle<()>>,
}

impl PipeWireCapture {
    pub fn start(configuration: CaptureConfiguration) -> Result<Self, CaptureError> {
        if configuration.target_node_name.trim().is_empty() {
            return Err(CaptureError::BlankTarget);
        }
        let counters = Arc::new(CaptureCounters::default());
        let worker_counters = Arc::clone(&counters);
        let (stop, receiver) = pw::channel::channel();
        let worker = thread::Builder::new()
            .name("magnolia-pipewire-capture".to_owned())
            .spawn(move || {
                if run_capture_loop(configuration, receiver, Arc::clone(&worker_counters)).is_err()
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
        CaptureSnapshot {
            state: decode_state(self.counters.state.load(Ordering::Acquire)),
            sample_format,
            sample_rate: self.counters.sample_rate.load(Ordering::Relaxed),
            channels: self.counters.channels.load(Ordering::Relaxed),
            quantum_frames: self.counters.quantum_frames.load(Ordering::Relaxed),
            callbacks: self.counters.callbacks.load(Ordering::Relaxed),
            source_frames: self.counters.source_frames.load(Ordering::Relaxed),
            emitted_blocks: self.counters.emitted_blocks.load(Ordering::Relaxed),
            faults: self.counters.faults.load(Ordering::Relaxed),
            callback_max_ns: self.counters.callback_max_ns.load(Ordering::Relaxed),
        }
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
    adapter: Option<QuantumAdapter>,
    counters: Arc<CaptureCounters>,
}

fn run_capture_loop(
    configuration: CaptureConfiguration,
    stop: pw::channel::Receiver<()>,
    counters: Arc<CaptureCounters>,
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
        adapter: None,
        counters: Arc::clone(&counters),
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
            data.adapter = usize::try_from(channels)
                .ok()
                .and_then(|channels| QuantumAdapter::new(channels, rate).ok());
            data.counters.sample_format.store(
                data.native_format.map_or(0, |format| format as u8),
                Ordering::Relaxed,
            );
            data.counters.sample_rate.store(rate, Ordering::Relaxed);
            data.counters.channels.store(channels, Ordering::Relaxed);
            if data.native_format.is_none() || data.adapter.is_none() {
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
    let started = Instant::now();
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
    data.counters
        .quantum_frames
        .store(frames as u32, Ordering::Relaxed);
    let source_position = data
        .counters
        .source_frames
        .fetch_add(frames as u64, Ordering::Relaxed);
    let Some(adapter) = data.adapter.as_mut() else {
        data.counters.faults.fetch_add(1, Ordering::Relaxed);
        return;
    };
    let emitted = adapter.push(&data.converted[..samples], source_position, 0, |_, _| {});
    match emitted {
        Ok(blocks) => {
            data.counters
                .emitted_blocks
                .fetch_add(blocks as u64, Ordering::Relaxed);
        }
        Err(_) => {
            data.counters.faults.fetch_add(1, Ordering::Relaxed);
        }
    }
    data.counters.callbacks.fetch_add(1, Ordering::Relaxed);
    data.counters
        .callback_max_ns
        .fetch_max(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
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
