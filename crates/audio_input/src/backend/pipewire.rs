#![cfg(target_os = "linux")]

use std::mem;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pipewire as pw;
use pw::{properties::properties, spa};
use spa::param::format::{MediaSubtype, MediaType};
use spa::param::format_utils;
use spa::pod::Pod;

use talisman_signals::ring_buffer::RingBufferSender;

use super::{AudioInputBackend, BackendStream, DeviceInfo, NegotiatedFormat};

fn now_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

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

/// Native PipeWire input backend (Linux).
pub struct PipeWireInputBackend {
    devices: Vec<DeviceInfo>,
}

impl PipeWireInputBackend {
    pub fn new() -> anyhow::Result<Self> {
        // PipeWire requires global init once per process.
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

impl AudioInputBackend for PipeWireInputBackend {
    fn refresh_devices(&mut self) -> anyhow::Result<Vec<DeviceInfo>> {
        pw::init();

        let mainloop = pw::main_loop::MainLoopRc::new(None)?;
        let context = pw::context::ContextRc::new(&mainloop, None)?;
        let core = context.connect_rc(None)?;
        let registry = core.get_registry()?;

        let devices_acc: Arc<Mutex<Vec<DeviceInfo>>> = Arc::new(Mutex::new(Vec::new()));
        let devices_acc2 = devices_acc.clone();

        // Trigger sync and quit once registry enumeration is done.
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
                // Capture sources
                if !class.starts_with("Audio/Source") {
                    return;
                }

                let name = props
                    .get("node.description")
                    .or_else(|| props.get("node.nick"))
                    .or_else(|| props.get("node.name"))
                    .unwrap_or("Audio Source")
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
        tx: RingBufferSender<f32>,
        capture_us: Arc<AtomicU64>,
    ) -> anyhow::Result<(BackendStream, NegotiatedFormat, String)> {
        pw::init();

        // Ensure we have a fresh device list for name resolution.
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
                    log::error!("PipeWire input: failed to create mainloop: {e}");
                    return;
                }
            };
            let context = match pw::context::ContextRc::new(&mainloop, None) {
                Ok(v) => v,
                Err(e) => {
                    log::error!("PipeWire input: failed to create context: {e}");
                    return;
                }
            };
            let core = match context.connect_rc(None) {
                Ok(v) => v,
                Err(e) => {
                    log::error!("PipeWire input: failed to connect: {e}");
                    return;
                }
            };

            let props = properties! {
                *pw::keys::MEDIA_TYPE => "Audio",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::MEDIA_ROLE => "Music",
            };

            let stream = match pw::stream::StreamBox::new(&core, "talisman-audio-input", props) {
                Ok(v) => v,
                Err(e) => {
                    log::error!("PipeWire input: failed to create stream: {e}");
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
                .process(move |stream, _user_data| match stream.dequeue_buffer() {
                    None => {}
                    Some(mut buffer) => {
                        let datas = buffer.datas_mut();
                        if datas.is_empty() {
                            return;
                        }
                        let data = &mut datas[0];
                        let n_samples = data.chunk().size() / (mem::size_of::<f32>() as u32);
                        if let Some(bytes) = data.data() {
                            capture_us.store(now_micros(), Ordering::Relaxed);
                            for n in 0..n_samples {
                                let start = n as usize * mem::size_of::<f32>();
                                let end = start + mem::size_of::<f32>();
                                if end <= bytes.len() {
                                    let f = f32::from_le_bytes(bytes[start..end].try_into().unwrap());
                                    let _ = tx.try_send(f);
                                }
                            }
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
                spa::utils::Direction::Input,
                target,
                pw::stream::StreamFlags::AUTOCONNECT
                    | pw::stream::StreamFlags::MAP_BUFFERS
                    | pw::stream::StreamFlags::RT_PROCESS,
                &mut params,
            ) {
                log::error!("PipeWire input: stream.connect failed: {e}");
                return;
            }

            // Stop handling: poll stop channel when idle, then quit.
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


