#![cfg(target_os = "linux")]

use std::mem;
use std::sync::{mpsc, Mutex};
use std::thread;
use std::time::Duration;

use pipewire as pw;
use pw::{properties::properties, spa};
use spa::param::format::{MediaSubtype, MediaType};
use spa::param::format_utils;
use spa::pod::Pod;

use talisman_signals::ring_buffer::RingBufferReceiver;

use super::{AudioOutputBackend, BackendStream, DeviceInfo, NegotiatedFormat};

#[derive(Debug)]
struct PipeWireStreamHandle {
    stop_tx: mpsc::Sender<()>,
    join: Mutex<Option<thread::JoinHandle<()>>>,
}

impl Drop for PipeWireStreamHandle {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(());
        if let Ok(mut guard) = self.join.lock() {
            if let Some(j) = guard.take() {
                let _ = j.join();
            }
        }
    }
}

#[derive(Default)]
struct UserData {
    format: spa::param::audio::AudioInfoRaw,
    fmt_tx: Option<mpsc::Sender<NegotiatedFormat>>,
}

/// Native PipeWire output backend (Linux).
pub struct PipeWireOutputBackend {
    devices: Vec<DeviceInfo>,
}

impl PipeWireOutputBackend {
    pub fn new() -> anyhow::Result<Self> {
        pw::init();
        Ok(Self { devices: Vec::new() })
    }

    fn resolve_name(&self, device_id: &str) -> String {
        if device_id == "Default" {
            return "Default".to_string();
        }
        self.devices
            .iter()
            .find(|d| d.id == device_id)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| format!("PipeWire Node {}", device_id))
    }
}

impl AudioOutputBackend for PipeWireOutputBackend {
    fn refresh_devices(&mut self) -> anyhow::Result<Vec<DeviceInfo>> {
        pw::init();

        let mainloop = pw::main_loop::MainLoopRc::new(None)?;
        let context = pw::context::ContextRc::new(&mainloop, None)?;
        let core = context.connect_rc(None)?;
        let registry = core.get_registry()?;

        let devices_acc = std::sync::Arc::new(std::sync::Mutex::new(Vec::<DeviceInfo>::new()));
        let devices_acc2 = devices_acc.clone();

        let pending = core.sync(0)?;
        let loop_weak = mainloop.downgrade();
        let _listener_core = core
            .add_listener_local()
            .done(move |id, seq| {
                if id == pw::core::PW_ID_CORE && seq == pending {
                    if let Some(ml) = loop_weak.upgrade() {
                        ml.quit();
                    }
                }
            })
            .register();

        let _listener_reg = registry
            .add_listener_local()
            .global(move |global| {
                if global.type_ != pw::types::ObjectType::Node {
                    return;
                }
                let Some(props) = global.props else { return; };
                let props = props.as_ref();

                let Some(class) = props.get("media.class") else { return; };
                // Playback sinks
                if !class.starts_with("Audio/Sink") {
                    return;
                }

                let name = props
                    .get("node.description")
                    .or_else(|| props.get("node.nick"))
                    .or_else(|| props.get("node.name"))
                    .unwrap_or("Audio Sink")
                    .to_string();

                let mut guard = devices_acc2.lock().unwrap();
                guard.push(DeviceInfo {
                    id: global.id.to_string(),
                    name,
                });
            })
            .register();

        mainloop.run();

        let mut devices = devices_acc.lock().unwrap().clone();
        devices.sort_by(|a, b| a.name.cmp(&b.name));
        self.devices = devices.clone();
        Ok(devices)
    }

