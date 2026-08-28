use crate::{BlockConsumer, CallbackScope, ConsumeOutcome};
use pipewire as pw;
use pw::{properties::properties, spa};
use spa::pod::Pod;
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

const CHANNELS: usize = 2;
const BLOCK_SAMPLES: usize = 256 * CHANNELS;
const RAMP_FRAMES: f32 = 2_400.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputConfiguration {
    pub target_node_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputSnapshot {
    pub running: bool,
    pub callbacks: u64,
    pub underruns: u64,
    pub callback_max_ns: u64,
}

#[derive(Debug, Default)]
struct OutputControls {
    state: AtomicU8,
    target_gain_millionths: AtomicU32,
    muted: AtomicBool,
    callbacks: AtomicU64,
    underruns: AtomicU64,
    callback_max_ns: AtomicU64,
}

pub struct PipeWireOutput {
    controls: Arc<OutputControls>,
    stop: Option<pw::channel::Sender<()>>,
    worker: Option<JoinHandle<()>>,
}

impl PipeWireOutput {
    pub fn start(
        configuration: OutputConfiguration,
        consumer: BlockConsumer,
    ) -> Result<Self, OutputError> {
        if configuration.target_node_name.trim().is_empty() {
            return Err(OutputError::BlankTarget);
        }
        let controls = Arc::new(OutputControls {
            muted: AtomicBool::new(true),
            ..OutputControls::default()
        });
        let worker_controls = Arc::clone(&controls);
        let (stop, receiver) = pw::channel::channel();
        let worker = thread::Builder::new()
            .name("magnolia-pipewire-output".to_owned())
            .spawn(move || {
                if run_output_loop(configuration, receiver, consumer, &worker_controls).is_err() {
                    worker_controls.state.store(3, Ordering::Release);
                }
            })
            .map_err(OutputError::Thread)?;
        Ok(Self {
            controls,
            stop: Some(stop),
            worker: Some(worker),
        })
    }

    pub fn set_muted(&self, muted: bool) {
        self.controls.muted.store(muted, Ordering::Release);
    }

    pub fn set_gain_millionths(&self, gain: u32) {
        self.controls
            .target_gain_millionths
            .store(gain.min(1_000_000), Ordering::Release);
    }

    #[must_use]
    pub fn snapshot(&self) -> OutputSnapshot {
        OutputSnapshot {
            running: self.controls.state.load(Ordering::Acquire) == 1,
            callbacks: self.controls.callbacks.load(Ordering::Relaxed),
            underruns: self.controls.underruns.load(Ordering::Relaxed),
            callback_max_ns: self.controls.callback_max_ns.load(Ordering::Relaxed),
        }
    }
}

impl Drop for PipeWireOutput {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct OutputData {
    consumer: BlockConsumer,
    controls: Arc<OutputControls>,
    held: [f32; BLOCK_SAMPLES],
    held_offset: usize,
    held_samples: usize,
    current_gain: f32,
}

fn run_output_loop(
    configuration: OutputConfiguration,
    stop: pw::channel::Receiver<()>,
    consumer: BlockConsumer,
    controls: &Arc<OutputControls>,
) -> Result<(), OutputError> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let stream = pw::stream::StreamBox::new(
        &core,
        "magnolia-monitor",
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Playback",
            *pw::keys::MEDIA_ROLE => "Production",
            *pw::keys::AUDIO_CHANNELS => "2",
            *pw::keys::NODE_LATENCY => "256/48000",
            "target.object" => configuration.target_node_name,
        },
    )?;
    let data = OutputData {
        consumer,
        controls: Arc::clone(controls),
        held: [0.0; BLOCK_SAMPLES],
        held_offset: 0,
        held_samples: 0,
        current_gain: 0.0,
    };
    let _listener = stream
        .add_local_listener_with_user_data(data)
        .state_changed({
            let controls = Arc::clone(controls);
            move |_stream, _data, _old, state| {
                let value = match state {
                    pw::stream::StreamState::Streaming => 1,
                    pw::stream::StreamState::Paused => 2,
                    pw::stream::StreamState::Error(_) => 3,
                    _ => 0,
                };
                controls.state.store(value, Ordering::Release);
            }
        })
        .process(process_output)
        .register()?;
    let _stop = stop.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        move |_| mainloop.quit()
    });
    let encoded = serialize_output_format()?;
    let mut params = [Pod::from_bytes(&encoded).ok_or(OutputError::InvalidPod)?];
    stream.connect(
        spa::utils::Direction::Output,
        None,
        pw::stream::StreamFlags::AUTOCONNECT
            | pw::stream::StreamFlags::MAP_BUFFERS
            | pw::stream::StreamFlags::RT_PROCESS,
        &mut params,
    )?;
    mainloop.run();
    Ok(())
}

