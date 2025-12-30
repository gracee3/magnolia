mod backend;
mod settings;
#[cfg(feature = "tile-rendering")]
pub mod tile;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use log::{info, warn};

use crate::backend::{default_backend, AudioOutputBackend, BackendStream};
use talisman_core::{DataType, ModuleSchema, Port, PortDirection, Signal, Sink};
use talisman_signals::ring_buffer::{self, RingBufferSender};

use settings::AudioDeviceEntry;
pub use settings::AudioOutputSettings;

const OUTPUT_CAPACITY: usize = 32768;

fn now_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

#[derive(Default)]
pub struct AudioOutputState {
    latency_us: AtomicU64,
    level_milli: AtomicU64,
}

impl AudioOutputState {
    pub fn latency_us(&self) -> u64 {
        self.latency_us.load(Ordering::Relaxed)
    }

    pub fn level_milli(&self) -> u64 {
        self.level_milli.load(Ordering::Relaxed)
    }
}

pub struct AudioOutputSink {
    id: String,
    enabled: bool,
    inner: Arc<Mutex<AudioOutputInner>>,
    state: Arc<AudioOutputState>,
    settings: Arc<AudioOutputSettings>,
    backend: Arc<Mutex<Box<dyn AudioOutputBackend>>>,
    rebuild_thread: Mutex<Option<RebuildThread>>,
}

struct RebuildThread {
    stop_tx: mpsc::Sender<()>,
    join: Option<thread::JoinHandle<()>>,
}

struct AudioOutputInner {
    _stream: Option<BackendStream>,
    sender: RingBufferSender<f32>,
    sample_rate: u32,
    channels: u16,
    warned_mismatch: AtomicBool,
}

impl AudioOutputSink {
    pub fn new(
        id: &str,
        settings: Arc<AudioOutputSettings>,
    ) -> anyhow::Result<(Self, Arc<AudioOutputState>)> {
        let state = Arc::new(AudioOutputState::default());

        let mut backend = default_backend()?;
        let (inner, devices) = match Self::build_stream(&settings, backend.as_mut()) {
            Ok(v) => v,
            Err(e) => {
                // Keep the module alive so the user can fix devices / backend and retry.
                settings.set_last_error(Some(e.to_string()));
                (
                    AudioOutputInner {
                        _stream: None,
                        sender: ring_buffer::channel::<f32>(OUTPUT_CAPACITY).0,
                        sample_rate: 0,
                        channels: 0,
                        warned_mismatch: AtomicBool::new(false),
                    },
                    vec![],
                )
            }
        };
        settings.set_devices(devices);

        let inner = Arc::new(Mutex::new(inner));
        let backend = Arc::new(Mutex::new(backend));

        let sink = Self {
            id: id.to_string(),
            enabled: true,
            inner: inner.clone(),
            state: state.clone(),
            settings: settings.clone(),
            backend: backend.clone(),
            rebuild_thread: Mutex::new(None),
        };

        sink.start_rebuild_thread();

        Ok((sink, state))
    }

    fn build_stream(
        settings: &AudioOutputSettings,
        backend: &mut dyn AudioOutputBackend,
    ) -> anyhow::Result<(AudioOutputInner, Vec<AudioDeviceEntry>)> {
        let (tx, rx) = ring_buffer::channel::<f32>(OUTPUT_CAPACITY);

        let available = backend.refresh_devices().unwrap_or_default();
        let device_entries = available
            .iter()
            .map(|d| AudioDeviceEntry {
                id: d.id.clone(),
                name: d.name.clone(),
            })
            .collect::<Vec<_>>();

        let selected = settings.selected();
        let (stream, fmt, resolved_name) = backend.start(&selected, rx)?;
        info!(
            "AudioOutputSink initialized. SR: {}, Ch: {}, Device: {}",
            fmt.sample_rate, fmt.channels, resolved_name
        );

        settings.set_last_error(None);
        settings.set_active_device(Some(resolved_name.clone()));
        settings.set_format(fmt.sample_rate, fmt.channels);

        Ok((
            AudioOutputInner {
                _stream: Some(stream),
                sender: tx,
                sample_rate: fmt.sample_rate,
                channels: fmt.channels,
                warned_mismatch: AtomicBool::new(false),
            },
            device_entries,
        ))
    }
}