    fn start(
        &mut self,
        device_id: &str,
        rx: RingBufferReceiver<f32>,
    ) -> anyhow::Result<(BackendStream, NegotiatedFormat, String)> {
        pw::init();

        if self.devices.is_empty() {
            let _ = self.refresh_devices();
        }
        let resolved_name = self.resolve_name(device_id);

        let target = if device_id == "Default" {
            None
        } else {
            Some(device_id.parse::<u32>()?)
        };

        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (fmt_tx, fmt_rx) = mpsc::channel::<NegotiatedFormat>();

        let join = thread::spawn(move || {
            pw::init();

            let mainloop = match pw::main_loop::MainLoopRc::new(None) {
                Ok(v) => v,
                Err(e) => {
                    log::error!("PipeWire output: failed to create mainloop: {e}");
                    return;
                }
            };
            let context = match pw::context::ContextRc::new(&mainloop, None) {
                Ok(v) => v,
                Err(e) => {
                    log::error!("PipeWire output: failed to create context: {e}");
                    return;
                }
            };
            let core = match context.connect_rc(None) {
                Ok(v) => v,
                Err(e) => {
                    log::error!("PipeWire output: failed to connect: {e}");
                    return;
                }
            };

            let props = properties! {
                *pw::keys::MEDIA_TYPE => "Audio",
                *pw::keys::MEDIA_CATEGORY => "Playback",
                *pw::keys::MEDIA_ROLE => "Music",
            };

            let stream = match pw::stream::StreamBox::new(&core, "talisman-audio-output", props) {
                Ok(v) => v,
                Err(e) => {
                    log::error!("PipeWire output: failed to create stream: {e}");
                    return;
                }
            };

            let data = UserData {
                format: Default::default(),
                fmt_tx: Some(fmt_tx),
            };

            let _listener = stream
                .add_local_listener_with_user_data(data)
                .param_changed(|_, user_data, id, param| {
                    let Some(param) = param else { return; };
                    if id != pw::spa::param::ParamType::Format.as_raw() {
                        return;
                    }

                    let (media_type, media_subtype) = match format_utils::parse_format(param) {
                        Ok(v) => v,
                        Err(_) => return,
                    };
                    if media_type != MediaType::Audio || media_subtype != MediaSubtype::Raw {
                        return;
                    }

                    if user_data.format.parse(param).is_ok() {
                        if let Some(tx) = user_data.fmt_tx.take() {
                            let _ = tx.send(NegotiatedFormat {
                                sample_rate: user_data.format.rate(),
                                channels: user_data.format.channels() as u16,
                            });
                        }
                    }
                })
                .process(move |stream, user_data| match stream.dequeue_buffer() {
                    None => {}
                    Some(mut buffer) => {
                        let datas = buffer.datas_mut();
                        if datas.is_empty() {
                            return;
                        }
                        let data = &mut datas[0];
                        let channels = user_data.format.channels().max(1) as usize;
                        let stride = mem::size_of::<f32>() * channels;

                        if let Some(slice) = data.data() {
                            let n_frames = slice.len() / stride;
                            for i in 0..n_frames {
                                for c in 0..channels {
                                    let sample = rx.try_recv().unwrap_or(0.0);
                                    let start = i * stride + c * mem::size_of::<f32>();
                                    let end = start + mem::size_of::<f32>();
                                    if end <= slice.len() {
                                        slice[start..end].copy_from_slice(&sample.to_le_bytes());
                                    }
                                }
                            }
                            let chunk = data.chunk_mut();
                            *chunk.offset_mut() = 0;
                            *chunk.stride_mut() = stride as i32;
                            *chunk.size_mut() = (stride * n_frames) as u32;
                        }
                    }
                })
                .register();

            // Ask for raw f32 audio. Leave channels/rate unspecified to follow graph defaults.
            let mut audio_info = spa::param::audio::AudioInfoRaw::new();
            audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
            let obj = pw::spa::pod::Object {
                type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
                id: pw::spa::param::ParamType::EnumFormat.as_raw(),
                properties: audio_info.into(),
            };
            let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
                std::io::Cursor::new(Vec::new()),
                &pw::spa::pod::Value::Object(obj),
            )
            .unwrap()
            .0
            .into_inner();
            let mut params = [Pod::from_bytes(&values).unwrap()];

            if let Err(e) = stream.connect(
                spa::utils::Direction::Output,
                target,
                pw::stream::StreamFlags::AUTOCONNECT
                    | pw::stream::StreamFlags::MAP_BUFFERS
                    | pw::stream::StreamFlags::RT_PROCESS,
                &mut params,
            ) {
                log::error!("PipeWire output: stream.connect failed: {e}");
                return;
            }

            let loop_weak = mainloop.downgrade();
            let _idle = mainloop.loop_().add_idle(true, move || {
                if stop_rx.try_recv().is_ok() {
                    if let Some(ml) = loop_weak.upgrade() {
                        ml.quit();
                    }
                }
            });

            mainloop.run();
        });

        let fmt = fmt_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap_or(NegotiatedFormat {
                sample_rate: 48000,
                channels: 2,
            });

        let handle = PipeWireStreamHandle {
            stop_tx,
            join: Mutex::new(Some(join)),
        };

        Ok((BackendStream::new(handle), fmt, resolved_name))
    }
}