fn process_output(stream: &pw::stream::Stream, data: &mut OutputData) {
    let _callback_scope = CallbackScope::enter();
    let started = Instant::now();
    let Some(mut buffer) = stream.dequeue_buffer() else {
        data.controls.underruns.fetch_add(1, Ordering::Relaxed);
        return;
    };
    let requested_frames = usize::try_from(buffer.requested()).unwrap_or(0);
    let Some(plane) = buffer.datas_mut().first_mut() else {
        data.controls.underruns.fetch_add(1, Ordering::Relaxed);
        return;
    };
    let Some(bytes) = plane.data() else {
        data.controls.underruns.fetch_add(1, Ordering::Relaxed);
        return;
    };
    let capacity_frames = bytes.len() / (CHANNELS * size_of::<f32>());
    let frames = if requested_frames == 0 {
        capacity_frames
    } else {
        requested_frames.min(capacity_frames)
    };
    let samples_needed = frames * CHANNELS;
    let mut written = 0;
    while written < samples_needed {
        if data.held_offset == data.held_samples {
            match data.consumer.consume(|block| {
                let samples = block.samples();
                data.held[..samples.len()].copy_from_slice(samples);
                samples.len()
            }) {
                ConsumeOutcome::Consumed(samples) => {
                    data.held_offset = 0;
                    data.held_samples = samples;
                }
                ConsumeOutcome::Empty | ConsumeOutcome::RingFault => {
                    data.controls.underruns.fetch_add(1, Ordering::Relaxed);
                    break;
                }
            }
        }
        let available = data.held_samples - data.held_offset;
        let count = available.min(samples_needed - written);
        for sample in &data.held[data.held_offset..data.held_offset + count] {
            let target = if data.controls.muted.load(Ordering::Acquire) {
                0.0
            } else {
                data.controls.target_gain_millionths.load(Ordering::Acquire) as f32 / 1_000_000.0
            };
            if written.is_multiple_of(CHANNELS) {
                data.current_gain = ramp_gain(data.current_gain, target);
            }
            let encoded = (*sample * data.current_gain).to_le_bytes();
            let offset = written * size_of::<f32>();
            bytes[offset..offset + size_of::<f32>()].copy_from_slice(&encoded);
            written += 1;
        }
        data.held_offset += count;
    }
    if written < samples_needed {
        bytes[written * size_of::<f32>()..samples_needed * size_of::<f32>()].fill(0);
    }
    let chunk = plane.chunk_mut();
    *chunk.offset_mut() = 0;
    *chunk.stride_mut() = (CHANNELS * size_of::<f32>()) as i32;
    *chunk.size_mut() = (samples_needed * size_of::<f32>()) as u32;
    data.controls.callbacks.fetch_add(1, Ordering::Relaxed);
    data.controls
        .callback_max_ns
        .fetch_max(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
}

fn ramp_gain(current: f32, target: f32) -> f32 {
    current + (target - current).clamp(-1.0 / RAMP_FRAMES, 1.0 / RAMP_FRAMES)
}

fn serialize_output_format() -> Result<Vec<u8>, OutputError> {
    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);
    info.set_rate(48_000);
    info.set_channels(CHANNELS as u32);
    pw::spa::pod::serialize::PodSerializer::serialize(
        Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(pw::spa::pod::Object {
            type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
            id: pw::spa::param::ParamType::EnumFormat.as_raw(),
            properties: info.into(),
        }),
    )
    .map(|(cursor, _)| cursor.into_inner())
    .map_err(|error| OutputError::Serialize(error.to_string()))
}

#[derive(Debug, Error)]
pub enum OutputError {
    #[error("PipeWire output target must not be blank")]
    BlankTarget,
    #[error("failed to start PipeWire output thread: {0}")]
    Thread(std::io::Error),
    #[error("PipeWire output failed: {0}")]
    PipeWire(#[from] pw::Error),
    #[error("failed to serialize PipeWire output format: {0}")]
    Serialize(String),
    #[error("serialized PipeWire output format pod is invalid")]
    InvalidPod,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_gain_reaches_target_after_fifty_milliseconds_of_frames() {
        let mut gain = 0.0;
        for _ in 0..2_400 {
            gain = ramp_gain(gain, 1.0);
        }
        assert!((gain - 1.0).abs() < 0.000_1);
        assert!(ramp_gain(0.0, 1.0) < 0.001);
    }
}