impl AudioOutputSink {
    fn start_rebuild_thread(&self) {
        // Background thread that can rebuild the stream even when no audio is flowing
        // (important for device selection to work while disconnected).
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let settings = self.settings.clone();
        let inner = self.inner.clone();
        let backend = self.backend.clone();

        let join = thread::spawn(move || loop {
            if stop_rx.try_recv().is_ok() {
                break;
            }
            if settings.take_pending() {
                let mut backend_guard = match backend.lock() {
                    Ok(g) => g,
                    Err(_) => {
                        settings
                            .set_last_error(Some("AudioOutput backend lock poisoned".to_string()));
                        thread::sleep(Duration::from_millis(200));
                        continue;
                    }
                };

                match AudioOutputSink::build_stream(&settings, backend_guard.as_mut()) {
                    Ok((next, devices)) => {
                        if let Ok(mut inner_guard) = inner.lock() {
                            *inner_guard = next;
                        }
                        settings.set_devices(devices);
                    }
                    Err(e) => {
                        settings.set_last_error(Some(e.to_string()));
                    }
                }
            }

            thread::sleep(Duration::from_millis(200));
        });

        if let Ok(mut guard) = self.rebuild_thread.lock() {
            *guard = Some(RebuildThread {
                stop_tx,
                join: Some(join),
            });
        }
    }
}

impl Drop for AudioOutputSink {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.rebuild_thread.lock() {
            if let Some(t) = guard.take() {
                let _ = t.stop_tx.send(());
                if let Some(j) = t.join {
                    let _ = j.join();
                }
            }
        }
    }
}

#[async_trait]
impl Sink for AudioOutputSink {
    fn name(&self) -> &str {
        "Audio Output"
    }

    fn schema(&self) -> ModuleSchema {
        ModuleSchema {
            id: self.id.clone(),
            name: "Audio Output".to_string(),
            description: "Plays audio buffers to the system output device (PipeWire on Linux, CPAL elsewhere)".to_string(),
            ports: vec![Port {
                id: "audio_in".to_string(),
                label: "Audio In".to_string(),
                data_type: DataType::Audio,
                direction: PortDirection::Input,
            }],
            settings_schema: None,
        }
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    async fn consume(&self, signal: Signal) -> anyhow::Result<Option<Signal>> {
        if !self.enabled || self.settings.is_muted() {
            self.state.level_milli.store(0, Ordering::Relaxed);
            return Ok(None);
        }

        // Stream rebuilds are handled by a background thread to avoid needing incoming audio.

        let Signal::Audio {
            sample_rate,
            channels,
            timestamp_us,
            data,
        } = signal
        else {
            return Ok(None);
        };

        let inner = self.inner.lock().unwrap();
        if sample_rate != inner.sample_rate || channels != inner.channels {
            if !inner.warned_mismatch.swap(true, Ordering::Relaxed) {
                warn!(
                    "AudioOutputSink: format mismatch ({}Hz/{}ch) != output ({}Hz/{}ch)",
                    sample_rate, channels, inner.sample_rate, inner.channels
                );
            }
            return Ok(None);
        }

        if timestamp_us > 0 {
            let now = now_micros();
            if now >= timestamp_us {
                self.state
                    .latency_us
                    .store(now - timestamp_us, Ordering::Relaxed);
            }
        }

        let mut sum = 0.0f64;
        for sample in &data {
            sum += (*sample as f64) * (*sample as f64);
            let _ = inner.sender.try_send(*sample);
        }

        if !data.is_empty() {
            let rms = (sum / data.len() as f64).sqrt();
            let level_milli = (rms * 1000.0) as u64;
            self.state.level_milli.store(level_milli, Ordering::Relaxed);
        }

        Ok(None)
    }
}
